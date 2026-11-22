//! The sessions' tools (ADR-0057): the hub's caller opens a session, says a line to it, reads its transcript, lists
//! the sessions, interrupts a turn, hibernates or wakes a session, and archives one, each the capability's own ask.
//! Each is named as a verb and its noun, never a bare common word.

#[cfg(test)]
mod tests;

use crate::{
    Archive, Archived, Hibernate, Hibernated, Interrupt, Interrupted, ListSessions, NewSession, Numbered, Opened,
    ReadTranscript, Said, Say, SessionRow, Sessions, SessionsListed as Listed, Transcribed, Wake,
};
use aethereal_actor::{ActorRef, Answered, Anywhere, Fault, Held, ProtocolRef, Reply, Turn, actor, path};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Caller, Done, Output, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The tool that opens a session.
const NEW_SESSION: &str = "new_session";
/// The tool that says a line to a session.
const MESSAGE_SESSION: &str = "message_session";
/// The tool that reads a session's transcript.
const READ_SESSION: &str = "read_session";
/// The tool that lists the sessions.
const LIST_SESSIONS: &str = "list_sessions";
/// The tool that interrupts a session's turn.
const INTERRUPT_SESSION: &str = "interrupt_session";
/// The tool that hibernates a session.
const HIBERNATE_SESSION: &str = "hibernate_session";
/// The tool that wakes a session.
const WAKE_SESSION: &str = "wake_session";
/// The tool that archives a session.
const ARCHIVE_SESSION: &str = "archive_session";

/// The most bytes reading an input may take: a line said may be long.
const INPUT_BYTES: usize = 1 << 20;

/// What the tools keep for an ask they made: the call it was made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.tools_asking")]
pub struct Asking {
    number: u64,
}

/// What the tools are made from: nothing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.sessions_tools_config")]
pub struct SessionsToolsConfig;

/// What `message_session` and `interrupt_session` answer: the number of the transcript's entry they made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.entered")]
pub struct Entered {
    pub number: u64,
}

/// What `read_session` answers: the entries, in order, and the number of the transcript's last.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.read")]
pub struct Read {
    pub entries: Vec<Numbered>,
    pub last: u64,
}

/// What `hibernate_session` answers: where the session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.hibernation")]
pub enum Hibernation {
    /// It is being wound down: told, compacted, and its container stopped. `list_sessions` lists it hibernating once it
    /// is.
    WindingDown,
    /// It hibernates already.
    Hibernating,
}

/// What a call asks of the capability.
enum Order {
    New(NewSession),
    Say(Say),
    Read(ReadTranscript),
    List,
    Interrupt(Interrupt),
    Hibernate(Hibernate),
    Wake(Wake),
    Archive(Archive),
}

/// The sessions' eight tools for the hub's caller (ADR-0057), served by one worker: each takes the capability's own ask
/// as its input, and answers what it answers, a refusal as text the caller reads.
#[derive(Bound)]
#[kind(name = "aether.sessions.sessions_tools")]
pub struct SessionsTools {
    /// Each call held, by the number of the ask made for it.
    calls: BTreeMap<u64, Held<Called>>,
    next: u64,
}

#[actor(parent = Anywhere, depends(Sessions))]
impl SessionsTools {
    fn begin(_: SessionsToolsConfig) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    /// Reads the call's input, or refuses it; holds the call, and asks the capability. A session's call is refused:
    /// the sessions are the owner's to open, speak to, and end, and a session that drove another would be a pipeline
    /// of sessions, which the hub is not built for (ADR-0057).
    #[request]
    fn call(&mut self, call: Call, turn: &mut Turn<'_, Self>) -> Result<Reply<Called>, Fault> {
        if let Caller::Session { .. } = call.caller {
            return Ok(Reply::Now(Called::Refused(String::from("the session tools are the owner's, not a session's"))));
        }
        let order = match order(&call.tool, turn.read(&call.input)) {
            Ok(order) => order,
            Err(refusal) => return Ok(Reply::Now(refusal)),
        };
        let (held, later) = turn.hold();
        let asking = Asking { number: self.next };
        self.next += 1;
        let sessions = path::<Sessions>(());
        match order {
            Order::New(new) => turn.to(sessions).ask(&new, &asking)?,
            Order::Say(say) => turn.to(sessions).ask(&say, &asking)?,
            Order::Read(read) => turn.to(sessions).ask(&read, &asking)?,
            Order::List => turn.to(sessions).ask(&ListSessions, &asking)?,
            Order::Interrupt(interrupt) => turn.to(sessions).ask(&interrupt, &asking)?,
            Order::Hibernate(hibernate) => turn.to(sessions).ask(&hibernate, &asking)?,
            Order::Wake(wake) => turn.to(sessions).ask(&wake, &asking)?,
            Order::Archive(archive) => turn.to(sessions).ask(&archive, &asking)?,
        };
        self.calls.insert(asking.number, held);

        Ok(Reply::Later(later))
    }

    #[answer]
    fn opened(
        &mut self,
        (answered, asking): (Answered<Opened>, Asking),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answered(answered, asking, turn)
    }

    #[answer]
    fn said(&mut self, (answered, asking): (Answered<Said>, Asking), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.answered(answered, asking, turn)
    }

    #[answer]
    fn transcribed(
        &mut self,
        (answered, asking): (Answered<Transcribed>, Asking),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answered(answered, asking, turn)
    }

    #[answer]
    fn listed(
        &mut self,
        (answered, asking): (Answered<Listed>, Asking),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answered(answered, asking, turn)
    }

    #[answer]
    fn interrupted(
        &mut self,
        (answered, asking): (Answered<Interrupted>, Asking),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answered(answered, asking, turn)
    }

    #[answer]
    fn hibernated(
        &mut self,
        (answered, asking): (Answered<Hibernated>, Asking),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answered(answered, asking, turn)
    }

    #[answer]
    fn archived(
        &mut self,
        (answered, asking): (Answered<Archived>, Asking),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.answered(answered, asking, turn)
    }

    /// The sessions' answer to a call, answered to whoever called, as its tool's output or why it was refused.
    fn answered<A: Answers>(
        &mut self,
        answered: Answered<A>,
        Asking { number }: Asking,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&number) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(answer) => match answer.output() {
                Ok(bytes) => Called::output(turn, bytes),
                Err(why) => Called::Refused(why),
            },
            Answered::No(why) => Called::Refused(format!("the sessions did not answer: {why:?}")),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

impl Tool for SessionsTools {
    fn sheets() -> Vec<ToolSheet> {
        vec![
            ToolSheet::of::<NewSession>(
                NEW_SESSION,
                "Opens a new Claude session in a container of its own on the hub's machine, and answers once Claude \
                 Code runs in it, its start script having readied its work tree: its number, which every other session \
                 tool names it by. The name is only shown. Refused when as many sessions are awake as may, naming the \
                 awake ones: hibernate one first; and when its start fails, saying why, the session then hibernating \
                 and listed so, holding any line said to it meanwhile, until a line or `wake_session` starts it again.",
                Output::Value(SchemaNodes::of(&SessionRow::SHAPE)),
            ),
            ToolSheet::of::<Say>(
                MESSAGE_SESSION,
                "Says a line to a session, as a user's message. A line said while a turn runs is taken after it, in \
                 order; a hibernating session is woken, unless as many are awake as may, which refuses it naming them. \
                 Answers the number of the transcript's entry the line is.",
                Output::Value(SchemaNodes::of(&Entered::SHAPE)),
            ),
            ToolSheet::of::<ReadTranscript>(
                READ_SESSION,
                "Reads a session's transcript after the entry numbered `after`, 0 for the start: every event Claude \
                 wrote, every line said to it and who said it, and what the hub did with its container. Any session \
                 reads, hibernating and archived ones too. Answers the entries and the number of the last; read on \
                 after the last entry answered.",
                Output::Value(SchemaNodes::of(&Read::SHAPE)),
            ),
            ToolSheet::json::<ListSessions>(
                LIST_SESSIONS,
                "Lists every session: its number, name, its conversation's state, its container's state, when its \
                 transcript last grew on the hub's clock, and how many of its asks wait; the hub's time as it listed \
                 them; how many may be awake at once; and any container carrying the hub's label that the hub has no \
                 record of.",
                Output::Value(SchemaNodes::of(&Listed::SHAPE)),
            ),
            ToolSheet::of::<Interrupt>(
                INTERRUPT_SESSION,
                "Interrupts the turn a session runs. The session carries on and takes the next line.",
                Output::Value(SchemaNodes::of(&Entered::SHAPE)),
            ),
            ToolSheet::of::<Hibernate>(
                HIBERNATE_SESSION,
                "Hibernates a session now, as one idle a while is: it is told it will be compacted and hibernated, \
                 so it writes down its loose threads, then compacted, and its container stopped and removed. Its \
                 conversation, transcript, and home are kept, and a line or `wake_session` wakes it. Refused while a \
                 turn or a background task runs in it, or a turn waits on the owner's answer to an ask: interrupt or \
                 answer it first; and while it is being compacted, by its owner or Claude Code: ask again once the \
                 compaction ends. Answers whether it is winding down or hibernates already.",
                Output::Value(SchemaNodes::of(&Hibernation::SHAPE)),
            ),
            ToolSheet::of::<Wake>(
                WAKE_SESSION,
                "Wakes a hibernating session in a new container on its home, which resumes its conversation, and \
                 answers once Claude Code runs in it, as `new_session` does. Refused when as many sessions are awake as \
                 may, naming the awake ones: hibernate one first; and when its start fails, saying why, the session \
                 then hibernating and listed so, holding the lines said to it, until it is started again.",
                Output::Value(SchemaNodes::of(&SessionRow::SHAPE)),
            ),
            ToolSheet::of::<Archive>(
                ARCHIVE_SESSION,
                "Archives a session: its container is stopped and removed, and its transcript and home are kept. An \
                 archived session never wakes again, and still reads.",
                Output::Value(SchemaNodes::of(&Done::SHAPE)),
            ),
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// An answer of the capability's, as a tool's output: a value, or a refusal.
trait Answers: Sized {
    /// Its output's bytes, or what the refusal it is says.
    fn output(self) -> Result<Vec<u8>, String>;
}

impl Answers for Opened {
    fn output(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Open(session) => bytes(&session),
            Self::Refused(why) => Err(why.to_string()),
        }
    }
}

impl Answers for Said {
    fn output(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Taken { number } => bytes(&Entered { number }),
            Self::Refused(why) => Err(why.to_string()),
        }
    }
}

impl Answers for Transcribed {
    fn output(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Entries { entries, last } => bytes(&Read { entries, last }),
            Self::Refused(why) => Err(why.to_string()),
        }
    }
}

impl Answers for Listed {
    fn output(self) -> Result<Vec<u8>, String> {
        bytes(&self)
    }
}

impl Answers for Interrupted {
    fn output(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Sent { number } => bytes(&Entered { number }),
            Self::Refused(why) => Err(why.to_string()),
        }
    }
}

impl Answers for Hibernated {
    fn output(self) -> Result<Vec<u8>, String> {
        match self {
            Self::WindingDown => bytes(&Hibernation::WindingDown),
            Self::Hibernating => bytes(&Hibernation::Hibernating),
            Self::Refused(why) => Err(why.to_string()),
        }
    }
}

impl Answers for Archived {
    fn output(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Done => bytes(&Done {}),
            Self::Refused(why) => Err(why.to_string()),
        }
    }
}

/// The bytes of `value`, an output, or what says it could not be written.
fn bytes<T: Plain>(value: &T) -> Result<Vec<u8>, String> {
    to_bytes(value).map_err(|why| format!("the answer could not be written: {why:?}"))
}

/// What a call of `tool` with `input`, the bytes of its input, asks of the capability, or its refusal.
fn order(tool: &str, input: &[u8]) -> Result<Order, Called> {
    let budget = &mut Budget::new(INPUT_BYTES);
    let read = match tool {
        NEW_SESSION => from_bytes(input, budget).map(Order::New),
        MESSAGE_SESSION => from_bytes(input, budget).map(Order::Say),
        READ_SESSION => from_bytes(input, budget).map(Order::Read),
        LIST_SESSIONS => from_bytes::<ListSessions>(input, budget).map(|_| Order::List),
        INTERRUPT_SESSION => from_bytes(input, budget).map(Order::Interrupt),
        HIBERNATE_SESSION => from_bytes(input, budget).map(Order::Hibernate),
        WAKE_SESSION => from_bytes(input, budget).map(Order::Wake),
        ARCHIVE_SESSION => from_bytes(input, budget).map(Order::Archive),
        other => return Err(Called::Refused(format!("there is no tool `{other}` here"))),
    };

    read.map_err(|fault| Called::Refused(format!("the input of `{tool}`: {fault}")))
}
