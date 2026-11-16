//! The engine's log in the console (ADR-0053: Logs in the console): each entry the overlay reads printed as a row of
//! its step, its level, its actor, and its text; and the warnings and errors heard while the console is closed,
//! counted for the band.

use super::scrollback::Piece;
use crate::colour::Ink;
use crate::layout::GUTTER_CHARACTERS;
use aethereal_actor::{ActorName, Entry, Level};
use aethereal_data::Plain;

/// How wide a level is printed: as wide as `error`.
const LEVEL_WIDTH: usize = 5;

/// How wide a step is printed, right aligned, so the levels after it line up.
const STEP_WIDTH: usize = 8;

/// What the gutter calls the count of warnings and errors, the counts written flush right in the rest of the gutter.
const ALERTS_LABEL: &str = "alerts";

/// One entry of the engine's recent log, as the console prints it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    /// The step it was folded in.
    pub step: u64,
    pub level: Level,
    /// The path of the actor that made it, or its id in hex.
    pub actor: String,
    pub text: String,
}

impl Logged {
    /// `entry`, made by `actor` and folded in the step `step`: a line at its own level, a fault an error, and a drop or
    /// mail that did not read a warning, as the logs tool weighs them.
    pub fn of(step: u64, actor: &ActorName, entry: &Entry) -> Self {
        let (level, text) = match entry {
            Entry::Line { level, text } => (*level, text.clone()),
            Entry::Failed(fault) => (Level::Error, format!("ended on a fault of kind {:?}", fault.kind())),
            Entry::EndFailed(fault) => (Level::Error, format!("its closing turn faulted, kind {:?}", fault.kind())),
            Entry::JobFaulted(fault) => (Level::Error, format!("a job faulted, kind {:?}", fault.kind())),
            Entry::Dropped(name) => (Level::Warn, format!("a {name} was dropped on its way")),
            Entry::Unbrought(name) => (Level::Warn, format!("a {name} could not be brought as mail")),
            Entry::Unread(reason) => (Level::Warn, format!("mail did not read: {reason:?}")),
        };
        let actor = match actor {
            ActorName::Path(path) => path.to_string(),
            ActorName::Unnamed(id) => format!("{:#018x}", id.bits()),
        };

        Self { step, level, actor, text }
    }

    /// The row's pieces: the step and the actor dim, the level padded, and the level and the text in its ink.
    pub fn pieces(&self) -> Vec<Piece> {
        let ink = ink_of(self.level);
        let level = format!("{:<LEVEL_WIDTH$}", name_of(self.level));

        vec![
            Piece::new(&format!("{:>STEP_WIDTH$}  ", self.step), Ink::Dim),
            Piece::new(&level, ink),
            Piece::new(&format!("  {}  ", self.actor), Ink::Dim),
            Piece::new(&self.text, ink),
        ]
    }
}

/// The ink a line of `level` is printed in: info and debug dim, a warning the warning's, and an error the error's.
const fn ink_of(level: Level) -> Ink {
    match level {
        Level::Debug | Level::Info => Ink::Dim,
        Level::Warn => Ink::Warn,
        Level::Error => Ink::Error,
    }
}

/// `level` as a person names it.
const fn name_of(level: Level) -> &'static str {
    match level {
        Level::Debug => "debug",
        Level::Info => "info",
        Level::Warn => "warn",
        Level::Error => "error",
    }
}

/// The warnings and errors the engine logged since the console was last opened, shown in the gutter beside the frame
/// chart.
#[derive(Plain, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Alerts {
    warnings: u64,
    errors: u64,
}

impl Alerts {
    /// Counts an entry of `level`: a warning or an error; nothing else.
    pub const fn count(&mut self, level: Level) {
        match level {
            Level::Warn => self.warnings += 1,
            Level::Error => self.errors += 1,
            Level::Debug | Level::Info => {}
        }
    }

    /// Forgets what was counted: the console was opened, and shows them.
    pub const fn clear(&mut self) {
        *self = Self { warnings: 0, errors: 0 };
    }

    /// The gutter's line of the counts: `alerts` dim, then each count that is not zero flush right as the rows'
    /// values are, `! N` in the warning's ink and `× N` in the error's, as IBM Plex Mono has no `⚠` or `✕`; and
    /// `no alerts` dim when both are zero, so the line is never blank; and `after` at its end, the counts padded short
    /// by as much, so the line stays the gutter's width and the gutter, sized to its lines, as wide as every other.
    pub fn pieces(self, after: Vec<Piece>) -> Vec<Piece> {
        let trailing: usize = after.iter().map(|piece| piece.text.chars().count()).sum();
        let mut counts = Vec::new();
        if self.warnings > 0 {
            counts.push(Piece::new(&format!("! {}", self.warnings), Ink::Warn));
        }
        let both = self.warnings > 0 && self.errors > 0;
        if both {
            counts.push(Piece::new("  ", Ink::Dim));
        }
        if self.errors > 0 {
            counts.push(Piece::new(&format!("× {}", self.errors), Ink::Error));
        }
        if counts.is_empty() {
            return [vec![Piece::new("no alerts", Ink::Dim)], after].concat();
        }
        let written: usize = counts.iter().map(|piece| piece.text.chars().count()).sum();
        let pad = GUTTER_CHARACTERS.saturating_sub(ALERTS_LABEL.len() + written + trailing);
        let pieces = vec![Piece::new(ALERTS_LABEL, Ink::Dim), Piece::new(&" ".repeat(pad), Ink::Dim)];

        [pieces, counts, after].concat()
    }
}
