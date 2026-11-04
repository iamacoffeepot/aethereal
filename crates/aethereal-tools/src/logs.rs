//! The engine's own tool: `actor_logs`, which reads what an actor logged (ADR-0044).

use crate::{Call, Called, Output, Tool, ToolSheet, Tooling};
use aethereal_actor::{
    Actor, ActorRef, Anywhere, Beginning, ChildOf, Ending, Entry, Fault, Handles, Level, ProtocolRef, ToSender, Turn,
};
use aethereal_data::{ActorPath, Bare, Budget, Encode, EncodeError, Plain, Schema, SchemaNodes, Writer, from_bytes};
use core::fmt::Write as _;

/// The most bytes one answer is written in, its entries' own fields with their text: past it, the answer stops early,
/// and its `next` says where to go on. Text alone would leave many short entries unbounded.
const MOST_ANSWER_BYTES: usize = 256 * 1024;
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

/// An entry as a read weighs and writes it: a line, with its level, or another entry, with its selector in
/// [`LoggedEntry`], and its text, borrowed from the entry or written into a scratch the read reuses.
enum Spoken<'e> {
    Line { level: LogLevel, text: &'e str },
    Other { selector: u32, text: &'e str },
}

impl<'e> Spoken<'e> {
    /// `entry` as a read weighs it, any text it is not already written into `scratch`.
    fn of(entry: &'e Entry, scratch: &'e mut String) -> Self {
        scratch.clear();
        let other = |selector: u32, text: &'e str| Self::Other { selector, text };
        // Selectors as `LoggedEntry` declares its variants, in order: a tripwire below pins them.
        match entry {
            Entry::Line { level, text } => Self::Line { level: LogLevel::of(*level), text },
            Entry::Failed(fault) => other(1, spelled(fault, scratch)),
            Entry::EndFailed(fault) => other(2, spelled(fault, scratch)),
            Entry::Dropped(name) => other(3, name),
            Entry::Unbrought(name) => other(4, name),
            Entry::JobFaulted(fault) => other(5, spelled(fault, scratch)),
            Entry::Unread(reason) => {
                let _ = write!(scratch, "{reason:?}");
                other(6, scratch)
            }
        }
    }

    /// The text the entry holds, which `contains` is looked for in.
    const fn text(&self) -> &'e str {
        match self {
            Self::Line { text, .. } | Self::Other { text, .. } => text,
        }
    }

    /// Whether `read` keeps the entry: a line at its level or above, or any other entry, holding its text.
    fn is_kept(&self, read: &ReadLog) -> bool {
        let at_level = match self {
            Self::Line { level, .. } => level.rank() >= read.level.rank(),
            Self::Other { .. } => true,
        };

        at_level && self.text().contains(read.contains.as_str())
    }

    /// Writes the entry numbered `n` as the [`LogEntry`] it is (ADR-0002): its number, its variant's selector, and
    /// its fields in order.
    fn write(&self, n: u64, out: &mut Writer<'_, Bare>) -> Result<(), EncodeError> {
        n.encode(out)?;
        match self {
            Self::Line { level, text } => {
                0_u32.encode(out)?;
                level.encode(out)?;
                text.encode(out)
            }
            Self::Other { selector, text } => {
                selector.encode(out)?;
                text.encode(out)
            }
        }
    }
}

/// A fault as the agent reads it, written into `scratch`: its reason's kind and size, as no table names a kind by its
/// id here.
fn spelled<'s>(fault: &Fault, scratch: &'s mut String) -> &'s str {
    let _ = write!(scratch, "a fault of kind {:?}, {} bytes", fault.kind(), fault.bytes().len());

    scratch
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
                 text holds `contains` (\"\" for all); at most `most`, in an answer of at most {MOST_ANSWER_BYTES} bytes. Ask \
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
        let planned = plan(turn.logged(&read.actor).since(read.since), &read);
        let buffer = turn.buffer(planned.room);
        let logged = turn.logged(&read.actor);
        let skipped = logged.first().saturating_sub(read.since);
        let next = match planned.ended {
            Ended::Last => logged.next().max(read.since),
            Ended::Short(n) => n,
        };
        let written = written(logged.since(read.since), &read, &planned, &Tail { next, skipped }, buffer)?;
        let called = Called::output(turn, written);
        turn.reply(&called)?;

        Ok(())
    }
}

/// What a read keeps of an actor's entries, found before its answer is written: how many, the bytes they take, and
/// where it ended.
struct Plan {
    count: usize,
    room: usize,
    ended: Ended,
}

/// Where a read of an actor's entries ended.
enum Ended {
    /// At the last entry the log holds.
    Last,
    /// Short of the entry numbered this, past `most` or the bytes an answer holds.
    Short(u64),
}

/// The `next` and `skipped` an answer ends with.
struct Tail {
    next: u64,
    skipped: u64,
}

/// The bytes of an answer's own fields, beside its entries: their count, `next`, and `skipped`.
const ANSWER_BYTES: usize = 4 + 8 + 8;
/// The bytes of an entry's own fields, beside its text: its number, its selector, a line's level, and its text's
/// count.
const ENTRY_BYTES: usize = 8 + 4 + 4 + 4;

/// What `read` keeps of `entries`, those numbered from its `since`, oldest first: at most `most` of them, and
/// [`MOST_ANSWER_BYTES`] of answer.
fn plan<'e>(entries: impl Iterator<Item = (u64, &'e Entry)>, read: &ReadLog) -> Plan {
    let most = usize::try_from(read.most).unwrap_or(usize::MAX);
    let mut scratch = String::new();
    let mut plan = Plan { count: 0, room: ANSWER_BYTES, ended: Ended::Last };
    for (n, entry) in entries {
        let spoken = Spoken::of(entry, &mut scratch);
        if !spoken.is_kept(read) {
            continue;
        }
        let length = spoken.text().len();
        let full = plan.count >= most || plan.room + ENTRY_BYTES + length > MOST_ANSWER_BYTES;
        if full {
            plan.ended = Ended::Short(n);
            break;
        }
        plan.count += 1;
        plan.room += ENTRY_BYTES + length;
    }

    plan
}

/// The [`ActorLog`] `read` answers with, written into `buffer` with no value of it made (ADR-0002): the first
/// `plan.count` entries of `entries` it keeps, then `tail`.
fn written<'e>(
    entries: impl Iterator<Item = (u64, &'e Entry)>,
    read: &ReadLog,
    plan: &Plan,
    tail: &Tail,
    buffer: Vec<u8>,
) -> Result<Vec<u8>, EncodeError> {
    let mut scratch = String::new();
    let mut out = Writer::<Bare>::appending(buffer, ());
    let count = u32::try_from(plan.count).map_err(|_| EncodeError::CountOverflow { count: plan.count })?;
    count.encode(&mut out)?;
    let mut left = plan.count;
    for (n, entry) in entries {
        if left == 0 {
            break;
        }
        let spoken = Spoken::of(entry, &mut scratch);
        if !spoken.is_kept(read) {
            continue;
        }
        spoken.write(n, &mut out)?;
        left -= 1;
    }
    tail.next.encode(&mut out)?;
    tail.skipped.encode(&mut out)?;

    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethereal_actor::Unreadable;
    use aethereal_data::to_bytes;

    /// A read of every entry from `since`, at `level` and above, holding `contains`, at most `most`.
    fn read(since: u64, level: LogLevel, contains: &str, most: u32) -> ReadLog {
        ReadLog { actor: ActorPath::new("aether.test.logs"), since, level, contains: String::from(contains), most }
    }

    /// The answer `read` writes of `entries`, numbered from `first`, the log's next entry numbered `next`.
    fn answered(entries: &[Entry], first: u64, next: u64, read: &ReadLog) -> Vec<u8> {
        let numbered = || (first..).zip(entries).filter(|(n, _)| *n >= read.since);
        let planned = plan(numbered(), read);
        let next = match planned.ended {
            Ended::Last => next.max(read.since),
            Ended::Short(n) => n,
        };
        let tail = Tail { next, skipped: first.saturating_sub(read.since) };
        let Ok(bytes) = written(numbered(), read, &planned, &tail, Vec::new()) else {
            panic!("the answer is written");
        };

        bytes
    }

    /// A fault whose reason is `LogsToolConfig`, and how the agent reads it.
    fn fault() -> (Fault, String) {
        let Ok(fault) = Fault::new(&LogsToolConfig) else {
            panic!("the reason encodes");
        };
        let mut scratch = String::new();
        let text = String::from(spelled(&fault, &mut scratch));

        (fault, text)
    }

    /// A line entry numbered `n`, as the agent reads it.
    fn line(n: u64, level: LogLevel, text: &str) -> LogEntry {
        LogEntry { n, entry: LoggedEntry::Line { level, text: String::from(text) } }
    }

    #[test]
    fn an_answer_is_written_as_the_actor_log_it_holds() {
        // Tripwire: the bytes written by hand are those `to_bytes` writes of the `ActorLog` they hold, for every
        // variant of `LoggedEntry` and a level other than the first, so a variant reordered or a field added to a
        // kind the sheet names cannot drift from what is written.
        let (fault, spelled) = fault();
        let entries = [
            Entry::Line { level: Level::Warn, text: String::from("slow frame") },
            Entry::Failed(fault.clone()),
            Entry::EndFailed(fault.clone()),
            Entry::Dropped("a texture"),
            Entry::Unbrought("aether.window.key"),
            Entry::JobFaulted(fault),
            Entry::Unread(Unreadable::Malformed),
        ];
        // Read from 3 while the log holds 5 on: two had left it.
        let read = read(3, LogLevel::Debug, "", u32::MAX);

        let expected = ActorLog {
            entries: vec![
                line(5, LogLevel::Warn, "slow frame"),
                LogEntry { n: 6, entry: LoggedEntry::Failed(spelled.clone()) },
                LogEntry { n: 7, entry: LoggedEntry::EndFailed(spelled.clone()) },
                LogEntry { n: 8, entry: LoggedEntry::Dropped(String::from("a texture")) },
                LogEntry { n: 9, entry: LoggedEntry::Unbrought(String::from("aether.window.key")) },
                LogEntry { n: 10, entry: LoggedEntry::JobFaulted(spelled) },
                LogEntry { n: 11, entry: LoggedEntry::Unread(String::from("Malformed")) },
            ],
            next: 12,
            skipped: 2,
        };
        assert_eq!(Ok(answered(&entries, 5, 12, &read)), to_bytes(&expected));
    }

    #[test]
    fn a_read_stops_at_most_and_names_the_first_it_left_out() {
        // Catches `next` set past an entry left out, entries filtered out counted toward `most`, and `skipped` lost.
        let entries: Vec<Entry> = ["a", "skip", "b", "c"]
            .into_iter()
            .map(|text| Entry::Line { level: Level::Info, text: String::from(text) })
            .collect();
        let filtered = ReadLog { contains: String::from("a"), ..read(0, LogLevel::Debug, "", 1) };
        let stopped = read(0, LogLevel::Debug, "", 2);

        let expected_filtered = ActorLog { entries: vec![line(2, LogLevel::Info, "a")], next: 6, skipped: 2 };
        let expected_stopped = ActorLog {
            entries: vec![line(2, LogLevel::Info, "a"), line(3, LogLevel::Info, "skip")],
            next: 4,
            skipped: 2,
        };
        assert_eq!(Ok(answered(&entries, 2, 6, &filtered)), to_bytes(&expected_filtered));
        assert_eq!(Ok(answered(&entries, 2, 6, &stopped)), to_bytes(&expected_stopped));
    }

    #[test]
    fn an_answer_of_many_short_entries_stays_within_its_bytes() {
        // Catches a bound on text alone, under which entries with little text make an answer of any size: its own
        // fields count too, and `next` names the first entry left out.
        let entries: Vec<Entry> =
            (0..100_000).map(|_| Entry::Line { level: Level::Info, text: String::new() }).collect();
        let read = read(0, LogLevel::Debug, "", u32::MAX);

        let bytes = answered(&entries, 0, 100_000, &read);
        let Ok(log) = from_bytes::<ActorLog>(&bytes, &mut Budget::new(bytes.len() * 8)) else {
            panic!("the answer reads");
        };

        assert!(bytes.len() <= MOST_ANSWER_BYTES, "{} bytes", bytes.len());
        assert_eq!(log.next, log.entries.len() as u64);
    }
}
