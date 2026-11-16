//! The sessions' tools (ADR-0057): the hub's caller opens a session, says a line to it, reads its transcript, lists
//! the sessions, interrupts a turn, and archives a session, each the capability's own ask.

#[cfg(test)]
mod tests;

use crate::{
    Archive, Archived, Interrupt, Interrupted, ListSessions, ListedSession, NewSession, Numbered, Opened,
    ReadTranscript, Refusal, Said, Say, Sessions, SessionsListed as Listed, Transcribed,
};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Done, Output, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The tool that opens a session.
const SESSION_NEW: &str = "session_new";
/// The tool that says a line to a session.
const SESSION_SAY: &str = "session_say";
/// The tool that reads a session's transcript.
const SESSION_READ: &str = "session_read";
/// The tool that lists the sessions.
const SESSION_LIST: &str = "session_list";
/// The tool that interrupts a session's turn.
const SESSION_INTERRUPT: &str = "session_interrupt";
/// The tool that archives a session.
const SESSION_ARCHIVE: &str = "session_archive";

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

/// What `session_say` and `session_interrupt` answer: the number of the transcript's entry they made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.entered")]
pub struct Entered {
    pub number: u64,
}

/// What `session_read` answers: the entries, in order, and the number of the transcript's last.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.sessions.read")]
pub struct Read {
    pub entries: Vec<Numbered>,
    pub last: u64,
}

/// What a call asks of the capability.
enum Order {
    New(NewSession),
    Say(Say),
    Read(ReadTranscript),
    List,
    Interrupt(Interrupt),
    Archive(Archive),
}

/// The sessions' six tools for the hub's caller (ADR-0057), served by one worker: each takes the capability's own ask
/// as its input, and answers what it answers, a refusal as text the caller reads.
#[derive(Bound)]
#[kind(name = "aether.sessions.sessions_tools")]
pub struct SessionsTools {
    /// Each call held, by the number of the ask made for it.
    calls: BTreeMap<u64, Held<Called>>,
    next: u64,
}

impl ChildOf for SessionsTools {
    type Parent = Anywhere;
}

impl Actor for SessionsTools {
    type Config = SessionsToolsConfig;
    type Key = ();
    type Handles = (
        Call,
        Answer<Opened, Asking>,
        Answer<Said, Asking>,
        Answer<Transcribed, Asking>,
        Answer<Listed, Asking>,
        Answer<Interrupted, Asking>,
        Answer<Archived, Asking>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: SessionsToolsConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for SessionsTools {
    fn sheets() -> Vec<ToolSheet> {
        vec![
            ToolSheet::of::<NewSession>(
                SESSION_NEW,
                "Opens a new Claude session in a container of its own on the hub's machine, and answers once it runs: \
                 its number, which every other session tool names it by. The name is only shown. Refused when as many \
                 sessions run as may, naming them.",
                Output::Value(SchemaNodes::of(&ListedSession::SHAPE)),
            ),
            ToolSheet::of::<Say>(
                SESSION_SAY,
                "Says a line to a session, as a user's message. A line said while a turn runs is taken after it, in \
                 order; a session stopped as idle is resumed. Answers the number of the transcript's entry the line \
                 is.",
                Output::Value(SchemaNodes::of(&Entered::SHAPE)),
            ),
            ToolSheet::of::<ReadTranscript>(
                SESSION_READ,
                "Reads a session's transcript after the entry numbered `after`, 0 for the start: every event Claude \
                 wrote, every line said to it and who said it, and what the hub did with its container. Answers the \
                 entries and the number of the last; read on after the last entry answered.",
                Output::Value(SchemaNodes::of(&Read::SHAPE)),
            ),
            ToolSheet::json::<ListSessions>(
                SESSION_LIST,
                "Lists every session: its number, name, state, and its transcript's last entry; how many may run at \
                 once; and any container carrying the hub's label that the hub has no record of.",
                Output::Value(SchemaNodes::of(&Listed::SHAPE)),
            ),
            ToolSheet::of::<Interrupt>(
                SESSION_INTERRUPT,
                "Interrupts the turn a session runs. The session carries on and takes the next line.",
                Output::Value(SchemaNodes::of(&Entered::SHAPE)),
            ),
            ToolSheet::of::<Archive>(
                SESSION_ARCHIVE,
                "Archives a session: its container is stopped and removed, and its transcript and home are kept. An \
                 archived session never runs again.",
                Output::Value(SchemaNodes::of(&Done::SHAPE)),
            ),
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for SessionsTools {
    type Sends = (
        Asks<Path<Sessions>, NewSession, Asking>,
        Asks<Path<Sessions>, Say, Asking>,
        Asks<Path<Sessions>, ReadTranscript, Asking>,
        Asks<Path<Sessions>, ListSessions, Asking>,
        Asks<Path<Sessions>, Interrupt, Asking>,
        Asks<Path<Sessions>, Archive, Asking>,
        ToSender<Called>,
    );

    /// Reads the call's input, or refuses it; holds the call, and asks the capability.
    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let order = match order(&call.tool, turn.read(&call.input)) {
            Ok(order) => order,
            Err(refusal) => {
                turn.reply(&refusal)?;
                return Ok(());
            }
        };
        let held = turn.hold()?;
        let asking = Asking { number: self.next };
        self.next += 1;
        let sessions = path::<Sessions>(());
        match order {
            Order::New(new) => turn.to(sessions).ask(&new, &asking)?,
            Order::Say(say) => turn.to(sessions).ask(&say, &asking)?,
            Order::Read(read) => turn.to(sessions).ask(&read, &asking)?,
            Order::List => turn.to(sessions).ask(&ListSessions, &asking)?,
            Order::Interrupt(interrupt) => turn.to(sessions).ask(&interrupt, &asking)?,
            Order::Archive(archive) => turn.to(sessions).ask(&archive, &asking)?,
        };
        self.calls.insert(asking.number, held);

        Ok(())
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
            Self::Refused(why) => Err(refusal(&why)),
        }
    }
}

impl Answers for Said {
    fn output(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Taken { number } => bytes(&Entered { number }),
            Self::Refused(why) => Err(refusal(&why)),
        }
    }
}

impl Answers for Transcribed {
    fn output(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Entries { entries, last } => bytes(&Read { entries, last }),
            Self::Refused(why) => Err(refusal(&why)),
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
            Self::Refused(why) => Err(refusal(&why)),
        }
    }
}

impl Answers for Archived {
    fn output(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Done => bytes(&Done {}),
            Self::Refused(why) => Err(refusal(&why)),
        }
    }
}

macro_rules! answers {
    ($($answer:ty),*) => {$(
        impl Handles<Answer<$answer, Asking>> for SessionsTools {
            type Sends = ();

            fn handle(
                &mut self,
                (answered, Asking { number }): (Answered<$answer>, Asking),
                turn: &mut Turn<'_, Self, ()>,
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
    )*};
}

answers!(Opened, Said, Transcribed, Listed, Interrupted, Archived);

/// The bytes of `value`, an output, or what says it could not be written.
fn bytes<T: Plain>(value: &T) -> Result<Vec<u8>, String> {
    to_bytes(value).map_err(|why| format!("the answer could not be written: {why:?}"))
}

/// What a refusal says, for the caller to read.
fn refusal(why: &Refusal) -> String {
    match why {
        Refusal::NoSuchSession => String::from("no session has that number"),
        Refusal::Archived => String::from("the session is archived"),
        Refusal::Full { most, running } => {
            format!("at most {most} sessions run at once, and these run: {running:?}")
        }
        Refusal::NotRunning => String::from("the session's container does not run, so no turn runs"),
        Refusal::Busy => String::from("the session is starting or stopping: ask again once it is not"),
        Refusal::Disk => String::from("the hub could not make the session's directories or keep its records"),
        Refusal::NoCredential => String::from("the hub could not read the credential's file as KEY=VALUE lines"),
        Refusal::Docker(error) => format!("Docker: {error:?}"),
    }
}

/// What a call of `tool` with `input`, the bytes of its input, asks of the capability, or its refusal.
fn order(tool: &str, input: &[u8]) -> Result<Order, Called> {
    let budget = &mut Budget::new(INPUT_BYTES);
    let read = match tool {
        SESSION_NEW => from_bytes(input, budget).map(Order::New),
        SESSION_SAY => from_bytes(input, budget).map(Order::Say),
        SESSION_READ => from_bytes(input, budget).map(Order::Read),
        SESSION_LIST => from_bytes::<ListSessions>(input, budget).map(|_| Order::List),
        SESSION_INTERRUPT => from_bytes(input, budget).map(Order::Interrupt),
        SESSION_ARCHIVE => from_bytes(input, budget).map(Order::Archive),
        other => return Err(Called::Refused(format!("there is no tool `{other}` here"))),
    };

    read.map_err(|fault| Called::Refused(format!("the input of `{tool}`: {fault}")))
}
