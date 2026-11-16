//! What the capability is asked (ADR-0057): a session opened, a line said, a transcript read, the sessions listed, a
//! turn interrupted, and a session archived.

use super::asks::{
    Archive, Archived, Interrupt, Interrupted, ListSessions, Listed, NewSession, Opened, ReadTranscript, Refusal, Said,
    Say, Transcribed,
};
use super::saves::Indexed;
use super::session::{Activity, Life, Session, Then};
use super::transcript::{Ended, Entry, Speaker};
use super::{Does, Phase, Sessions};
use aethereal_actor::{Fault, Handles, Held, ToSender, Turn};
use aethereal_data::Bound;

/// The most entries a read answers when it states no most of its own, and the most it may state.
const MOST_ENTRIES: u32 = 200;

/// An ask the capability holds until it has found its sessions' containers, and answers once it has.
#[derive(Bound)]
pub(super) enum Waiting {
    New { held: Held<Opened>, name: String },
    Say { held: Held<Said>, session: u32, text: String },
    Read { held: Held<Transcribed>, read: ReadTranscript },
    Interrupt { held: Held<Interrupted>, session: u32 },
    Archive { held: Held<Archived>, session: u32 },
}

impl Handles<NewSession> for Sessions {
    type Sends = acts!(ToSender<Opened>);

    /// A new session, numbered after every other, its first container started: answered once it runs.
    fn handle(&mut self, new: NewSession, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        self.take(turn, Waiting::New { held, name: new.name })
    }
}

impl Handles<Say> for Sessions {
    type Sends = acts!(ToSender<Said>);

    /// A line for a session: written to it in order, a turn running or not; a stopped session is resumed first.
    fn handle(&mut self, say: Say, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        self.take(turn, Waiting::Say { held, session: say.session, text: say.text })
    }
}

impl Handles<ReadTranscript> for Sessions {
    type Sends = acts!(ToSender<Transcribed>);

    fn handle(&mut self, read: ReadTranscript, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        self.take(turn, Waiting::Read { held, read })
    }
}

impl Handles<ListSessions> for Sessions {
    type Sends = acts!(ToSender<Listed>);

    fn handle(&mut self, ListSessions: ListSessions, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sessions = self.held.iter().map(|(&session, held)| held.listed(session)).collect();
        let listed = Listed { sessions, most_running: self.settings.most_running, strays: self.strays.clone() };
        turn.reply(&listed)?;

        Ok(())
    }
}

impl Handles<Interrupt> for Sessions {
    type Sends = acts!(ToSender<Interrupted>);

    /// The session's running turn interrupted, through the control request Claude Code reads on its stdin.
    fn handle(&mut self, interrupt: Interrupt, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        self.take(turn, Waiting::Interrupt { held, session: interrupt.session })
    }
}

impl Handles<Archive> for Sessions {
    type Sends = acts!(ToSender<Archived>);

    /// The session archived: its container stopped and removed, its transcript and home kept. Answered once no
    /// container of it is left.
    fn handle(&mut self, archive: Archive, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        self.take(turn, Waiting::Archive { held, session: archive.session })
    }
}

impl Sessions {
    /// Answers `waiting`, or holds it until the capability has found its sessions' containers.
    pub(super) fn take<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, waiting: Waiting) -> Result<(), Fault> {
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
            Waiting::New { held, name } => self.open(turn, held, name)?,
            Waiting::Say { held, session, text } => {
                let said = self.say(turn, session, text)?;
                turn.answer(held, &said)?;
            }
            Waiting::Read { held, read } => turn.answer(held, &self.read(&read))?,
            Waiting::Interrupt { held, session } => {
                let interrupted = self.interrupt(turn, session)?;
                turn.answer(held, &interrupted)?;
            }
            Waiting::Archive { held, session } => self.archive(turn, held, session)?,
        }

        self.flush(turn)
    }

    /// Opens a session named `name`, numbered after every other, its first container started: `held` is answered
    /// once it runs.
    fn open<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, held: Held<Opened>, name: String) -> Result<(), Fault> {
        if let Err(why) = self.may_start() {
            turn.answer(held, &Opened::Refused(why))?;
            return Ok(());
        }
        let session = self.held.keys().next_back().map_or(1, |last| last + 1);
        self.held.insert(session, Session::new(name, held));
        self.indexed = Indexed::Changed;

        self.start(turn, session, 1)
    }

    /// The entries `read` asks for.
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

    /// Interrupts `session`'s running turn.
    fn interrupt<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, session: u32) -> Result<Interrupted, Fault> {
        let Some(held) = self.held.get(&session) else {
            return Ok(Interrupted::Refused(Refusal::NoSuchSession));
        };
        let is_running = matches!(held.life, Life::Running(_));
        if !is_running {
            return Ok(Interrupted::Refused(Refusal::NotRunning));
        }
        let number = self.note(session, Entry::Interrupted { by: Speaker::Caller });
        self.type_interrupt(turn, session, number)?;

        Ok(Interrupted::Sent { number })
    }

    /// Archives `session`, answering `archiving` once no container of it is left.
    fn archive<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        archiving: Held<Archived>,
        session: u32,
    ) -> Result<(), Fault> {
        let Some(held) = self.held.get_mut(&session) else {
            turn.answer(archiving, &Archived::Refused(Refusal::NoSuchSession))?;
            return Ok(());
        };
        match &mut held.life {
            Life::Archived => turn.answer(archiving, &Archived::Done)?,
            Life::Stopped => {
                held.life = Life::Archived;
                self.dirty(session);
                turn.answer(archiving, &Archived::Done)?;
            }
            Life::Starting { .. } | Life::Stopping { then: Then::Rest { .. }, .. } => {
                turn.answer(archiving, &Archived::Refused(Refusal::Busy))?;
            }
            Life::Stopping { then: Then::Archive { archiving: waiting }, .. } => waiting.push(archiving),
            Life::Running(_) => {
                self.stop(turn, session, Ended::Archived, Then::Archive { archiving: vec![archiving] })?;
            }
        }

        Ok(())
    }

    /// Takes `text` for `session`, as its caller said it.
    fn say<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, session: u32, text: String) -> Result<Said, Fault> {
        let Some(held) = self.held.get(&session) else {
            return Ok(Said::Refused(Refusal::NoSuchSession));
        };
        let taking = match &held.life {
            Life::Archived => Taking::Refused(Refusal::Archived),
            Life::Stopping { then: Then::Archive { .. }, .. } => Taking::Refused(Refusal::Busy),
            Life::Stopping { then: Then::Rest { .. }, .. } | Life::Starting { .. } => Taking::Kept,
            Life::Running(_) => Taking::Typed,
            Life::Stopped => match self.may_start() {
                Ok(()) => Taking::Resumed,
                Err(why) => Taking::Refused(why),
            },
        };
        if let Taking::Refused(why) = &taking {
            return Ok(Said::Refused(why.clone()));
        }
        let number = self.note(session, Entry::Said { by: Speaker::Caller, text: text.clone() });
        match taking {
            Taking::Kept => {
                if let Some(Life::Stopping { then: Then::Rest { lines }, .. } | Life::Starting { lines, .. }) =
                    self.held.get_mut(&session).map(|held| &mut held.life)
                {
                    lines.push(text);
                }
            }
            Taking::Typed => {
                if let Some(Life::Running(running)) = self.held.get_mut(&session).map(|held| &mut held.life) {
                    running.activity.cancel(turn);
                    running.activity = Activity::Working;
                }
                self.type_line(turn, session, &text)?;
            }
            Taking::Resumed => self.restart(turn, session, vec![text])?,
            Taking::Refused(_) => {}
        }

        Ok(Said::Taken { number })
    }
}

/// What becomes of a line said to a session.
enum Taking {
    /// It is refused.
    Refused(Refusal),
    /// It waits for the session's container to be attached.
    Kept,
    /// It is written to the session's stdin.
    Typed,
    /// It resumes the session, which is stopped.
    Resumed,
}

impl Sessions {
    /// Whether one more session may start: the capability is ready, and fewer than the most run.
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
