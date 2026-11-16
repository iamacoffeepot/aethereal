//! What a screen's last layout found of each scroll ([`Window`]): its view's length, the children it built, each with
//! its place among what it holds, its key, and where it was laid along the axis, and whether it windows its items.
//! Where a scroll stands ([`Stand`]) is worked out against it in pixels and back: as the screen settles the scroll at
//! a layout, and as the wheel slides it before the next.
//!
//! The children it built are laid out from its content's start, the first one there; in what the scroll holds they
//! stand `base` further on, the place of the first built in the content's coordinates, which follows a child built in
//! both layouts by its key, and each child built in both keeps where it stood to the bit while it stands within
//! [`STILL`] of it. So a child that stays built is drawn where it was drawn as the window moves past others, and only
//! the scroll's shift moves: the window is placed afresh only on a jump, or once its base passes [`REBASE`].
//!
//! ```text
//! stand ──> offset_of ──> pixels along the children built ──> settled: clamped at what it holds' true ends
//!       <── stand_of  <── the child the view's start is in, and how far into it, kept by its key
//! ```

use crate::tree::{KeyPath, Segment};
use crate::widget::{Anchor, Axis, Keeps, Span, Stand};

/// How near its end, in pixels, a scroll stands at it: a part of a pixel, as an offset clamped to its end is its end.
const AT_END: f32 = 0.5;

/// How far from the content's origin a scroll's base may stand before its window is placed afresh at the origin: far
/// enough that it is met once in thousands of rows, and near enough that an `f32` there still counts a hundredth of a
/// pixel, so a row and the shift sliding it are drawn where they should be.
pub const REBASE: f32 = 65_536.0;

/// How near, in pixels, where a child built in two layouts stands now is to where it stood for it to keep where it
/// stood to the bit: the window's two layouts sum its children in another order, which moves a child by a part of a
/// pixel that is not there, and would draw it anew.
const STILL: f32 = 0.01;

/// What a screen's last layout found of the scroll at `path`, along `axis`: how long its view is, how far what it built
/// reaches past it, how many items it holds, each child it built, in order, whether it windows them, and where in its
/// content's coordinates the first child built stands.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    path: KeyPath,
    axis: Axis,
    view: f32,
    travel: f32,
    count: u32,
    items: Vec<Lined>,
    windowing: Windowing,
    base: f32,
}

/// One child a scroll built, as its last layout laid it: its place among what the scroll holds, its key, where it
/// starts and how long it is along the scroll's axis, from the scroll's content box's start, before it slides, as its
/// window was laid out; and where it stands in what the scroll holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lined {
    pub place: u32,
    pub key: Segment,
    pub start: f32,
    pub length: f32,
    pub placed: f32,
}

/// Whether a scroll builds only the items that can show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Windowing {
    /// It holds children its markup writes, no list: it builds them all.
    Whole,
    /// It holds children its markup writes, one of which holds a list's items: it builds them all, every item of that
    /// list with them, which the list's own scroll would not.
    Wraps,
    /// It holds a list's items and built none, so it knows no item's least length: its next view builds the item it
    /// stands at.
    Unmeasured,
    /// It holds a list's items, and builds those that can show: no item is shorter than `least` pixels along its axis,
    /// the gap after it counted.
    Bounded { least: f32 },
    /// It holds a list's items and builds every one, as its items or its style fail a condition of windowing.
    Every(Failed),
}

/// The condition of windowing a scroll's list fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failed {
    /// Its items have no least length along its axis: no style's least length, and no text, gives one.
    Unbounded,
    /// Its style sets its items apart by all of them, as it spaces or wraps them, so where one stands depends on items
    /// after it.
    Distributed,
    /// Its length across its axis is sized by what it holds, so it depends on items it does not show.
    Across,
}

impl Window {
    /// What a layout found of the scroll at `path` along `axis`, its view `view` pixels long and what it built
    /// reaching `travel` past it, holding `count` items, having built `items`, windowing them as `windowing` says: its
    /// first child built at its content's origin until it is [rebased](Self::rebase).
    pub fn new(
        (path, axis): (KeyPath, Axis),
        (view, travel): (f32, f32),
        count: u32,
        items: Vec<Lined>,
        windowing: Windowing,
    ) -> Self {
        Self { path, axis, view, travel, count, items, windowing, base: 0.0 }
    }

    /// The path of its scroll.
    pub const fn path(&self) -> &KeyPath {
        &self.path
    }

    /// Whether its scroll windows its items.
    pub const fn windowing(&self) -> Windowing {
        self.windowing
    }

    /// How long its view is along its axis, in pixels.
    pub const fn view(&self) -> f32 {
        self.view
    }

    /// The place in its list of the child keyed `key`, if it built it.
    pub fn place_of(&self, key: Segment) -> Option<u32> {
        self.items.iter().find(|lined| lined.key == key).map(|lined| lined.place)
    }

    /// Its axis.
    pub const fn axis(&self) -> Axis {
        self.axis
    }

    /// Where each child it built stands in what its scroll holds, along its axis from its content box's start, in
    /// order.
    pub fn placed(&self) -> impl Iterator<Item = f32> + '_ {
        self.items.iter().map(|lined| lined.placed)
    }

    /// Places its first child built where [`Self::based`] says, `was` what the layout before found, if one did, and
    /// each child after it on from there: one built in both where it stood, to the bit, while it stands within
    /// [`STILL`] of it.
    pub fn rebase(&mut self, was: Option<&Self>, stand: Stand) {
        self.base = was.map_or(0.0, |was| self.based(was, stand));
        let base = self.base;
        for lined in &mut self.items {
            let now = base + lined.start;
            let stood = was.and_then(|was| was.items.iter().find(|old| old.key == lined.key)).map(|old| old.placed);
            lined.placed = match stood {
                Some(stood) if (stood - now).abs() < STILL => stood,
                Some(_) | None => now,
            };
        }
    }

    /// How far its scroll slides what it holds, in its content's coordinates, its view starting `shown` pixels into
    /// what it built.
    pub fn slid(&self, shown: f32) -> f32 {
        self.base + shown
    }

    /// Whether it builds every item it holds.
    pub fn every_built(&self) -> bool {
        u32::try_from(self.items.len()).is_ok_and(|built| built == self.count)
    }

    /// The least and the most offset it can stand at, in pixels from its built content's start: its start while it
    /// built its first item, and its end while it built its last, unbounded each way past the items it built.
    fn bounds(&self) -> (f32, f32) {
        let starts = self.items.first().is_some_and(|first| first.place == 0);
        let ends = self.items.last().is_some_and(|last| last.place + 1 == self.count);
        let least = if starts {
            0.0
        } else {
            f32::NEG_INFINITY
        };
        let most = if ends {
            self.travel
        } else {
            f32::INFINITY
        };

        (least, most.max(least))
    }

    /// The offset `stand` names, in pixels from its built content's start, kept to its bounds: a scroll that keeps
    /// `keeps` and was never laid out at its end or its start.
    pub fn offset_of(&self, stand: Stand, keeps: Keeps) -> f32 {
        let (least, most) = self.bounds();
        let offset = match stand {
            Stand::Unplaced => match keeps {
                Keeps::End => self.travel,
                Keeps::Offset => 0.0,
            },
            Stand::At(anchor) => self.start_of(anchor) + anchor.into,
            Stand::Item(item) => self.start_at(item),
            Stand::Share(share) => self.shared(share),
            Stand::End { .. } => self.travel,
        };

        offset.clamp(least, most)
    }

    /// Where it stands at `offset` pixels from its built content's start, kept to its bounds: at the child its view's
    /// start is in, or, at its end, kept to it when it `keeps` its end; and how far it slides what it holds, within
    /// what it built.
    pub fn stand_of(&self, offset: f32, keeps: Keeps) -> (Stand, f32) {
        let (least, most) = self.bounds();
        let offset = offset.clamp(least, most);
        let shown = offset.clamp(0.0, self.travel);
        let before = self.items.iter().rev().find(|lined| lined.start <= offset);
        let anchor = before.or_else(|| self.items.first()).map_or(Anchor::FIRST, |lined| Anchor {
            item: lined.place,
            key: lined.key,
            into: offset - lined.start,
        });
        let ends = self.items.last().is_none_or(|last| last.place + 1 == self.count);
        let at_end = ends && offset >= self.travel - AT_END;
        let stand = match keeps {
            Keeps::End if at_end => Stand::End { start: anchor },
            Keeps::End | Keeps::Offset => Stand::At(anchor),
        };

        (stand, shown)
    }

    /// Where `stand` settles as it is laid out in it, keeping `keeps`, and how far it slides what it holds.
    pub fn settle(&self, stand: Stand, keeps: Keeps) -> (Stand, f32) {
        self.stand_of(self.offset_of(stand, keeps), keeps)
    }

    /// Where `stand` stands slid `by` pixels, keeping `keeps`, and how far it moved: none at its end that way.
    pub fn slide(&self, stand: Stand, keeps: Keeps, by: f32) -> (Stand, f32) {
        let from = self.offset_of(stand, keeps);
        let (least, most) = self.bounds();
        let to = (from + by).clamp(least, most);
        let (slid, _) = self.stand_of(to, keeps);

        (slid, to - from)
    }

    /// Where its view stands within the whole it holds, slid `shown` pixels: by pixels when it built every item, and by
    /// items, each of its count, when it built only those that can show.
    pub fn bar(&self, shown: f32) -> Span {
        let every = self.every_built();
        if every {
            let whole = self.view + self.travel;
            let sized = whole > 0.0;
            if !sized {
                return Span::WHOLE;
            }
            return Span { start: shown / whole, shown: self.view / whole };
        }
        let end = shown + self.view;
        let mut start = 0.0;
        let mut seen = 0.0;
        for lined in &self.items {
            let length = lined.length.max(f32::MIN_POSITIVE);
            let lined_end = lined.start + lined.length;
            let holds_start = lined.start <= shown && shown < lined_end;
            if holds_start {
                start = items(lined.place) + (shown - lined.start) / length;
            }
            let overlap = (lined_end.min(end) - lined.start.max(shown)).max(0.0);
            seen += overlap / length;
        }
        let count = items(self.count).max(1.0);

        Span { start: start / count, shown: (seen / count).min(1.0) }
    }

    /// Where its first child built stands in its content's coordinates, `was` what the layout before found, the scroll
    /// standing at `stand` before it settles: where the layout before put a child built in both, the child it stood at
    /// first, so a child that stays built stays where it was drawn; at the origin on a jump, with no child built in both,
    /// or past [`REBASE`].
    fn based(&self, was: &Self, stand: Stand) -> f32 {
        let jumped = matches!(stand, Stand::Item(_) | Stand::Share(_) | Stand::Unplaced);
        if jumped {
            return 0.0;
        }
        let anchored = match stand {
            Stand::At(anchor) | Stand::End { start: anchor } => self.both(was, anchor.key),
            Stand::Item(_) | Stand::Share(_) | Stand::Unplaced => None,
        };
        let common = anchored.or_else(|| self.items.iter().find_map(|lined| self.both(was, lined.key)));
        let Some((old, new)) = common else {
            return 0.0;
        };
        let base = was.base + old.start - new.start;
        let far = base.abs() > REBASE;
        if far {
            return 0.0;
        }

        base
    }

    /// The child keyed `key` as `was` and as this found it, when both built it.
    fn both(&self, was: &Self, key: Segment) -> Option<(Lined, Lined)> {
        let old = was.items.iter().find(|lined| lined.key == key)?;
        let new = self.items.iter().find(|lined| lined.key == key)?;

        Some((*old, *new))
    }

    /// Where the child `anchor` names starts: the one of its key, or, its key gone from what was built, the first at
    /// its place or after it.
    fn start_of(&self, anchor: Anchor) -> f32 {
        let keyed = self.items.iter().find(|lined| lined.key == anchor.key);
        keyed.map_or_else(|| self.start_at(anchor.item), |lined| lined.start)
    }

    /// Where the first child built at `place` or after it starts: past what it built when it built none there.
    fn start_at(&self, place: u32) -> f32 {
        let found = self.items.iter().find(|lined| lined.place >= place);

        found.map_or(self.travel + self.view, |lined| lined.start)
    }

    /// Where a share `share` of what it holds ends: of its pixels when it built every item, and of its items when it
    /// built only those that can show.
    fn shared(&self, share: f32) -> f32 {
        let every = self.every_built();
        if every {
            return share * (self.view + self.travel);
        }
        // A share of a list's count, which a u32 counts.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let item = (share.clamp(0.0, 1.0) * items(self.count)).floor() as u32;

        self.start_at(item)
    }
}

/// `count` items, as a length is counted.
pub fn items(count: u32) -> f32 {
    // A list's items are far fewer than an f32 counts exactly.
    #[allow(clippy::cast_precision_loss)]
    let items = count as f32;

    items
}
