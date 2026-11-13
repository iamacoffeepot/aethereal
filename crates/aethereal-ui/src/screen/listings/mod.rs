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
use std::collections::BTreeMap;

/// What each widget listed as its screen last laid it out, by its key path, with what its listing read; and the number
/// of the layout listing now, counted from the screen's first.
#[derive(Debug, Default)]
pub(super) struct Listings {
    by: BTreeMap<KeyPath, Kept>,
    pass: u64,
}

/// What one widget listed: its places in its screen's list, those listed where it stands and those listed over its
/// anchor, and whether it blocks the pointer from what is within it; what its listing read beside its tree's node; and
/// the number of the layout that last listed it.
#[derive(Debug)]
struct Kept {
    read: Read,
    now: Vec<Slot>,
    later: Vec<Slot>,
    blocks: bool,
    pass: u64,
}

/// What a widget's listing reads beside its tree's node: where it is drawn, its state's bytes, how it stands with its
/// screen's input, and whether its surface is live.
#[derive(Debug)]
struct Read {
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

impl Kept {
    /// What `listed` lists, read as `reading` found it, in the layout numbered `pass`.
    fn of(reading: Reading<'_>, listed: &Listed, pass: u64) -> Self {
        let Listed { now, later, blocks } = listed;

        Self { read: Read::of(reading), now: now.clone(), later: later.clone(), blocks: *blocks, pass }
    }

    /// What it lists, again.
    fn again(&self) -> Listed {
        Listed { now: self.now.clone(), later: self.later.clone(), blocks: self.blocks }
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
    /// Begins a layout's pass over what was kept, numbered after the last.
    pub(super) fn begin(&mut self) {
        self.pass += 1;
    }

    /// What the widget at `path` lists as its listing reads what `reading` finds: what it listed last, when it kept that
    /// and read the same; otherwise what `list` lists. Either way kept for the next layout.
    pub(super) fn list(&mut self, path: &KeyPath, reading: Reading<'_>, list: impl FnOnce() -> Listed) -> Listed {
        let pass = self.pass;
        match self.by.get_mut(path) {
            Some(kept) if kept.read.is(reading) => {
                kept.pass = pass;
                kept.again()
            }
            Some(kept) => {
                let listed = list();
                *kept = Kept::of(reading, &listed, pass);
                listed
            }
            None => {
                let listed = list();
                self.by.insert(path.clone(), Kept::of(reading, &listed, pass));
                listed
            }
        }
    }

    /// Ends the layout's pass: what a widget it did not list kept is let go, and, when it `waited` for a font's metrics
    /// and so sends no draws, everything is, as what it listed was never drawn.
    pub(super) fn end(&mut self, waited: bool) {
        let pass = self.pass;
        if waited {
            self.by.clear();
            return;
        }
        self.by.retain(|_, kept| kept.pass == pass);
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
