//! The chat of trusted engines (ADR-0057): what an engine's owner does, forwarded by the hub's `Engine` for that
//! engine with its id, and what every trusted engine is shown: every session as a row of the list, the events of each
//! tab it has open, the backlog first and then each as it comes, what waits for the owner's answer, and each of its
//! own chat frames that could not be done.
//!
//! ```text
//! Up::Say ──> Engine(37) ──> Chatted { engine: 37, chat } ──> Sessions: the line, tagged, into the transcript and stdin
//! Up::Ran ──> likewise: the command and what it printed, tagged and marked `!`, as a line of its own
//! Sessions: any change ──> ShowSessions / ShowEvents / ShowAsk ──> each joined Engine ──> Down::Sessions / Show / Ask
//! a frame refused ──> ShowDeclined, with the refusal the tools would answer ──> its Engine alone ──> Down::Declined
//! ```
//!
//! Only an `Engine` whose engine the hub trusts joins, and the capability takes a frame from an engine only from the
//! actor that joined as it, so no engine speaks as another. An actor joins as itself, the sender of its [`Joined`],
//! as one engine, and never under a number another actor holds, so no engine is shown as another either. What each
//! engine is shown is worked out as each turn flushes, so nothing that changes the list or a transcript goes unshown.

#[cfg(test)]
mod tests;

use super::Sessions;
use super::asked::{Asker, Waiting};
use super::asks::{Refusal, Refuses};
use super::row::SessionRow;
use super::session::Session;
use super::transcript::{Entry, Numbered, Speaker};
use aethereal_actor::{Ended, Fault, Level, ProtocolRef, Speaks, Turn};
use aethereal_data::{ActorId, Bound, Plain, write_json_string};
use core::fmt::Write as _;
use std::collections::BTreeMap;

/// The most events one [`ShowEvents`] carries: a backlog goes as several.
const EVENTS_PER_SHOW: usize = 32;

/// What the owner said no to, as Claude Code reads it.
const DENIED: &str = "The owner did not allow it.";

/// What a trusted engine's `Engine` is told to show it, each written down the link as a frame of its own: tells
/// alone, so the kinds themselves, which the sender of a [`Joined`] is cast to.
pub type Showing = (ShowSessions, ShowEvents, ShowAsk, ShowDeclined);

/// Every session, as the list a trusted engine is shown: sent whole on every change, and as the engine joins. Each
/// row's time is on the clock of whoever holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.show_sessions")]
pub struct ShowSessions {
    pub list: Vec<SessionRow>,
}

/// Events of `session`'s transcript, in order, for an engine with its tab open: its backlog first, then each as it
/// comes.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.show_events")]
pub struct ShowEvents {
    pub session: u32,
    pub events: Vec<Numbered>,
}

/// Something in `session` waits for the owner's answer, numbered `ask`, and says `what`.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.show_ask")]
pub struct ShowAsk {
    pub ask: u64,
    pub session: u32,
    pub what: String,
}

/// A chat frame of the engine shown it that the hub could not do, and why: the refusal the hub's tools would answer the
/// same ask. Shown only to the engine that sent it; a frame that is done is answered by nothing but the list.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.show_declined")]
pub struct ShowDeclined {
    pub chat: Chat,
    pub why: Refusal,
}

/// What a trusted engine's owner did, as its link's frame says it (ADR-0057).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.chat")]
pub enum Chat {
    /// A line typed in chat mode, a `/` line among them.
    Say { session: u32, text: String },
    /// A command the owner ran in `!` mode at the engine, with `session`'s tab open, and what it printed: a line to the
    /// session, as `Say` is.
    Ran { session: u32, line: String, printed: String },
    /// An answer to the ask numbered `ask`.
    Answer { ask: u64, allow: bool },
    /// Stop the session's turn.
    Interrupt { session: u32 },
    /// A tab opened, to be shown every event after `after`.
    Open { session: u32, after: u64 },
    /// A tab closed.
    Close { session: u32 },
    /// A session made.
    New { name: String },
    /// A session ended.
    Archive { session: u32 },
}

/// Told by the `Engine` of `engine`, a trusted engine: what its owner did.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.chatted")]
pub struct Chatted {
    pub engine: u32,
    pub chat: Chat,
}

/// Told by the `Engine` of `engine` as it links trusted: its sender is where what the engine is shown goes, so an
/// actor joins only as itself. The capability watches it, and lets the engine go as it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.joined")]
pub struct Joined {
    pub engine: u32,
}

/// Where an actor that joins as an engine stands among those joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Joining {
    /// Neither the engine nor the actor has joined: it joins.
    New,
    /// The actor joined as the engine already: it is shown everything again.
    Again,
    /// Another actor, `by`, joined as the engine: it is refused.
    Taken { by: ActorId },
    /// The actor joined as another engine, `engine`: it is refused.
    Elsewhere { engine: u32 },
}

/// A trusted engine joined: where it is shown, and the number of the last event shown of each tab it has open.
#[derive(Bound)]
pub(super) struct Watcher {
    shown: ProtocolRef<Showing>,
    tabs: BTreeMap<u32, u64>,
}

/// An ask of Claude Code's that waits for the owner: whether it may use a tool (`can_use_tool`), in `session`, by the
/// id of its control request, with the tool's input as JSON, and what the owner is shown.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) struct Question {
    session: u32,
    request: String,
    input: String,
    what: String,
}

/// What the capability keeps of the chat: each trusted engine joined, by its id, the list last shown, and each ask
/// that waits, by its number.
#[derive(Bound)]
pub(super) struct Chats {
    engines: BTreeMap<u32, Watcher>,
    shown: Vec<SessionRow>,
    questions: BTreeMap<u64, Question>,
    next_ask: u64,
}

impl Chats {
    /// No engine joined, and nothing asked.
    pub(super) const fn new() -> Self {
        Self { engines: BTreeMap::new(), shown: Vec::new(), questions: BTreeMap::new(), next_ask: 0 }
    }

    /// Lets go of each ask of `session`'s, whose container is gone, and its process with what it asked.
    pub(super) fn forget(&mut self, session: u32) {
        self.questions.retain(|_, question| question.session != session);
    }

    /// How many asks of `session`'s wait for the owner.
    fn asking(&self, session: u32) -> u32 {
        let waiting = self.questions.values().filter(|question| question.session == session).count();

        u32::try_from(waiting).unwrap_or(u32::MAX)
    }

    /// Where the actor `by` stands, joining as `engine`.
    fn joining(&self, engine: u32, by: ActorId) -> Joining {
        let holder = self.engines.get(&engine).map(|watcher| watcher.shown.id());
        let joined_as = self.engines.iter().find(|(_, watcher)| watcher.shown.id() == by).map(|(&number, _)| number);
        match (holder, joined_as) {
            (Some(holder), _) if holder == by => Joining::Again,
            (Some(holder), _) => Joining::Taken { by: holder },
            (None, Some(number)) => Joining::Elsewhere { engine: number },
            (None, None) => Joining::New,
        }
    }
}

impl Sessions {
    /// The ids of the engines that joined, in order: those the chat is shown to.
    #[must_use]
    pub fn joined(&self) -> Vec<u32> {
        self.chats.engines.keys().copied().collect()
    }

    /// A trusted engine linked: its sender is watched, and shown every session and each ask that waits at once. A
    /// sender that takes nothing an engine is shown is refused by its guard; one that names an engine another actor
    /// joined as, and one joined as another engine already, are each refused here, and nothing joined changes.
    pub(super) fn handle_joined(
        &mut self,
        joined: Joined,
        turn: &mut Turn<'_, Self, Speaks<Showing>>,
    ) -> Result<(), Fault> {
        let Joined { engine } = joined;
        let shown = *turn.from();
        match self.chats.joining(engine, shown.id()) {
            Joining::New => turn.watch(shown),
            Joining::Again => {}
            Joining::Taken { by } => {
                turn.log(Level::Warn, format_args!("{shown:?} joined as engine {engine}, which {by:?} joined as"));
                return Ok(());
            }
            Joining::Elsewhere { engine: joined } => {
                turn.log(Level::Warn, format_args!("{shown:?} joined as engine {engine}, already engine {joined}"));
                return Ok(());
            }
        }
        turn.to(shown).send(&ShowSessions { list: self.rows() })?;
        for (&ask, question) in &self.chats.questions {
            turn.to(shown).send(&ShowAsk { ask, session: question.session, what: question.what.clone() })?;
        }
        self.chats.engines.insert(engine, Watcher { shown, tabs: BTreeMap::new() });

        Ok(())
    }

    /// An engine that joined has ended: its tabs end with it. A turn running carries on, and an ask waits on.
    #[allow(
        clippy::needless_pass_by_value,
        clippy::unnecessary_wraps,
        reason = "a handler's body, it takes and answers what its handler does"
    )]
    pub(super) fn handle_ended(&mut self, ended: Ended) -> Result<(), Fault> {
        let gone = ended.actor().id();
        self.chats.engines.retain(|_, watcher| watcher.shown.id() != gone);

        Ok(())
    }

    /// What a trusted engine's owner did: taken only from the actor that joined as that engine, and done once the
    /// capability has found its sessions.
    pub(super) fn handle_chatted(&mut self, chatted: Chatted, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Chatted { engine, chat } = chatted;
        let sender = turn.sender();
        let joined = self.chats.engines.get(&engine).map(|watcher| watcher.shown.id());
        if joined != Some(sender.id()) {
            turn.log(
                Level::Warn,
                format_args!("a chat frame from {sender:?} said to be engine {engine}'s was passed over"),
            );
            return Ok(());
        }

        self.take(turn, Waiting::Chat { engine, chat })
    }
}

impl Sessions {
    /// Does what the owner at `engine` did: a frame that cannot be done is declined to it, with the refusal the hub's
    /// tools would answer.
    pub(super) fn chat(&mut self, turn: &mut Turn<'_, Self>, engine: u32, chat: Chat) -> Result<(), Fault> {
        let by = Speaker::Engine { engine };
        let asked = chat.clone();
        match chat {
            Chat::Say { session, text } => {
                let said = self.say(turn, session, by, text)?;
                self.decline(turn, engine, asked, &said)?;
            }
            Chat::Ran { session, line, printed } => {
                let ran = self.ran(turn, session, engine, line, printed)?;
                self.decline(turn, engine, asked, &ran)?;
            }
            Chat::Answer { ask, allow } => self.answer(turn, engine, ask, allow)?,
            Chat::Interrupt { session } => {
                let interrupted = self.interrupt(turn, session, by)?;
                self.decline(turn, engine, asked, &interrupted)?;
            }
            Chat::Open { session, after } => {
                if !self.held.contains_key(&session) {
                    return self.declined(turn, engine, asked, Refusal::NoSuchSession);
                }
                if let Some(watcher) = self.chats.engines.get_mut(&engine) {
                    watcher.tabs.insert(session, after);
                }
            }
            Chat::Close { session } => {
                if let Some(watcher) = self.chats.engines.get_mut(&engine) {
                    watcher.tabs.remove(&session);
                }
            }
            Chat::New { name } => self.open(turn, Asker::Engine { engine, chat: asked }, name)?,
            Chat::Archive { session } => self.archive(turn, Asker::Engine { engine, chat: asked }, session)?,
        }

        Ok(())
    }

    /// Declines `chat`, a frame of `engine`'s, when `answer` refuses it: nothing when it does not.
    pub(super) fn decline<A: Refuses>(
        &self,
        turn: &mut Turn<'_, Self>,
        engine: u32,
        chat: Chat,
        answer: &A,
    ) -> Result<(), Fault> {
        let Some(why) = answer.refusal() else {
            return Ok(());
        };

        self.declined(turn, engine, chat, why.clone())
    }

    /// Shows `engine`, when it is still joined, that its frame `chat` could not be done, for `why`.
    fn declined(&self, turn: &mut Turn<'_, Self>, engine: u32, chat: Chat, why: Refusal) -> Result<(), Fault> {
        turn.log(Level::Info, format_args!("engine {engine}'s {chat:?} was declined: {why:?}"));
        let Some(watcher) = self.chats.engines.get(&engine) else {
            return Ok(());
        };
        turn.to(watcher.shown).send(&ShowDeclined { chat, why })?;

        Ok(())
    }

    /// Every session, as its row.
    pub(super) fn rows(&self) -> Vec<SessionRow> {
        self.held.iter().map(|(&session, held)| self.row_of(session, held)).collect()
    }

    /// The row of the session numbered `session`, when it is held.
    pub(super) fn row(&self, session: u32) -> Option<SessionRow> {
        self.held.get(&session).map(|held| self.row_of(session, held))
    }

    /// The row of `held`, numbered `session`, with its asks that wait.
    fn row_of(&self, session: u32, held: &Session) -> SessionRow {
        held.row(session, self.chats.asking(session))
    }

    /// Shows each joined engine what changed: the list, when any row changed, and every event after the last shown of
    /// each tab it has open, a few at a time.
    pub(super) fn show(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let rows = self.rows();
        if rows != self.chats.shown {
            for watcher in self.chats.engines.values() {
                turn.to(watcher.shown).send(&ShowSessions { list: rows.clone() })?;
            }
            self.chats.shown = rows;
        }
        for watcher in self.chats.engines.values_mut() {
            for (session, shown) in &mut watcher.tabs {
                let Some(held) = self.held.get(session) else {
                    continue;
                };
                while *shown < held.transcript.last() {
                    let events = held.transcript.after(*shown, EVENTS_PER_SHOW);
                    let Some(last) = events.last().map(|numbered| numbered.number) else {
                        break;
                    };
                    turn.to(watcher.shown).send(&ShowEvents { session: *session, events })?;
                    *shown = last;
                }
            }
        }

        Ok(())
    }

    /// Claude Code in `session` asks whether it may use `tool` with `input`, by its control request `request`: the
    /// ask is numbered, kept until the owner answers it, and shown to every joined engine.
    pub(super) fn asked(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        request: String,
        tool: &str,
        input: String,
    ) -> Result<(), Fault> {
        let ask = self.chats.next_ask;
        self.chats.next_ask += 1;
        let what = format!("{tool} {input}");
        for watcher in self.chats.engines.values() {
            turn.to(watcher.shown).send(&ShowAsk { ask, session, what: what.clone() })?;
        }
        self.chats.questions.insert(ask, Question { session, request, input, what });

        Ok(())
    }

    /// The owner at `engine` answered the ask numbered `ask`: Claude Code is told, through the control response it
    /// reads on its stdin, and the transcript notes it. An ask answered already, or of a container gone, is passed
    /// over.
    fn answer(&mut self, turn: &mut Turn<'_, Self>, engine: u32, ask: u64, allow: bool) -> Result<(), Fault> {
        let Some(question) = self.chats.questions.remove(&ask) else {
            turn.log(Level::Info, format_args!("engine {engine} answered ask {ask}, which waits no more"));
            return Ok(());
        };
        let Question { session, request, input, .. } = question;
        self.note(turn, session, Entry::Answered { ask, allow, by: Speaker::Engine { engine } });

        self.type_raw(turn, session, control_response(&request, allow, &input))
    }
}

/// The control response Claude Code reads on its stdin for its control request `request`, letting it use its tool
/// with `input`, as it asked, or not.
fn control_response(request: &str, allow: bool, input: &str) -> String {
    let mut line = String::from(r#"{"type":"control_response","response":{"subtype":"success","request_id":"#);
    write_json_string(request, &mut line);
    line.push_str(r#","response":"#);
    if allow {
        let _ = write!(line, r#"{{"behavior":"allow","updatedInput":{input}}}"#);
    } else {
        line.push_str(r#"{"behavior":"deny","message":"#);
        write_json_string(DENIED, &mut line);
        line.push('}');
    }
    line.push_str("}}\n");

    line
}
