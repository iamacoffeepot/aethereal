//! The keys a screen declares, and how its focus moves (ADR-0056: Focus and keys).
//!
//! The library writes the screen's `Keys` entries (ADR-0055) from its focus and its hotkeys:
//!
//! - **A widget that types has focus,** a text input: every key, blocking, so a key typed into it never reaches the
//!   game, and the window's input method is allowed while it has (ADR-0051).
//! - **Otherwise,** the screen's hotkeys; and, while a widget has focus, Tab, Escape, and the keys that widget hears;
//!   all blocking. Beside them, the modifiers, which the screen watches go down and come up and lets through, so
//!   Shift-Tab and a chord read their modifiers without taking Shift from the game.
//!
//! So a shown screen whose widgets have no focus takes only its hotkeys: a HUD does not take the game's Tab or Escape. A
//! screen that wants Tab to bring focus to its first widget lists Tab among its hotkeys.

use super::{Screen, Target};
use crate::input::{Hears, Held};
use crate::table::Focus;
use crate::tree::{Flat, KeyPath};
use aethereal_data::Plain;
use aethereal_render::frame::Frame;
use aethereal_window::{AllowIme, KeySet};

/// The key that moves focus to the next focusable widget, and with Shift to the one before.
pub(super) const TAB: &str = "Tab";

/// The key that clears focus.
pub(super) const ESCAPE: &str = "Escape";

/// The Shift keys, by place.
const SHIFTS: [&str; 2] = ["ShiftLeft", "ShiftRight"];

/// The keys a command is chorded with: Control, and Meta, the command key of a Mac.
const COMMANDS: [&str; 4] = ["ControlLeft", "ControlRight", "MetaLeft", "MetaRight"];

/// Keys a screen declares: the set, whether it blocks them from what is beneath, and what the screen does with them.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) struct Declared {
    pub keys: KeySet,
    pub blocks: bool,
    pub role: Role,
}

/// What a screen does with keys it declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Role {
    /// The keys its focus and the screen hear: each goes to the focused widget, then the focus keys, then the screen.
    Screen,
    /// The modifiers, which it watches go down and come up, and lets through.
    Modifiers,
}

/// What a screen last asked of its window's input method: it starts stopped, as every window's does (ADR-0051).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Ime {
    Stopped,
    Allowed,
}

/// The modifier keys down, by place, as the screen saw them go down and come up.
#[derive(Debug, Clone, PartialEq, Eq, Default, Plain)]
pub(super) struct Modifiers {
    down: Vec<String>,
}

impl Modifiers {
    /// Notes that the key at `code` went down, when it is a modifier.
    pub(super) fn pressed(&mut self, code: &str) {
        let modifier = is_modifier(code);
        let noted = self.down.iter().any(|down| down == code);
        if modifier && !noted {
            self.down.push(String::from(code));
        }
    }

    /// Notes that the key at `code` came up.
    pub(super) fn released(&mut self, code: &str) {
        self.down.retain(|down| down != code);
    }

    /// The modifiers held.
    pub(super) fn held(&self) -> Held {
        let shift = self.down.iter().any(|down| SHIFTS.contains(&down.as_str()));
        let command = self.down.iter().any(|down| COMMANDS.contains(&down.as_str()));

        Held { shift, command }
    }
}

/// Whether the key at `code` is a modifier the screen watches.
fn is_modifier(code: &str) -> bool {
    SHIFTS.contains(&code) || COMMANDS.contains(&code)
}

/// What the widget with `focus` among `flat` hears, when one has it.
fn heard(flat: &[Flat<'_>], focus: &Focus) -> Option<Hears> {
    let Focus::On(path) = focus else {
        return None;
    };

    flat.iter().find(|flat| flat.path == *path).map(|flat| flat.node.widget().hears())
}

/// Whether the widget with `focus` among `flat` types, so it hears every key and the input method.
pub(super) fn typing(flat: &[Flat<'_>], focus: &Focus) -> bool {
    heard(flat, focus) == Some(Hears::Typing)
}

/// The keys a screen whose widgets are `flat` declares, with `focus`, and `hotkeys` its own, in the order they are
/// listed.
pub(super) fn declared(flat: &[Flat<'_>], focus: &Focus, hotkeys: &[&str]) -> Vec<Declared> {
    let mut codes: Vec<&str> = hotkeys.to_vec();
    match heard(flat, focus) {
        Some(Hears::Typing) => {
            return vec![Declared { keys: KeySet::All, blocks: true, role: Role::Screen }];
        }
        Some(Hears::Keys(hears)) => {
            codes.extend([TAB, ESCAPE]);
            codes.extend(hears);
        }
        None => {}
    }
    codes.sort_unstable();
    codes.dedup();
    if codes.is_empty() {
        return Vec::new();
    }
    let watched = SHIFTS.iter().chain(&COMMANDS).map(|code| String::from(*code)).collect();

    vec![
        Declared { keys: KeySet::Of(codes.into_iter().map(String::from).collect()), blocks: true, role: Role::Screen },
        Declared { keys: KeySet::Of(watched), blocks: false, role: Role::Modifiers },
    ]
}

/// The focus Tab moves to from `focus` among `flat`: the next focusable widget in tree order, or with `back` the one
/// before, round from the last to the first; from none, the first, or the last; none when none is focusable.
pub(super) fn tabbed(flat: &[Flat<'_>], focus: &Focus, back: bool) -> Focus {
    let focusable: Vec<&KeyPath> =
        flat.iter().filter(|flat| flat.node.widget().focusable()).map(|flat| &flat.path).collect();
    let count = focusable.len();
    if count == 0 {
        return Focus::Nowhere;
    }
    let at = match focus {
        Focus::On(path) => focusable.iter().position(|focusable| *focusable == path),
        Focus::Nowhere => None,
    };
    let next = match (at, back) {
        (Some(at), false) => (at + 1) % count,
        (Some(at), true) => (at + count - 1) % count,
        (None, false) => 0,
        (None, true) => count - 1,
    };

    Focus::On(focusable[next].clone())
}

impl Screen {
    /// What to ask of the window's input method as a widget that types gains or loses focus, `typing`: allowed while
    /// one has it, and stopped after (ADR-0051), each asked only as it changes, and never for a screen that draws into
    /// a texture, which has no window.
    pub(super) fn typing(&mut self, typing: bool) -> Vec<AllowIme> {
        let Target { frame: Frame::Main(window), .. } = self.target else {
            return Vec::new();
        };
        let wanted = if typing {
            Ime::Allowed
        } else {
            Ime::Stopped
        };
        if wanted == self.ime {
            return Vec::new();
        }
        self.ime = wanted;

        vec![AllowIme { window, allowed: typing }]
    }
}
