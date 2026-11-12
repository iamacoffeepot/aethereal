//! Input through a screen's list (ADR-0056: Widget state is kept by key; Events call the screen's methods; Focus and
//! keys). The library keeps what each entry of the list it sent is, so a `Pointed` through a hit reaches the widget it
//! was listed over, in its own coordinates, or, for the wheel, the innermost scroll around it that can move; and a
//! `Keyed` through the keys the screen declared reaches its focus, then each widget its focus is within that hears the
//! key, innermost first, then its focus keys, then the screen.
//!
//! A press of the main button on a widget that drags grips it, and a move past the threshold begins its drag, whose
//! release lands it rather than clicking (ADR-0056: Drag and drop is one path).
//!
//! A press shuts each open popup it lands outside of, and dismisses the tooltip the pointer is on; a click within a
//! popup's content that raises an event shuts it when it closes so; Escape shuts the topmost open popup; and a press behind a modal popup
//! reaches only its backdrop, which shuts it. The pointer coming onto and leaving each hit is kept, for the tooltips the
//! next `Prepare` looks at (ADR-0056: Popups draw in one top layer per screen).
//!
//! A widget changes its own state, kept in the table, and a change lays the screen out at the next `Prepare` though
//! its tree is the same, as a change of focus does. What it raises, and the keys nothing used, are handed back in
//! [`Heard`] for the screen's handler to take in with its `&mut self` and its turn.

#[cfg(test)]
pub(super) mod tests;

use super::drags::{Away, Ended};
use super::keys::{self, ESCAPE, Role, TAB};
use super::popups::{self, Pressing};
use super::{Fresh, Last, Listed, Orders, Screen, Worked};
use crate::input::{Erased, Hears, Input, Key, Pointing, Raised, Used};
use crate::table::Focus;
use crate::tree::{KeyPath, Node, Tree};
use crate::widget::Built;
use aethereal_actor::{Answered, Level};
use aethereal_data::{ActorId, Plain};
use aethereal_math::Vec2;
use aethereal_render::draw_list::{Entry, Pointer};
use aethereal_window::{AllowIme, Button, ImeAllowed, KeyEvent, Keyed, Over, Pointed, PointerEvent};

/// The window's input method was asked to compose, or to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.ime_asked")]
pub struct ImeAsked;

/// What a turn of input leaves for the screen's handler: the events its widgets raised, for the screen's methods; the
/// keys that neither its focus nor its focus keys used, for its `key`; what to ask of its window's input method; the
/// drags dropped on other actors, to mail; and the lines to log.
#[derive(Debug, Default)]
pub struct Heard {
    pub(crate) raised: Vec<Raised<Erased>>,
    pub(crate) keys: Vec<Key>,
    pub(crate) imes: Vec<AllowIme>,
    pub(crate) away: Vec<Away>,
    pub(crate) lines: Vec<(Level, String)>,
}

/// The place in a list of the entry numbered `entry`.
pub(super) fn place(entry: u32) -> usize {
    usize::try_from(entry).unwrap_or(usize::MAX)
}

impl Screen {
    /// Takes in a pointer event through an entry of the list it sent, `me` its actor: through a hit, it goes to the
    /// widget the hit is over, in the widget's own coordinates. A press of the main button there focuses the widget,
    /// when it can hold focus, and clears focus when it cannot, unless the hit passes the pointer on to what is beneath
    /// it, as a tooltip's does; and any press shuts each open popup it lands outside of. A click within a popup's
    /// content that raises an event, a choice made, shuts it when it closes so. The wheel slides the innermost scroll at or around the widget that can still
    /// move that way, on the popup content it is in. Through the backdrop behind a modal popup, a press shuts it.
    /// Through the hit over a child it places, a press into the child is taken in as one elsewhere in the screen.
    ///
    /// A press of the main button on a widget that drags grips it, a move through it past the threshold begins its
    /// drag, and the release through it lands the drag where it was over, and is no click; while a drag is under way,
    /// the pointer coming onto and leaving each hit lays the screen out, as the target it is over draws lit.
    pub fn pointed(&mut self, pointed: Pointed, me: ActorId) -> Heard {
        let mut heard = Heard::default();
        let path = match self.listing.get(place(pointed.entry)) {
            Some(Listed::Hit(path)) => path.clone(),
            Some(Listed::Backdrop(popup)) => {
                let behind = Pressing::Behind(popup.clone());
                if let PointerEvent::PointerDown { .. } = pointed.event {
                    self.pressed_in(&behind);
                }
                return heard;
            }
            Some(Listed::Child(slot)) => {
                let into = slot.clone();
                if let PointerEvent::PointerDown { button, .. } = pointed.event {
                    self.pressed_into(into, button, &mut heard);
                }
                return heard;
            }
            Some(Listed::Keys(_) | Listed::Draw(_) | Listed::Call(_)) | None => return heard,
        };
        let ended = match pointed.event {
            PointerEvent::PointerEntered | PointerEvent::PointerLeft => {
                let entered = pointed.event == PointerEvent::PointerEntered;
                self.reach(&path, entered);
                self.passed();
                Ended::Press
            }
            PointerEvent::PointerDown { x, y, button } => {
                self.pressed_in(&Pressing::At(path.clone()));
                if button == Button::Main {
                    self.grip(&path, Vec2::new(x, y));
                }
                Ended::Press
            }
            PointerEvent::PointerMoved { x, y } => {
                self.drag_to(&path, Vec2::new(x, y));
                Ended::Press
            }
            PointerEvent::PointerUp { button: Button::Main, .. } => self.let_go(&path),
            PointerEvent::PointerUp { .. } | PointerEvent::Wheel { .. } => Ended::Press,
        };
        let dragged = matches!(ended, Ended::Drag(_));
        if let (Ended::Drag(payload), PointerEvent::PointerUp { over, .. }) = (ended, pointed.event) {
            self.land(&path, payload, over, me, &mut heard);
        }
        let Some(origin) = self.origin(&path) else {
            return heard;
        };
        let at = |x: f32, y: f32| Vec2::new(x, y) - origin;
        let pointing = match pointed.event {
            PointerEvent::PointerEntered => Pointing::Entered,
            PointerEvent::PointerLeft => Pointing::Left,
            PointerEvent::PointerMoved { x, y } => Pointing::Moved { at: at(x, y) },
            PointerEvent::PointerDown { x, y, button } => Pointing::Down { at: at(x, y), button },
            PointerEvent::PointerUp { x, y, button, over } => {
                let over_it = self.is_over(&path, over, me);
                let clicks = over_it && !dragged;
                Pointing::Up { at: at(x, y), button, over: clicks }
            }
            PointerEvent::Wheel { delta_x, delta_y, .. } => {
                self.wheel(&path, Vec2::new(delta_x, delta_y));
                return heard;
            }
        };
        let pressed = matches!(pointing, Pointing::Down { button: Button::Main, .. });
        let passes =
            matches!(self.listed.get(place(pointed.entry)), Some(Entry::Hit { pointer: Pointer::Passes(_), .. }));
        let focuses = pressed && !passes;
        if focuses {
            let focusable = self.widget(&path).is_some_and(Built::focusable);
            let focus = if focusable {
                Focus::On(path.clone())
            } else {
                Focus::Nowhere
            };
            self.refocus(focus, &mut heard);
        }
        let before = heard.raised.len();
        self.input(&path, &mut heard, |widget, state, input| widget.pointed(state, pointing, input));
        let clicked = matches!(pointing, Pointing::Up { button: Button::Main, over: true, .. });
        let chose = heard.raised.len() > before;
        let chosen = clicked && chose;
        if chosen {
            self.chosen(&path);
        }

        heard
    }

    /// Takes in a press of `button` into the child placed in the slot at `slot`, as a press on the screen that reaches
    /// no widget that holds focus: the main button clears focus, stopping the window's input method, and any button
    /// shuts each open popup it lands outside of. The child hears the press itself, so the screen raises nothing.
    fn pressed_into(&mut self, slot: KeyPath, button: Button, heard: &mut Heard) {
        self.pressed_in(&Pressing::At(slot));
        if button == Button::Main {
            self.refocus(Focus::Nowhere, heard);
        }
    }

    /// Takes in a key or typed text through the keys the screen declared. A key goes to the focused widget, then, when
    /// it passed it, to the focus keys, Tab and Escape, then to the screen's own `key`; typed text goes to the focused
    /// widget. The modifiers are watched as they go down and come up, whichever declaration they come through.
    pub fn keyed(&mut self, keyed: Keyed) -> Heard {
        let mut heard = Heard::default();
        let Some(Listed::Keys(role)) = self.listing.get(place(keyed.entry)) else {
            return heard;
        };
        let screen = *role == Role::Screen;
        match keyed.event {
            KeyEvent::KeyDown { code, key, repeat } => {
                self.modifiers.pressed(&code);
                if screen {
                    let key = Key { code, key, repeat, held: self.modifiers.held() };
                    self.pressed(key, &mut heard);
                }
            }
            KeyEvent::KeyUp { code, .. } => self.modifiers.released(&code),
            KeyEvent::Text { text } if screen => {
                if let Focus::On(path) = self.table.focus().clone() {
                    self.input(&path, &mut heard, |widget, state, input| widget.typed(state, &text, input));
                }
            }
            KeyEvent::Text { .. } => {}
        }

        heard
    }

    /// Takes in the tree the screen's view built after its methods took in what input raised, so the next input reaches
    /// its widgets as they now are; the next `Prepare` lays it out.
    pub fn rebuilt(&mut self, tree: Tree) {
        self.unkept.last = Last::Built(Box::new(tree));
        self.fresh = Fresh::Stale;
    }

    /// The window's input method was changed as asked, or not: one not changed is logged.
    pub fn ime_answered(&mut self, answered: Answered<ImeAllowed>, _: ImeAsked) -> Orders {
        let mut orders = Orders::default();
        if let refused @ (Answered::Yes(ImeAllowed::Refused(_)) | Answered::No(_)) = answered {
            orders.log(Level::Debug, format!("the window's input method was not changed: {refused:?}"));
        }

        orders
    }

    /// Takes in `key`, pressed: the focused widget, each widget it is within that hears the key, innermost first and up
    /// to the popup whose content it is in, the focus keys, and the screen, in turn, until one uses it.
    fn pressed(&mut self, key: Key, heard: &mut Heard) {
        let focus = self.table.focus().clone();
        let (focused, around) = match &focus {
            Focus::On(path) => {
                let used = self.input(path, heard, |widget, state, input| widget.key(state, &key, input));
                (used, self.surfaced(path))
            }
            Focus::Nowhere => (None, Vec::new()),
        };
        if focused == Some(Used::Used) {
            return;
        }
        for within in around {
            let listens = self.widget(&within).is_some_and(|widget| widget.hears().holds(&key.code));
            if !listens {
                continue;
            }
            let used = self.input(&within, heard, |widget, state, input| widget.key(state, &key, input));
            if used == Some(Used::Used) {
                return;
            }
        }
        if self.focus_key(&key, heard) == Used::Used {
            return;
        }
        heard.keys.push(key);
    }

    /// Takes in `key` as a focus key: Tab moves focus to the next widget it reaches in tree order, and with Shift to
    /// the one before, within the topmost modal popup while one is open, and slides each scroll around it to show it;
    /// Escape shuts the topmost open popup, or with none open clears focus while a widget has it.
    fn focus_key(&mut self, key: &Key, heard: &mut Heard) -> Used {
        match key.code.as_str() {
            TAB => self.tab(key.held.shift, heard),
            ESCAPE => self.escape(heard),
            _ => Used::Passed,
        }
    }

    /// Moves focus to the next widget Tab reaches, or with `back` the one before, and brings it into view.
    fn tab(&mut self, back: bool, heard: &mut Heard) -> Used {
        let Last::Built(tree) = &self.unkept.last else {
            return Used::Passed;
        };
        let Ok(flattened) = tree.flat() else {
            return Used::Passed;
        };
        let surfaces = popups::surfaces(&flattened.nodes, &self.table);
        let reaches = popups::tabbable(&flattened.nodes, &surfaces, &self.table);
        let to = keys::tabbed(&flattened.nodes, &reaches, self.table.focus(), back);
        self.refocus(to.clone(), heard);
        if let Focus::On(path) = &to {
            self.reveal(path);
        }

        Used::Used
    }

    /// Shuts the topmost open popup, or, with none open, clears focus while a widget has it.
    fn escape(&mut self, heard: &mut Heard) -> Used {
        let shut = self.shut_topmost(heard);
        if shut == Used::Used {
            return Used::Used;
        }
        let unfocused = *self.table.focus() == Focus::Nowhere;
        if unfocused {
            return Used::Passed;
        }
        self.refocus(Focus::Nowhere, heard);

        Used::Used
    }

    /// Gives the screen's focus to `focus`: a change lays the screen out at the next `Prepare`, and allows or stops the
    /// window's input method as a widget that types gains or loses it.
    pub(super) fn refocus(&mut self, focus: Focus, heard: &mut Heard) {
        if *self.table.focus() == focus {
            return;
        }
        let typing = match &focus {
            Focus::On(path) => self.widget(path).is_some_and(|widget| widget.hears() == Hears::Typing),
            Focus::Nowhere => false,
        };
        self.table.focus_on(focus);
        self.fresh = Fresh::Stale;
        heard.imes.extend(self.typing(typing));
    }

    /// The widget at `path` in the last tree built, if it has one there.
    fn widget(&self, path: &KeyPath) -> Option<&dyn Built> {
        let Last::Built(tree) = &self.unkept.last else {
            return None;
        };

        tree.find(path).map(Node::widget)
    }

    /// Where the last layout put the widget at `path`, its border box's top left in the screen's pixels.
    fn origin(&self, path: &KeyPath) -> Option<Vec2> {
        self.unkept.placed.iter().find(|(placed, _)| placed == path).map(|(_, laid)| laid.rect.min)
    }

    /// Whether `over`, what a release was over, is the hit over the widget at `path` in the list of `me`.
    fn is_over(&self, path: &KeyPath, over: Over, me: ActorId) -> bool {
        let Over::Entry { actor, entry, .. } = over else {
            return false;
        };
        let ours = actor.id() == me;
        let listed = self.listing.get(place(entry));
        let this = listed == Some(&Listed::Hit(path.clone()));

        ours && this
    }

    /// Hands input to the widget at `path`, with its state as the table keeps it, or first made, and where layout put
    /// it, and answers what it said: none when the last tree or layout has no widget there. A state it changed is kept,
    /// and lays the screen out at the next `Prepare`; what it raised is handed back in `heard`.
    fn input<T>(
        &mut self,
        path: &KeyPath,
        heard: &mut Heard,
        take: impl FnOnce(&dyn Built, &mut Vec<u8>, &mut Input<'_, '_>) -> T,
    ) -> Option<T> {
        let Worked { last: Last::Built(tree), fonts, placed, .. } = &*self.unkept else {
            return None;
        };
        let node = tree.find(path)?;
        let (_, laid) = placed.iter().find(|(placed, _)| placed == path)?;
        let widget = node.widget();
        let mut state = self.table.bytes(path).map_or_else(|| widget.first_state(), <[u8]>::to_vec);
        let before = state.clone();
        let mut measuring = fonts.measuring(self.viewport.scale);
        let mut input = Input::new(node.style(), *laid, &mut measuring, &mut heard.raised);
        let took = take(widget, &mut state, &mut input);
        if state != before {
            self.table.write(path, widget.state_kind(), state);
            self.fresh = Fresh::Stale;
        }

        Some(took)
    }
}
