//! What the capability is asked (ADR-0057): a session opened, a line said, a transcript read, the sessions listed, a
//! turn interrupted, and a session archived. Any session reads in full from the transcript it holds, with no container,
//! whether it is awake, hibernating, or archived.

use super::asks::{
    Archive, Archived, Interrupt, Interrupted, ListSessions, Listed, NewSession, Opened, ReadTranscript, Refusal,
    Refuses, Said, Say, Transcribed,
};
use super::chat::Chat;
use super::saves::Indexed;
use super::session::{Activity, Life, Session, Then};
use super::transcript::{Ended, Entry, Speaker, is_command};
use super::{Phase, Sessions};
use aethereal_actor::{Actors, Fault, Held, Later, Turn};
use aethereal_data::{Bound, Encode, Kind};
use core::fmt::Write as _;

/// The most entries a read answers when it states no most of its own, and the most it may state.
const MOST_ENTRIES: u32 = 200;

/// An ask the capability holds until it has found its sessions' containers, and answers once it has.
#[derive(Bound)]
pub(super) enum Waiting {
    New {
        held: Held<Opened>,
        name: String,
    },
    Say {
        held: Held<Said>,
        session: u32,
        text: String,
    },
    Read {
        held: Held<Transcribed>,
        read: ReadTranscript,
    },
    Interrupt {
        held: Held<Interrupted>,
        session: u32,
    },
    Archive {
        held: Held<Archived>,
        session: u32,
    },
    /// What the owner did at the trusted engine `engine` (ADR-0057), which is answered only when it cannot be done.
    Chat {
        engine: u32,
        chat: Chat,
    },
}

/// Who waits on what an ask comes to: a caller of the capability, answered with it, or the trusted engine `engine`
/// whose chat frame `chat` it was, which is declined when it is refused and told nothing otherwise, as the session list
/// shows it what came of it.
#[derive(Bound)]
pub(super) enum Asker<A> {
    Caller(Held<A>),
    Engine { engine: u32, chat: Chat },
}

impl Sessions {
    /// A new session, numbered after every other, its first container started: answered once it runs.
    pub(super) fn handle_new_session(
        &mut self,
        new: NewSession,
        turn: &mut Turn<'_, Self>,
    ) -> Result<Later<Opened>, Fault> {
        let (held, later) = turn.hold();
        self.take(turn, Waiting::New { held, name: new.name })?;

        Ok(later)
    }

    /// A line for a session: written to it in order, a turn running or not; a hibernating session is woken first.
    pub(super) fn handle_say(&mut self, say: Say, turn: &mut Turn<'_, Self>) -> Result<Later<Said>, Fault> {
        let (held, later) = turn.hold();
        self.take(turn, Waiting::Say { held, session: say.session, text: say.text })?;

        Ok(later)
    }

    pub(super) fn handle_read_transcript(
        &mut self,
        read: ReadTranscript,
        turn: &mut Turn<'_, Self>,
    ) -> Result<Later<Transcribed>, Fault> {
        let (held, later) = turn.hold();
        self.take(turn, Waiting::Read { held, read })?;

        Ok(later)
    }

    #[allow(clippy::unnecessary_wraps, reason = "a handler's body, it takes and answers what its handler does")]
    pub(super) fn handle_list_sessions(
        &mut self,
        ListSessions: ListSessions,
        turn: &Turn<'_, Self>,
    ) -> Result<Listed, Fault> {
        let sessions = self.rows();
        let most_running = self.settings.most_running;

        Ok(Listed { sessions, now: turn.now(), most_running, strays: self.strays.clone() })
    }

    /// The session's running turn interrupted, through the control request Claude Code reads on its stdin.
    #[allow(clippy::needless_pass_by_value, reason = "a handler's body, it takes and answers what its handler does")]
    pub(super) fn handle_interrupt(
        &mut self,
        interrupt: Interrupt,
        turn: &mut Turn<'_, Self>,
    ) -> Result<Later<Interrupted>, Fault> {
        let (held, later) = turn.hold();
        self.take(turn, Waiting::Interrupt { held, session: interrupt.session })?;

        Ok(later)
    }

    /// The session archived: its container stopped and removed, its transcript and home kept. Answered once no
    /// container of it is left.
    #[allow(clippy::needless_pass_by_value, reason = "a handler's body, it takes and answers what its handler does")]
    pub(super) fn handle_archive(
        &mut self,
        archive: Archive,
        turn: &mut Turn<'_, Self>,
    ) -> Result<Later<Archived>, Fault> {
        let (held, later) = turn.hold();
        self.take(turn, Waiting::Archive { held, session: archive.session })?;

        Ok(later)
    }
}

impl Sessions {
    /// Answers `waiting`, or holds it until the capability has found its sessions' containers.
    pub(super) fn take(&mut self, turn: &mut Turn<'_, Self>, waiting: Waiting) -> Result<(), Fault> {
        match &mut self.phase {
            Phase::Indexing { waiting: held }
            | Phase::Reading { waiting: held, .. }
            | Phase::Listing { waiting: held, .. } => {
                held.push(waiting);
                return Ok(());
            }
            Phase::Ready { .. } => {}
        }
        match waiting {
            Waiting::New { held, name } => self.open(turn, Asker::Caller(held), name)?,
            Waiting::Say { held, session, text } => {
                let said = self.say(turn, session, Speaker::Caller, text)?;
                turn.answer(held, &said)?;
            }
            Waiting::Read { held, read } => turn.answer(held, &self.read(&read))?,
            Waiting::Interrupt { held, session } => {
                let interrupted = self.interrupt(turn, session, Speaker::Caller)?;
                turn.answer(held, &interrupted)?;
            }
            Waiting::Archive { held, session } => self.archive(turn, Asker::Caller(held), session)?,
            Waiting::Chat { engine, chat } => self.chat(turn, engine, chat)?,
        }

        self.flush(turn)
    }

    /// Opens a session named `name`, numbered after every other, its first container started: `opener` is answered
    /// once it runs, or why not.
    pub(super) fn open(&mut self, turn: &mut Turn<'_, Self>, opener: Asker<Opened>, name: String) -> Result<(), Fault> {
        if let Err(why) = self.may_start() {
            return self.reply(turn, opener, &Opened::Refused(why));
        }
        let session = self.held.keys().next_back().map_or(1, |last| last + 1);
        self.held.insert(session, Session::new(name, opener));
        self.indexed = Indexed::Changed;
        self.start(session);

        Ok(())
    }

    /// The entries `read` asks for, from the transcript held, whatever the session stands as.
    fn read(&self, read: &ReadTranscript) -> Transcribed {
        let ReadTranscript { session, after, most } = *read;
        let Some(held) = self.held.get(&session) else {
            return Transcribed::Refused(Refusal::NoSuchSession);
        };
        let most = if most == 0 {
            MOST_ENTRIES
        } else {
            most.min(MOST_ENTRIES)
        };
        let most = usize::try_from(most).unwrap_or(usize::MAX);

        Transcribed::Entries { entries: held.transcript.after(after, most), last: held.transcript.last() }
    }

    /// Interrupts `session`'s running turn, as `by` asked.
    pub(super) fn interrupt(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        by: Speaker,
    ) -> Result<Interrupted, Fault> {
        let Some(held) = self.held.get(&session) else {
            return Ok(Interrupted::Refused(Refusal::NoSuchSession));
        };
        let is_running = matches!(held.life, Life::Running(_));
        if !is_running {
            return Ok(Interrupted::Refused(Refusal::NotRunning));
        }
        let number = self.note(turn, session, Entry::Interrupted { by });
        self.type_interrupt(turn, session, number)?;

        Ok(Interrupted::Sent { number })
    }

    /// Archives `session`, answering `archiver` once no container of it is left.
    pub(super) fn archive(
        &mut self,
        turn: &mut Turn<'_, Self>,
        archiver: Asker<Archived>,
        session: u32,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            return self.reply(turn, archiver, &Archived::Refused(Refusal::NoSuchSession));
        };
        match &mut held.life {
            Life::Archived => self.reply(turn, archiver, &Archived::Done)?,
            Life::Hibernating { .. } => {
                held.life = Life::Archived;
                self.dirty(session);
                self.reply(turn, archiver, &Archived::Done)?;
            }
            Life::Starting { .. } | Life::Stopping { then: Then::Rest { .. }, .. } => {
                self.reply(turn, archiver, &Archived::Refused(Refusal::Busy))?;
            }
            Life::Stopping { then: Then::Archive { archiving }, .. } => archiving.push(archiver),
            Life::Running(_) => {
                let then = Then::Archive { archiving: vec![archiver] };
                self.stop(turn, session, Ended::Archived, then)?;
            }
        }

        Ok(())
    }

    /// Answers `asker` with `answer`: a caller with it, and an engine, when it refuses, with the refusal, declined.
    pub(super) fn reply<A: Kind + Encode<Actors> + Refuses>(
        &self,
        turn: &mut Turn<'_, Self>,
        asker: Asker<A>,
        answer: &A,
    ) -> Result<(), Fault> {
        match asker {
            Asker::Caller(held) => turn.answer(held, answer)?,
            Asker::Engine { engine, chat } => self.decline(turn, engine, chat, answer)?,
        }

        Ok(())
    }

    /// Takes `text` for `session`, as `by` said it: noted as said, and written to the session tagged with the engine it
    /// was typed at.
    pub(super) fn say(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        by: Speaker,
        text: String,
    ) -> Result<Said, Fault> {
        let written = said(by, &text);

        self.take_line(turn, session, written, Entry::Said { by, text })
    }

    /// Takes `line`, a command the owner ran in `!` mode at the trusted engine `engine`, and what it `printed`, for
    /// `session`: noted as run, and written to the session as a line of its own, which starts a turn, as Claude Code's
    /// own `!` does (ADR-0057: The console is the chat).
    pub(super) fn ran(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        engine: u32,
        line: String,
        printed: String,
    ) -> Result<Said, Fault> {
        let written = ran(engine, &line, &printed);

        self.take_line(turn, session, written, Entry::Ran { engine, line, printed })
    }

    /// Takes the line `entry` notes for `session`, `written` as the session reads it: noted, and written to the
    /// session's stdin, kept until its container is attached, or resuming it, as the session stands.
    fn take_line(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        written: String,
        entry: Entry,
    ) -> Result<Said, Fault> {
        let Some(held) = self.held.get(&session) else {
            return Ok(Said::Refused(Refusal::NoSuchSession));
        };
        let taking = match &held.life {
            Life::Archived => Taking::Refused(Refusal::Archived),
            Life::Stopping { then: Then::Archive { .. }, .. } => Taking::Refused(Refusal::Busy),
            Life::Stopping { then: Then::Rest { .. }, .. } | Life::Starting { .. } => Taking::Kept,
            Life::Running(_) => Taking::Typed,
            Life::Hibernating { .. } => match self.may_start() {
                Ok(()) => Taking::Woken,
                Err(why) => Taking::Refused(why),
            },
        };
        if let Taking::Refused(why) = &taking {
            return Ok(Said::Refused(why.clone()));
        }
        let number = self.note(turn, session, entry);
        match taking {
            Taking::Kept => {
                if let Some(Life::Stopping { then: Then::Rest { lines, .. }, .. } | Life::Starting { lines, .. }) =
                    self.held.get_mut(&session).map(|held| &mut held.life)
                {
                    lines.push(written);
                }
            }
            Taking::Typed => {
                if let Some(Life::Running(running)) = self.held.get_mut(&session).map(|held| &mut held.life) {
                    running.activity.cancel(turn);
                    running.activity = Activity::Working;
                }
                self.type_line(turn, session, &written)?;
            }
            Taking::Woken => self.restart(session, vec![written]),
            Taking::Refused(_) => {}
        }

        Ok(Said::Taken { number })
    }
}

/// What is written to a session of `text`, said by `by`: tagged `[engine]` with the engine it was typed at (ADR-0057).
/// One of Claude Code's own commands, `/compact` and the like, is written alone and untagged, as Claude Code reads a
/// command only so.
fn said(by: Speaker, text: &str) -> String {
    match by {
        Speaker::Engine { engine } if !is_command(text) => format!("[{engine}] {text}"),
        Speaker::Engine { .. } | Speaker::Caller | Speaker::Hub => String::from(text),
    }
}

/// What is written to a session of `line`, a command run in `!` mode at the engine `engine`: the command tagged
/// `[engine]` and marked `!`, then what it `printed` on the lines after it.
fn ran(engine: u32, line: &str, printed: &str) -> String {
    let mut written = format!("[{engine}] !{line}");
    let printed = printed.trim_end();
    if !printed.is_empty() {
        let _ = write!(written, "\n{printed}");
    }

    written
}

/// What becomes of a line said to a session.
enum Taking {
    /// It is refused.
    Refused(Refusal),
    /// It waits for the session's container to be attached.
    Kept,
    /// It is written to the session's stdin.
    Typed,
    /// It wakes the session, which hibernates.
    Woken,
}

impl Sessions {
    /// Whether one more session may wake: fewer than the most are awake.
    fn may_start(&self) -> Result<(), Refusal> {
        let running: Vec<u32> =
            self.held.iter().filter(|(_, held)| held.counts()).map(|(&session, _)| session).collect();
        let most = self.settings.most_running;
        let is_full = running.len() >= usize::try_from(most).unwrap_or(usize::MAX);
        if is_full {
            return Err(Refusal::Full { most, running });
        }

        Ok(())
    }
}
