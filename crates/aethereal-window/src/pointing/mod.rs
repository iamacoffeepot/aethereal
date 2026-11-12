//! The pointer's cascade (ADR-0055): a window's pointer events delivered top-down through what was drawn under the
//! pointer, in the declared order of the draw calls, until one blocks.
//!
//! The renderer subscribes to a window's probes, [`SubscribeProbes`], and is sent each pointer event's place, numbered,
//! [`Probed`]. It finds, at the pointer and at each press and release, which entries of which actors' draw lists drew
//! there, the last drawn on top, and answers the window a frame or two later, [`Picked`]. The window keeps each pointer
//! event until the stacks of a frame drawn after it come back, and delivers it then, as [`Pointed`], to the actor whose
//! list holds each entry of the stack, from the top, until the first that blocks: so what is seen under the pointer is
//! what hears it. Each entry hears the pointer in its own coordinates: the place it is told of is the window's, less
//! the origin its list was drawn at, so a child its parent places hears clicks as it draws, from its own `0, 0`.
//!
//! - **Presses alone:** an entry that hears only presses, [`Hearing::Presses`], is sent a press that reaches it, and
//!   passes it on to what is beneath it; nothing else reaches it. The pointer's stack is read as though it were not
//!   there, for hover, moves, the wheel, and the cursor; a release does not reach it, nor is it over it; and no capture
//!   holds it.
//! - **Hover:** when the entries the pointer's stack reaches change, those it no longer reaches are sent
//!   [`PointerEvent::PointerLeft`], and those it now reaches [`PointerEvent::PointerEntered`].
//! - **Capture:** the entries a press reached are sent every move and release until every button is up, wherever the
//!   pointer goes, each in the coordinates its list was drawn in when the press reached it.
//! - **A release says what it was over:** the topmost entry of the stack at the release's own place, as though no
//!   button were held, with the place in that entry's coordinates, or that nothing reachable was there, [`Over`]. So an
//!   actor that began a drag learns where it was dropped, whatever holds the capture and wherever the pointer went
//!   after.
//! - **The wheel follows the pointer**, never a capture.
//! - **The cursor follows the same stacks:** the window shows the [`Cursor`] of the topmost entry the pointer's stack
//!   reaches, or the default over nothing; while a button is held, that of the topmost entry the press reached,
//!   wherever the pointer goes, as the press's stack showed it, and as each later stack under the pointer that holds
//!   it shows it, so an entry that declares another cursor once its drag begins shows it; once every button is up, the
//!   pointer's again.
//!
//! The raw subscription stays: every subscriber of the window is sent every pointer event as its host heard it.
//!
//! The window's keys cascade the same way, through the keys the draw lists declare (`keys`).

#[cfg(test)]
pub mod tests;

use crate::cursor::Cursor;
use crate::event::{Button, Event};
use crate::window::WindowId;
use aethereal_actor::ProtocolRef;
use aethereal_data::{Bound, Plain};
use aethereal_math::Vec2;
use std::collections::VecDeque;

/// Asks a window's actor, at `path::<Window>(window)`, for its probes: from the next pointer event it hears, the sender
/// is sent [`Probed`] for each, and answers what was drawn there with [`Picked`]. The renderer asks it of each window it
/// draws to. A second ask from an actor that asked already sends nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.subscribe_probes")]
pub struct SubscribeProbes;

/// A pointer event a window heard, numbered `at`, from 1, in the order the window heard them: where the pointer was,
/// in the window's surface pixels, and whether its place is pinned, as a press's and a release's are: probed whatever
/// the pointer does after it.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.window.probed")]
pub struct Probed {
    pub window: WindowId,
    pub at: u64,
    pub x: f32,
    pub y: f32,
    pub pinned: bool,
}

/// What a frame drew at a window's probes, sent to the window's actor by the subscriber of its probes: the stack under
/// the pointer's newest place, and under each place pinned, each from the top, of every pointer event up to the one
/// numbered `through`. The window delivers each of those events through them.
#[derive(Debug, Clone, PartialEq, Bound)]
#[kind(name = "aether.window.picked")]
pub struct Picked {
    pub through: u64,
    pub pointer: Vec<Reach>,
    pub pinned: Vec<Pinned>,
}

/// The stack under the place of the press or release numbered `at`, from the top.
#[derive(Debug, Clone, PartialEq, Bound)]
#[kind(name = "aether.window.pinned")]
pub struct Pinned {
    pub at: u64,
    pub stack: Vec<Reach>,
}

/// One entry of a stack: the entry numbered `entry`, from 0, of the draw list `actor` sent, the origin its list was
/// drawn at, in the window's surface pixels, which the places it is told of are given from, and what of the pointer it
/// hears.
#[derive(Debug, Clone, PartialEq, Bound)]
#[kind(name = "aether.window.reach")]
pub struct Reach {
    pub actor: ProtocolRef<Hearer>,
    pub entry: u32,
    pub origin: Vec2,
    pub hearing: Hearing,
}

/// What of the pointer an entry of a stack hears.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.hearing")]
pub enum Hearing {
    /// Every pointer event that reaches it: whether it blocks the pointer from what is beneath it, and the cursor it
    /// declares, which the window shows while it is the top.
    Everything { blocks: bool, cursor: Cursor },
    /// A press alone, which it passes on to what is beneath it: no move, release, wheel, or hover reaches it, no capture
    /// holds it, and it declares no cursor, so the pointer's stack is read as though it were not there. A parent lists
    /// it over a child it calls, so a press into the child reaches the parent as well.
    Presses,
}

/// What an actor whose draw list holds an entry the pointer reaches handles.
pub type Hearer = (Pointed,);

/// A pointer event, sent to an actor through the entry numbered `entry` of its draw list, which the pointer reached in
/// `window`'s stack: so the actor need not test its own widgets again.
#[derive(Debug, Clone, Copy, PartialEq, Bound)]
#[kind(name = "aether.window.pointed")]
pub struct Pointed {
    pub window: WindowId,
    pub entry: u32,
    pub event: PointerEvent,
}

/// What the pointer did at an entry, positions in the coordinates its list was drawn in: the window's surface pixels,
/// as [`Event`]'s are, less the origin its list was drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Bound)]
pub enum PointerEvent {
    /// The pointer moved, to `x`, `y`.
    PointerMoved { x: f32, y: f32 },
    /// A button went down, at `x`, `y`: every move and release is sent here until every button is up.
    PointerDown { x: f32, y: f32, button: Button },
    /// A button came up, at `x`, `y`, over `over`: what the pointer reaches there, whoever the release is sent to.
    PointerUp { x: f32, y: f32, button: Button, over: Over },
    /// The wheel turned, with the pointer at `x`, `y`, as [`Event::Wheel`] says.
    Wheel { x: f32, y: f32, delta_x: f32, delta_y: f32 },
    /// The pointer now reaches the entry.
    PointerEntered,
    /// The pointer no longer reaches the entry.
    PointerLeft,
}

/// What a release was over: the topmost entry of the stack at the release's own place, whether it blocks the pointer
/// or lets it through, as though no button were held, so it may be an entry the release is sent to; or nothing the
/// pointer reaches. An entry the pointer ignores is never in a stack, and one that hears presses alone is passed over.
///
/// The stack is that of the first frame drawn after the release whose stacks came back, at the release's own place,
/// wherever the pointer went after it.
#[derive(Debug, Clone, Copy, PartialEq, Bound)]
pub enum Over {
    /// Nothing the pointer reaches was under it.
    Nothing,
    /// The entry numbered `entry` of the draw list `actor` sent, which the receiver may cast to a protocol it mails,
    /// and the release's place, `x`, `y`, in the coordinates that list was drawn in, as `actor` would hear it.
    Entry { actor: ProtocolRef<Hearer>, entry: u32, x: f32, y: f32 },
}

/// The most pointer events a window keeps waiting for their stacks. Far more than a frame or two brings, it bounds
/// what a window no frame draws keeps: past it, the oldest is dropped, as nothing was shown there to point at.
const MOST_WAITING: usize = 1024;

/// An entry a pointer event is delivered to: an entry of an actor's draw list, with the origin its list was drawn at,
/// in the window's surface pixels.
#[derive(Debug, Clone, Copy, PartialEq, Bound)]
struct Reached {
    actor: ProtocolRef<Hearer>,
    entry: u32,
    origin: Vec2,
}

/// Who a window's pointer events go to while a button is down, and what the window shows.
#[derive(Debug, Clone, PartialEq, Bound)]
enum Capture {
    /// No button is down: each goes as the stacks say, and the cursor is the top of the pointer's stack's.
    Free,
    /// `buttons` are down, and every move and release goes to what the first of them reached, `reached`, at the
    /// origins they were drawn at when it reached them; the cursor is `cursor`, that of the topmost entry it reached,
    /// as a stack last showed it, or the default when it reached nothing.
    Held { reached: Vec<Reached>, buttons: Vec<Button>, cursor: Cursor },
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
        let (x, y, pinned) = match event {
            Event::PointerMoved { x, y } | Event::Wheel { x, y, .. } => (*x, *y, false),
            Event::PointerDown { x, y, .. } | Event::PointerUp { x, y, .. } => (*x, *y, true),
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

        Some(Probed { window, at, x, y, pinned })
    }

    /// Takes in what a frame drew at `window`'s probes, and answers each pointer event it delivers, in order: first
    /// what the pointer no longer and now reaches is told so, then each event waiting up to `picked.through`, through
    /// the stack at its own place, the capture, or the pointer's stack. What the window shows at the pointer follows,
    /// as [`cursor`](Self::cursor) answers.
    pub fn picked(&mut self, window: WindowId, picked: Picked) -> Vec<(ProtocolRef<Hearer>, Pointed)> {
        let Picked { through, pointer, pinned } = picked;
        let mut deliveries = Vec::new();
        let mut send = |reached: &[Reached], event: PointerEvent| {
            for to in reached {
                deliveries.push((to.actor, Pointed { window, entry: to.entry, event: event.relative_to(to.origin) }));
            }
        };

        let reached = cascade(&pointer);
        let left: Vec<Reached> = self.hovered.iter().filter(|was| !holds(&reached, was)).copied().collect();
        let entered: Vec<Reached> = reached.iter().filter(|now| !holds(&self.hovered, now)).copied().collect();
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
                    let stack = own_stack(&pinned, &self.pointer, at);
                    let reached = self.capture.press(stack, button);
                    send(&reached, PointerEvent::PointerDown { x, y, button });
                }
                Event::PointerUp { x, y, button } => {
                    let stack = own_stack(&pinned, &self.pointer, at);
                    let over = Over::top_of(stack, x, y);
                    let reached = self.capture.release(stack, button);
                    send(&reached, PointerEvent::PointerUp { x, y, button, over });
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
        self.capture.seen(&self.pointer);

        deliveries
    }

    /// The cursor the window shows: while a button is held, that of the topmost entry the press reached, as a stack
    /// last showed it; otherwise that of the top of the newest stack under the pointer, an entry that hears presses
    /// alone left out; and the default over nothing the pointer reaches.
    #[must_use]
    pub fn cursor(&self) -> &Cursor {
        match &self.capture {
            Capture::Held { cursor, .. } => cursor,
            Capture::Free => everything(&self.pointer).next().map_or(&Cursor::Default, |(_, _, cursor)| cursor),
        }
    }
}

impl PointerEvent {
    /// The event as an entry drawn with its list's origin at `origin` hears it: each place less the origin, but for what
    /// a release was over, which is in its own entry's coordinates whoever hears it.
    #[must_use]
    fn relative_to(self, origin: Vec2) -> Self {
        let local = |x: f32, y: f32| (x - origin.x, y - origin.y);
        match self {
            Self::PointerMoved { x, y } => {
                let (x, y) = local(x, y);
                Self::PointerMoved { x, y }
            }
            Self::PointerDown { x, y, button } => {
                let (x, y) = local(x, y);
                Self::PointerDown { x, y, button }
            }
            Self::PointerUp { x, y, button, over } => {
                let (x, y) = local(x, y);
                Self::PointerUp { x, y, button, over }
            }
            Self::Wheel { x, y, delta_x, delta_y } => {
                let (x, y) = local(x, y);
                Self::Wheel { x, y, delta_x, delta_y }
            }
            Self::PointerEntered | Self::PointerLeft => self,
        }
    }
}

impl Over {
    /// What a release at `x`, `y`, in the window's surface pixels, was over: the top of `stack`, the one at its place,
    /// an entry that hears presses alone left out, at the place less the origin its list was drawn at; or nothing,
    /// when the stack holds no other.
    fn top_of(stack: &[Reach], x: f32, y: f32) -> Self {
        everything(stack).next().map_or(Self::Nothing, |(top, _, _)| Self::Entry {
            actor: top.actor,
            entry: top.entry,
            x: x - top.origin.x,
            y: y - top.origin.y,
        })
    }
}

impl Reached {
    /// The entry `reach` is, drawn where it was.
    const fn of(reach: &Reach) -> Self {
        Self { actor: reach.actor, entry: reach.entry, origin: reach.origin }
    }

    /// Whether it is the same entry of the same actor's list as `other`, wherever either was drawn: an entry its parent
    /// moves is still the one the pointer is over.
    fn same_entry(&self, other: &Self) -> bool {
        let same_actor = self.actor == other.actor;
        let same_entry = self.entry == other.entry;
        same_actor && same_entry
    }

    /// Whether `reach` is this entry of this actor's list, wherever it was drawn.
    fn is(&self, reach: &Reach) -> bool {
        let same_actor = self.actor == reach.actor;
        let same_entry = self.entry == reach.entry;
        same_actor && same_entry
    }
}

/// The stack under the press or release numbered `at`, of those `pinned`: its own, or, past the most places a frame
/// probes, the pointer's, `pointer`.
fn own_stack<'a>(pinned: &'a [Pinned], pointer: &'a [Reach], at: u64) -> &'a [Reach] {
    let own = pinned.iter().find(|place| place.at == at);

    own.map_or(pointer, |place| place.stack.as_slice())
}

/// Whether `reached` holds the entry `entry`, wherever it was drawn.
fn holds(reached: &[Reached], entry: &Reached) -> bool {
    reached.iter().any(|held| held.same_entry(entry))
}

impl Capture {
    /// The entries a press of `button` is sent to: those it reaches through `stack`, which it captures but for those
    /// that hear presses alone, or with a button down already, those captured.
    fn press(&mut self, stack: &[Reach], button: Button) -> Vec<Reached> {
        match self {
            Self::Free => {
                let (sent, captured) = pressed(stack);
                let cursor = everything(stack).next().map_or(Cursor::Default, |(_, _, cursor)| cursor.clone());
                *self = Self::Held { reached: captured, buttons: vec![button], cursor };
                sent
            }
            Self::Held { reached, buttons, .. } => {
                buttons.push(button);
                reached.clone()
            }
        }
    }

    /// The entries a release of `button` is sent to: those captured, freed as the last button comes up, or with no
    /// capture, those it reaches through the stack at its place, `stack`.
    fn release(&mut self, stack: &[Reach], button: Button) -> Vec<Reached> {
        let Self::Held { reached, buttons, .. } = self else {
            return cascade(stack);
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

    /// Takes in the newest stack under the pointer, `pointer`: while a button is held, the cursor the topmost entry
    /// the press reached declares there, when the stack holds it, as one that begins a drag declares the image it
    /// drags. Where the stack does not hold it, the cursor it last showed stands.
    fn seen(&mut self, pointer: &[Reach]) {
        let Self::Held { reached, cursor, .. } = self else {
            return;
        };
        let Some(pressed) = reached.first() else {
            return;
        };
        let shown = everything(pointer).find(|(reach, _, _)| pressed.is(reach));
        if let Some((_, _, shown)) = shown {
            cursor.clone_from(shown);
        }
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

/// The entries of `stack` that hear every pointer event, from its top, each with whether it blocks the pointer and the
/// cursor it declares: the stack as all but a press reads it, those that hear presses alone left out.
fn everything(stack: &[Reach]) -> impl Iterator<Item = (&Reach, bool, &Cursor)> {
    stack.iter().filter_map(|reach| match &reach.hearing {
        Hearing::Everything { blocks, cursor } => Some((reach, *blocks, cursor)),
        Hearing::Presses => None,
    })
}

/// The entries an event other than a press reaches through `stack`, from its top: each that hears every pointer event,
/// until the first that blocks, which is reached too.
fn cascade(stack: &[Reach]) -> Vec<Reached> {
    let mut reached = Vec::new();
    for (reach, blocks, _) in everything(stack) {
        reached.push(Reached::of(reach));
        if blocks {
            break;
        }
    }

    reached
}

/// The entries a press reaches through `stack`, from its top, until the first that blocks, which is reached too, and
/// of those, the ones it captures: each but those that hear presses alone, which pass it on.
fn pressed(stack: &[Reach]) -> (Vec<Reached>, Vec<Reached>) {
    let mut sent = Vec::new();
    let mut captured = Vec::new();
    for reach in stack {
        let reached = Reached::of(reach);
        sent.push(reached);
        let Hearing::Everything { blocks, .. } = reach.hearing else {
            continue;
        };
        captured.push(reached);
        if blocks {
            break;
        }
    }

    (sent, captured)
}
