//! An actor: the calls the engine makes of it (ADR-0009, ADR-0013).

use crate::{Actors, Beginning, Closing, Declared, Ending, Fault, HandledList, Item, Sends, Turning, Within};
use aethereal_data::{ActorKey, Decode, Encode, Kind};
use alloc::boxed::Box;

/// An actor: a kind that is its own state, made from a config, handed what it must react to one item at a time,
/// and ended once (ADR-0009).
///
/// What an actor handles is a tuple of the things it has a
/// [`Handles`](crate::Handles) impl for, and each turn it runs declares what it
/// may send (ADR-0013). A handler changes its own state and nothing else:
/// everything it wants of the engine it asks for through its turn, and nothing
/// it asks for happens during the turn.
///
/// An actor is `Send`: a native host may run it on any of its workers, one turn at a time (ADR-0011).
pub trait Actor: Kind + Encode<Within> + Decode<Within> + Send + Sized + 'static {
    /// What the actor is made from. It crosses from the actor that spawns this one.
    type Config: Kind + Encode<Actors> + Decode<Actors>;
    /// What tells the actors of this type apart: `()` for a singleton, a typed key otherwise.
    type Key: ActorKey;
    /// What the actor handles: kinds of mail, [`Ended`](crate::Ended) when it watches, and
    /// [`Others`](crate::Others) when it falls back. Items from one source come in the order that source caused
    /// them, so an actor handles what another sent it before it is shown that the other has ended.
    type Handles: HandledList<Self>;
    /// What its `begin` may send.
    type BeginSends: Sends;
    /// What its closing turn may send.
    type EndSends: Sends;

    /// Makes the actor from `config`. It runs inside the turn of the parent that spawns it.
    fn begin(config: Self::Config, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault>;

    /// The actor's closing turn. It runs once, and the actor ends whatever it returns.
    fn end(self, turn: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault>;
}

/// An actor the engine runs without naming its type (ADR-0009).
///
/// Every [`Actor`] is one. The engine holds a native actor as this, since the
/// actors of a program are of many types.
pub trait Runs: Send {
    /// What the actor's type declares (ADR-0013).
    fn declared(&self) -> &'static Declared;

    /// Hands one item to the handler for it: a turn of the actor's own.
    fn handle(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault>;

    /// The actor's closing turn, which takes the actor.
    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault>;
}

impl<A: Actor> Runs for A {
    fn declared(&self) -> &'static Declared {
        Declared::of::<A>()
    }

    fn handle(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        A::Handles::dispatch(self, item, turning)
    }

    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault> {
        Actor::end(*self, &mut closing.declaring::<A>())
    }
}
