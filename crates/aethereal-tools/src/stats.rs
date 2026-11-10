//! The engine's own tool `engine_stats`, which reads the engine's memory against its limits, and the largest guests
//! and longest backlogs by path (ADR-0053).

use crate::logs::spoken_name;
use crate::{Call, Called, Output, Tool, ToolSheet, Tooling};
use aethereal_actor::{
    Actor, ActorRef, Against, Anywhere, Beginning, ChildOf, Ending, EngineStats, Fault, Figures, Handles, LARGEST,
    Process, ProtocolRef, ToSender, Turn,
};
use aethereal_data::{Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};

/// The tool's name.
const ENGINE_STATS: &str = "engine_stats";
/// The most bytes reading an input may take. It holds one number.
const INPUT_BYTES: usize = 1024;

/// Asks for the engine's memory against its limits, with at most `most` of its largest guests and longest backlogs.
/// The tool reads a field left out as its fallback, which the field's doc names.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.read_engine_stats")]
pub struct ReadEngineStats {
    /// The most guests, and the most backlogs, named: none for 8, the most the engine keeps.
    #[flag]
    pub most: Option<u32>,
}

/// The engine's memory against its limits, as the agent reads it (ADR-0053).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.engine_figures")]
pub struct EngineFigures {
    /// The memory the program holds, as its chassis reads it.
    pub process: ProcessMemory,
    /// The bytes the blob store holds, against the most it may.
    pub blobs: Meter,
    /// The most bytes any one actor holds unsent, against its most: none against none when no actor holds any.
    pub backlog: Meter,
    /// The bytes of every guest's memory, added up.
    pub guest_bytes: u64,
    /// The guests with the most memory, the most first.
    pub largest_guests: Vec<GuestFigure>,
    /// The actors holding the most bytes unsent, the most first.
    pub longest_backlogs: Vec<BacklogFigure>,
}

/// The memory the program holds, as the agent reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.process_memory")]
pub enum ProcessMemory {
    /// The bytes resident in memory, as the operating system counts them.
    Resident { bytes: u64 },
    /// The bytes of the script heap the browser reports for the page.
    Heap { bytes: u64 },
    /// The chassis reads no figure.
    Unknown,
}

/// Bytes held against the most that may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.meter")]
pub struct Meter {
    pub bytes: u64,
    pub most: u64,
}

/// One guest's memory: an instance of one blob's code in one part of the engine, named by the first actor that runs
/// in it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.guest_figure")]
pub struct GuestFigure {
    /// The first actor's path, or its id in hex when the engine no longer remembers its name.
    pub actor: String,
    pub bytes: u64,
    /// How many actors run in it.
    pub actors: u32,
}

/// The bytes one actor holds unsent, against the most it may.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.backlog_figure")]
pub struct BacklogFigure {
    /// The actor's path, or its id in hex when the engine no longer remembers its name.
    pub actor: String,
    pub bytes: u64,
    pub most: u64,
}

impl Meter {
    /// `against` as the agent reads it.
    const fn of(against: Against) -> Self {
        Self { bytes: against.bytes, most: against.most }
    }
}

impl ProcessMemory {
    /// `process` as the agent reads it.
    const fn of(process: Process) -> Self {
        match process {
            Process::Resident { bytes } => Self::Resident { bytes },
            Process::Heap { bytes } => Self::Heap { bytes },
            Process::Unknown => Self::Unknown,
        }
    }
}

impl EngineFigures {
    /// `figures` as the agent reads them, naming at most `most` guests and as many backlogs.
    fn of(figures: Figures, most: usize) -> Self {
        let backlog = Meter::of(figures.backlog());
        let largest_guests = figures.largest_guests.into_iter().take(most).map(|guest| GuestFigure {
            actor: spoken_name(&guest.actor),
            bytes: guest.bytes,
            actors: guest.actors,
        });
        let longest_backlogs = figures.longest_backlogs.into_iter().take(most).map(|longest| BacklogFigure {
            actor: spoken_name(&longest.actor),
            bytes: longest.unsent.bytes,
            most: longest.unsent.most,
        });

        Self {
            process: ProcessMemory::of(figures.process),
            blobs: Meter::of(figures.blobs),
            backlog,
            guest_bytes: figures.guest_bytes,
            largest_guests: largest_guests.collect(),
            longest_backlogs: longest_backlogs.collect(),
        }
    }
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.tools.engine_stats_tool_config")]
pub struct EngineStatsToolConfig;

/// The tool `engine_stats`: answers the engine's memory against its limits, as the step began.
#[derive(Debug, Plain)]
#[kind(name = "aether.tools.engine_stats_tool")]
pub struct EngineStatsTool;

impl ChildOf for EngineStatsTool {
    type Parent = Anywhere;
}

impl Actor for EngineStatsTool {
    type Config = EngineStatsToolConfig;
    type Key = ();
    type Handles = (Call,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EngineStatsToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for EngineStatsTool {
    fn sheets() -> Vec<ToolSheet> {
        let description = format!(
            "Reads the engine's memory against its limits, as the step began: `process`, the program's memory as \
             its chassis reads it (resident on a native host, the script heap a browser reports, or unknown); \
             `blobs`, the bytes the blob store holds against its most; `backlog`, the most bytes any one actor \
             holds unsent against its most; `guest_bytes`, every guest's memory added up; and the guests with the \
             most memory and the actors holding the most bytes unsent, each by path, the most first, at most `most` \
             of each ({LARGEST} when left out, and never more)."
        );

        vec![ToolSheet::of::<ReadEngineStats>(
            ENGINE_STATS,
            &description,
            Output::Value(SchemaNodes::of(&EngineFigures::SHAPE)),
        )]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for EngineStatsTool {
    type Sends = (ToSender<Called>,);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let read = match from_bytes::<ReadEngineStats>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) {
            Ok(read) => read,
            Err(fault) => {
                turn.reply(&Called::Refused(format!("the input of `{ENGINE_STATS}`: {fault}")))?;
                return Ok(());
            }
        };
        let EngineStats::Read(figures) = turn.engine_stats() else {
            turn.reply(&Called::Refused(format!("`{ENGINE_STATS}` runs where no engine's figures are read")))?;
            return Ok(());
        };
        let most = read.most.map_or(LARGEST, |most| usize::try_from(most).unwrap_or(usize::MAX));
        let answer = EngineFigures::of(figures, most);
        let called = Called::output(turn, to_bytes(&answer)?);
        turn.reply(&called)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CommandLine, FlagsFault};

    #[test]
    fn every_sheet_s_flags_fit_its_input() {
        // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the tool would be missing from a running
        // engine, and nothing would answer when it is called.
        let sheets = EngineStatsTool::sheets();
        let refused: Vec<(&str, FlagsFault)> = sheets
            .iter()
            .filter_map(|sheet| CommandLine::of(sheet).err().map(|fault| (sheet.name.as_str(), fault)))
            .collect();

        assert!(refused.is_empty(), "{refused:?}");
    }
}
