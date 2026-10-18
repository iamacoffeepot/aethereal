//! An actor: the calls the engine makes of it (ADR-0009, ADR-0013).

use crate::{
    Actors, Beginning, Chain, Closing, Declared, Ending, Fault, HandledList, Item, Seated, Sends, Turning, Within,
};
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
/// Every [`Actor`] is one, and so is a guest's actor, whose code runs in its
/// instance (ADR-0015). The engine hands each actor its items in batches: it
/// shows the actor a batch, then hands it a turn for each item of it in order,
/// routing what each turn asked for before the next.
pub trait Runs: Send {
    /// What the actor's type declares (ADR-0013).
    fn declared(&self) -> &'static Declared;

    /// Shown the batch of items its next turns are for, before the first of them: a guest runs them all here, in
    /// one call into its instance (ADR-0015). Its turns follow in order, until one fails or the batch is done.
    fn open(&mut self, batch: &dyn Batch);

    /// Hands one item to the handler for it: a turn of the actor's own, the next of the batch it was shown.
    fn handle(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault>;

    /// The actor's closing turn, which takes the actor.
    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault>;
}

/// An actor's items for one go, as the engine shows them to it: as many as the rest of its slice is expected to cover
/// (ADR-0011, ADR-0015).
pub trait Batch {
    /// How many items it holds.
    fn fits(&self) -> usize;

    /// Shows `visit` each of its items, in order.
    fn each(&self, visit: &mut dyn FnMut(Peeked<'_>));
}

/// An item of a batch, looked at before its turn: what it is, who it comes from, and where it stands in its chain.
pub struct Peeked<'a> {
    pub item: Item<'a>,
    pub source: Seated,
    pub chain: Chain,
}

impl<A: Actor> Runs for A {
    fn declared(&self) -> &'static Declared {
        Declared::of::<A>()
    }

    /// A native actor runs each turn as it is handed it, so there is nothing to do before them.
    fn open(&mut self, _: &dyn Batch) {}

    fn handle(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        A::Handles::dispatch(self, item, turning)
    }

    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault> {
        Actor::end(*self, &mut closing.declaring::<A>())
    }
}
