//! What a press holds (ADR-0055: Capture; ADR-0056: Widget state is kept by key). While a button is down, the window
//! sends the presses, moves, and release to the entries the first press reached, by the numbers they had then; but the
//! screen may have sent another list since, in which a widget before one of them drew more or less, so that its number
//! now names something else. So the screen keeps, beside its focus, what each entry of its list a press reached was
//! when the press reached it, and what the window's capture sends through that entry goes there until every button
//! pressed on it is up. Nothing is kept on the widgets, and no copy of a list is kept.

use super::super::{Listed, Screen};
use super::place;
use aethereal_data::Plain;
use aethereal_window::{Button, PointerEvent};

/// An entry of the screen's list that a press reached, which the window's capture sends input through: its number;
/// what it was when the press reached it, a widget's hit or the backdrop behind a modal popup; and the buttons down on
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(in crate::screen) struct Hold {
    entry: u32,
    listed: Listed,
    buttons: Vec<Button>,
}

impl Hold {
    /// Whether a button is still down on it.
    fn down(&self) -> bool {
        !self.buttons.is_empty()
    }
}

impl Screen {
    /// What the entry numbered `entry` is to `event` through it: what a press held it as, when one did and `event` is
    /// one the window's capture sends, a press, a move, or a release; otherwise what it is in the list last sent, if
    /// that has it.
    pub(super) fn reached_by(&self, entry: u32, event: PointerEvent) -> Option<Listed> {
        let held = self.held.iter().find(|held| held.entry == entry).map(|held| &held.listed);
        let captured = match event {
            PointerEvent::PointerDown { .. } | PointerEvent::PointerMoved { .. } | PointerEvent::PointerUp { .. } => {
                held
            }
            PointerEvent::PointerEntered | PointerEvent::PointerLeft | PointerEvent::Wheel { .. } => None,
        };

        captured.or_else(|| self.listing.get(place(entry))).cloned()
    }

    /// Takes in `event` through the entry numbered `entry`, which it reached as `listed`: a press holds a widget's hit
    /// or a backdrop, which the window captures, and its release lets go once no button pressed on it is down.
    pub(super) fn hold(&mut self, entry: u32, event: PointerEvent, listed: &Listed) {
        match event {
            PointerEvent::PointerDown { button, .. } => self.pressed_on(entry, button, listed),
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
    fn pressed_on(&mut self, entry: u32, button: Button, listed: &Listed) {
        let held = self.held.iter_mut().find(|held| held.entry == entry);
        if let Some(held) = held {
            held.buttons.push(button);
            return;
        }
        let captured = matches!(listed, Listed::Hit(_) | Listed::Backdrop(_));
        if captured {
            self.held.push(Hold { entry, listed: listed.clone(), buttons: vec![button] });
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
