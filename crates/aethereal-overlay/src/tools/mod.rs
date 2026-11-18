//! The overlay's tools (ADR-0053: Commands are tools), all served by one worker, [`OverlayTools`], which the overlay
//! spawns as it begins: `mark`, which pins a mark at this moment on the band's chart and in the scrollback; `memory`,
//! which prints the engine's memory against its limits with its largest guests and longest backlogs; `set_theme`
//! (`theme`), which tints the overlay's greys; and the chat's words (`words`), which the overlay offers with the same
//! worker only once the hub has shown it the chat. A person at the console and an agent through the hub call them alike,
//! and an agent's mark shows on the owner's screen.
//!
//! The worker reads each call's input and keeps no state of the overlay's: what needs it, a mark, a theme, or a change
//! of the chat, is mailed to the overlay, told when the answer does not hang on it and asked when it does, and the call
//! is answered once the overlay has answered.
//!
//! ```text
//! Call ──> OverlayTools ──> the input read ──> refused, or:
//!   mark            ──> Marked told to the overlay ──> Called: done
//!   memory          ──> the engine's figures ──> Called: MemoryShown
//!   set_theme       ──> Choice asked of the overlay ──> ThemesShown ──> Called: ThemesShown
//!   the chat's words ──> the owner's alone ──> ChatWord asked of the overlay ──> Worded ──> Called: done, or refused
//! ```

#[cfg(test)]
mod tests;
mod theme;
mod words;

pub use theme::{Choice, ChooseTheme, ThemesShown, chosen};
pub use words::{ChatWord, NewSession, Worded, chat_sheets};

use crate::overlay::Overlay;
use aethereal_actor::{
    Actor, ActorName, ActorRef, Against, Answer, Answered, Anyone, Anywhere, Beginning, ChildOf, Ending, EngineStats,
    Fault, Figures, Handles, Held, LARGEST, Process, ProtocolRef, Receives, Turn, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Caller, Done, Output, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;
use theme::{SET_THEME, theme_sheet};
use words::{ARCHIVE_SESSION, ArchiveSession, NEW_SESSION, STOP_SESSION, StopSession};

/// The tool that pins a mark.
const MARK: &str = "mark";

/// The tool that prints the engine's memory.
const MEMORY: &str = "memory";

/// The most bytes reading an input may take: a mark's text, or a session's name, is typed by hand.
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

/// What the worker keeps for its ask of the overlay: which call it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.answering")]
pub struct Answering {
    call: u64,
}

/// What the overlay's tools' worker is made from: nothing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.overlay_tools_config")]
pub struct OverlayToolsConfig;

/// The worker of every tool the overlay offers, spawned by the overlay as it begins: it answers `memory` itself, tells
/// the overlay a mark, and asks the overlay for a theme or a change of the chat, answering the call with what the
/// overlay answered.
#[derive(Bound)]
#[kind(name = "aether.overlay.overlay_tools")]
pub struct OverlayTools {
    /// The calls asked of the overlay and not yet answered, by number.
    calls: BTreeMap<u64, Held<Called>>,
    /// The number of the next call.
    next: u64,
}

/// What a call of one of the overlay's tools comes to, its input read: what the worker does for it.
enum Order {
    Mark(Mark),
    Memory(Memory),
    Theme(Choice),
    Word(ChatWord),
}

impl ChildOf for OverlayTools {
    type Parent = Anywhere;
}

impl Actor for OverlayTools {
    type Config = OverlayToolsConfig;
    type Key = ();
    type Handles = (Call, Answer<ThemesShown, Answering>, Answer<Worded, Answering>);
    type Depends = (Overlay,);
    type Spawns = ();

    fn begin(_: OverlayToolsConfig, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What the overlay offers as it begins: `mark`, `memory`, and `set_theme`, in every engine, trusted or not. The chat's
/// words, served by the same worker, are offered apart, once the hub shows the overlay the chat ([`chat_sheets`]).
impl Tool for OverlayTools {
    fn sheets() -> Vec<ToolSheet> {
        let mark = "Pins a mark at this moment on the dev overlay. The mark is a tick in the mark's ink on the overlay's frame \
                    chart at this time, sliding left with the columns for sixty seconds, and a line in the mark's ink in its \
                    console's scrollback. Answers once the overlay has been told.";
        let memory = format!(
            "Prints the engine's memory against its limits. It reads them as the step began: `process`, the \
             program's memory as its chassis reads it; `blobs`, the bytes the blob store holds against its most; \
             `backlog`, the most bytes any one actor holds unsent against its most; `guests`, every guest's memory \
             added up; and the guests with the most memory and the actors holding the most bytes unsent, each by \
             path, the most first, at most `most` of each ({LARGEST} when left out, and never more)."
        );

        vec![
            ToolSheet::of::<Mark>(MARK, mark, Output::Value(SchemaNodes::of(&Done::SHAPE))),
            ToolSheet::of::<Memory>(MEMORY, &memory, Output::Value(SchemaNodes::of(&MemoryShown::SHAPE))),
            theme_sheet(),
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// A call read: `memory` answered at once, a mark told to the overlay and answered done, and a theme or a chat's word
/// held and asked of the overlay, answered once the overlay answers. A call whose input is not its tool's, or a chat's
/// word a session calls, is refused.
impl Handles<Call> for OverlayTools {
    type From = Anyone;

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let input = turn.read(&call.input);
        let order = match call.tool.as_str() {
            MARK => read(MARK, input).map(Order::Mark),
            MEMORY => read(MEMORY, input).map(Order::Memory),
            SET_THEME => read(SET_THEME, input).map(|choose: ChooseTheme| Order::Theme(choose.choice())),
            NEW_SESSION => owned(call.caller)
                .and_then(|()| read(NEW_SESSION, input))
                .map(|new: NewSession| Order::Word(ChatWord::New { name: new.name })),
            STOP_SESSION => owned(call.caller)
                .and_then(|()| read(STOP_SESSION, input))
                .map(|stop: StopSession| Order::Word(ChatWord::Stop(stop.which()))),
            ARCHIVE_SESSION => owned(call.caller)
                .and_then(|()| read(ARCHIVE_SESSION, input))
                .map(|archive: ArchiveSession| Order::Word(ChatWord::Archive(archive.which()))),
            other => Err(format!("the overlay has no tool named `{other}`")),
        };

        match order {
            Ok(Order::Mark(mark)) => {
                turn.to(path::<Overlay>(())).send(&Marked { text: mark.text })?;
                let called = Called::output(turn, to_bytes(&Done {})?);
                turn.reply(&called)?;
            }
            Ok(Order::Memory(memory)) => {
                let called = Self::memory(&memory, turn)?;
                turn.reply(&called)?;
            }
            Ok(Order::Theme(choice)) => {
                let answering = self.hold(turn)?;
                turn.to(path::<Overlay>(())).ask(&choice, &answering)?;
            }
            Ok(Order::Word(word)) => {
                let answering = self.hold(turn)?;
                turn.to(path::<Overlay>(())).ask(&word, &answering)?;
            }
            Err(why) => turn.reply(&Called::Refused(why))?,
        }

        Ok(())
    }
}

/// The overlay chose the theme, and answered the themes and the one in use; or no overlay answered.
impl Receives<Answer<ThemesShown, Answering>> for OverlayTools {
    fn handle(
        &mut self,
        (answered, answering): (Answered<ThemesShown>, Answering),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let called = match answered {
            Answered::Yes(shown) => Called::output(turn, to_bytes(&shown)?),
            Answered::No(why) => Called::Refused(format!("the overlay did not answer: {why:?}")),
        };

        self.answer(answering, &called, turn)
    }
}

/// The overlay told the link the chat a word came to, or said why it told nothing; or no overlay answered.
impl Receives<Answer<Worded, Answering>> for OverlayTools {
    fn handle(
        &mut self,
        (answered, answering): (Answered<Worded>, Answering),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let called = match answered {
            Answered::Yes(Worded::Told) => Called::output(turn, to_bytes(&Done {})?),
            Answered::Yes(Worded::Refused(why)) => Called::Refused(why),
            Answered::No(why) => Called::Refused(format!("the overlay did not answer: {why:?}")),
        };

        self.answer(answering, &called, turn)
    }
}

impl OverlayTools {
    /// Holds the call `turn` handles, to answer once the overlay has, and answers what to keep for the ask.
    fn hold(&mut self, turn: &mut Turn<'_, Self>) -> Result<Answering, Fault> {
        let answering = Answering { call: self.next };
        self.calls.insert(answering.call, turn.hold()?);
        self.next += 1;

        Ok(answering)
    }

    /// Answers the call `answering` names with `called`: nothing when it is held no longer.
    fn answer(
        &mut self,
        Answering { call }: Answering,
        called: &Called,
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&call) else {
            return Ok(());
        };
        turn.answer(held, called)?;

        Ok(())
    }

    /// Answers `memory`: the engine's figures as the step began, at most `most` guests and backlogs named.
    fn memory(memory: &Memory, turn: &mut Turn<'_, Self>) -> Result<Called, Fault> {
        let EngineStats::Read(figures) = turn.engine_stats() else {
            return Ok(Called::Refused(format!("`{MEMORY}` runs where no engine's figures are read")));
        };
        let most = memory.most.map_or(LARGEST, |most| usize::try_from(most).unwrap_or(usize::MAX));

        Ok(Called::output(turn, to_bytes(&MemoryShown::of(&figures, most))?))
    }
}

/// The input `bytes` of the tool `tool` as a `K`, or why they are not one.
fn read<K: Plain>(tool: &str, bytes: &[u8]) -> Result<K, String> {
    from_bytes::<K>(bytes, &mut Budget::new(INPUT_BYTES)).map_err(|fault| format!("the input of `{tool}`: {fault}"))
}

/// Whether `caller` may call the chat's words: the owner may, and a session is refused, as the sessions are the owner's.
fn owned(caller: Caller) -> Result<(), String> {
    match caller {
        Caller::Owner => Ok(()),
        Caller::Session { session } => Err(format!("the chat's words are the owner's, not session {session}'s")),
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
pub fn named(actor: &ActorName) -> String {
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
