//! `engine_stats` on the native host (ADR-0053, ADR-0036): a root that says it holds bytes unsent as it is told, and
//! the tool reading the engine's figures, the test asking the tool's worker each call as a holder would.

use aethereal_actor::{Actor, Answered, Beginning, ChildOf, Ending, Fault, Handles, ProtocolRef, Top, Turn, path};
use aethereal_chassis_headless::Headless;
use aethereal_data::{Budget, Plain, from_bytes, to_bytes};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_tools::{
    BacklogFigure, Call, Called, EngineFigures, EngineStatsTool, EngineStatsToolConfig, Meter, ProcessMemory,
    ReadEngineStats, Tool, Tooling,
};

/// The most bytes reading an answer takes.
const WALK_BYTES: usize = 1 << 20;

/// The most bytes the sender may hold unsent.
const MOST_UNSENT: u64 = 1 << 20;

/// Tells the sender it holds `bytes` unsent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.stats.hold")]
struct Hold {
    bytes: u64,
}

/// Tells the sender to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.stats.stop")]
struct Stop;

/// A root that says it holds bytes unsent as it is told, as a connection does, and closes when told to.
#[derive(Debug, Default, Plain)]
#[kind(name = "aether.test.stats.sender")]
struct Sender;

impl ChildOf for Sender {
    type Parent = Top;
}

impl Actor for Sender {
    type Config = Self;
    type Key = ();
    type Handles = (Hold, Stop);
    type BeginSends = ();
    type EndSends = ();

    fn begin(sender: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(sender)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Hold> for Sender {
    type Sends = ();

    fn handle(&mut self, Hold { bytes }: Hold, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.holds_unsent(bytes, MOST_UNSENT);

        Ok(())
    }
}

impl Handles<Stop> for Sender {
    type Sends = ();

    fn handle(&mut self, Stop: Stop, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// A program holding the sender and the tool, with the tool's worker.
fn stats_tool() -> Result<(Harness<Headless>, ProtocolRef<Tooling>), Unheard> {
    let mut harness = Harness::of(Headless::program())?;
    harness.add_root::<Sender>(&(), Sender)?;
    harness.add_root::<EngineStatsTool>(&(), EngineStatsToolConfig)?;
    let worker = EngineStatsTool::worker(harness.root::<EngineStatsTool>());

    Ok((harness, worker))
}

/// The sender told it holds `bytes` unsent, and steps run until what it said is folded.
fn hold(harness: &mut Harness<Headless>, bytes: u64) -> Result<(), Unheard> {
    let sender = harness.root::<Sender>();
    harness.tell(sender, &Hold { bytes })?;
    harness.settle();

    Ok(())
}

/// The sender told to close, and steps run until its closing turn has.
fn stop(harness: &mut Harness<Headless>) -> Result<(), Unheard> {
    let sender = harness.root::<Sender>();
    harness.tell(sender, &Stop)?;
    harness.settle();

    Ok(())
}

/// What the tool answers.
fn figures(harness: &mut Harness<Headless>, worker: ProtocolRef<Tooling>) -> Result<EngineFigures, Unheard> {
    let Ok(bytes) = to_bytes(&ReadEngineStats { most: None }) else {
        panic!("the read encodes");
    };
    let input = harness.put(bytes)?;
    let asked = harness.ask(worker, &Call { tool: String::from("engine_stats"), input })?;
    let Answered::Yes(Called::Output(output)) = harness.answer(asked)? else {
        panic!("the tool answers an output");
    };
    let Some(bytes) = harness.read(&output) else {
        panic!("the output's blob is held");
    };
    let Ok(figures) = from_bytes(bytes, &mut Budget::new(WALK_BYTES)) else {
        panic!("the output reads as the engine's figures");
    };

    Ok(figures)
}

#[test]
fn the_figures_name_an_actors_backlog_by_path_until_it_ends() -> Result<(), Unheard> {
    // Catches a backlog the engine never folds from what a turn said, one named by no path, a figure kept after the
    // actor said it holds less or ended, and blob or process figures the tool leaves unread.
    let (mut harness, worker) = stats_tool()?;
    let sender = path::<Sender>(()).names().to_string();

    hold(&mut harness, 300)?;
    let holding = figures(&mut harness, worker)?;
    hold(&mut harness, 120)?;
    let shrunk = figures(&mut harness, worker)?;
    stop(&mut harness)?;
    let ended = figures(&mut harness, worker)?;

    let backlog = |bytes| BacklogFigure { actor: sender.clone(), bytes, most: MOST_UNSENT };
    assert_eq!(holding.longest_backlogs, [backlog(300)]);
    assert_eq!(holding.backlog, Meter { bytes: 300, most: MOST_UNSENT });
    assert_eq!(shrunk.longest_backlogs, [backlog(120)]);
    assert_eq!((ended.longest_backlogs.len(), ended.backlog), (0, Meter { bytes: 0, most: 0 }));
    // The input of each call is a blob the store holds as the tool reads it.
    assert!(holding.blobs.bytes > 0 && holding.blobs.bytes <= holding.blobs.most, "{:?}", holding.blobs);
    assert!(matches!(holding.process, ProcessMemory::Resident { bytes } if bytes > 0), "{:?}", holding.process);
    assert_eq!((holding.guest_bytes, holding.largest_guests.len()), (0, 0));

    Ok(())
}
