//! The overlay's own tools (ADR-0053: Commands are tools): `mark`, which pins a mark at this moment on the band's chart
//! and in the scrollback, and `memory`, which prints the engine's memory against its limits with its largest guests
//! and longest backlogs. The overlay offers them to the engine's tools root with a worker of their own, so a person at
//! the console and an agent through the hub call them alike, and an agent's mark shows on the owner's screen.

#[cfg(test)]
mod tests;

use crate::overlay::Overlay;
use aethereal_actor::{
    Actor, ActorName, ActorRef, Against, Anywhere, Beginning, ChildOf, Ending, EngineStats, Fault, Figures, Handles,
    LARGEST, Path, Process, ProtocolRef, Tells, ToSender, Turn, path,
};
use aethereal_data::{Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Done, Output, ToolSheet, Tooling};

/// The tool that pins a mark.
const MARK: &str = "mark";

/// The tool that prints the engine's memory.
const MEMORY: &str = "memory";

/// The most bytes reading an input may take: a mark's text is typed by hand.
const INPUT_BYTES: usize = 8 * 1024;

/// Pins a mark at this moment, saying `text`.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.mark")]
pub struct Mark {
    /// What the mark says, in the scrollback beside its time. Quote it to say more than a word.
    #[flag(position)]
    pub text: String,
}

/// Asks for the engine's memory, naming at most `most` of its largest guests and longest backlogs.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.memory")]
pub struct Memory {
    /// The most guests, and the most backlogs, named: none for 8, the most the engine keeps.
    #[flag(short = 'm')]
    pub most: Option<u32>,
}

/// The engine's memory against its limits, as a person reads it: each figure in bytes counted in powers of 1,024.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.memory_shown")]
pub struct MemoryShown {
    /// The memory the program holds, as its chassis reads it.
    pub process: String,
    /// The bytes the blob store holds, against the most it may.
    pub blobs: String,
    /// The most bytes any one actor holds unsent, against its most.
    pub backlog: String,
    /// Every guest's memory, added up.
    pub guests: String,
    /// The guests with the most memory, the most first.
    pub largest_guests: Vec<GuestShown>,
    /// The actors holding the most bytes unsent, the most first.
    pub longest_backlogs: Vec<BacklogShown>,
}

/// One guest's memory, named by the first actor that runs in it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.guest_shown")]
pub struct GuestShown {
    pub actor: String,
    pub memory: String,
    /// How many actors run in it.
    pub actors: u32,
}

/// The bytes one actor holds unsent, against its most.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.backlog_shown")]
pub struct BacklogShown {
    pub actor: String,
    pub unsent: String,
}

/// A mark pinned through the `mark` tool, told to the overlay, which pins it at the newest moment it has heard.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.marked")]
pub struct Marked {
    pub text: String,
}

/// What the overlay's tools' worker is made from: nothing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.overlay_tools_config")]
pub struct OverlayToolsConfig;

/// The worker of the overlay's tools, spawned by the overlay as it begins and offered to the engine's tools root.
#[derive(Debug, Plain)]
#[kind(name = "aether.overlay.overlay_tools")]
pub struct OverlayTools;

impl ChildOf for OverlayTools {
    type Parent = Anywhere;
}

impl Actor for OverlayTools {
    type Config = OverlayToolsConfig;
    type Key = ();
    type Handles = (Call,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: OverlayToolsConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl OverlayTools {
    /// What the worker offers: `mark` and `memory`.
    pub fn sheets() -> Vec<ToolSheet> {
        let mark = "Pins a mark at this moment: an ochre tick on the dev overlay's frame chart at this time, sliding \
                    left with the columns for sixty seconds, and an ochre line in its console's scrollback. Answers \
                    once the overlay has been told.";
        let memory = format!(
            "Prints the engine's memory against its limits, as the step began: `process`, the program's memory as its \
             chassis reads it; `blobs`, the bytes the blob store holds against its most; `backlog`, the most bytes \
             any one actor holds unsent against its most; `guests`, every guest's memory added up; and the guests \
             with the most memory and the actors holding the most bytes unsent, each by path, the most first, at \
             most `most` of each ({LARGEST} when left out, and never more)."
        );

        vec![
            ToolSheet::of::<Mark>(MARK, mark, Output::Value(SchemaNodes::of(&Done::SHAPE))),
            ToolSheet::of::<Memory>(MEMORY, &memory, Output::Value(SchemaNodes::of(&MemoryShown::SHAPE))),
        ]
    }

    /// The worker as the tools root asks it.
    pub fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for OverlayTools {
    type Sends = (ToSender<Called>, Tells<Path<Overlay>, Marked>);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let called = match call.tool.as_str() {
            MARK => match from_bytes::<Mark>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) {
                Ok(mark) => {
                    turn.to(path::<Overlay>(())).send(&Marked { text: mark.text })?;
                    Called::output(turn, to_bytes(&Done {})?)
                }
                Err(fault) => Called::Refused(format!("the input of `{MARK}`: {fault}")),
            },
            MEMORY => match from_bytes::<Memory>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) {
                Ok(memory) => Self::memory(&memory, turn)?,
                Err(fault) => Called::Refused(format!("the input of `{MEMORY}`: {fault}")),
            },
            other => Called::Refused(format!("the overlay has no tool named `{other}`")),
        };
        turn.reply(&called)?;

        Ok(())
    }
}

impl OverlayTools {
    /// Answers `memory`: the engine's figures as the step began, at most `most` guests and backlogs named.
    fn memory<S>(memory: &Memory, turn: &mut Turn<'_, Self, S>) -> Result<Called, Fault> {
        let EngineStats::Read(figures) = turn.engine_stats() else {
            return Ok(Called::Refused(format!("`{MEMORY}` runs where no engine's figures are read")));
        };
        let most = memory.most.map_or(LARGEST, |most| usize::try_from(most).unwrap_or(usize::MAX));

        Ok(Called::output(turn, to_bytes(&MemoryShown::of(&figures, most))?))
    }
}

impl MemoryShown {
    /// `figures` as a person reads them, naming at most `most` guests and as many backlogs.
    fn of(figures: &Figures, most: usize) -> Self {
        let process = match figures.process {
            Process::Resident { bytes } => format!("{} resident", bytes_shown(bytes)),
            Process::Heap { bytes } => format!("{} of script heap", bytes_shown(bytes)),
            Process::Unknown => String::from("unknown"),
        };
        let largest_guests = figures.largest_guests.iter().take(most).map(|guest| GuestShown {
            actor: named(&guest.actor),
            memory: bytes_shown(guest.bytes),
            actors: guest.actors,
        });
        let longest_backlogs = figures
            .longest_backlogs
            .iter()
            .take(most)
            .map(|backlog| BacklogShown { actor: named(&backlog.actor), unsent: against(backlog.unsent) });

        Self {
            process,
            blobs: against(figures.blobs),
            backlog: against(figures.backlog()),
            guests: bytes_shown(figures.guest_bytes),
            largest_guests: largest_guests.collect(),
            longest_backlogs: longest_backlogs.collect(),
        }
    }
}

/// `actor` as a person names it: its path, or its id in hex.
fn named(actor: &ActorName) -> String {
    match actor {
        ActorName::Path(path) => path.to_string(),
        ActorName::Unnamed(id) => format!("{:#018x}", id.bits()),
    }
}

/// Bytes held against their most, and the share of it: `12.0 MiB of 1.0 GiB (1%)`; the bytes alone against no most.
fn against(held: Against) -> String {
    if held.most == 0 {
        return bytes_shown(held.bytes);
    }
    let percent = held.bytes.saturating_mul(100) / held.most;

    format!("{} of {} ({percent}%)", bytes_shown(held.bytes), bytes_shown(held.most))
}

/// `bytes` in the largest power of 1,024 they reach, with one place: `412.3 MiB`; bytes below a KiB whole. The
/// band's GPU memory and upload rows show their values so too.
pub fn bytes_shown(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    // Bytes of memory, which an f64 holds closely enough to show with one place.
    #[allow(clippy::cast_precision_loss)]
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    loop {
        let reaches_next = value >= 1024.0;
        let has_next = unit + 1 < UNITS.len();
        let climbs = reaches_next && has_next;
        if !climbs {
            break;
        }
        value /= 1024.0;
        unit += 1;
    }

    format!("{value:.1} {}", UNITS[unit])
}
