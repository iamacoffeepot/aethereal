//! An actor: the three calls the engine makes of it (ADR-0009).

use crate::{Actors, Beginning, Ending, Fault, Item, Turn, Within};
use aethereal_data::{ActorKey, Decode, Encode, Kind};
use alloc::boxed::Box;

/// An actor: a kind that is its own state, made from a config, handed what it must react to one item at a time,
/// and ended once (ADR-0009).
///
/// These three are what the engine calls. A handler changes its own state and
/// nothing else: everything it wants of the engine it asks for through its
/// turn, and nothing it asks for happens during the turn.
///
/// An actor is `Send`: a native host may run it on any of its workers, one turn at a time (ADR-0011).
pub trait Actor: Kind + Encode<Within> + Decode<Within> + Send + Sized {
    /// What the actor is made from. It crosses from the actor that spawns this one.
    type Config: Kind + Encode<Actors> + Decode<Actors>;
    /// What tells the actors of this type apart: `()` for a singleton, a typed key otherwise.
    type Key: ActorKey;

    /// Makes the actor from `config`. It runs inside the turn of the parent that spawns it.
    fn begin(config: Self::Config, turn: &mut Beginning<'_>) -> Result<Self, Fault>;

    /// Handles one item: a piece of mail, or the ending of an actor it watches. A turn of its own.
    ///
    /// Items from one source come in the order that source caused them, so an actor handles what another sent it
    /// before it is shown that the other has ended.
    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault>;

    /// The actor's closing turn. It runs once, and the actor ends whatever it returns.
    fn end(self, turn: &mut Ending<'_>) -> Result<(), Fault>;
}

/// An actor the engine runs without naming its type (ADR-0009).
///
/// Every [`Actor`] is one. The engine holds a native actor as this, since the
/// actors of a program are of many types.
pub trait Runs: Send {
    /// Handles one item: a turn of the actor's own.
    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault>;

    /// The actor's closing turn, which takes the actor.
    fn end(self: Box<Self>, turn: &mut Ending<'_>) -> Result<(), Fault>;
}

impl<A: Actor> Runs for A {
    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        Actor::handle(self, item, turn)
    }

    fn end(self: Box<Self>, turn: &mut Ending<'_>) -> Result<(), Fault> {
        Actor::end(*self, turn)
    }
}
