//! What a press holds (ADR-0055: Capture; ADR-0056: Widget state is kept by key). While a button is down, the window
//! sends the presses, moves, and release to the entries the first press reached, by the numbers they had then; but the
//! screen may have sent another list since, in which a widget before one of them drew more or less, so that its number
//! now names something else. So the screen keeps, beside its focus, what each entry of its list a press reached was
//! when the press reached it, and what the window's capture sends through that entry goes there until every button
//! pressed on it is up. Nothing is kept on the widgets, and no copy of a list is kept.

use super::super::{Listed, Screen};
use super::place;
use aethereal_data::Plain;
use aethereal_math::Vec2;
use aethereal_window::{Button, PointerEvent};

/// An entry of the screen's list that a press reached, which the window's capture sends input through: its number;
/// what it was when the press reached it, a widget's hit or the backdrop behind a modal popup, and how far it was drawn
/// from the screen's own coordinates, as the window tells of the pointer from where it was at the press; and the buttons
/// down on it.
#[derive(Debug, Clone, PartialEq, Plain)]
pub(in crate::screen) struct Hold {
    entry: u32,
    listed: Listed,
    moved: Vec2,
    buttons: Vec<Button>,
}

impl Hold {
    /// Whether a button is still down on it.
    fn down(&self) -> bool {
        !self.buttons.is_empty()
    }
}

impl Screen {
    /// What the entry numbered `entry` is to `event` through it, and how far it is drawn from the screen's own
    /// coordinates: as a press held it, when one did and `event` is one the window's capture sends, a press, a move, or
    /// a release; otherwise as it is in the list last sent, if that has it.
    pub(super) fn reached_by(&self, entry: u32, event: PointerEvent) -> Option<(Listed, Vec2)> {
        let held = self.held.iter().find(|held| held.entry == entry).map(|held| (&held.listed, held.moved));
        let captured = match event {
            PointerEvent::PointerDown { .. } | PointerEvent::PointerMoved { .. } | PointerEvent::PointerUp { .. } => {
                held
            }
            PointerEvent::PointerEntered | PointerEvent::PointerLeft | PointerEvent::Wheel { .. } => None,
        };
        let listed = || {
            let at = place(entry);
            let moved = self.moved.get(at).copied().unwrap_or(Vec2::ZERO);
            self.listing.get(at).map(|listed| (listed, moved))
        };

        captured.or_else(listed).map(|(listed, moved)| (listed.clone(), moved))
    }

    /// Takes in `event` through the entry numbered `entry`, which it reached as `listed`, drawn `moved` from the
    /// screen's own coordinates: a press holds a widget's hit or a backdrop, which the window captures, and its release
    /// lets go once no button pressed on it is down.
    pub(super) fn hold(&mut self, entry: u32, event: PointerEvent, (listed, moved): (&Listed, Vec2)) {
        match event {
            PointerEvent::PointerDown { button, .. } => self.pressed_on(entry, button, (listed, moved)),
            PointerEvent::PointerUp { button, .. } => self.released_on(entry, button),
            PointerEvent::PointerEntered
            | PointerEvent::PointerLeft
            | PointerEvent::PointerMoved { .. }
            | PointerEvent::Wheel { .. } => {}
        }
    }

    /// Takes in a press of `button` through the entry numbered `entry`, `listed`: one already held holds it down too,
    /// and a widget's hit or a backdrop is held from now. A hit that hears presses alone is never captured, so it is
    /// not held.
    fn pressed_on(&mut self, entry: u32, button: Button, (listed, moved): (&Listed, Vec2)) {
        let held = self.held.iter_mut().find(|held| held.entry == entry);
        if let Some(held) = held {
            held.buttons.push(button);
            return;
        }
        let captured = matches!(listed, Listed::Hit(_) | Listed::Backdrop(_));
        if captured {
            self.held.push(Hold { entry, listed: listed.clone(), moved, buttons: vec![button] });
        }
    }

    /// Takes in a release of `button` through the entry numbered `entry`: held, it lets go once no button pressed on it
    /// is down, as the window's capture does.
    fn released_on(&mut self, entry: u32, button: Button) {
        let held = self.held.iter_mut().find(|held| held.entry == entry);
        if let Some(held) = held {
            held.buttons.retain(|down| *down != button);
        }
        self.held.retain(Hold::down);
    }
}
