//! The meters' text (ADR-0053: Memory): one line a meter's row, in the gutter at the right column's right, as the left
//! column's rows' are: its label in stone in the gutter's first characters, and its value set flush right in bone at
//! the gutter's end, the band's right padding, so the labels start at one edge and the digits end at another in the
//! mono face; bytes in KiB, MiB, and GiB as the `memory` tool shows them, and a value that is no figure, `unknown`,
//! `none`, `no guests`, in stone. A meter with a limit writes the bytes it holds alone, as its bar shows their share of
//! the limit, so every value fits the gutter and no line is cut as its value changes.

use super::{EngineMemory, Held, MemoryFigures, Meter, ProcessMemory};
use crate::colour::{BONE, STONE, Swatch};
use crate::layout::{GUTTER_CHARACTERS, GUTTER_LABEL_CHARACTERS};
use crate::tools::bytes_shown;
use aethereal_render::text::Run;

/// How many characters a value is given, flush right: the gutter's after its label's.
const VALUE_CHARACTERS: usize = GUTTER_CHARACTERS - GUTTER_LABEL_CHARACTERS;

impl Meter {
    /// What the right column calls the meter, its memory `memory`: the process's is `heap` where the chassis reads the
    /// script heap a browser reports.
    const fn label(self, memory: &EngineMemory) -> &'static str {
        match (self, memory) {
            (Self::Process, EngineMemory::Read(MemoryFigures { process: ProcessMemory::Heap { .. }, .. })) => "heap",
            (Self::Process, _) => "process",
            (Self::Blobs, _) => "blobs",
            (Self::Backlog, _) => "backlog",
            (Self::Guests, _) => "guests",
            (Self::Gpu, _) => "gpu",
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

    /// The bytes held, `412.0 MiB`, as the bar shows their share of the most; and `none` for nothing against nothing.
    fn held(held: Held) -> Self {
        let nothing = held.bytes == 0 && held.most == 0;
        if nothing {
            return Self::none("none");
        }

        Self::figure(bytes_shown(held.bytes))
    }
}

impl EngineMemory {
    /// The runs of the line of `meter`'s row, with the GPU's memory `gpu_bytes`: as wide as the gutter, its label at
    /// its left and its value ending at its right.
    pub fn row_runs(&self, meter: Meter, gpu_bytes: u64) -> Vec<Run> {
        let value = self.value(meter, gpu_bytes);
        let label = format!("{:<GUTTER_LABEL_CHARACTERS$}", meter.label(self));
        let value_text = format!("{:>VALUE_CHARACTERS$}", value.text);

        vec![Run::new(&label, STONE.opaque()), Run::new(&value_text, value.swatch.opaque())]
    }

    /// The value of `meter`, with the GPU's memory `gpu_bytes`.
    fn value(&self, meter: Meter, gpu_bytes: u64) -> Value {
        let Self::Read(figures) = self else {
            return match meter {
                Meter::Gpu => Value::figure(bytes_shown(gpu_bytes)),
                Meter::Process | Meter::Blobs | Meter::Backlog | Meter::Guests => Value::none("unread"),
            };
        };
        let MemoryFigures { process, blobs, backlog, guest_bytes } = figures;
        match meter {
            Meter::Process => match process {
                ProcessMemory::Resident { bytes } | ProcessMemory::Heap { bytes } => Value::figure(bytes_shown(*bytes)),
                ProcessMemory::Unknown => Value::none("unknown"),
            },
            Meter::Blobs => Value::held(*blobs),
            Meter::Backlog => Value::held(*backlog),
            Meter::Guests => match guest_bytes {
                0 => Value::none("no guests"),
                bytes => Value::figure(bytes_shown(*bytes)),
            },
            Meter::Gpu => Value::figure(bytes_shown(gpu_bytes)),
        }
    }
}
