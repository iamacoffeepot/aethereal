//! The engine's own tool: `actor_logs`, which reads what an actor logged (ADR-0044).

use crate::{Call, Called, Output, Tool, ToolSheet, Tooling};
use aethereal_actor::{
    Actor, ActorRef, Anywhere, Beginning, ChildOf, Ending, Entry, Fault, Handles, Level, ProtocolRef, ToSender, Turn,
};
use aethereal_data::{ActorPath, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};

/// The most bytes of text one answer holds: past it, the answer stops early, and its `next` says where to go on.
const MOST_TEXT_BYTES: usize = 256 * 1024;
/// The most bytes reading an input may take. A path and a filter are short.
const INPUT_BYTES: usize = 8 * 1024;

/// How much a logged line matters, as the agent names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.log_level")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    /// The level a line was logged at.
    const fn of(level: Level) -> Self {
        match level {
            Level::Debug => Self::Debug,
            Level::Info => Self::Info,
            Level::Warn => Self::Warn,
            Level::Error => Self::Error,
        }
    }

    /// Where the level stands, least first.
    const fn rank(self) -> u8 {
        self as u8
    }
}

/// Asks for what the actor at `actor` logged, live or ended: its entries numbered `since` and after, its lines at
/// `level` and above and every entry that is not a line, those whose text holds `contains`, at most `most` of them.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.read_log")]
pub struct ReadLog {
    pub actor: ActorPath,
    pub since: u64,
    pub level: LogLevel,
    pub contains: String,
    pub most: u32,
}

/// What an actor logged, as the agent reads it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.actor_log")]
pub struct ActorLog {
    /// The entries, oldest first.
    pub entries: Vec<LogEntry>,
    /// The number to ask from next.
    pub next: u64,
    /// How many entries asked for had already left the log.
    pub skipped: u64,
}

/// One entry, with its number within its actor.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.log_entry")]
pub struct LogEntry {
    pub n: u64,
    pub entry: LoggedEntry,
}

/// What an entry records: a line, or why a turn, a resource, a job, or mail went wrong, written as text.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.logged_entry")]
pub enum LoggedEntry {
    /// A line the actor logged.
    Line { level: LogLevel, text: String },
    /// A turn returned a fault, and the actor ended.
    Failed(String),
    /// The actor's closing turn returned a fault.
    EndFailed(String),
    /// A resource was dropped on its way.
    Dropped(String),
    /// What a source brought could not be written as mail.
    Unbrought(String),
    /// A job the actor ran faulted.
    JobFaulted(String),
    /// Mail the actor was handed did not read.
    Unread(String),
}

impl LoggedEntry {
    /// `entry` as the agent reads it.
    fn of(entry: &Entry) -> Self {
        match entry {
            Entry::Line { level, text } => Self::Line { level: LogLevel::of(*level), text: text.clone() },
            Entry::Failed(fault) => Self::Failed(spelled(fault)),
            Entry::EndFailed(fault) => Self::EndFailed(spelled(fault)),
            Entry::Dropped(name) => Self::Dropped(String::from(*name)),
            Entry::Unbrought(name) => Self::Unbrought(String::from(*name)),
            Entry::JobFaulted(fault) => Self::JobFaulted(spelled(fault)),
            Entry::Unread(reason) => Self::Unread(format!("{reason:?}")),
        }
    }

    /// The text the entry holds, which `contains` is looked for in.
    fn text(&self) -> &str {
        match self {
            Self::Line { text, .. }
            | Self::Failed(text)
            | Self::EndFailed(text)
            | Self::Dropped(text)
            | Self::Unbrought(text)
            | Self::JobFaulted(text)
            | Self::Unread(text) => text,
        }
    }

    /// Whether the entry is kept by a read at `level`: a line at that level or above, and every entry that is not a
    /// line.
    const fn is_at(&self, level: LogLevel) -> bool {
        match self {
            Self::Line { level: logged, .. } => logged.rank() >= level.rank(),
            _ => true,
        }
    }
}

/// A fault as the agent reads it: its reason's kind and size, as no table names a kind by its id here.
fn spelled(fault: &Fault) -> String {
    format!("a fault of kind {:?}, {} bytes", fault.kind(), fault.bytes().len())
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.logs_tool_config")]
pub struct LogsToolConfig;

/// The tool `actor_logs`: answers what an actor logged, live or ended, from the engine's log as the step began.
#[derive(Debug, Plain)]
#[kind(name = "aether.tools.logs_tool")]
pub struct LogsTool;

impl ChildOf for LogsTool {
    type Parent = Anywhere;
}

impl Actor for LogsTool {
    type Config = LogsToolConfig;
    type Key = ();
    type Handles = (Call,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: LogsToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for LogsTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![ToolSheet {
            name: String::from("actor_logs"),
            description: format!(
                "Reads what the actor at `actor` (its path) logged, live or ended: its entries numbered `since` and \
                 after, oldest first; lines at `level` and above and every fault, drop, or unread mail; those whose \
                 text holds `contains` (\"\" for all); at most `most`, and {MOST_TEXT_BYTES} bytes of text. Ask \
                 again from `next` to go on; `skipped` counts entries that had left the log."
            ),
            input: SchemaNodes::of(&ReadLog::SHAPE),
            output: Output::Value(SchemaNodes::of(&ActorLog::SHAPE)),
        }]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for LogsTool {
    type Sends = (ToSender<Called>,);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let read = match from_bytes::<ReadLog>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) {
            Ok(read) => read,
            Err(fault) => {
                turn.reply(&Called::Refused(format!("the input of `actor_logs`: {fault}")))?;
                return Ok(());
            }
        };
        let log = read_log(turn, &read);
        let called = Called::output(turn, to_bytes(&log)?);
        turn.reply(&called)?;

        Ok(())
    }
}

/// What `read` asks of the log, as `turn` reads it.
fn read_log<S>(turn: &Turn<'_, LogsTool, S>, read: &ReadLog) -> ActorLog {
    let logged = turn.logged(&read.actor);
    let skipped = logged.first().saturating_sub(read.since);
    let most = usize::try_from(read.most).unwrap_or(usize::MAX);
    let mut entries = Vec::new();
    let mut text_bytes = 0;
    let mut next = logged.next().max(read.since);
    for (n, entry) in logged.since(read.since) {
        let entry = LoggedEntry::of(entry);
        let kept = entry.is_at(read.level) && entry.text().contains(read.contains.as_str());
        if !kept {
            continue;
        }
        let full = entries.len() >= most || text_bytes + entry.text().len() > MOST_TEXT_BYTES;
        if full {
            next = n;
            break;
        }
        text_bytes += entry.text().len();
        entries.push(LogEntry { n, entry });
    }

    ActorLog { entries, next, skipped }
}
