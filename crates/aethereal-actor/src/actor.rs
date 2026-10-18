//! An actor: the calls the engine makes of it (ADR-0009, ADR-0013).

use crate::{
    Above, Actors, Asked, Beginning, Chain, Children, Closing, Declared, Ending, Fault, HandledList, Item, Seated,
    Sends, Table, Turning, Within,
};
use aethereal_data::ActorPath;
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
/// instance (ADR-0015). The engine hands each actor its items in batches, and
/// the actor runs a turn for each, in order, into the turn's buffers, marking
/// where each ends; the engine routes them turn by turn once the batch is done.
pub trait Runs: Send {
    /// What the actor's type declares (ADR-0013).
    fn declared(&self) -> &'static Declared;

    /// Runs a turn for each item of `batch`, in order, through [`Batch::next`] and [`Batch::mark`], until one fails
    /// or the batch is done.
    fn run(&mut self, batch: &mut Batch<'_, '_>);

    /// The actor's closing turn, which takes the actor.
    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault>;
}

impl<A: Actor> Runs for A {
    fn declared(&self) -> &'static Declared {
        Declared::of::<A>()
    }

    fn run(&mut self, batch: &mut Batch<'_, '_>) {
        while let Some((item, turning)) = batch.next() {
            let result = A::Handles::dispatch(self, item, turning);
            let failed = batch.mark(result);
            if failed {
                break;
            }
        }
    }

    fn end(self: Box<Self>, closing: Closing<'_>) -> Result<(), Fault> {
        Actor::end(*self, &mut closing.declaring::<A>())
    }
}

/// Where an actor's items come from, one after another, for the engine to say (ADR-0011).
pub trait Items<'a> {
    /// The next item.
    fn next(&mut self) -> Option<Peeked<'a>>;

    /// Shows `visit` each of the next `count` items, in order, without taking them.
    fn each(&self, count: usize, visit: &mut dyn FnMut(Peeked<'a>));
}

/// An item, where it comes from, and where it stands in its chain.
#[derive(Clone)]
pub struct Peeked<'a> {
    pub item: Item<'a>,
    pub source: Seated,
    pub chain: Chain,
}

/// The items of a list, for a caller that has them in hand.
impl<'a> Items<'a> for alloc::vec::IntoIter<Peeked<'a>> {
    fn next(&mut self) -> Option<Peeked<'a>> {
        Iterator::next(self)
    }

    fn each(&self, count: usize, visit: &mut dyn FnMut(Peeked<'a>)) {
        for peeked in self.as_slice().iter().take(count) {
            visit(peeked.clone());
        }
    }
}

/// Where one turn of a batch ends in each buffer, and the chain of the item it handled.
#[derive(Debug, Clone, Copy)]
pub struct Mark {
    pub mailed: usize,
    pub lines: usize,
    pub effects: usize,
    pub stood: usize,
    pub chain: Chain,
}

impl Mark {
    /// Where the first turn of a batch, of an item at `chain`, begins: at the start of every buffer.
    #[must_use]
    pub const fn start(chain: Chain) -> Self {
        Self { mailed: 0, lines: 0, effects: 0, stood: 0, chain }
    }
}

/// An actor's items for one go, and what its turns of them write into: as many as the rest of its slice is expected
/// to cover (ADR-0011, ADR-0015). Each turn is opened by [`Self::next`] and closed by [`Self::mark`].
pub struct Batch<'b, 'a> {
    items: Source<'b, 'a>,
    left: usize,
    me: Seated,
    path: &'b ActorPath,
    above: Above,
    table: &'b dyn Table,
    made: &'b dyn Children,
    asked: &'b mut Asked,
    /// How many pieces of mail the actor has sent, its turns of this batch so far included.
    sent: u64,
    /// The chain of the item whose turn is open.
    chain: Chain,
    /// Whether a turn of it has ended the actor, which ends the batch.
    ended: Ended,
}

/// Where a batch's items come from: one in hand, as a hop is, or many, read one after another.
enum Source<'b, 'a> {
    One(Waiting<'a>),
    Many(&'b mut dyn Items<'a>),
}

/// The one item of a batch of one, until it is taken.
enum Waiting<'a> {
    Here(Peeked<'a>),
    Taken,
}

/// Whether a batch's turns have ended its actor.
enum Ended {
    /// No: its next item may run.
    Not,
    /// A turn closed it.
    Closed,
    /// A turn failed.
    Failed(Fault),
}

impl<'b, 'a> Batch<'b, 'a> {
    /// The batch of the next `fits` of `items`, for the actor `me` names, at `path`, beneath `above`, reading `table`
    /// and having made `made`, its turns adding to `asked`, after the `sent` pieces of mail it has sent.
    #[allow(clippy::too_many_arguments, reason = "what a batch of turns is opened with, each fixed by the engine")]
    #[must_use]
    pub fn new(
        items: &'b mut dyn Items<'a>,
        fits: usize,
        me: Seated,
        path: &'b ActorPath,
        above: Above,
        table: &'b dyn Table,
        made: &'b dyn Children,
        asked: &'b mut Asked,
        sent: u64,
    ) -> Self {
        asked.begin_batch();
        let chain = Chain { root: crate::MailId { sender: me.id, count: 0 }, depth: 0 };
        let items = Source::Many(items);

        Self { items, left: fits, me, path, above, table, made, asked, sent, chain, ended: Ended::Not }
    }

    /// The batch of the one item `item`, as [`Self::new`] makes a batch of many: a hop's (ADR-0013).
    #[allow(clippy::too_many_arguments, reason = "what a batch of turns is opened with, each fixed by the engine")]
    #[must_use]
    pub fn one(
        item: Peeked<'a>,
        me: Seated,
        path: &'b ActorPath,
        above: Above,
        table: &'b dyn Table,
        made: &'b dyn Children,
        asked: &'b mut Asked,
        sent: u64,
    ) -> Self {
        asked.begin_batch();
        let chain = item.chain;
        let items = Source::One(Waiting::Here(item));

        Self { items, left: 1, me, path, above, table, made, asked, sent, chain, ended: Ended::Not }
    }

    /// Shows `visit` each item of the batch, in order, before its first turn: for an actor that hands its items on
    /// whole, as a guest's host copies them into the guest.
    pub fn each(&self, visit: &mut dyn FnMut(Peeked<'a>)) {
        match &self.items {
            Source::One(Waiting::Here(item)) => visit(item.clone()),
            Source::One(Waiting::Taken) => {}
            Source::Many(items) => items.each(self.left, visit),
        }
    }

    /// Opens the turn of the next item: nothing once the batch is done, or a turn of it failed.
    pub fn next(&mut self) -> Option<(Item<'a>, Turning<'_>)> {
        let is_over = !matches!(self.ended, Ended::Not);
        if is_over || self.left == 0 {
            return None;
        }
        let next = match &mut self.items {
            Source::One(waiting) => match core::mem::replace(waiting, Waiting::Taken) {
                Waiting::Here(item) => Some(item),
                Waiting::Taken => None,
            },
            Source::Many(items) => items.next(),
        };
        let Peeked { item, source, chain } = next?;
        self.left -= 1;
        self.chain = chain;
        self.asked.begin_turn(self.sent);
        let turning =
            Turning::new(self.me, self.path, self.above, self.table, self.made, self.asked, self.sent, source, chain);

        Some((item, turning))
    }

    /// Closes the turn opened last, which returned `result`: marks where its entries end, and answers whether it
    /// failed, which ends the batch, as a turn that closes the actor does.
    pub fn mark(&mut self, result: Result<(), Fault>) -> bool {
        self.sent = self.asked.sent();
        let closed = self.asked.closes();
        self.asked.mark(self.chain);
        match result {
            Err(fault) => {
                self.ended = Ended::Failed(fault);
                true
            }
            Ok(()) if closed => {
                self.ended = Ended::Closed;
                false
            }
            Ok(()) => false,
        }
    }

    /// What the batch's last turn returned, once it is done: a failure, or that none failed.
    pub fn into_outcome(self) -> Result<(), Fault> {
        match self.ended {
            Ended::Failed(fault) => Err(fault),
            Ended::Not | Ended::Closed => Ok(()),
        }
    }
}
