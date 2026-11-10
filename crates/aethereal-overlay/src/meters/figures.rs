//! The meters' text (ADR-0053: Memory): one line a meter's row, as wide as the right column, its label in stone at the
//! left, the room of its graph or bar, and its value set flush right in bone at the column's right edge, the band's
//! right padding, so the digits line up in the mono face, bytes in KiB, MiB, and GiB as the `memory` tool shows them;
//! a meter with a limit `412.0 MiB / 8.0 GiB`; a value that is no figure, `unknown`, `none`, `no guests`, in stone.
//! The backlog's line names the actor holding the most just before its value, in stone, cut at its front to at most
//! [`MOST_ACTOR_CHARACTERS`] and to the room the column has left beside the fewest characters its bar keeps, or left
//! out where too little is. What a line writes right of its graph, its actor and its value, is its tail, which the
//! graph ends a character before.

use super::{Backlog, EngineMemory, Held, MemoryFigures, Meter, ProcessMemory};
use crate::colour::{BONE, STONE, Swatch};
use crate::layout::{METER_GAP_CHARACTERS, METER_LABEL_CHARACTERS};
use crate::tools::bytes_shown;
use aethereal_render::text::Run;

/// How many characters the widest value takes: `1023.9 MiB / 1023.9 GiB`.
#[cfg(test)]
const VALUE_CHARACTERS: usize = 23;

/// The fewest characters a meter's graph or bar keeps on a narrow window.
#[cfg(test)]
const FEWEST_GRAPH_CHARACTERS: usize = 8;

/// The fewest characters a meter's row takes: its label, its fewest graph and the space after it, and the widest value.
#[cfg(test)]
pub const ROW_CHARACTERS: usize =
    METER_LABEL_CHARACTERS + FEWEST_GRAPH_CHARACTERS + METER_GAP_CHARACTERS + VALUE_CHARACTERS;

/// The fewest characters the backlog's bar keeps beside its actor: the actor is cut, or left out, to leave them.
const FEWEST_BAR_BESIDE_ACTOR_CHARACTERS: usize = 16;

/// The most characters of the backlog's actor shown, so a long path does not take its bar's room.
const MOST_ACTOR_CHARACTERS: usize = 24;

/// The space between the backlog's actor and its value.
const ACTOR_GAP: &str = "  ";

/// The fewest characters of an actor's path worth showing, its ellipsis among them.
const FEWEST_ACTOR_CHARACTERS: usize = 8;

/// The mark that stands for what is cut from a text.
const ELLIPSIS: char = '…';

impl Meter {
    /// What the right column calls the meter.
    const fn label(self) -> &'static str {
        match self {
            Self::Process => "process",
            Self::Blobs => "blobs",
            Self::Backlog => "backlog",
            Self::Guests => "guests",
            Self::Gpu => "gpu",
        }
    }
}

/// A meter's value as it is written, and its colour: bone for a figure, stone for what is none.
struct Value {
    text: String,
    swatch: Swatch,
}

impl Value {
    fn figure(text: String) -> Self {
        Self { text, swatch: BONE }
    }

    fn none(text: &str) -> Self {
        Self { text: String::from(text), swatch: STONE }
    }

    /// Bytes against their most: `412.0 MiB / 8.0 GiB`; the bytes alone against no most, and `none` for nothing
    /// against nothing.
    fn held(held: Held) -> Self {
        let nothing = held.bytes == 0 && held.most == 0;
        if nothing {
            return Self::none("none");
        }
        if held.most == 0 {
            return Self::figure(bytes_shown(held.bytes));
        }

        Self::figure(format!("{} / {}", bytes_shown(held.bytes), bytes_shown(held.most)))
    }
}

impl EngineMemory {
    /// The runs of the line of `meter`'s row, with the GPU's memory `gpu_bytes`, on a right column `columns`
    /// characters wide: as wide as the column, its tail ending at the column's right edge.
    pub fn row_runs(&self, meter: Meter, gpu_bytes: u64, columns: usize) -> Vec<Run> {
        let (value, actor) = self.tail(meter, gpu_bytes, columns);
        let pad = columns.saturating_sub(actor.chars().count() + value.text.chars().count());
        let label = format!("{:<pad$}{actor}", meter.label());

        vec![Run::new(&label, STONE.opaque()), Run::new(&value.text, value.swatch.opaque())]
    }

    /// How many characters `meter`'s line writes right of its graph, with the GPU's memory `gpu_bytes`, on a right
    /// column `columns` characters wide: its actor, where it shows one, and its value.
    pub fn tail_characters(&self, meter: Meter, gpu_bytes: u64, columns: usize) -> usize {
        let (value, actor) = self.tail(meter, gpu_bytes, columns);

        actor.chars().count() + value.text.chars().count()
    }

    /// What `meter`'s line writes right of its graph: its value, and before it the actor it names and the gap after
    /// it, cut to the room left beside the fewest characters its bar keeps, or nothing.
    fn tail(&self, meter: Meter, gpu_bytes: u64, columns: usize) -> (Value, String) {
        let (value, actor) = self.value(meter, gpu_bytes);
        let kept = METER_LABEL_CHARACTERS
            + FEWEST_BAR_BESIDE_ACTOR_CHARACTERS
            + METER_GAP_CHARACTERS
            + ACTOR_GAP.len()
            + value.text.chars().count();
        let room = columns.saturating_sub(kept).min(MOST_ACTOR_CHARACTERS);
        let fits = room >= FEWEST_ACTOR_CHARACTERS;
        let names = !actor.is_empty();
        let shows_actor = fits && names;
        let actor_part = if shows_actor {
            format!("{}{ACTOR_GAP}", cut_front(actor, room))
        } else {
            String::new()
        };

        (value, actor_part)
    }

    /// The value of `meter`, and the actor its line names after it: none but the backlog's.
    fn value(&self, meter: Meter, gpu_bytes: u64) -> (Value, &str) {
        let Self::Read(figures) = self else {
            let value = match meter {
                Meter::Gpu => Value::figure(bytes_shown(gpu_bytes)),
                Meter::Process | Meter::Blobs | Meter::Backlog | Meter::Guests => Value::none("unread"),
            };
            return (value, "");
        };
        let MemoryFigures { process, blobs, backlog, guest_bytes } = figures;
        match meter {
            Meter::Process => {
                let value = match process {
                    ProcessMemory::Resident { bytes } => Value::figure(bytes_shown(*bytes)),
                    ProcessMemory::Heap { bytes } => Value::figure(format!("{} heap", bytes_shown(*bytes))),
                    ProcessMemory::Unknown => Value::none("unknown"),
                };
                (value, "")
            }
            Meter::Blobs => (Value::held(*blobs), ""),
            Meter::Backlog => match backlog {
                Backlog::Empty => (Value::none("none"), ""),
                Backlog::Held { unsent, actor } => (Value::held(*unsent), actor.as_str()),
            },
            Meter::Guests => {
                let value = match guest_bytes {
                    0 => Value::none("no guests"),
                    bytes => Value::figure(bytes_shown(*bytes)),
                };
                (value, "")
            }
            Meter::Gpu => (Value::figure(bytes_shown(gpu_bytes)), ""),
        }
    }
}

/// `text` in at most `room` characters: whole when it fits, or its end after an ellipsis, as a path's end names it.
pub fn cut_front(text: &str, room: usize) -> String {
    let count = text.chars().count();
    if count <= room {
        return String::from(text);
    }
    let kept: String = text.chars().skip(count - room.saturating_sub(1)).collect();

    format!("{ELLIPSIS}{kept}")
}
