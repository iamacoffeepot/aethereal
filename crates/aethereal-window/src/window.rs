//! A window's actor: beneath the window root, keyed by the window's id, it sends what its window heard to the actors
//! that subscribed, and keeps how the window stands for a new one.

use crate::event::{Event, Subscribe, Surface, Unsubscribe};
use crate::windows::Windows;
use aethereal_actor::{Actor, Beginning, ChildOf, Ended, Ending, Fault, Handles, Level, Subscribers, ToAny, Turn};
use aethereal_data::{Bound, KeyName, Kind, KindName, Plain, TypedKey};
use core::mem;

/// A window, by the number the window capability gave it as its host opened it: the first `0`, the next `1`. It is
/// the key of the window's actor, and never a host's own name for the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct WindowId(u64);

impl WindowId {
    /// The window opened `nth`, from `0`.
    pub(crate) const fn nth(nth: u64) -> Self {
        Self(nth)
    }
}

/// Written as its number.
impl TypedKey for WindowId {
    fn to_name(&self) -> KeyName {
        self.0.to_name()
    }

    fn from_name(name: &KeyName) -> Option<Self> {
        u64::from_name(name).map(Self)
    }
}

/// How a window stands as its actor is made: its surface, and whether it has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct WindowConfig {
    pub surface: Surface,
    pub focused: bool,
}

impl Kind for WindowConfig {
    const NAME: KindName = KindName::new("aether.window.config");
}

/// A window's actor: it sends each event its window heard to the actors subscribed to it, and sends a new subscriber
/// the window's surface and focus as they stand before anything else.
#[derive(Bound)]
pub struct Window {
    listeners: Subscribers<(Event,)>,
    surface: Surface,
    focused: bool,
}

impl Kind for Window {
    const NAME: KindName = KindName::new("aether.window");
}

/// Each window's actor stands beneath the window root, which makes it as its host opens the window.
impl ChildOf for Window {
    type Parent = Windows;
}

impl Actor for Window {
    type Config = WindowConfig;
    type Key = WindowId;
    type Handles = (Subscribe, Unsubscribe, Event, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: WindowConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { listeners: Subscribers::new(), surface: config.surface, focused: config.focused })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Window {
    /// Takes in `event`, and answers whether it says anything new: a resize to the surface the window has, or the
    /// focus it has, does not, as a host may say either again with nothing changed.
    fn heard(&mut self, event: &Event) -> bool {
        match event {
            Event::Resized(surface) => {
                let is_new = *surface != self.surface;
                self.surface = *surface;
                is_new
            }
            Event::FocusGained => !mem::replace(&mut self.focused, true),
            Event::FocusLost => mem::replace(&mut self.focused, false),
            Event::PointerMoved { .. }
            | Event::PointerDown { .. }
            | Event::PointerUp { .. }
            | Event::KeyDown { .. }
            | Event::KeyUp { .. }
            | Event::CloseRequested => true,
        }
    }

    /// The event that says whether the window has the keyboard, as it stands.
    const fn focus(&self) -> Event {
        if self.focused {
            Event::FocusGained
        } else {
            Event::FocusLost
        }
    }
}

/// Adds the sender, and sends it the surface and focus as they stand: nothing, when it is subscribed already.
impl Handles<Subscribe> for Window {
    type Sends = (ToAny<(Event,), Event>,);

    fn handle(&mut self, _: Subscribe, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(listener) = turn.cast::<(Event,)>(sender) else {
            turn.log(Level::Warn, "an actor that does not handle a window's events asked to be sent them");
            return Ok(());
        };
        let added = self.listeners.subscribe(turn, listener);
        if !added {
            return Ok(());
        }
        turn.to(listener).send(&Event::Resized(self.surface))?.send(&self.focus())?;

        Ok(())
    }
}

impl Handles<Unsubscribe> for Window {
    type Sends = ();

    fn handle(&mut self, _: Unsubscribe, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.listeners.unsubscribe(turn.sender());

        Ok(())
    }
}

/// What the window root sent on from its host, sent to every subscriber when it says anything new. An event any other
/// actor sends is not the host's, and is not sent on.
impl Handles<Event> for Window {
    type Sends = (ToAny<(Event,), Event>,);

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let root = turn.parent();
        let is_brought = turn.sender() == root.into();
        if !is_brought {
            turn.log(Level::Warn, "an event another actor sent a window was not sent on: only its host's are");
            return Ok(());
        }
        let is_new = self.heard(&event);
        if is_new {
            turn.to(&self.listeners).send(&event)?;
        }

        Ok(())
    }
}

impl Handles<Ended> for Window {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.listeners.ended(&ended);

        Ok(())
    }
}
