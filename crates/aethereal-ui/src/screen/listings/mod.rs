//! What each widget listed as its screen last laid it out, kept to list again (ADR-0056: Draws are kept by widget key;
//! *settled by the library's draw-cost stage*): a widget that would draw what it drew lists its draws' places and its
//! hit again, and neither draws nor writes anything, as its draws stand kept by its key.
//!
//! A widget draws what it drew when all its listing reads is what it read. Its settings, its style, and what its markup
//! drags and drops are the tree's, and the tree's comparison tells of a node whose any of them changed ([`Relaid`]),
//! which lets go of what it kept. The rest is compared, to the bit: where it is drawn, its state, and how it stands with
//! its screen's input and its surface. Beside them its drawing reads only the fonts' metrics and the UI scale its
//! measures are taken at: the scale is where it is drawn, and a font's metrics coming or refused lets go of everything
//! kept, as does a layout that met a font whose metrics had not come, whose draws are not sent.
//!
//! ```text
//! compare ──> Relaid: changed, redrawn, entered, left ──> what each widget kept is let go
//! lay out ──> each widget: kept, and reads what it read? ──> its places listed again, its draws stand
//!                                                    else ──> drawn anew, its draws compared and written, and kept
//! ```

#[cfg(test)]
mod tests;

use super::{Slot, Standing};
use crate::tree::{KeyPath, Relaid, Segment};
use crate::widget::Laid;
use core::mem;
use std::collections::BTreeMap;

/// What each widget listed as its screen last laid it out, by its key path, with what its listing read.
#[derive(Debug, Default)]
pub(super) struct Listings {
    by: BTreeMap<KeyPath, Kept>,
}

/// What one widget listed: its places in its screen's list, those listed where it stands and those listed over its
/// anchor, and whether it blocks the pointer from what is within it; and what its listing read beside its tree's node.
#[derive(Debug)]
pub(super) struct Kept {
    read: Read,
    now: Vec<Slot>,
    later: Vec<Slot>,
    blocks: bool,
}

/// What a widget's listing reads beside its tree's node: where it is drawn, its state's bytes, how it stands with its
/// screen's input, and whether its surface is live.
#[derive(Debug)]
pub(super) struct Read {
    laid: Laid,
    state: Vec<u8>,
    standing: Standing,
    live: bool,
}

/// What a widget's listing reads beside its tree's node, as a layout finds it: where it is drawn, its state's bytes as
/// its screen's table keeps them, how it stands with its screen's input, and whether its surface is live.
#[derive(Debug, Clone, Copy)]
pub(super) struct Reading<'state> {
    pub laid: Laid,
    pub state: &'state [u8],
    pub standing: Standing,
    pub live: bool,
}

/// What a widget lists: its places in its screen's list, those listed where it stands and those listed over its anchor,
/// and whether it blocks the pointer from what is within it.
pub(super) struct Listed {
    pub now: Vec<Slot>,
    pub later: Vec<Slot>,
    pub blocks: bool,
}

impl Read {
    /// What a listing reads as `reading` finds it, kept.
    fn of(reading: Reading<'_>) -> Self {
        let Reading { laid, state, standing, live } = reading;

        Self { laid, state: state.to_vec(), standing, live }
    }

    /// Whether it is what a listing reads as `reading` finds it: where it is drawn compared to the bit, so a widget is
    /// never drawn as it was at a place a fresh drawing would write otherwise.
    fn is(&self, reading: Reading<'_>) -> bool {
        let placed = bits(&self.laid) == bits(&reading.laid);
        let stated = self.state == reading.state;
        let stood = self.standing == reading.standing;
        let surfaced = self.live == reading.live;

        placed && stated && stood && surfaced
    }
}

/// The bits of each figure of `laid`.
fn bits(laid: &Laid) -> [u32; 15] {
    let Laid { rect, content, clip, extent, scale } = *laid;

    [
        rect.min.x,
        rect.min.y,
        rect.max.x,
        rect.max.y,
        content.min.x,
        content.min.y,
        content.max.x,
        content.max.y,
        clip.min.x,
        clip.min.y,
        clip.max.x,
        clip.max.y,
        extent.x,
        extent.y,
        scale,
    ]
    .map(f32::to_bits)
}

impl Listings {
    /// A pass of a layout over what was kept: what each widget kept is taken out as it lists again.
    pub(super) fn relist(&mut self) -> Relisting {
        Relisting { was: mem::take(&mut self.by), now: BTreeMap::new() }
    }

    /// Keeps what `relisting`, a layout whose draws are sent, listed, for the next. What a layout whose draws are not
    /// sent listed is never kept, and what it took out is let go.
    pub(super) fn keep(&mut self, relisting: Relisting) {
        self.by = relisting.now;
    }

    /// Lets go of what the widget at `path` kept, as its draws were not made.
    pub(super) fn forget(&mut self, path: &KeyPath) {
        self.by.remove(path);
    }

    /// Lets go of everything kept, as a font's metrics came or were refused, which any widget's drawing may read.
    pub(super) fn clear(&mut self) {
        self.by.clear();
    }

    /// Lets go of what the widget at `path`, and each within it, kept.
    fn within(&mut self, path: &[Segment]) {
        self.by.retain(|kept, _| !kept.segments().starts_with(path));
    }
}

/// A layout's pass over what was kept: what the last listed, taken out as each widget lists again, and what this one
/// listed.
pub(super) struct Relisting {
    was: BTreeMap<KeyPath, Kept>,
    now: BTreeMap<KeyPath, Kept>,
}

impl Relisting {
    /// What the widget at `path` lists as its listing reads what `reading` finds: what it listed last, when it kept that and read the
    /// same; otherwise what `list` lists. Either way kept for the next.
    pub(super) fn list(&mut self, path: &KeyPath, reading: Reading<'_>, list: impl FnOnce() -> Listed) -> Listed {
        let kept = self.was.remove_entry(path);
        let same = kept.as_ref().is_some_and(|(_, kept)| kept.read.is(reading));
        match kept {
            Some((key, kept)) if same => {
                let again = Listed { now: kept.now.clone(), later: kept.later.clone(), blocks: kept.blocks };
                self.now.insert(key, kept);
                again
            }
            Some(_) | None => {
                let listed = list();
                let read = Read::of(reading);
                let kept = Kept { read, now: listed.now.clone(), later: listed.later.clone(), blocks: listed.blocks };
                self.now.insert(path.clone(), kept);
                listed
            }
        }
    }
}

/// A widget whose node changed in any way lets go of what it kept; one that left the tree takes what it kept with it,
/// with all within it; and one new to the tree takes nothing kept at or within its path, which may be left from a tree
/// before the last, when a tree between was never laid out.
impl Relaid for Listings {
    fn changed(&mut self, path: &[Segment]) {
        self.by.remove(path);
    }

    fn redrawn(&mut self, path: &[Segment]) {
        self.by.remove(path);
    }

    fn entered(&mut self, path: &[Segment]) {
        self.within(path);
    }

    fn left(&mut self, path: &[Segment]) {
        self.within(path);
    }
}
