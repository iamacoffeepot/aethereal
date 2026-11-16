//! A screen's selection (ADR-0056: Copy and paste): the text its reader selected, held as its two ends, each the key
//! path of a text that reads and a byte offset into its text, or the start or the end of what its scope holds; kept in
//! the screen's table beside its focus, so it outlives the rows of a windowed scroll that are not built, and moves onto
//! new code with the screen.

use crate::tree::KeyPath;
use aethereal_data::Plain;
use aethereal_math::Vec2;

/// What a screen's reader has selected: nothing, or a selection made.
#[derive(Debug, Clone, PartialEq, Default, Plain)]
pub enum Selection {
    #[default]
    Nothing,
    Made(Made),
}

/// A selection made: the widget it runs within, `scope`, the scroll the text it began in stands in or that text
/// itself; where it began, `anchor`, and where it ends, `caret`; and whether the press that makes it is still held.
#[derive(Debug, Clone, PartialEq, Plain)]
pub struct Made {
    pub scope: KeyPath,
    pub anchor: Mark,
    pub caret: Mark,
    pub drag: Dragging,
}

/// One end of a selection.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Mark {
    /// The start of what its scope holds, before its first text.
    Start,
    /// The byte `offset` into the text of the widget at `path`.
    At { path: KeyPath, offset: u32 },
    /// The end of what its scope holds, after its last text: what Select All runs to, so it holds what the scope holds
    /// when it is copied.
    End,
}

/// Whether the press that makes a selection is held.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub enum Dragging {
    /// It is, the pointer last at `at` in the screen's pixels: a move takes the caret there, and the pointer past the
    /// scope's view slides it on at each `Prepare`.
    Held { at: Vec2 },
    /// It came up.
    Let,
}

impl Selection {
    /// Whether it stands with some text in it, or a drag is making it: what a screen that moves its focus of its own
    /// accord leaves be, so the selection is not left behind.
    #[must_use]
    pub fn selects(&self) -> bool {
        match self {
            Self::Nothing => false,
            Self::Made(made) => {
                let held = matches!(made.drag, Dragging::Held { .. });
                held || made.anchor != made.caret
            }
        }
    }

    /// Whether it still holds in a tree whose node paths are `paths`: nothing, or a selection whose scope and each end
    /// at a text are there.
    pub(crate) fn holds(&self, paths: impl Fn(&KeyPath) -> bool) -> bool {
        let Self::Made(made) = self else {
            return true;
        };
        let scoped = paths(&made.scope);
        let marked = made.marked().all(&paths);

        scoped && marked
    }
}

impl Made {
    /// The paths of the texts its ends stand at.
    pub(crate) fn marked(&self) -> impl Iterator<Item = &KeyPath> {
        [&self.anchor, &self.caret].into_iter().filter_map(|mark| match mark {
            Mark::At { path, .. } => Some(path),
            Mark::Start | Mark::End => None,
        })
    }
}
