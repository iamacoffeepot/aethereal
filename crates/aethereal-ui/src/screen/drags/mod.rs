//! A screen's drags (ADR-0056: Drag and drop is one path). A press of the main button on a widget its view gives a
//! [`DragSource`](crate::DragSource) grips it, its payload copied from the tree the press reached; a move through that
//! widget's hit past [`DRAG_THRESHOLD`] logical pixels from the press begins the drag, which lists the hit with the
//! drag's cursor, so the window shows it wherever the pointer goes while the button is held; and the release through
//! that hit lands it where its `over` says:
//!
//! - **on a hit of the screen's own:** the innermost widget at or around it, on its own surface, whose
//!   [`DropTarget`](crate::DropTarget) takes the payload's kind raises its drop with the payload;
//! - **on another actor's entry:** the screen casts the actor to [`Drops`](crate::Drops) and mails it
//!   [`Dropped`](crate::Dropped), its payload put as a blob, as [`take_in`](crate::take_in) does with what the turn of
//!   input leaves; that actor's library finds its target the same way, from the entry ([`Screen::dropped`]), and
//!   replies [`DropRefused`](crate::DropRefused) when none takes it ([`Landing::Refused`]);
//! - **on nothing, on what nothing takes, on an actor that takes no drops, or refused:** the source raises its
//!   `drop_outside` with the payload.
//!
//! While a drag is under way, the target it is over in its own screen, found the same way from the topmost hit the
//! pointer reaches, is drawn lit ([`Draws::drop_over`](crate::Draws::drop_over)).

#[cfg(test)]
mod tests;

use super::input::place;
use super::{Fresh, Heard, Last, Listed, Screen};
use crate::drag::{DRAG_THRESHOLD, Payload};
use crate::input::{Erased, Raised};
use crate::tree::{KeyPath, Node};
use aethereal_actor::ProtocolRef;
use aethereal_data::{ActorId, KindId, Plain};
use aethereal_math::Vec2;
use aethereal_window::{Hearer, Over};
use core::iter;
use core::mem;

/// Where a screen stands with a drag.
#[derive(Debug, Clone, PartialEq, Plain)]
pub(super) enum Grip {
    /// No press of the main button on a source is held.
    Loose,
    /// The main button went down on the source at `path`, at `from` in the screen's pixels, and has not moved past the
    /// threshold: `payload` is what it drags, copied from the tree the press reached.
    Pressed { path: KeyPath, from: Vec2, payload: Payload },
    /// The pointer moved past the threshold with the button held on the source at `path`, which drags `payload`.
    Dragging { path: KeyPath, payload: Payload },
}

/// What a release of the main button through the hit over the widget at its path ended.
pub(super) enum Ended {
    /// A press: a click, when it was over the widget.
    Press,
    /// A drag of `payload`, which lands where the release was over, and is no click.
    Drag(Payload),
}

/// A drag dropped on another actor's entry, for the screen's handler to mail: the actor the release was over; the
/// payload; the entry it was over and the release's place in that entry's coordinates; the widget it was dragged from;
/// and the source's `drop_outside` raised with the payload, raised in its place when the actor takes no drops.
#[derive(Debug)]
pub struct Away {
    pub to: ProtocolRef<Hearer>,
    pub payload: Payload,
    pub entry: u32,
    pub at: Vec2,
    pub source: KeyPath,
    /// The source's `drop_outside`, fixed when the drag lands: none for a source that has none.
    pub outside: Option<Raised<Erased>>,
}

/// What became of a drag another actor dropped on a screen.
#[derive(Debug)]
pub enum Landing {
    /// A target took it: what it raised, for the screen's handler to take in.
    Taken(Heard),
    /// Nothing at the entry it was dropped on takes its payload: the screen's handler replies its refusal.
    Refused,
}

impl Screen {
    /// Takes in a press of the main button at `at`, in the screen's pixels, through the hit over the widget at `path`:
    /// a source grips it, with the payload the tree the press reached copied.
    pub(super) fn grip(&mut self, path: &KeyPath, at: Vec2) {
        let Some(payload) = self.node(path).and_then(Node::drag).map(|source| source.payload().clone()) else {
            return;
        };
        self.grip = Grip::Pressed { path: path.clone(), from: at, payload };
    }

    /// Takes in a move to `at`, in the screen's pixels, through the hit over the widget at `path`: past the threshold
    /// from the press of the source it gripped, its drag begins, and the next `Prepare` lists its hit with the drag's
    /// cursor.
    pub(super) fn drag_to(&mut self, path: &KeyPath, at: Vec2) {
        let Grip::Pressed { path: gripped, from, payload } = &self.grip else {
            return;
        };
        let here = gripped == path;
        let past = (at - *from).length() > DRAG_THRESHOLD * self.viewport.scale;
        let begins = here && past;
        if !begins {
            return;
        }
        self.grip = Grip::Dragging { path: path.clone(), payload: payload.clone() };
        self.fresh = Fresh::Stale;
    }

    /// Takes in a release of the main button through the hit over the widget at `path`, and answers what it ended: the
    /// grip of that widget lets go, and a drag from it ends, its hit listed again as its widget says.
    pub(super) fn let_go(&mut self, path: &KeyPath) -> Ended {
        let held = match &self.grip {
            Grip::Pressed { path: gripped, .. } | Grip::Dragging { path: gripped, .. } => gripped == path,
            Grip::Loose => false,
        };
        if !held {
            return Ended::Press;
        }
        match mem::replace(&mut self.grip, Grip::Loose) {
            Grip::Dragging { payload, .. } => {
                self.fresh = Fresh::Stale;
                Ended::Drag(payload)
            }
            Grip::Pressed { .. } | Grip::Loose => Ended::Press,
        }
    }

    /// Takes in a move or release the capture sent to the widget at `path`, which has gone from the tree since the
    /// press reached it: a grip of it lets go, and a drag from it ends there, landing nothing.
    pub(super) fn lost(&mut self, path: &KeyPath) {
        self.let_go(path);
    }

    /// Takes in the pointer coming onto or leaving a hit while a drag is under way: the target it is over may change,
    /// so the next `Prepare` lays the screen out.
    pub(super) fn passed(&mut self) {
        let dragging = matches!(self.grip, Grip::Dragging { .. });
        if dragging {
            self.fresh = Fresh::Stale;
        }
    }

    /// Lands `payload`, dragged from the widget at `source`, where its release was over, `me` the screen's actor: on a
    /// hit of its own, the innermost target around it that takes it raises its drop; on another actor's entry, it is
    /// left in `heard` to mail; and where nothing takes it, the source raises its `drop_outside`.
    pub(super) fn land(&self, source: &KeyPath, payload: Payload, over: Over, me: ActorId, heard: &mut Heard) {
        let Over::Entry { actor, entry, x, y } = over else {
            heard.raised.extend(self.outside(source, payload));
            return;
        };
        let ours = actor.id() == me;
        if !ours {
            let outside = self.outside(source, payload.clone());
            let at = Vec2::new(x, y);
            heard.away.push(Away { to: actor, payload, entry, at, source: source.clone(), outside });
            return;
        }
        let raised = self.taken_at(entry, &payload).or_else(|| self.outside(source, payload));
        heard.raised.extend(raised);
    }

    /// Takes in `payload`, which another actor dropped on the entry numbered `entry` of the screen's list: the innermost
    /// target at or around the widget whose hit it is that takes the payload's kind raises its drop with it; when none
    /// does, the drop is refused.
    pub fn dropped(&mut self, entry: u32, payload: &Payload) -> Landing {
        let Some(raised) = self.taken_at(entry, payload) else {
            return Landing::Refused;
        };

        Landing::Taken(Heard { raised: vec![raised], ..Heard::default() })
    }

    /// Takes in `payload`, which the screen dropped from the widget at `source` on another actor, refused: that widget
    /// raises its `drop_outside` with the payload, as the view now builds it.
    pub fn refused(&mut self, source: &KeyPath, payload: Payload) -> Heard {
        let mut heard = Heard::default();
        heard.raised.extend(self.outside(source, payload));

        heard
    }

    /// The path of the target a drag of the screen's own under way is over, which draws lit: the innermost one around
    /// the topmost hit the pointer reaches that takes its payload's kind.
    pub(super) fn lit(&self) -> Option<KeyPath> {
        let Grip::Dragging { payload, .. } = &self.grip else {
            return None;
        };
        let rank = |path: &KeyPath| self.listing.iter().position(|listed| *listed == Listed::Hit(path.clone()));
        let top = self.reached.iter().max_by_key(|path| rank(path))?;

        self.target_around(top, payload.kind())
    }

    /// The drop raised with `payload` by the innermost target that takes it around the widget whose hit is the entry
    /// numbered `entry` of the list last sent, if any.
    fn taken_at(&self, entry: u32, payload: &Payload) -> Option<Raised<Erased>> {
        let Some(Listed::Hit(path)) = self.listing.get(place(entry)) else {
            return None;
        };
        let target = self.target_around(path, payload.kind())?;

        self.node(&target).and_then(Node::drop_target).map(|target| target.raised(payload.clone()))
    }

    /// The path of the innermost widget at or around the one at `path`, on its own surface, whose target takes a
    /// payload of the kind `kind`.
    fn target_around(&self, path: &KeyPath, kind: KindId) -> Option<KeyPath> {
        iter::once(path.clone()).chain(self.surfaced(path)).find(|at| {
            let target = self.node(at).and_then(Node::drop_target);
            target.is_some_and(|target| target.takes(kind))
        })
    }

    /// The `drop_outside` of the source at `source` in the last tree built, raised with `payload`, if it has one.
    fn outside(&self, source: &KeyPath, payload: Payload) -> Option<Raised<Erased>> {
        self.node(source).and_then(Node::drag).and_then(|drag| drag.outside_raised(payload))
    }

    /// The node at `path` in the last tree built, if it has one there.
    fn node(&self, path: &KeyPath) -> Option<&Node> {
        let Last::Built(tree) = &self.unkept.last else {
            return None;
        };

        tree.find(path)
    }
}
