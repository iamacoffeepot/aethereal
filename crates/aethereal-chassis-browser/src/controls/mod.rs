//! The page's controls (ADR-0024): the elements it marks `data-control`, each by its id, brought in through one
//! source to the root that fronts them, [`Controls`], which keeps each as it stands and sends what changes to the
//! actors that subscribed. They are the page's, and no window's: a native host has none.

#[cfg(target_arch = "wasm32")]
mod listen;
#[cfg(any(target_arch = "wasm32", test))]
mod read;
#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
pub(crate) use listen::listen;

use aethereal_actor::{
    Actor, Beginning, ChildOf, Ended, Ending, Fault, Given, Handle, Handles, Level, Source, Subscribers, ToAny, Top,
    Turn,
};
use aethereal_data::{Bound, Plain};
use std::collections::BTreeMap;

/// A control the page marks changed, or was read as the page began listening; or this is how it stands, sent as the
/// actor subscribed. `name` is its element's id.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.controls.control")]
pub struct Control {
    pub name: String,
    pub value: Value,
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

/// Asks the controls root to send this actor every control as it stands, and each change from the next: the sender
/// must handle [`Control`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.controls.subscribe")]
pub struct Subscribe;

/// Asks the controls root to send this actor nothing more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.controls.unsubscribe")]
pub struct Unsubscribe;

/// The controls root: the capability that holds the page's source of controls, and sends what it brings to its
/// subscribers.
///
/// It keeps what each control holds as the page last said, by its name, and sends a new subscriber every control as
/// it stands before anything else, so an actor made after the page was read, or subscribed after it changed, knows
/// each control without waiting for it to move.
#[derive(Bound)]
#[kind(name = "aether.controls")]
pub struct Controls {
    source: Handle<Source<Control>>,
    listeners: Subscribers<(Control,)>,
    standing: BTreeMap<String, Value>,
}

/// The controls root is a root, made by the chassis that listens to the page.
impl ChildOf for Controls {
    type Parent = Top;
}

/// What the controls root is made from: the source the chassis feeds the page's controls onto.
#[derive(Bound)]
#[kind(name = "aether.controls.config")]
pub struct ControlsConfig {
    pub source: Given<Source<Control>>,
}

impl Actor for Controls {
    type Config = ControlsConfig;
    type Key = ();
    type Handles = (Subscribe, Unsubscribe, Control, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: ControlsConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let source = turn.take(config.source)?;

        Ok(Self { source, listeners: Subscribers::new(), standing: BTreeMap::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Adds the sender, and sends it every control as it stands: nothing, when it is subscribed already.
impl Handles<Subscribe> for Controls {
    type Sends = (ToAny<(Control,), Control>,);

    fn handle(&mut self, _: Subscribe, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let sender = turn.sender();
        let Some(listener) = turn.cast::<(Control,)>(sender) else {
            turn.log(Level::Warn, "an actor that does not handle the page's controls asked to be sent them");
            return Ok(());
        };
        let added = self.listeners.subscribe(turn, listener);
        if !added {
            return Ok(());
        }
        let mut target = turn.to(listener);
        for (name, value) in &self.standing {
            target.send(&Control { name: name.clone(), value: value.clone() })?;
        }

        Ok(())
    }
}

impl Handles<Unsubscribe> for Controls {
    type Sends = ();

    fn handle(&mut self, _: Unsubscribe, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.listeners.unsubscribe(turn.sender());

        Ok(())
    }
}

/// What the source brought, from this actor to itself (ADR-0010), kept and sent on to every subscriber. A control
/// another actor sends is not the page's, and is neither kept nor sent on.
impl Handles<Control> for Controls {
    type Sends = (ToAny<(Control,), Control>,);

    fn handle(&mut self, control: Control, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let me = turn.me();
        let is_brought = turn.sender() == me.into();
        if !is_brought {
            turn.log(Level::Warn, "a control another actor sent was not sent on: only the page's are");
            return Ok(());
        }
        self.standing.insert(control.name.clone(), control.value.clone());
        turn.to(&self.listeners).send(&control)?;

        Ok(())
    }
}

impl Handles<Ended> for Controls {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.listeners.ended(&ended);

        Ok(())
    }
}
