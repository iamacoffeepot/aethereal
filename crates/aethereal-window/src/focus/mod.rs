//! Key focus (ADR-0053): which of a window's subscribers its keys and typed text are sent to.
//!
//! Each window's actor keeps one slot, empty or held by one actor. While it is held, the window's `KeyDown`, `KeyUp`,
//! and `Text`, the input method's commits among them (ADR-0051), are sent only to the holder, when it is subscribed;
//! every other event is sent to every subscriber. A key that is down keeps whom its press was sent to, and its repeats
//! and its release are sent to them, so a character walking as a console takes the keys still hears its key come up.
//!
//! It is not the window's focus, which says whether the window has the keyboard at all (`FocusGained`, `FocusLost`).

#[cfg(test)]
mod tests;

use crate::event::Event;
use crate::window::WindowId;
use aethereal_actor::{ErasedActorRef, ProtocolRef};
use aethereal_data::{Bound, Plain};
use std::collections::BTreeMap;

/// Asks a window's actor, at `path::<Window>(window)`, for the window's key focus: from the next event, its keys and
/// typed text are sent only to the sender, which subscribes to the window to hear them. The latest take wins: the holder
/// it replaces is sent [`KeyFocusLost`], then the sender [`KeyFocusGained`]. A take by the holder sends nothing. It
/// is not answered.
///
/// The sender handles both notices. Its type must handle [`KeyFocusGained`] to declare the send, as the window replies
/// with it; one whose type does not handle [`KeyFocusLost`] as well is refused as the take is handled, and logged. A
/// console that handles both declares the take:
///
/// ```
/// use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Path, Tells, Top, Turn};
/// use aethereal_data::Plain;
/// use aethereal_window::{KeyFocusGained, KeyFocusLost, TakeKeyFocus, Window};
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.console")]
/// struct Console;
///
/// impl ChildOf for Console {
///     type Parent = Top;
/// }
///
/// impl Actor for Console {
///     type Config = Self;
///     type Key = ();
///     type Handles = (KeyFocusGained, KeyFocusLost);
///     type BeginSends = (Tells<Path<Window>, TakeKeyFocus>,);
///     type EndSends = ();
///
///     fn begin(console: Self, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(console)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Handles<KeyFocusGained> for Console {
///     type Sends = ();
///
///     fn handle(&mut self, _: KeyFocusGained, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// impl Handles<KeyFocusLost> for Console {
///     type Sends = ();
///
///     fn handle(&mut self, _: KeyFocusLost, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
///
/// One that handles neither does not build:
///
/// ```compile_fail
/// use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Path, Tells, Top};
/// use aethereal_data::Plain;
/// use aethereal_window::{TakeKeyFocus, Window};
///
/// #[derive(Plain)]
/// #[kind(name = "aether.example.console")]
/// struct Console;
///
/// impl ChildOf for Console {
///     type Parent = Top;
/// }
///
/// impl Actor for Console {
///     type Config = Self;
///     type Key = ();
///     type Handles = ();
///     type BeginSends = (Tells<Path<Window>, TakeKeyFocus>,);
///     type EndSends = ();
///
///     fn begin(console: Self, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
///         Ok(console)
///     }
///
///     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.take_key_focus")]
pub struct TakeKeyFocus;

/// Asks a window's actor to empty its key focus slot, when the sender holds it: the sender is sent [`KeyFocusLost`],
/// and the window's keys and typed text are sent to every subscriber again. Nothing is handed back to a holder the
/// sender replaced. From an actor that does not hold the slot, nothing changes and nothing is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.release_key_focus")]
pub struct ReleaseKeyFocus;

/// Sent to the actor whose take made it the holder of `window`'s key focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.key_focus_gained")]
pub struct KeyFocusGained {
    pub window: WindowId,
}

/// Sent to the actor that held `window`'s key focus, when it no longer does: another actor took it, it released it, or
/// the window closed. An actor that ends while holding it is sent nothing, and the slot is emptied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.key_focus_lost")]
pub struct KeyFocusLost {
    pub window: WindowId,
}

/// What a key focus holder handles: the two notices a window sends it.
pub type KeyFocusHolder = (KeyFocusGained, KeyFocusLost);

/// The most keys a window keeps a record of as down. Far more than a keyboard has, it bounds what performed keys that
/// never come up can hold (ADR-0046); a key pressed past it is sent as the slot then says, its release too.
const MOST_KEYS_DOWN: usize = 256;

/// Whom a window's keys are sent to: every subscriber, while the slot is empty, or its holder. It is the whole of the
/// slot, and each key that is down keeps a copy of the one its press was sent under.
#[derive(Bound, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// Nobody holds it: every subscriber is sent the window's keys.
    Empty,
    /// This actor holds it, and only it is sent the window's keys.
    Held(ProtocolRef<KeyFocusHolder>),
}

impl Slot {
    /// Whether `actor` holds it.
    fn is_held_by(self, actor: ErasedActorRef) -> bool {
        match self {
            Self::Empty => false,
            Self::Held(holder) => holder.erase() == actor,
        }
    }
}

/// What a take did to the slot.
pub enum Took {
    /// The taker held it already: nothing changed.
    Kept,
    /// The taker holds the empty slot.
    Gained,
    /// The taker holds the slot this actor held.
    Replaced(ProtocolRef<KeyFocusHolder>),
}

/// A window's key focus, as its actor keeps it: its slot, and each key that is down, by its `code`, with the slot its
/// press was sent under.
#[derive(Bound)]
pub struct KeyFocus {
    slot: Slot,
    down: BTreeMap<String, Slot>,
}

/// An empty slot, and no key down.
impl Default for KeyFocus {
    fn default() -> Self {
        Self { slot: Slot::Empty, down: BTreeMap::new() }
    }
}

impl KeyFocus {
    /// The slot as it stands.
    pub const fn slot(&self) -> Slot {
        self.slot
    }

    /// Gives the slot to `taker`, the latest take winning, and answers what that did to it.
    pub fn take(&mut self, taker: ProtocolRef<KeyFocusHolder>) -> Took {
        let previous = self.slot;
        self.slot = Slot::Held(taker);
        match previous {
            Slot::Empty => Took::Gained,
            Slot::Held(holder) if holder == taker => Took::Kept,
            Slot::Held(holder) => Took::Replaced(holder),
        }
    }

    /// Empties the slot when `actor` holds it, and answers whether it did. Keys down keep whom their presses were sent
    /// to.
    pub fn release(&mut self, actor: ErasedActorRef) -> bool {
        let holds = self.slot.is_held_by(actor);
        if holds {
            self.slot = Slot::Empty;
        }

        holds
    }

    /// Whom `event` is sent to, taking in what it says of the keys: a press is recorded under the slot as it stands,
    /// unless its key is down already, and is sent as its record says, as a repeat is; a release is sent as its press
    /// was, and its record dropped; typed text is sent as the slot stands. The window losing the keyboard forgets every
    /// key down, as their releases never come. Every other event is sent to every subscriber.
    pub fn route(&mut self, event: &Event) -> Slot {
        match event {
            Event::KeyDown { code, .. } => self.press(code),
            Event::KeyUp { code, .. } => self.down.remove(code).unwrap_or(self.slot),
            Event::Text { .. } => self.slot,
            Event::FocusLost => {
                self.down.clear();
                Slot::Empty
            }
            Event::PointerMoved { .. }
            | Event::PointerDown { .. }
            | Event::PointerUp { .. }
            | Event::Wheel { .. }
            | Event::Resized(_)
            | Event::Moded(_)
            | Event::FocusGained
            | Event::CloseRequested
            | Event::Closed => Slot::Empty,
        }
    }

    /// Records the key `code` as down under the slot as it stands, unless it is down already or the record is full,
    /// and answers whom the press is sent to.
    fn press(&mut self, code: &str) -> Slot {
        if let Some(pressed) = self.down.get(code) {
            return *pressed;
        }
        let is_full = self.down.len() >= MOST_KEYS_DOWN;
        if !is_full {
            self.down.insert(String::from(code), self.slot);
        }

        self.slot
    }
}
