//! The pointer's cascade (ADR-0055): a window's pointer events delivered top-down through what was drawn under the
//! pointer, in the declared order of the draw calls, until one blocks.
//!
//! The renderer subscribes to a window's probes, [`SubscribeProbes`], and is sent each pointer event's place,
//! numbered, [`Probed`]. It finds, at the pointer and at each press, which entries of which actors' draw lists drew
//! there, the last drawn on top, and answers the window a frame or two later, [`Picked`]. The window keeps each pointer
//! event until the stacks of a frame drawn after it come back, and delivers it then, as [`Pointed`], to the actor whose
//! list holds each entry of the stack, from the top, until the first that blocks: so what is seen under the pointer is
//! what hears it.
//!
//! - **Hover:** when the entries the pointer's stack reaches change, those it no longer reaches are sent
//!   [`PointerEvent::PointerLeft`], and those it now reaches [`PointerEvent::PointerEntered`].
//! - **Capture:** the entries a press reached are sent every move and release until every button is up, wherever the
//!   pointer goes.
//! - **The wheel follows the pointer**, never a capture.
//!
//! The raw subscription stays: every subscriber of the window is sent every pointer event as its host heard it.
//!
//! The window's keys cascade the same way, through the keys the draw lists declare (`keys`).

#[cfg(test)]
pub mod tests;

use crate::event::{Button, Event};
use crate::window::WindowId;
use aethereal_actor::ProtocolRef;
use aethereal_data::{Bound, Plain};
use std::collections::VecDeque;

/// Asks a window's actor, at `path::<Window>(window)`, for its probes: from the next pointer event it hears, the sender
/// is sent [`Probed`] for each, and answers what was drawn there with [`Picked`]. The renderer asks it of each window it
/// draws to. A second ask from an actor that asked already sends nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.subscribe_probes")]
pub struct SubscribeProbes;

/// A pointer event a window heard, numbered `at`, from 1, in the order the window heard them: where the pointer was,
/// in the window's surface pixels, and whether it was a press, whose place is probed whatever the pointer does after
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.window.probed")]
pub struct Probed {
    pub window: WindowId,
    pub at: u64,
    pub x: f32,
    pub y: f32,
    pub press: bool,
}

/// What a frame drew at a window's probes, sent to the window's actor by the subscriber of its probes: the stack under
/// the pointer's newest place, and under each press probed, each from the top, of every pointer event up to the one
/// numbered `through`. The window delivers each of those events through them.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.picked")]
pub struct Picked {
    pub through: u64,
    pub pointer: Vec<Reach>,
    pub presses: Vec<Pressed>,
}

/// The stack under the press numbered `at`, from the top.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.pressed")]
pub struct Pressed {
    pub at: u64,
    pub stack: Vec<Reach>,
}

/// One entry of a stack: the entry numbered `entry`, from 0, of the draw list `actor` sent, and whether it blocks the
/// pointer from what is beneath it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.reach")]
pub struct Reach {
    pub actor: ProtocolRef<Hearer>,
    pub entry: u32,
    pub blocks: bool,
}

/// What an actor whose draw list holds an entry the pointer reaches handles.
pub type Hearer = (Pointed,);

/// A pointer event, sent to an actor through the entry numbered `entry` of its draw list, which the pointer reached in
/// `window`'s stack: so the actor need not test its own widgets again.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.window.pointed")]
pub struct Pointed {
    pub window: WindowId,
    pub entry: u32,
    pub event: PointerEvent,
}

/// What the pointer did at an entry, positions in the window's surface pixels, as [`Event`]'s are.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub enum PointerEvent {
    /// The pointer moved, to `x`, `y`.
    PointerMoved { x: f32, y: f32 },
    /// A button went down, at `x`, `y`: every move and release is sent here until every button is up.
    PointerDown { x: f32, y: f32, button: Button },
    /// A button came up, at `x`, `y`.
    PointerUp { x: f32, y: f32, button: Button },
    /// The wheel turned, with the pointer at `x`, `y`, as [`Event::Wheel`] says.
    Wheel { x: f32, y: f32, delta_x: f32, delta_y: f32 },
    /// The pointer now reaches the entry.
    PointerEntered,
    /// The pointer no longer reaches the entry.
    PointerLeft,
}

/// The most pointer events a window keeps waiting for their stacks. Far more than a frame or two brings, it bounds
/// what a window no frame draws keeps: past it, the oldest is dropped, as nothing was shown there to point at.
const MOST_WAITING: usize = 1024;

/// An entry a pointer event is delivered to: an entry of an actor's draw list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
struct Reached {
    actor: ProtocolRef<Hearer>,
    entry: u32,
}

/// Who a window's pointer events go to while a button is down.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
enum Capture {
    /// No button is down: each goes as the stacks say.
    Free,
    /// `buttons` are down, and every move and release goes to what the first of them reached, `reached`.
    Held { reached: Vec<Reached>, buttons: Vec<Button> },
}

/// A window's cascade, as its actor keeps it: how many pointer events it has numbered, those waiting for their stacks,
/// the newest stack under the pointer, the entries it reached, and the capture.
#[derive(Debug, Bound)]
pub struct Cascade {
    numbered: u64,
    waiting: VecDeque<(u64, Event)>,
    pointer: Vec<Reach>,
    hovered: Vec<Reached>,
    capture: Capture,
}

/// Nothing numbered, nothing under the pointer, and no button down.
impl Default for Cascade {
    fn default() -> Self {
        Self { numbered: 0, waiting: VecDeque::new(), pointer: Vec::new(), hovered: Vec::new(), capture: Capture::Free }
    }
}

impl Cascade {
    /// Takes in `event`, heard by `window`: a pointer event is numbered, kept until its stacks come back, and answered
    /// as the probe it is; any other is not the cascade's.
    pub fn heard(&mut self, window: WindowId, event: &Event) -> Option<Probed> {
        let (x, y, press) = match event {
            Event::PointerMoved { x, y } | Event::PointerUp { x, y, .. } | Event::Wheel { x, y, .. } => (*x, *y, false),
            Event::PointerDown { x, y, .. } => (*x, *y, true),
            Event::KeyDown { .. }
            | Event::KeyUp { .. }
            | Event::Text { .. }
            | Event::Resized(_)
            | Event::Moded(_)
            | Event::FocusGained
            | Event::FocusLost
            | Event::CloseRequested
            | Event::Closed => return None,
        };
        self.numbered += 1;
        let at = self.numbered;
        let is_full = self.waiting.len() >= MOST_WAITING;
        if is_full {
            self.waiting.pop_front();
        }
        self.waiting.push_back((at, event.clone()));

        Some(Probed { window, at, x, y, press })
    }

    /// Takes in what a frame drew at `window`'s probes, and answers each pointer event it delivers, in order: first
    /// what the pointer no longer and now reaches is told so, then each event waiting up to `picked.through`, through
    /// its press's stack, the capture, or the pointer's stack.
    pub fn picked(&mut self, window: WindowId, picked: Picked) -> Vec<(ProtocolRef<Hearer>, Pointed)> {
        let Picked { through, pointer, presses } = picked;
        let mut deliveries = Vec::new();
        let mut send = |reached: &[Reached], event: PointerEvent| {
            for to in reached {
                deliveries.push((to.actor, Pointed { window, entry: to.entry, event }));
            }
        };

        let reached = cascade(&pointer);
        let left: Vec<Reached> = self.hovered.iter().filter(|was| !reached.contains(was)).copied().collect();
        let entered: Vec<Reached> = reached.iter().filter(|now| !self.hovered.contains(now)).copied().collect();
        send(&left, PointerEvent::PointerLeft);
        send(&entered, PointerEvent::PointerEntered);
        self.hovered = reached;
        self.pointer = pointer;

        while let Some((at, _)) = self.waiting.front() {
            let is_due = *at <= through;
            if !is_due {
                break;
            }
            let Some((at, event)) = self.waiting.pop_front() else {
                break;
            };
            match event {
                Event::PointerDown { x, y, button } => {
                    let pressed = presses.iter().find(|pressed| pressed.at == at);
                    let stack = pressed.map_or(self.pointer.as_slice(), |pressed| pressed.stack.as_slice());
                    let reached = self.capture.press(stack, button);
                    send(&reached, PointerEvent::PointerDown { x, y, button });
                }
                Event::PointerUp { x, y, button } => {
                    let reached = self.capture.release(&self.pointer, button);
                    send(&reached, PointerEvent::PointerUp { x, y, button });
                }
                Event::PointerMoved { x, y } => {
                    let reached = self.capture.held(&self.pointer);
                    send(&reached, PointerEvent::PointerMoved { x, y });
                }
                Event::Wheel { x, y, delta_x, delta_y } => {
                    send(&cascade(&self.pointer), PointerEvent::Wheel { x, y, delta_x, delta_y });
                }
                Event::KeyDown { .. }
                | Event::KeyUp { .. }
                | Event::Text { .. }
                | Event::Resized(_)
                | Event::Moded(_)
                | Event::FocusGained
                | Event::FocusLost
                | Event::CloseRequested
                | Event::Closed => {}
            }
        }

        deliveries
    }
}

impl Capture {
    /// The entries a press of `button` is sent to: those it reaches through `stack`, which it captures, or with a
    /// button down already, those captured.
    fn press(&mut self, stack: &[Reach], button: Button) -> Vec<Reached> {
        match self {
            Self::Free => {
                let reached = cascade(stack);
                *self = Self::Held { reached: reached.clone(), buttons: vec![button] };
                reached
            }
            Self::Held { reached, buttons } => {
                buttons.push(button);
                reached.clone()
            }
        }
    }

    /// The entries a release of `button` is sent to: those captured, freed as the last button comes up, or with no
    /// capture, those it reaches through the pointer's stack, `pointer`.
    fn release(&mut self, pointer: &[Reach], button: Button) -> Vec<Reached> {
        let Self::Held { reached, buttons } = self else {
            return cascade(pointer);
        };
        let held = reached.clone();
        if let Some(at) = buttons.iter().position(|down| *down == button) {
            buttons.remove(at);
        }
        if buttons.is_empty() {
            *self = Self::Free;
        }

        held
    }

    /// The entries a move is sent to: those captured, or with no capture, those it reaches through the pointer's
    /// stack, `pointer`.
    fn held(&self, pointer: &[Reach]) -> Vec<Reached> {
        match self {
            Self::Free => cascade(pointer),
            Self::Held { reached, .. } => reached.clone(),
        }
    }
}

/// The entries an event reaches through `stack`, from its top: each, until the first that blocks, which is reached too.
fn cascade(stack: &[Reach]) -> Vec<Reached> {
    let mut reached = Vec::new();
    for reach in stack {
        reached.push(Reached { actor: reach.actor, entry: reach.entry });
        if reach.blocks {
            break;
        }
    }

    reached
}
