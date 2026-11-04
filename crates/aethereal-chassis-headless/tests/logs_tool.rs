//! `actor_logs` on the native host (ADR-0044, ADR-0036): a root that logs as it is told, and the tool reading what
//! it said, live and after it has ended, the test asking the tool's worker each call as a holder would.

use aethereal_actor::{
    Actor, Answered, Beginning, ChildOf, Ending, Fault, Handles, Level, ProtocolRef, Top, Turn, path,
};
use aethereal_chassis_headless::Headless;
use aethereal_data::{ActorPath, Budget, Plain, from_bytes, to_bytes};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_tools::{
    ActorLog, Call, Called, LogEntry, LogLevel, LoggedEntry, LogsTool, LogsToolConfig, ReadLog, Tool, Tooling,
};

/// The most bytes reading an answer takes.
const WALK_BYTES: usize = 1 << 20;

/// Tells the talker to log `text` at `level`.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.logs.say")]
struct Say {
    level: LogLevel,
    text: String,
}

/// Tells the talker to fail its turn, and so end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.logs.give_up")]
struct GiveUp;

/// A root that logs what it is told to, and fails when told to give up.
#[derive(Debug, Default, Plain)]
#[kind(name = "aether.test.logs.talker")]
struct Talker;

impl ChildOf for Talker {
    type Parent = Top;
}

impl Actor for Talker {
    type Config = Self;
    type Key = ();
    type Handles = (Say, GiveUp);
    type BeginSends = ();
    type EndSends = ();

    fn begin(talker: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(talker)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Say> for Talker {
    type Sends = ();

    fn handle(&mut self, Say { level, text }: Say, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let level = match level {
            LogLevel::Debug => Level::Debug,
            LogLevel::Info => Level::Info,
            LogLevel::Warn => Level::Warn,
            LogLevel::Error => Level::Error,
        };
        turn.log(level, text);

        Ok(())
    }
}

impl Handles<GiveUp> for Talker {
    type Sends = ();

    fn handle(&mut self, give_up: GiveUp, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.log(Level::Error, "giving up");

        Err(Fault::new(&give_up)?)
    }
}

/// A program holding the talker and the tool, with the tool's worker.
fn logs_tool() -> Result<(Harness<Headless>, ProtocolRef<Tooling>), Unheard> {
    let mut harness = Harness::of(Headless::program())?;
    harness.add_root::<Talker>(&(), Talker)?;
    harness.add_root::<LogsTool>(&(), LogsToolConfig)?;
    let worker = LogsTool::worker(harness.root::<LogsTool>());

    Ok((harness, worker))
}

/// The talker's path, as the agent writes it.
fn talker() -> ActorPath {
    path::<Talker>(()).names().clone()
}

/// A read of `actor` from `since`, at `level` and above, holding `contains`, at most `most`.
fn read(actor: ActorPath, since: u64, level: LogLevel, contains: &str, most: u32) -> ReadLog {
    ReadLog { actor, since, level, contains: String::from(contains), most }
}

/// Every entry of `actor` from `since`.
fn every(actor: ActorPath, since: u64) -> ReadLog {
    read(actor, since, LogLevel::Debug, "", u32::MAX)
}

/// What the tool answers `read` with.
fn logged(harness: &mut Harness<Headless>, worker: ProtocolRef<Tooling>, read: &ReadLog) -> Result<ActorLog, Unheard> {
    let Ok(bytes) = to_bytes(read) else {
        panic!("the read encodes");
    };
    let input = harness.put(bytes)?;
    let asked = harness.ask(worker, &Call { tool: String::from("actor_logs"), input })?;
    let Answered::Yes(Called::Output(output)) = harness.answer(asked)? else {
        panic!("the tool answers an output");
    };
    let Some(bytes) = harness.read(&output) else {
        panic!("the output's blob is held");
    };
    let Ok(log) = from_bytes(bytes, &mut Budget::new(WALK_BYTES)) else {
        panic!("the output reads as an actor's log");
    };

    Ok(log)
}

/// The talker told to say each of `lines`, and a step run, so its lines are in the log the next step reads.
fn say(harness: &mut Harness<Headless>, lines: &[(LogLevel, &str)]) -> Result<(), Unheard> {
    let talker = harness.root::<Talker>();
    for (level, text) in lines {
        harness.tell(talker, &Say { level: *level, text: String::from(*text) })?;
    }
    harness.step();

    Ok(())
}

/// A line entry numbered `n`.
fn line(n: u64, level: LogLevel, text: &str) -> LogEntry {
    LogEntry { n, entry: LoggedEntry::Line { level, text: String::from(text) } }
}

#[test]
fn an_actors_lines_are_read_numbered_and_paged_from_next() -> Result<(), Unheard> {
    // Catches entries read out of order, numbered from where the read began rather than the actor's first, a `next`
    // that repeats or skips an entry, and a read past `most` that loses the rest.
    let (mut harness, worker) = logs_tool()?;
    say(&mut harness, &[(LogLevel::Info, "one"), (LogLevel::Info, "two"), (LogLevel::Info, "three")])?;

    let first = logged(&mut harness, worker, &read(talker(), 0, LogLevel::Debug, "", 2))?;
    let rest = logged(&mut harness, worker, &every(talker(), first.next))?;

    assert_eq!(first.entries, [line(0, LogLevel::Info, "one"), line(1, LogLevel::Info, "two")]);
    assert_eq!(first.next, 2);
    assert_eq!(rest.entries, [line(2, LogLevel::Info, "three")]);
    assert_eq!((rest.next, rest.skipped), (3, 0));

    Ok(())
}

#[test]
fn a_read_keeps_lines_at_its_level_holding_its_text() -> Result<(), Unheard> {
    // Catches the level read as exact rather than at least, a filter that drops a fault, and `contains` ignored.
    let (mut harness, worker) = logs_tool()?;
    say(
        &mut harness,
        &[(LogLevel::Debug, "frame 1"), (LogLevel::Warn, "frame 2 slow"), (LogLevel::Error, "lost the device")],
    )?;

    let warned = logged(&mut harness, worker, &read(talker(), 0, LogLevel::Warn, "", u32::MAX))?;
    let frames = logged(&mut harness, worker, &read(talker(), 0, LogLevel::Debug, "frame", u32::MAX))?;

    let warned_texts: Vec<u64> = warned.entries.iter().map(|entry| entry.n).collect();
    let frame_texts: Vec<u64> = frames.entries.iter().map(|entry| entry.n).collect();
    assert_eq!(warned_texts, [1, 2]);
    assert_eq!(frame_texts, [0, 1]);

    Ok(())
}

#[test]
fn an_ended_actors_log_is_read_by_its_path_with_why_it_ended() -> Result<(), Unheard> {
    // Catches a read that finds only live actors, the log of an actor that has ended, usually the one most wanted.
    let (mut harness, worker) = logs_tool()?;
    say(&mut harness, &[(LogLevel::Info, "starting")])?;
    let talker_ref = harness.root::<Talker>();
    harness.tell(talker_ref, &GiveUp)?;
    harness.step();

    let log = logged(&mut harness, worker, &every(talker(), 0))?;

    let failed = log.entries.iter().any(|entry| matches!(entry.entry, LoggedEntry::Failed(_)));
    assert_eq!(log.entries.first(), Some(&line(0, LogLevel::Info, "starting")));
    assert_eq!(log.entries.get(1), Some(&line(1, LogLevel::Error, "giving up")));
    assert!(failed, "the fault that ended it is read: {:?}", log.entries);

    Ok(())
}

#[test]
fn an_actor_that_logged_nothing_reads_as_no_entries() -> Result<(), Unheard> {
    // Catches a read of a path nothing logged at that finds another actor's entries.
    let (mut harness, worker) = logs_tool()?;
    say(&mut harness, &[(LogLevel::Info, "only the talker")])?;
    let nowhere = ActorPath::new("aether.test.logs.nobody");

    let log = logged(&mut harness, worker, &every(nowhere, 0))?;

    assert_eq!((log.entries.len(), log.next, log.skipped), (0, 0, 0));

    Ok(())
}
