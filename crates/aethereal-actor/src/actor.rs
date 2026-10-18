//! An actor: the calls the engine makes of it (ADR-0009, ADR-0013).

use crate::{
    Actors, Beginning, Chain, Children, Closing, Declared, Ending, Fault, HandledList, Instances, Item, Puts, Seated,
    Seats, Sends, Table, Turning, Within,
};
use aethereal_data::{ActorKey, Decode, Encode, Kind};
use alloc::boxed::Box;
use core::any::Any;

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

/// An actor the engine runs without naming its type, as its seat holds it: by how its turns are computed, which is
/// fixed when it is made (ADR-0009, ADR-0015).
///
/// The engine decides what follows a turn one item at a time: it routes the turn's mail, fuses its first send, and
/// ends the actor at a turn that fails or closes it (ADR-0011, ADR-0013). So it takes turns one at a time. Most actors
/// compute each turn as it is asked for, and entering them is a call. But an actor can be costly to enter: a guest's
/// code runs in its instance, and every call into it pays for the crossing (ADR-0015). Such an actor is shown each
/// batch of its items first, computes their turns at once, and hands them back one by one. Which of the two an actor
/// is, is its type's, so the engine never shows a batch to an actor that would do nothing with it, and never asks a
/// batched actor for a turn it did not prepare.
pub struct Runner(Box<dyn Flows>);

/// How a [`Runner`] holds its actor: one box whatever the actor's flow, which says it when the engine opens a batch.
trait Flows: Any + Send {
    fn declared(&self) -> &'static Declared;
    fn as_any(&self) -> &dyn Any;
    fn turns<'a>(&'a mut self, prepare: &mut dyn FnMut(&'a mut dyn Batched) -> Prepared<'a>) -> Turns<'a>;
    fn into_each(self: Box<Self>) -> Option<Box<dyn Each>>;
    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault>;
}

impl<T: Each> Flows for T {
    fn declared(&self) -> &'static Declared {
        Each::declared(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn turns<'a>(&'a mut self, _: &mut dyn FnMut(&'a mut dyn Batched) -> Prepared<'a>) -> Turns<'a> {
        Turns::Each(self)
    }

    fn into_each(self: Box<Self>) -> Option<Box<dyn Each>> {
        Some(self)
    }

    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault> {
        Each::end(self, closing)
    }
}

/// A [`Batched`] actor, as a [`Runner`] holds it.
struct AsBatched<B>(B);

impl<B: Batched> Flows for AsBatched<B> {
    fn declared(&self) -> &'static Declared {
        self.0.declared()
    }

    fn as_any(&self) -> &dyn Any {
        &self.0
    }

    fn turns<'a>(&'a mut self, prepare: &mut dyn FnMut(&'a mut dyn Batched) -> Prepared<'a>) -> Turns<'a> {
        Turns::Prepared(prepare(&mut self.0))
    }

    fn into_each(self: Box<Self>) -> Option<Box<dyn Each>> {
        None
    }

    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault> {
        Box::new(self.0).end(closing)
    }
}

impl Runner {
    /// The actor `actor`, which computes each turn when it is asked for.
    #[must_use]
    pub fn each(actor: impl Each) -> Self {
        Self(Box::new(actor))
    }

    /// The actor `actor`, which computes its turns ahead.
    #[must_use]
    pub fn batched(actor: impl Batched) -> Self {
        Self(Box::new(AsBatched(actor)))
    }

    /// What the actor's type declares (ADR-0013).
    #[must_use]
    pub fn declared(&self) -> &'static Declared {
        self.0.declared()
    }

    /// The actor, for code that holds actors of its own types and reads them back by type.
    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self.0.as_any()
    }

    /// The actor, when it computes each turn as it is asked for: nothing when it computes its turns ahead.
    #[must_use]
    pub fn into_each(self) -> Option<Box<dyn Each>> {
        self.0.into_each()
    }

    /// The turns of the actor's next batch: taken as they come from an actor that computes each when it is asked for,
    /// and from what `prepare` answers with for one that computes ahead, which is only then shown the batch.
    #[inline]
    pub fn turns<'a>(&'a mut self, mut prepare: impl FnMut(&'a mut dyn Batched) -> Prepared<'a>) -> Turns<'a> {
        self.0.turns(&mut prepare)
    }

    /// The actor's closing turn, which takes the actor.
    ///
    /// # Errors
    ///
    /// The fault the closing turn ended with.
    pub fn end(self, closing: Closing<'_>) -> Result<(), Fault> {
        self.0.end(closing)
    }
}

/// An actor that computes each turn when the engine asks for it (ADR-0009). Every [`Actor`] is one.
pub trait Each: Any + Send {
    /// What the actor's type declares (ADR-0013).
    fn declared(&self) -> &'static Declared;

    /// The turn of `item`: what the turn asks for goes into `turning`, and it answers with how the turn ended.
    ///
    /// # Errors
    ///
    /// The fault the turn ended with.
    fn turn(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault>;

    /// The actor's closing turn, which takes the actor.
    ///
    /// # Errors
    ///
    /// The fault the closing turn ended with.
    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault>;
}

/// An actor that computes its turns ahead, a batch at a time, and hands them back one by one (ADR-0015).
///
/// For each batch of the actor's items, the engine calls [`Self::prepare`] once, showing it the items, then takes
/// each turn in order from the [`Prepared`] it answers with, routing what each asked for before it takes the next.
/// [`Self::end`] runs the closing turn, which belongs to no batch.
///
/// **What the engine gives.** An implementation may rely on each of these:
///
/// 1. Every turn belongs to the batch prepared last. Once a batch is prepared, the actor is called for nothing but
///    the turns of it until the batch is done: no other `prepare`, and no `end`, as the [`Prepared`] it answers with holds it.
/// 2. Turns are asked for in the order `prepare` showed their items, each item once, and `turn` is handed the item
///    itself.
/// 3. Every prepared item's turn is asked for, unless one of the actor's own turns fails or asks to close it: then no
///    turn after that one is asked for, in this batch or any other. Nothing else cuts a batch short: the engine looks
///    at the clock for the actor's slice only between batches.
/// 4. A batch holds at least one item. A hop is a batch of one (ADR-0013).
///
/// **How to use it.** Only the engine calls these. An actor written in Rust implements [`Actor`], and every `Actor`
/// is an [`Each`], so it needs nothing more. A host implements `Batched` for
/// code the engine cannot see, as a guest's host does. It:
///
/// - computes in `prepare`, and hands each turn back from the [`Prepared`] it answers with, into the [`Turning`] it is given. Everything a turn
///   asks of the engine is asked there. `prepare` asks nothing, but it is lent what a turn's code reads: the table
///   as the batch's turns see it, the children the actor has made, and the seats its children are given, so a turn
///   computed there is answered as it would be in its own turn. By (1) nothing changes them between the two, but the
///   batch's own turns, whose children the host keeps track of itself.
/// - hands each child it stood up in `prepare` to the engine in the turn that stood it up, and gives back, in a turn
///   of the batch, each seat it took and gave no child.
/// - stops computing after the first turn that fails or closes the actor, since by (3) no later turn is asked for,
///   and a turn computed but never asked for would have changed the actor for an item the engine did not run.
///
/// Code that runs in an instance, as a guest's does, owns none: it takes the instance it wants from the
/// [`Instances`] its part lends it in each [`Preparing`] and [`Turning`], and loads it there when the part has none
/// yet. The part's other actors of the same code take the same instance (ADR-0015).
pub trait Batched: Any + Send {
    /// What the actor's type declares (ADR-0013).
    fn declared(&self) -> &'static Declared;

    /// Shown the items of its next batch, in the order their turns will be asked for, before the first of them:
    /// computes their turns, and answers with what hands each back.
    fn prepare<'a>(&'a mut self, preparing: Preparing<'_>) -> Prepared<'a>;

    /// The actor's closing turn, which takes the actor.
    ///
    /// # Errors
    ///
    /// The fault the closing turn ended with.
    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault>;
}

/// The turns of a batch a [`Batched`] actor computed ahead, handed back one at a time: what its `prepare` answers
/// with, and the only way to them. It holds the actor until the batch is done, so nothing else is called on it in
/// between.
pub struct Prepared<'a> {
    computed: &'a mut dyn Computed,
}

/// What hands back the turns a [`Batched`] actor computed, behind the [`Prepared`] its `prepare` answers with.
pub trait Computed {
    /// The turn of `item`, the next of the batch: what it asked for goes into `turning`, and it answers with how
    /// the turn ended.
    ///
    /// # Errors
    ///
    /// The fault the turn ended with.
    fn turn(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault>;
}

impl<'a> Prepared<'a> {
    /// The batch whose turns `computed` hands back.
    #[must_use]
    pub fn new(computed: &'a mut dyn Computed) -> Self {
        Self { computed }
    }
}

/// The turns of one batch of an actor's items, as the engine takes them: from the actor as they come, or from what
/// it prepared.
pub enum Turns<'a> {
    Each(&'a mut dyn Each),
    Prepared(Prepared<'a>),
}

impl Turns<'_> {
    /// The turn of `item`, the next of the batch: what it asks for goes into `turning`.
    ///
    /// # Errors
    ///
    /// The fault the turn ended with.
    #[inline]
    pub fn turn(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        match self {
            Self::Each(actor) => actor.turn(item, turning),
            Self::Prepared(prepared) => prepared.computed.turn(item, turning),
        }
    }
}

/// The items of a batch, as [`Batched::prepare`] is shown them: as many as the engine takes turns of between two looks at
/// the clock, or fewer when fewer are waiting, and at least one (ADR-0011, ADR-0015).
pub trait Batch {
    /// Shows `visit` each of its items, in the order their turns will be asked for.
    fn each(&self, visit: &mut dyn FnMut(Peeked<'_>));
}

/// A batch as the engine opens it for an actor to prepare, before its first turn: its items, and the instances of the
/// actor's part, lent for the call (ADR-0015). What [`Batched::prepare`] is given, as a [`Turning`] is what
/// a turn is.
pub struct Preparing<'a> {
    batch: &'a dyn Batch,
    instances: &'a mut Instances,
    reads: Reads<'a>,
}

/// What a batch's turns read of the engine, the seats their children are given, and where the blobs they put go,
/// lent to [`Batched::prepare`] so a turn computed ahead is answered as it would be in its own turn (ADR-0015).
pub struct Reads<'a> {
    /// The table, as the batch's turns read it.
    pub table: &'a dyn Table,
    /// The children the actor has made.
    pub made: &'a dyn Children,
    /// The seats the batch's children are given.
    pub seats: &'a mut Seats,
    /// The puts of the worker, which a blob a turn computed ahead puts, or a child it spawns puts, goes into.
    pub puts: &'a mut Puts,
}

impl<'a> Preparing<'a> {
    /// The preparing of `batch`, by an actor lent its part's `instances` and what its turns read, `reads`: for the
    /// engine.
    #[must_use]
    pub const fn new(batch: &'a dyn Batch, instances: &'a mut Instances, reads: Reads<'a>) -> Self {
        Self { batch, instances, reads }
    }

    /// Shows `visit` each item of the batch, in the order their turns will be asked for.
    pub fn each(&self, visit: &mut dyn FnMut(Peeked<'_>)) {
        self.batch.each(visit);
    }

    /// The batch, the instances of the part, and what the batch's turns read, as a host prepares it: for code that
    /// runs in an instance, which it takes from them while it hands the batch's items in, and answers the reads of.
    #[must_use]
    pub fn hosted(self) -> (&'a dyn Batch, &'a mut Instances, Reads<'a>) {
        (self.batch, self.instances, self.reads)
    }
}

/// An item of a batch, looked at before its turn: what it is, who it comes from, where it stands in its chain, and
/// where the references in its bytes lie, for a host that hands them to code that takes pins of its own (ADR-0014).
pub struct Peeked<'a> {
    pub item: Item<'a>,
    pub source: Seated,
    pub chain: Chain,
    #[cfg(feature = "std")]
    pub pinned: crate::Pinned<'a>,
}

impl<A: Actor> Each for A {
    fn declared(&self) -> &'static Declared {
        Declared::of::<A>()
    }

    fn turn(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        A::Handles::dispatch(self, item, turning)
    }

    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault> {
        Actor::end(*self, &mut closing.declaring::<A>())
    }
}
