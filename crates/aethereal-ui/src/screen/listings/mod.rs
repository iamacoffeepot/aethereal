//! What each widget listed as its screen last laid it out, kept to list again (ADR-0056: Draws are kept by widget key;
//! *settled by the library's draw-cost stage*): a widget that would draw what it drew lists its draws' places and its
//! hit again, and neither draws nor writes anything, as its draws stand kept by its key.
//!
//! A widget draws what it drew when all its listing reads is what it read. Its settings, its style, and what its markup
//! drags and drops are the tree's, and the tree's comparison tells of a node whose any of them changed ([`Relaid`]),
//! which lets go of what it kept. The rest is compared, to the bit: where it is drawn, its state, and how it stands with
//! its screen's input and its surface. Beside them its drawing reads only the fonts' metrics and the UI scale its
//! measures are taken at. The scale is where it is drawn. A font's metrics are read once, as they come or are refused,
//! and a drawing that met a font whose metrics had not come waited for them: a layout that waited sends no draws, and
//! keeps nothing of what it listed, so no listing kept was drawn without the metrics a fresh one would read.
//!
//! ```text
//! compare ──> Relaid: changed, redrawn, entered, left ──> what each widget kept is let go
//! lay out ──> each widget: kept, and reads what it read? ──> its places listed again, its draws stand
//!                                                    else ──> drawn anew, its draws compared and written, and kept
//! ```

#[cfg(test)]
mod tests;

use super::{Slot, Standing};
use crate::tree::{Flattened, Stamps, Was};
use crate::widget::Laid;
use core::mem;

/// What each widget listed as its screen last laid it out, in the tree order of the flattening numbered `generation`,
/// with what its listing read; and what is listed in the layout under way, in its flattening's order.
#[derive(Debug, Default)]
pub(super) struct Listings {
    by: Vec<Kept>,
    generation: u64,
    next: Vec<Kept>,
}

/// What one widget listed, if it listed anything: what its listing read beside its tree's node, what it lists, and the
/// build of the tree it was listed from.
#[derive(Debug, Default)]
enum Kept {
    /// Nothing: it was hidden, new, or let go.
    #[default]
    Unlisted,
    Listed {
        read: Read,
        listed: Listed,
        built: u64,
    },
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
#[derive(Debug)]
pub(super) struct Listed {
    pub now: Vec<Slot>,
    pub later: Vec<Slot>,
    pub blocks: bool,
}

/// What a widget lists, as a layout reads it: its places where it stands and over its anchor, and whether it blocks
/// the pointer from what is within it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Listing<'listed> {
    pub now: &'listed [Slot],
    pub later: &'listed [Slot],
    pub blocks: bool,
}

impl Listed {
    /// It, as a layout reads it.
    pub(super) fn listing(&self) -> Listing<'_> {
        Listing { now: &self.now, later: &self.later, blocks: self.blocks }
    }
}

impl Kept {
    /// What it lists, as a layout reads it: nothing, unlisted.
    fn listing(&self) -> Listing<'_> {
        match self {
            Self::Listed { listed, .. } => listed.listing(),
            Self::Unlisted => Listing { now: &[], later: &[], blocks: false },
        }
    }
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

/// How a widget's listing stood against what was kept of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Again {
    /// It read what it read, and lists what it listed.
    Kept,
    /// It was listed anew.
    Fresh,
}

impl Listings {
    /// Begins a layout's pass over `flattened`: what each widget kept follows it to where it stands, and what was kept
    /// in the order of any flattening but the last is let go.
    pub(super) fn begin(&mut self, flattened: &Flattened<'_>) {
        let following = self.generation + 1 == flattened.generation;
        let reshaped = !following;
        if reshaped {
            self.by.clear();
        }
        self.generation = flattened.generation;
        self.next.clear();
        self.next.resize_with(flattened.nodes.len(), Kept::default);
    }

    /// What the widget at `at` in the flattening, which stood at `was` in the last, and whose node's stamps are
    /// `stamps`, lists as its listing reads what `reading` finds, from the tree of the build `built`: what it listed
    /// last, when it kept that, its node has drawn as it did since, and it reads the same; otherwise what `list` lists.
    /// Either way kept for the next layout.
    pub(super) fn list(
        &mut self,
        (at, was, stamps): (usize, Was, Stamps),
        reading: Reading<'_>,
        built: u64,
        list: impl FnOnce() -> Listed,
    ) -> (Listing<'_>, Again) {
        let kept = match was {
            Was::At(old) if old < self.by.len() => mem::take(&mut self.by[old]),
            Was::At(_) | Was::New => Kept::Unlisted,
        };
        let (kept, again) = match kept {
            Kept::Listed { read, listed, built: then } if stamps.drawn <= then && read.is(reading) => {
                (Kept::Listed { read, listed, built }, Again::Kept)
            }
            Kept::Listed { .. } | Kept::Unlisted => {
                (Kept::Listed { read: Read::of(reading), listed: list(), built }, Again::Fresh)
            }
        };
        self.next[at] = kept;

        (self.next[at].listing(), again)
    }

    /// Ends the layout's pass: what a widget it did not list kept is let go, and, when it `waited` for a font's metrics
    /// and so sends no draws, everything is, as what it listed was never drawn.
    pub(super) fn end(&mut self, waited: bool) {
        mem::swap(&mut self.by, &mut self.next);
        self.next.clear();
        if waited {
            self.by.clear();
        }
    }

    /// Lets go of what the widget at `at` in the last flattening kept, as its draws were not made.
    pub(super) fn forget(&mut self, at: usize) {
        if let Some(kept) = self.by.get_mut(at) {
            *kept = Kept::Unlisted;
        }
    }
}
