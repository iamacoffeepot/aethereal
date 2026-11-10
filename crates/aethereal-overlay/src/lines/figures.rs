//! The gutter beside the lines' rows (ADR-0053: The three stops): each line's label in stone and its newest value in
//! bone, one line of text a row, the values set flush right so their digits line up in the mono face; counts written
//! in thousands and millions past ten thousand, and bytes in KiB, MiB, and GiB as the `memory` tool shows them.

use super::{FrameLines, Line};
use crate::colour::{BONE, STONE};
use crate::layout::GUTTER_CHARACTERS;
use crate::tools::bytes_shown;
use aethereal_render::text::Run;

/// How many characters a label is given, its value the rest of the gutter.
const LABEL_CHARACTERS: usize = 10;

/// How many characters a value is given, flush right.
const VALUE_CHARACTERS: usize = GUTTER_CHARACTERS - LABEL_CHARACTERS;

/// The units a count is written in past ten thousand, each a thousand of the one before.
const COUNT_UNITS: [&str; 3] = ["k", "M", "G"];

impl Line {
    /// What the gutter calls the line.
    const fn label(self) -> &'static str {
        match self {
            Self::Draws => "draws",
            Self::Triangles => "triangles",
            Self::Uploaded => "uploaded",
            Self::Memory => "gpu memory",
        }
    }

    /// `value` written as the line's unit is.
    fn written(self, value: u64) -> String {
        match self {
            Self::Draws | Self::Triangles => count(value),
            Self::Uploaded | Self::Memory => bytes_shown(value),
        }
    }
}

impl FrameLines {
    /// The runs of the gutter's text: a line of it for each row, its label and its newest value.
    pub fn runs(&self) -> Vec<Run> {
        let latest = self.latest();
        let mut runs = Vec::with_capacity(Line::ALL.len() * 2);
        for (index, line) in Line::ALL.into_iter().enumerate() {
            let parted = if index == 0 {
                ""
            } else {
                "\n"
            };
            let written = line.written(latest.of_line(line));
            runs.push(Run::new(&format!("{parted}{:<LABEL_CHARACTERS$}", line.label()), STONE.opaque()));
            runs.push(Run::new(&format!("{written:>VALUE_CHARACTERS$}"), BONE.opaque()));
        }

        runs
    }
}

/// A count: whole up to ten thousand, then in thousands, millions, and billions, with one place: `9999`, `12.3k`,
/// `4.6M`.
pub(super) fn count(value: u64) -> String {
    if value < 10_000 {
        return value.to_string();
    }
    // A count far under an f64's exact range, shown with one place.
    #[allow(clippy::cast_precision_loss)]
    let mut amount = value as f64 / 1_000.0;
    let mut unit = 0;
    loop {
        let reaches_next = amount >= 1_000.0;
        let has_next = unit + 1 < COUNT_UNITS.len();
        let climbs = reaches_next && has_next;
        if !climbs {
            break;
        }
        amount /= 1_000.0;
        unit += 1;
    }

    format!("{amount:.1}{}", COUNT_UNITS[unit])
}
