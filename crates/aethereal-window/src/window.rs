//! A window's actor: beneath the window root, keyed by the window's id, it sends what its window heard to the actors
//! that subscribed, its keys, typed text, and wheel turns only to the holder of its key focus while one holds it (ADR-0053), and
//! keeps how the window stands for a new one. Its pointer events also cascade through what was drawn under the pointer,
//! as the subscriber of its probes finds it (ADR-0055).

use crate::event::{Event, Mode, Subscribe, Surface, Unsubscribe};
use crate::focus::{KeyFocus, KeyFocusGained, KeyFocusHolder, KeyFocusLost, ReleaseKeyFocus, Slot, TakeKeyFocus, Took};
use crate::perform::{Perform, Performed, unperformable};
use crate::pointing::{Cascade, Hearer, Picked, Pointed, Probed, SubscribeProbes};
use crate::windows::Windows;
use aethereal_actor::{
    Actor, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, Level, ProtocolRef, Subscribers, ToAny,
    ToSender, Turn,
};
use aethereal_data::{Bound, Key, KeyName, Plain, TypedKey};
use core::{mem, slice};

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

/// How a window stands as its actor is made: the window, its actor's key, which its key focus notices name; its
/// surface; the mode it is shown in; and whether it has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.window.config")]
pub struct WindowConfig {
    pub window: WindowId,
    pub surface: Surface,
    pub mode: Mode,
    pub focused: bool,
}

/// A window's actor: it sends each event its window heard to the actors subscribed to it, its keys, typed text, and
/// wheel turns only to the holder of its key focus while one holds it, and sends a new subscriber the window's surface, mode, and focus
/// as they stand before anything else. While an actor subscribes to its probes, each pointer event is probed, and
/// cascades through the stacks it answers (ADR-0055).
#[derive(Bound)]
#[kind(name = "aether.window")]
pub struct Window {
    /// The window, as its key focus notices and its probes name it.
    id: WindowId,
    listeners: Subscribers<(Event,)>,
    focus: KeyFocus,
    probers: Subscribers<(Probed,)>,
    cascade: Cascade,
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
    type Handles =
        (Subscribe, Unsubscribe, Event, Perform, TakeKeyFocus, ReleaseKeyFocus, SubscribeProbes, Picked, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: WindowConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let WindowConfig { window, surface, mode, focused } = config;

        Ok(Self {
            id: window,
            listeners: Subscribers::new(),
            focus: KeyFocus::default(),
            probers: Subscribers::new(),
            cascade: Cascade::default(),
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

    /// The subscribers `event` is sent to, as the key focus routes it: every one, or the holder alone, when it is
    /// subscribed.
    fn hearers(&mut self, event: &Event) -> &[ProtocolRef<(Event,)>] {
        let slot = self.focus.route(event);
        let listeners = self.listeners.references();
        match slot {
            Slot::Empty => listeners,
            Slot::Held(holder) => {
                listeners.iter().find(|listener| listener.erase() == holder.erase()).map_or(&[], slice::from_ref)
            }
        }
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

/// What the window root sent on from its host, sent to the subscribers the key focus routes it to when it says
/// anything new, and a pointer event probed. An event any other actor sends is not the host's, and is not sent on. Once
/// the window has closed, the holder of its key focus is sent `KeyFocusLost`, `Closed` is the last its subscribers are
/// sent, and the actor ends.
impl Handles<Event> for Window {
    type Sends = (ToAny<(Event,), Event>, ToAny<KeyFocusHolder, KeyFocusLost>, ToAny<(Probed,), Probed>);

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let root = turn.parent();
        let is_brought = turn.sender() == root.into();
        if !is_brought {
            turn.log(Level::Warn, "an event another actor sent a window was not sent on: only its host's are");
            return Ok(());
        }
        let is_closed = event == Event::Closed;
        if is_closed && let Slot::Held(holder) = self.focus.slot() {
            turn.to(holder).send(&KeyFocusLost { window: self.id })?;
        }
        let is_new = self.heard(&event);
        if is_new {
            let hearers = self.hearers(&event);
            turn.to(hearers).send(&event)?;
        }
        if let Some(probed) = self.probed(&event) {
            turn.to(&self.probers).send(&probed)?;
        }
        if is_closed {
            turn.close();
        }

        Ok(())
    }
}

/// Sends each input asked, in order, as the event of the same name, to the subscribers the key focus routes it to, as
/// it would one the host heard, and probes each pointer input, then answers: they are sent in the turn that answers,
/// so each subscriber handles them before anything arriving after the answer (ADR-0046), while the cascade delivers a
/// pointer input once its stacks come back, as it does the host's. It is asked by any actor, as an input says nothing
/// of how the window stands, which only its host may. Inputs that cannot be performed are refused, and none is sent.
impl Handles<Perform> for Window {
    type Sends = (ToAny<(Event,), Event>, ToAny<(Probed,), Probed>, ToSender<Performed>);

    fn handle(&mut self, perform: Perform, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if let Some(why) = unperformable(&perform.inputs) {
            turn.reply(&Performed::Refused(why))?;
            return Ok(());
        }
        for input in perform.inputs {
            let event = Event::from(input);
            let hearers = self.hearers(&event);
            turn.to(hearers).send(&event)?;
            if let Some(probed) = self.probed(&event) {
                turn.to(&self.probers).send(&probed)?;
            }
        }
        turn.reply(&Performed::Performed)?;

        Ok(())
    }
}

/// Gives the sender the window's key focus, the latest take winning, and tells the holder it replaced, then the sender:
/// nothing, when the sender holds it already. A sender that does not handle both notices is refused. The holder is
/// watched, so its end empties the slot.
impl Handles<TakeKeyFocus> for Window {
    type Sends = (ToSender<KeyFocusGained>, ToAny<KeyFocusHolder, KeyFocusLost>);

    fn handle(&mut self, _: TakeKeyFocus, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(taker) = turn.cast::<KeyFocusHolder>(sender) else {
            turn.log(
                Level::Warn,
                "an actor that does not handle both key focus notices asked for a window's key focus",
            );
            return Ok(());
        };
        let window = self.id;
        match self.focus.take(taker) {
            Took::Kept => return Ok(()),
            Took::Gained => {}
            Took::Replaced(replaced) => {
                turn.to(replaced).send(&KeyFocusLost { window })?;
            }
        }
        turn.watch(taker);
        turn.reply(&KeyFocusGained { window })?;

        Ok(())
    }
}

/// Empties the window's key focus slot when the sender holds it, and tells the sender: nothing is handed back, and from
/// an actor that does not hold it, nothing changes.
impl Handles<ReleaseKeyFocus> for Window {
    type Sends = (ToSender<KeyFocusLost>,);

    fn handle(&mut self, _: ReleaseKeyFocus, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let released = self.focus.release(turn.sender());
        if released {
            turn.reply(&KeyFocusLost { window: self.id })?;
        }

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
        let sender = turn.sender();
        let is_prober = self.probers.references().iter().any(|prober| prober.erase() == sender);
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

/// A subscriber that ended leaves the list, a subscriber of its probes too, and a key focus holder that ended empties
/// the slot, told nothing.
impl Handles<Ended> for Window {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.listeners.ended(&ended);
        self.probers.ended(&ended);
        self.focus.release(ended.actor());

        Ok(())
    }
}
