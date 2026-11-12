//! A window's actor: beneath the window root, keyed by the window's id, it sends what its window heard to the actors
//! that subscribed, but its keys and typed text, and keeps how the window stands for a new one. Its pointer events also
//! cascade through what was drawn under the pointer, as the subscriber of its probes finds it, and its keys and typed
//! text go only through the keys the draw lists declared, as that subscriber lists them (ADR-0055).

use crate::event::{Event, Mode, Subscribe, Surface, Unsubscribe};
use crate::keys::{KeyCascade, KeyHearer, Keyed, KeysListed};
use crate::perform::{Perform, Performed, unperformable};
use crate::pointing::{Cascade, Hearer, Picked, Pointed, Probed, SubscribeProbes};
use crate::windows::Windows;
use aethereal_actor::{
    Actor, Allows, Beginning, Carries, ChildOf, Ended, Ending, ErasedActorRef, Fault, Handles, Level, Subscribers,
    ToAny, ToSender, Turn,
};
use aethereal_data::{Bound, Key, KeyName, Plain, TypedKey};
use core::mem;

/// A window, by the number the window capability gave it: the first opened as the program was set up `0`, the next
/// `1`, and each asked for while it runs the next after those, in the order asked. No number is given twice. It is
/// the key of the window's actor, and never a host's own name for the window.
#[derive(Debug, Clone, Copy, Hash, Plain, Key)]
pub struct WindowId(u64);

impl WindowId {
    /// The window given the `nth` number, from `0`.
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

/// How a window stands as its actor is made: the window, its actor's key, which its probes and what it delivers name;
/// its surface; the mode it is shown in; and whether it has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.window.config")]
pub struct WindowConfig {
    pub window: WindowId,
    pub surface: Surface,
    pub mode: Mode,
    pub focused: bool,
}

/// A window's actor: it sends each event its window heard to the actors subscribed to it, but its keys and typed
/// text, and sends a new subscriber the window's surface, mode, and focus as they stand before anything else. While an
/// actor subscribes to its probes, each pointer event is probed, and cascades through the stacks it answers, and the
/// keys and typed text go through the `Keys` entries it lists (ADR-0055).
#[derive(Bound)]
#[kind(name = "aether.window")]
pub struct Window {
    /// The window, as its probes and what it delivers name it.
    id: WindowId,
    listeners: Subscribers<(Event,)>,
    probers: Subscribers<(Probed,)>,
    cascade: Cascade,
    keys: KeyCascade,
    surface: Surface,
    mode: Mode,
    focused: bool,
}

/// Each window's actor stands beneath the window root, which makes it as its host opens the window, and which tells
/// it when its host has closed the window.
impl ChildOf for Window {
    type Parent = Windows;
}

/// What asking a window's actor at its typed path, `path::<Window>(window)`, gets.
impl Carries<Perform> for Window {
    type Answers = (Performed,);
}

impl Actor for Window {
    type Config = WindowConfig;
    type Key = WindowId;
    type Handles = (Subscribe, Unsubscribe, Event, Perform, SubscribeProbes, Picked, KeysListed, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: WindowConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let WindowConfig { window, surface, mode, focused } = config;

        Ok(Self {
            id: window,
            listeners: Subscribers::new(),
            probers: Subscribers::new(),
            cascade: Cascade::default(),
            keys: KeyCascade::default(),
            surface,
            mode,
            focused,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Window {
    /// Takes in `event`, and answers whether it says anything new: a resize to the surface the window has, the mode it
    /// is shown in, or the focus it has, does not, as a host may say any of them again with nothing changed.
    fn heard(&mut self, event: &Event) -> bool {
        match event {
            Event::Resized(surface) => {
                let is_new = *surface != self.surface;
                self.surface = *surface;
                is_new
            }
            Event::Moded(mode) => mem::replace(&mut self.mode, *mode) != *mode,
            Event::FocusGained => !mem::replace(&mut self.focused, true),
            Event::FocusLost => mem::replace(&mut self.focused, false),
            Event::PointerMoved { .. }
            | Event::PointerDown { .. }
            | Event::PointerUp { .. }
            | Event::Wheel { .. }
            | Event::KeyDown { .. }
            | Event::KeyUp { .. }
            | Event::Text { .. }
            | Event::CloseRequested
            | Event::Closed => true,
        }
    }

    /// Sends `event`, which says something new, on: a key or typed text through the keys' cascade, to the entries it
    /// reaches, and any other event to every subscriber. The keys' cascade takes in every event, as the window losing
    /// the keyboard forgets the keys down.
    fn send_on<S, I, J>(&mut self, event: &Event, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToAny<(Event,), Event>, I> + Allows<ToAny<KeyHearer, Keyed>, J>,
    {
        let keyed = self.keys.heard(self.id, event);
        if event.is_keys() {
            for (hearer, keyed) in keyed {
                turn.to(hearer).send(&keyed)?;
            }
        } else {
            turn.to(&self.listeners).send(event)?;
        }

        Ok(())
    }

    /// What probes `event`, a pointer event the cascade numbers and keeps until its stacks come back: none for another
    /// event, or while no actor subscribes to the window's probes, when nothing would answer them.
    fn probed(&mut self, event: &Event) -> Option<Probed> {
        let is_probed = !self.probers.references().is_empty();
        if !is_probed {
            return None;
        }

        self.cascade.heard(self.id, event)
    }

    /// Whether `actor` subscribes to the window's probes: the renderer, which alone says what was drawn and declared
    /// there.
    fn is_prober(&self, actor: ErasedActorRef) -> bool {
        self.probers.references().iter().any(|prober| prober.erase() == actor)
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

/// Adds the sender, and sends it the surface, mode, and focus as they stand: nothing, when it is subscribed already.
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
        turn.to(listener).send(&Event::Resized(self.surface))?.send(&Event::Moded(self.mode))?.send(&self.focus())?;

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

/// What the window root sent on from its host, sent on when it says anything new, its keys and typed text through
/// the keys' cascade and every other event to the subscribers, and a pointer event probed. An event any other actor
/// sends is not the host's, and is not sent on. Once the window has closed, `Closed` is the last its subscribers are
/// sent, and the actor ends.
impl Handles<Event> for Window {
    type Sends = (ToAny<(Event,), Event>, ToAny<KeyHearer, Keyed>, ToAny<(Probed,), Probed>);

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let root = turn.parent();
        let is_brought = turn.sender() == root.into();
        if !is_brought {
            turn.log(Level::Warn, "an event another actor sent a window was not sent on: only its host's are");
            return Ok(());
        }
        let is_new = self.heard(&event);
        if is_new {
            self.send_on(&event, turn)?;
        }
        if let Some(probed) = self.probed(&event) {
            turn.to(&self.probers).send(&probed)?;
        }
        let is_closed = event == Event::Closed;
        if is_closed {
            turn.close();
        }

        Ok(())
    }
}

/// Sends each input asked, in order, as the event of the same name, on as it would one the host heard, and probes each
/// pointer input, then answers: they are sent in the turn that answers, so each actor sent one handles it before
/// anything arriving after the answer (ADR-0046), while the cascade delivers a pointer input once its stacks come
/// back, as it does the host's. It is asked by any actor, as an input says nothing of how the window stands, which
/// only its host may. Inputs that cannot be performed are refused, and none is sent.
impl Handles<Perform> for Window {
    type Sends = (ToAny<(Event,), Event>, ToAny<KeyHearer, Keyed>, ToAny<(Probed,), Probed>, ToSender<Performed>);

    fn handle(&mut self, perform: Perform, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if let Some(why) = unperformable(&perform.inputs) {
            turn.reply(&Performed::Refused(why))?;
            return Ok(());
        }
        for input in perform.inputs {
            let event = Event::from(input);
            self.send_on(&event, turn)?;
            if let Some(probed) = self.probed(&event) {
                turn.to(&self.probers).send(&probed)?;
            }
        }
        turn.reply(&Performed::Performed)?;

        Ok(())
    }
}

/// Adds the sender to the subscribers of the window's probes, which are sent each pointer event's place from the next
/// the window hears: nothing more, when it is subscribed already. An actor that does not handle [`Probed`] is not.
impl Handles<SubscribeProbes> for Window {
    type Sends = ();

    fn handle(&mut self, _: SubscribeProbes, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(prober) = turn.cast::<(Probed,)>(sender) else {
            turn.log(Level::Warn, "an actor that does not handle a window's probes asked to be sent them");
            return Ok(());
        };
        self.probers.subscribe(turn, prober);

        Ok(())
    }
}

/// Delivers the pointer events the stacks answer, as the cascade says, each to the actor whose list holds the entry
/// it reaches. Stacks from an actor that does not subscribe to the window's probes are not the renderer's, and are
/// dropped.
impl Handles<Picked> for Window {
    type Sends = (ToAny<Hearer, Pointed>,);

    fn handle(&mut self, picked: Picked, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let is_prober = self.is_prober(turn.sender());
        if !is_prober {
            turn.log(Level::Warn, "stacks an actor that does not probe the window sent were dropped");
            return Ok(());
        }
        for (hearer, pointed) in self.cascade.picked(self.id, picked) {
            turn.to(hearer).send(&pointed)?;
        }

        Ok(())
    }
}

/// Takes in the `Keys` entries a frame lists, which the window's keys and typed text go through from the next it hears.
/// Entries from an actor that does not subscribe to the window's probes are not the renderer's, and are dropped.
impl Handles<KeysListed> for Window {
    type Sends = ();

    fn handle(&mut self, listed: KeysListed, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let is_prober = self.is_prober(turn.sender());
        if !is_prober {
            turn.log(Level::Warn, "keys an actor that does not probe the window listed were dropped");
            return Ok(());
        }
        self.keys.listed(listed);

        Ok(())
    }
}

/// A subscriber that ended leaves the list, and a subscriber of its probes too.
impl Handles<Ended> for Window {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.listeners.ended(&ended);
        self.probers.ended(&ended);

        Ok(())
    }
}
