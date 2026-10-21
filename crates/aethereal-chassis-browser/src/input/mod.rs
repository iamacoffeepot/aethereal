//! The page's input (ADR-0019): the pointer over the canvas, the keys, and the controls the page marks, brought in
//! through one source to the root that fronts them, [`Input`], which sends each event on to the actors that
//! subscribed, in the order the page heard them.

#[cfg(any(target_arch = "wasm32", test))]
mod dom;
#[cfg(target_arch = "wasm32")]
mod listen;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub use listen::{Listening, listen};

use aethereal_actor::{
    Actor, Beginning, ChildOf, Ended, Ending, Fault, Given, Handle, Handles, Level, Source, Subscribers, ToAny, Top,
    Turn,
};
use aethereal_data::{Bound, Kind, KindName, Plain};
use std::collections::BTreeMap;

/// What the page heard: one stream for the pointer, the keys, and the controls, so its order is the order they
/// happened in.
#[derive(Debug, Clone, PartialEq, Plain)]
pub enum Event {
    /// The pointer moved over the canvas, to `x`, `y`: in the pixels of the canvas's backing store, from its top
    /// left, as the renderer draws in.
    PointerMoved { x: f32, y: f32 },
    /// A button of the pointer went down over the canvas, at `x`, `y` in the canvas's pixels.
    PointerDown { x: f32, y: f32, button: Button },
    /// A button of the pointer came up over the canvas, at `x`, `y` in the canvas's pixels.
    PointerUp { x: f32, y: f32, button: Button },
    /// A key went down: `code` is the place of the key on the keyboard, whatever its layout (`"KeyW"`), and `key`
    /// what it means under the layout (`"w"`, `"Shift"`), as the page names them. `repeat` when it is held down and
    /// this is the keyboard's repeat.
    KeyDown { code: String, key: String, repeat: bool },
    /// A key came up, named as when it went down.
    KeyUp { code: String, key: String },
    /// A control the page marks changed, or was read as the page began listening; or this is how it stands, sent as
    /// the actor subscribed. `name` is its element's id.
    Control { name: String, value: Value },
}

impl Kind for Event {
    const NAME: KindName = KindName::new("aether.input.event");
}

/// A button of the pointer, as the page numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Button {
    /// The page's 0: the left button, or a touch or pen's contact.
    Main,
    /// The page's 1: the wheel, or the middle button.
    Middle,
    /// The page's 2: the right button.
    Secondary,
    /// The page's 3.
    Back,
    /// The page's 4.
    Forward,
    /// Any other, by the page's number for it.
    Other(i16),
}

/// What a control holds.
#[derive(Debug, Clone, PartialEq, Plain)]
pub enum Value {
    /// A range or number input's value, when it reads as a finite number.
    Number(f64),
    /// Whether a checkbox or radio button is checked.
    Checked(bool),
    /// Any other control's value as the page holds it: a select's, a text field's, and a number input's that does
    /// not read as a number, such as one left empty.
    Text(String),
}

/// Asks the input root to send this actor every event from the next it brings: the sender must handle [`Event`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Subscribe;

impl Kind for Subscribe {
    const NAME: KindName = KindName::new("aether.input.subscribe");
}

/// Asks the input root to send this actor nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Unsubscribe;

impl Kind for Unsubscribe {
    const NAME: KindName = KindName::new("aether.input.unsubscribe");
}

/// The input root: the capability that holds the page's input source, and sends what it brings to its subscribers.
///
/// It keeps what each control holds as the page last said, by its name, and sends a new subscriber every control as
/// it stands before anything else, so an actor made after the page was read, or subscribed after it changed, knows
/// each control without waiting for it to move.
#[derive(Bound)]
pub struct Input {
    source: Handle<Source<Event>>,
    listeners: Subscribers<(Event,)>,
    controls: BTreeMap<String, Value>,
}

impl Kind for Input {
    const NAME: KindName = KindName::new("aether.input");
}

/// The input root is a root, made by the chassis that listens to the page.
impl ChildOf for Input {
    type Parent = Top;
}

/// What the input root is made from: the source the chassis feeds what the page hears onto.
#[derive(Bound)]
pub struct InputConfig {
    pub source: Given<Source<Event>>,
}

impl Kind for InputConfig {
    const NAME: KindName = KindName::new("aether.input.config");
}

impl Actor for Input {
    type Config = InputConfig;
    type Key = ();
    type Handles = (Subscribe, Unsubscribe, Event, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: InputConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let source = turn.take(config.source)?;

        Ok(Self { source, listeners: Subscribers::new(), controls: BTreeMap::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Adds the sender, and sends it every control as it stands: nothing, when it is subscribed already.
impl Handles<Subscribe> for Input {
    type Sends = (ToAny<(Event,), Event>,);

    fn handle(&mut self, _: Subscribe, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(listener) = turn.cast::<(Event,)>(sender) else {
            turn.log(Level::Warn, "an actor that does not handle the page's events asked to be sent them");
            return Ok(());
        };
        let added = self.listeners.subscribe(turn, listener);
        if !added {
            return Ok(());
        }
        let mut target = turn.to(listener);
        for (name, value) in &self.controls {
            target.send(&Event::Control { name: name.clone(), value: value.clone() })?;
        }

        Ok(())
    }
}

impl Handles<Unsubscribe> for Input {
    type Sends = ();

    fn handle(&mut self, _: Unsubscribe, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.listeners.unsubscribe(turn.sender());

        Ok(())
    }
}

/// What the source brought, from this actor to itself (ADR-0010), sent on to every subscriber. An event another actor
/// sends is not the page's, and is not sent on.
impl Handles<Event> for Input {
    type Sends = (ToAny<(Event,), Event>,);

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let me = turn.me();
        let is_brought = turn.sender() == me.into();
        if !is_brought {
            turn.log(Level::Warn, "an event another actor sent was not sent on: only the page's are");
            return Ok(());
        }
        if let Event::Control { name, value } = &event {
            self.controls.insert(name.clone(), value.clone());
        }
        turn.to(&self.listeners).send(&event)?;

        Ok(())
    }
}

impl Handles<Ended> for Input {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.listeners.ended(&ended);

        Ok(())
    }
}
