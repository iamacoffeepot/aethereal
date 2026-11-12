//! The child actors a screen places (ADR-0056: Child actors): what size each was last told, and the mail that tells
//! each its size when it changed.
//!
//! ```text
//! layout ──> each slot shown: its child, its content box's size in the screen's frame and scale
//!        ──> not what that child was last told? ──> Resize, mailed through the child's reference
//!        ──> what this layout told is what is kept: a child left out of it is told again when it shows
//! ```
//!
//! A move of a slot changes nothing a child is told, so it sends nothing: an inheriting child is placed by its call,
//! in the screen's coordinates, and moves with it. A child is told its size from the `Prepare` that laid its slot out,
//! and lays out to it at its own next `Prepare`, so it learns a new size a frame after its parent decides it; the
//! call's clip contains the mismatch for that frame.

#[cfg(test)]
mod tests;

use super::orders::Orders;
use super::{Screen, Viewport};
use crate::child::{Calling, Resize, Resizes};
use crate::widget::Laid;
use aethereal_actor::ProtocolRef;
use aethereal_data::Bound;

/// A size a child was told: the child, by the reference its size is mailed through, and the viewport it lays out to.
#[derive(Debug, Clone, Copy, PartialEq, Bound)]
pub(super) struct Told {
    pub child: ProtocolRef<Resizes>,
    pub viewport: Viewport,
}

impl Told {
    /// What the child placed in `slot`, laid at `laid` in a screen whose viewport is `screen`, is told: the size of the
    /// slot's content box, in the screen's frame and at its UI scale.
    pub(super) fn of(slot: &Calling, laid: &Laid, screen: Viewport) -> Self {
        Self { child: slot.sized(), viewport: Viewport { size: laid.content.size(), ..screen } }
    }
}

impl Screen {
    /// Tells each child of `told`, the children the layout just made placed, the size it lays out to, unless it was
    /// told that size by the layout before: a child resized, or shown again after it was left out, is told, and one
    /// moved is not. What this layout told is kept, so a child left out of it is told again when it shows, as it may
    /// have missed a change while it was hidden.
    pub(super) fn tell(&mut self, told: Vec<Told>, orders: &mut Orders) {
        for size in &told {
            let known = self.told.contains(size);
            if !known {
                orders.tell_size(size.child, Resize { viewport: size.viewport });
            }
        }
        self.told = told;
    }
}
