//! The chat of trusted engines (ADR-0057): what an engine's owner does, forwarded by the hub's `Engine` for that
//! engine with its id, and what every trusted engine is shown: every session as a tab, the events of each tab it has
//! open, the backlog first and then each as it comes, and what waits for the owner's answer.
//!
//! ```text
//! Up::Say ──> Engine(37) ──> Chatted { engine: 37, chat } ──> Sessions: the line, tagged, into the transcript and stdin
//! Sessions: any change ──> ShowTabs / ShowEvents / ShowAsk ──> each joined Engine ──> Down::Sessions / Show / Ask
//! ```
//!
//! Only an `Engine` whose engine the hub trusts joins, and the capability takes a frame from an engine only from the
//! actor that joined as it, so no engine speaks as another. An actor joins as itself, the sender of its [`Joined`],
//! as one engine, and never under a number another actor holds, so no engine is shown as another either. What each
//! engine is shown is worked out as each turn flushes, so nothing that changes the list or a transcript goes unshown.

#[cfg(test)]
mod tests;

use super::asked::Waiting;
use super::asks::{Archived, Opened, Said, State};
use super::transcript::{Entry, Numbered, Speaker};
use super::{Acts, Does, Sessions};
use aethereal_actor::{Ended, Fault, Handles, Held, Level, ProtocolRef, Turn};
use aethereal_data::{ActorId, Bound, Plain, write_json_string};
use core::fmt::Write as _;
use std::collections::BTreeMap;

/// The most events one [`ShowEvents`] carries: a backlog goes as several.
const EVENTS_PER_SHOW: usize = 32;

/// What the owner said no to, as Claude Code reads it.
const DENIED: &str = "The owner did not allow it.";

/// What a trusted engine's `Engine` is told to show it, each written down the link as a frame of its own: tells
/// alone, so the kinds themselves, which the sender of a [`Joined`] is cast to.
pub type Showing = (ShowTabs, ShowEvents, ShowAsk);

/// Every session, as the tabs a trusted engine lists: sent whole on every change, and as the engine joins.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.show_tabs")]
pub struct ShowTabs {
    pub tabs: Vec<Tab>,
}

/// One session as a tab: its number, its name, where it is, and how many of its asks wait for the owner.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.tab")]
pub struct Tab {
    pub session: u32,
    pub name: String,
    pub state: State,
    pub asking: u32,
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

/// What a trusted engine's owner did, as its link's frame says it (ADR-0057).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.chat")]
pub enum Chat {
    /// A line typed without `/`.
    Say { session: u32, text: String },
    /// A `/` command the owner ran at the engine, with `session`'s tab open, and what it printed.
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

/// What the capability keeps of the chat: each trusted engine joined, by its id, the tabs last shown, and each ask that
/// waits, by its number.
#[derive(Bound)]
pub(super) struct Chats {
    engines: BTreeMap<u32, Watcher>,
    shown: Vec<Tab>,
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
}

impl Handles<Joined> for Sessions {
    type Sends = Acts;

    /// A trusted engine linked: its sender is watched, and shown every session and each ask that waits at once. A
    /// sender that takes nothing an engine is shown, one that names an engine another actor joined as, and one joined
    /// as another engine already are each refused, and nothing joined changes.
    fn handle(&mut self, joined: Joined, turn: &mut Turn<'_, Self, Acts>) -> Result<(), Fault> {
        let Joined { engine } = joined;
        let sender = turn.sender().id();
        let Some(shown) = turn.cast::<Showing>(turn.sender()) else {
            turn.log(
                Level::Warn,
                format_args!("{sender:?}, which takes nothing an engine is shown, joined as {engine}"),
            );
            return Ok(());
        };
        match self.chats.joining(engine, sender) {
            Joining::New => turn.watch(shown),
            Joining::Again => {}
            Joining::Taken { by } => {
                turn.log(Level::Warn, format_args!("{sender:?} joined as engine {engine}, which {by:?} joined as"));
                return Ok(());
            }
            Joining::Elsewhere { engine: joined } => {
                turn.log(Level::Warn, format_args!("{sender:?} joined as engine {engine}, already engine {joined}"));
                return Ok(());
            }
        }
        turn.to(shown).send(&ShowTabs { tabs: self.tabs() })?;
        for (&ask, question) in &self.chats.questions {
            turn.to(shown).send(&ShowAsk { ask, session: question.session, what: question.what.clone() })?;
        }
        self.chats.engines.insert(engine, Watcher { shown, tabs: BTreeMap::new() });

        Ok(())
    }
}

impl Handles<Ended> for Sessions {
    type Sends = ();

    /// An engine that joined has ended: its tabs end with it. A turn running carries on, and an ask waits on.
    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let gone = ended.actor().id();
        self.chats.engines.retain(|_, watcher| watcher.shown.id() != gone);

        Ok(())
    }
}

impl Handles<Chatted> for Sessions {
    type Sends = Acts;

    /// What a trusted engine's owner did: taken only from the actor that joined as that engine, and done once the
    /// capability has found its sessions.
    fn handle(&mut self, chatted: Chatted, turn: &mut Turn<'_, Self, Acts>) -> Result<(), Fault> {
        let Chatted { engine, chat } = chatted;
        let sender = turn.sender().id();
        let joined = self.chats.engines.get(&engine).map(|watcher| watcher.shown.id());
        if joined != Some(sender) {
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
    /// Does what the owner at `engine` did.
    pub(super) fn chat<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>, engine: u32, chat: Chat) -> Result<(), Fault> {
        let by = Speaker::Engine { engine };
        match chat {
            Chat::Say { session, text } => {
                let said = self.say(turn, session, by, text)?;
                if let Said::Refused(why) = said {
                    turn.log(Level::Warn, format_args!("engine {engine}'s line to session {session}: {why:?}"));
                }
            }
            Chat::Ran { session, line, printed } => {
                if self.held.contains_key(&session) {
                    self.note(session, Entry::Ran { engine, line, printed });
                }
            }
            Chat::Answer { ask, allow } => self.answer(turn, engine, ask, allow)?,
            Chat::Interrupt { session } => {
                let interrupted = self.interrupt(turn, session, by)?;
                turn.log(Level::Info, format_args!("engine {engine} interrupted session {session}: {interrupted:?}"));
            }
            Chat::Open { session, after } => {
                let is_held = self.held.contains_key(&session);
                let watcher = self.chats.engines.get_mut(&engine);
                if let (true, Some(watcher)) = (is_held, watcher) {
                    watcher.tabs.insert(session, after);
                }
            }
            Chat::Close { session } => {
                if let Some(watcher) = self.chats.engines.get_mut(&engine) {
                    watcher.tabs.remove(&session);
                }
            }
            Chat::New { name } => self.open(turn, Vec::<Held<Opened>>::new(), name)?,
            Chat::Archive { session } => self.archive(turn, Vec::<Held<Archived>>::new(), session)?,
        }

        Ok(())
    }

    /// Every session, as its tab.
    fn tabs(&self) -> Vec<Tab> {
        let questions = &self.chats.questions;
        self.held
            .iter()
            .map(|(&session, held)| {
                let waiting = questions.values().filter(|question| question.session == session).count();
                let asking = u32::try_from(waiting).unwrap_or(u32::MAX);
                Tab { session, name: held.name.clone(), state: held.state(), asking }
            })
            .collect()
    }

    /// Shows each joined engine what changed: every tab, when any changed, and every event after the last shown of
    /// each tab it has open, a few at a time.
    pub(super) fn show<S: Does>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        let tabs = self.tabs();
        if tabs != self.chats.shown {
            for watcher in self.chats.engines.values() {
                turn.to(watcher.shown).send(&ShowTabs { tabs: tabs.clone() })?;
            }
            self.chats.shown = tabs;
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
    pub(super) fn asked<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
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
    fn answer<S: Does>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        engine: u32,
        ask: u64,
        allow: bool,
    ) -> Result<(), Fault> {
        let Some(question) = self.chats.questions.remove(&ask) else {
            turn.log(Level::Info, format_args!("engine {engine} answered ask {ask}, which waits no more"));
            return Ok(());
        };
        let Question { session, request, input, .. } = question;
        self.note(session, Entry::Answered { ask, allow, by: Speaker::Engine { engine } });

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
