//! An actor: the three calls the engine makes of it (ADR-0009).

use crate::{Actors, Beginning, Ended, Ending, Fault, Mail, Noticing, Turn, Within};
use aethereal_data::{ActorKey, Decode, Encode, Kind};
use alloc::boxed::Box;

/// An actor: a kind that is its own state, made from a config, handed mail and the endings it watches one at a
/// time, and ended once (ADR-0009).
///
/// These four are what the engine calls. A handler changes its own state and
/// nothing else: everything it wants of the engine it asks for through its
/// turn, and nothing it asks for happens during the turn.
pub trait Actor: Kind + Encode<Within> + Decode<Within> + Sized {
    /// What the actor is made from. It crosses from the actor that spawns this one.
    type Config: Kind + Encode<Actors> + Decode<Actors>;
    /// What tells the actors of this type apart: `()` for a singleton, a typed key otherwise.
    type Key: ActorKey;

    /// Makes the actor from `config`. It runs inside the turn of the parent that spawns it.
    fn begin(config: Self::Config, turn: &mut Beginning<'_>) -> Result<Self, Fault>;

    /// Handles one piece of mail: a turn of its own.
    fn handle(&mut self, mail: Mail<'_>, turn: &mut Turn<'_>) -> Result<(), Fault>;

    /// Reacts to the ending of an actor it watches: a turn of its own, one for each ending.
    ///
    /// The engine runs these after the step's mail, so an actor handles what another sent it before it is shown
    /// that the other has ended.
    fn ended(&mut self, ended: &Ended, turn: &mut Noticing<'_>) -> Result<(), Fault>;

    /// The actor's closing turn. It runs once, and the actor ends whatever it returns.
    fn end(self, turn: &mut Ending<'_>) -> Result<(), Fault>;
}

/// An actor the engine runs without naming its type (ADR-0009).
///
/// Every [`Actor`] is one. The engine holds a native actor as this, since the
/// actors of a program are of many types.
pub trait Runs {
    /// Handles one piece of mail: a turn of the actor's own.
    fn handle(&mut self, mail: Mail<'_>, turn: &mut Turn<'_>) -> Result<(), Fault>;

    /// Reacts to the ending of an actor it watches: a turn of the actor's own.
    fn ended(&mut self, ended: &Ended, turn: &mut Noticing<'_>) -> Result<(), Fault>;

    /// The actor's closing turn, which takes the actor.
    fn end(self: Box<Self>, turn: &mut Ending<'_>) -> Result<(), Fault>;
}

impl<A: Actor> Runs for A {
    fn handle(&mut self, mail: Mail<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        Actor::handle(self, mail, turn)
    }

    fn ended(&mut self, ended: &Ended, turn: &mut Noticing<'_>) -> Result<(), Fault> {
        Actor::ended(self, ended, turn)
    }

    fn end(self: Box<Self>, turn: &mut Ending<'_>) -> Result<(), Fault> {
        Actor::end(*self, turn)
    }
}
