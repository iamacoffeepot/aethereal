//! What a widget that arranges its children as a popup says of its layer (ADR-0056: Popups draw in one top layer per
//! screen): which side of its anchor its content is placed on, and what opens it, its own input or the pointer resting
//! on its anchor. Its first child is its anchor and the rest its content; whatever its type, stock or a program's own,
//! the library reads and shuts it through [`Widget`](super::Widget)'s methods alone.

use crate::tree::{KeyPath, Segment};
use aethereal_data::Plain;
use core::time::Duration;

/// How the content of a widget that arranges its children as a popup shows in its screen's top layer: placed on `side`
/// of its anchor, and opened as `opens` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    pub side: Side,
    pub opens: Opens,
}

/// What opens a layer, and so what its screen does to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    /// Its own input, as a popup's anchor is pressed: whether it is open is its own state, which the library reads with
    /// [`Widget::open`](super::Widget::open). Its screen shuts it with [`Widget::shut`](super::Widget::shut) when a
    /// press it hears lands outside it, when Escape is pressed while it is the topmost open, when its anchor goes, and,
    /// when `inside` closes, when a click within its content raises an event. Its own drawing and hit are listed over
    /// its anchor's, as it is its anchor's button; and when `modal` it blocks the pointer from the rest of its screen
    /// and holds Tab within its content while it is open.
    Pressed { modal: bool, inside: Inside },
    /// The pointer resting on its anchor for `delay`: where it stands with the pointer is a [`Tip`] in its state, which
    /// the library reads with [`Widget::tip`](super::Widget::tip) and keeps with
    /// [`Widget::tip_to`](super::Widget::tip_to) as the pointer comes onto its anchor, rests there, leaves it, or
    /// presses, and as its anchor goes. Its content lists no hit and nothing Tab reaches, as the pointer leaving its
    /// anchor for it would shut it.
    Rested { delay: Duration },
}

/// Which side of what it is opened from a popup's content is placed on, before it flips to the other side for room
/// and shifts to stay inside its screen. Content placed below or above starts at its anchor's left edge, and content
/// placed right or left at its top edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    #[default]
    Below,
    Above,
    Right,
    Left,
}

/// What a click within a popup's content does to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Inside {
    /// Nothing: it stays open, as a popup of settings does while they are set.
    #[default]
    Stays,
    /// It shuts once a click within its content raises an event, after the event is raised: as a menu does once a choice
    /// is made, and not for a click on an anchor of a popup within it or on empty space.
    Closes,
}

/// Where a layer opened by the pointer resting on its anchor stands with the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.tip")]
pub enum Tip {
    /// The pointer is not on its anchor.
    #[default]
    Away,
    /// The pointer came onto its anchor in the hover its screen numbered `hover`, and it waits for that hover's rest to
    /// end: the number is its screen's, kept here and compared by the library alone.
    Waiting { hover: u64 },
    /// Its content shows.
    Open,
    /// A press was made while the pointer was on its anchor: it stays shut until the pointer leaves.
    Dismissed,
}

/// The path of the anchor of the popup at `popup`: its first child in the markup.
#[must_use]
pub fn anchor(popup: &KeyPath) -> KeyPath {
    popup.child(Segment::Place(0))
}

/// Whether the widget at `path` is the anchor of the popup at `popup`, or stands within it.
#[must_use]
pub fn anchors(popup: &KeyPath, path: &KeyPath) -> bool {
    let anchor = anchor(popup);

    anchor == *path || anchor.holds(path)
}

/// Whether the widget at `path` stands in the content of the popup at `popup`: within it, and not its anchor or within
/// that.
#[must_use]
pub fn contains(popup: &KeyPath, path: &KeyPath) -> bool {
    let within = popup.holds(path);
    let anchored = anchors(popup, path);

    within && !anchored
}
