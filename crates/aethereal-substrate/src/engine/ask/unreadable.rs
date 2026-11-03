//! Mail its receiver cannot read never fails the receiver: a request is answered `Unreadable`, an answer is handed over
//! as one that did not come, and other mail is dropped (ADR-0017).

use crate::Engine;
use crate::harness::{Bench, Harness, Unheard};
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Protocol, ProtocolRef,
    ToSender, Top, Turn, Unanswered,
};
use aethereal_data::{Bound, Plain};
use std::num::NonZeroUsize;

/// The most bytes a read of one piece of mail may take, in these tests' engines.
const READ_LIMIT: usize = 256;

/// Bytes enough that mail holding them reads past [`READ_LIMIT`].
const PAST: usize = 4 * READ_LIMIT;

/// Bytes few enough that mail holding them reads within [`READ_LIMIT`].
const WITHIN: usize = 16;

/// Each engine the tests run in: how many parts, and how many fused hops each part runs at most in a step.
const ENGINES: [(usize, usize); 4] = [(1, 0), (1, 64), (4, 0), (4, 64)];

/// Some bytes to be weighed, asked or told.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.unreadable.parcel")]
struct Parcel {
    bytes: Vec<u8>,
}

/// What a parcel is answered with: as many bytes as it held.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.unreadable.receipt")]
struct Receipt {
    bytes: Vec<u8>,
}

/// A parcel of `len` bytes.
fn parcel(len: usize) -> Parcel {
    Parcel { bytes: vec![7; len] }
}

/// A root that answers each parcel it is asked with a receipt of as many bytes, and counts each it is told.
#[derive(Bound)]
#[kind(name = "aether.test.unreadable.weigher")]
struct Weigher {
    /// How many parcels its handler was asked.
    asked: u64,
    /// How many parcels its handler was told.
    told: u64,
}

impl ChildOf for Weigher {
    type Parent = Top;
}

impl Actor for Weigher {
    type Config = Blank;
    type Key = ();
    type Handles = (Parcel, Told);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Blank, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { asked: 0, told: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Parcel> for Weigher {
    type Answers = (Receipt,);
}

impl Handles<Parcel> for Weigher {
    type Sends = (ToSender<Receipt>,);

    fn handle(&mut self, parcel: Parcel, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.asked += 1;

        Ok(turn.reply(&Receipt { bytes: parcel.bytes })?)
    }
}

/// A parcel told, which asks for nothing.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.unreadable.told")]
struct Told {
    parcel: Parcel,
}

impl Handles<Told> for Weigher {
    type Sends = ();

    fn handle(&mut self, _: Told, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.told += 1;

        Ok(())
    }
}

/// A config that holds nothing.
#[derive(Plain)]
#[kind(name = "aether.test.unreadable.blank")]
struct Blank;

/// What a sender asks its parcels of: the outside, as it takes every kind.
struct Weighing;

impl Protocol for Weighing {
    type Kinds = (Parcel,);
}

impl Carries<Parcel> for Weighing {
    type Answers = (Receipt,);
}

/// The order to ask the outside a parcel, keeping `at` until it is answered.
#[derive(Plain)]
#[kind(name = "aether.test.unreadable.send")]
struct Send {
    at: u64,
}

/// What a sender keeps for a parcel it asks: which it was.
#[derive(Plain)]
#[kind(name = "aether.test.unreadable.sent")]
struct Sent {
    at: u64,
}

/// What became of a parcel a sender asked, as it keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Heard {
    /// It was answered with a receipt of `len` bytes, and what was kept for it is `at`.
    Receipt { len: u64, at: u64 },
    /// No answer came, for `why`, and what was kept for it is `at`.
    No { why: Unanswered, at: u64 },
}

/// The config of a [`Sender`]: whom it asks.
#[derive(Bound)]
#[kind(name = "aether.test.unreadable.sender_config")]
struct Asking {
    of: ProtocolRef<Weighing>,
}

/// A root that asks a parcel of the outside on each order, and keeps what became of each.
#[derive(Bound)]
#[kind(name = "aether.test.unreadable.sender")]
struct Sender {
    of: ProtocolRef<Weighing>,
    /// What became of each parcel it asked, in the order heard.
    heard: Vec<Heard>,
}

impl ChildOf for Sender {
    type Parent = Top;
}

impl Actor for Sender {
    type Config = Asking;
    type Key = ();
    type Handles = (Send, Answer<Receipt, Sent>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(asking: Asking, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { of: asking.of, heard: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Send> for Sender {
    type Sends = (Asks<Weighing, Parcel, Sent>,);

    fn handle(&mut self, Send { at }: Send, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(self.of).ask(&parcel(WITHIN), &Sent { at })?;

        Ok(())
    }
}

impl Handles<Answer<Receipt, Sent>> for Sender {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Sent { at }): (Answered<Receipt>, Sent),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let heard = match answered {
            Answered::Yes(receipt) => {
                Heard::Receipt { len: u64::try_from(receipt.bytes.len()).unwrap_or(u64::MAX), at }
            }
            Answered::No(why) => Heard::No { why, at },
        };
        self.heard.push(heard);

        Ok(())
    }
}

/// A harness over an engine in `parts` parts that each run at most `hops` fused hops a step and read at most
/// [`READ_LIMIT`] bytes of a piece of mail, holding a weigher.
fn weighing((parts, hops): (usize, usize)) -> Result<Harness<Bench>, Unheard> {
    let Some(parts) = NonZeroUsize::new(parts) else {
        panic!("an engine has parts");
    };
    let mut harness = Harness::bare(Engine::new(parts, READ_LIMIT, 1 << 20, 1 << 20, hops).ready())?;
    harness.add_root::<Weigher>(&(), Blank)?;
    harness.settle();

    Ok(harness)
}

#[test]
fn a_request_past_the_read_limit_is_answered_unreadable_and_its_receiver_lives_on() -> Result<(), Unheard> {
    // Catches a request that does not read failing its receiver's turn, which ends it; its handler run on it; and its
    // asker never answered, or answered `Failed` or `Unreplied`.
    for engine in ENGINES {
        let mut harness = weighing(engine)?;
        let weigher = harness.root::<Weigher>();

        let past = harness.ask(weigher, &parcel(PAST))?;
        let unread = harness.answer(past)?;
        let within = harness.ask(weigher, &parcel(WITHIN))?;
        let read = harness.answer(within)?;

        assert_eq!(unread, Answered::No(Unanswered::Unreadable), "{engine:?}");
        assert_eq!(read, Answered::Yes(Receipt { bytes: vec![7; WITHIN] }), "{engine:?}");
        assert_eq!(harness.state(weigher)?.asked, 1, "{engine:?}: only the parcel that read reached the handler");
    }

    Ok(())
}

#[test]
fn a_tell_past_the_read_limit_is_dropped_and_its_receiver_lives_on() -> Result<(), Unheard> {
    // Catches mail that does not read failing its receiver's turn, which ends it, and its handler run on it.
    for engine in ENGINES {
        let mut harness = weighing(engine)?;
        let weigher = harness.root::<Weigher>();

        harness.tell(weigher, &Told { parcel: parcel(PAST) })?;
        harness.tell(weigher, &Told { parcel: parcel(WITHIN) })?;
        harness.settle();

        assert_eq!(harness.state(weigher)?.told, 1, "{engine:?}: only the parcel that read reached the handler");
        assert_eq!(harness.rest(), [], "{engine:?}: nothing is said of a tell");
    }

    Ok(())
}

#[test]
fn an_answer_past_the_read_limit_reaches_its_asker_as_unreadable_with_its_context() -> Result<(), Unheard> {
    // Catches an answer that does not read failing its asker's turn, which ends it, and one handed over without the
    // context its asker kept, or with another request's.
    for engine in ENGINES {
        let mut harness = weighing(engine)?;
        let of = harness.protocol::<Weighing>();
        harness.add_root::<Sender>(&(), Asking { of })?;
        let sender = harness.root::<Sender>();

        harness.tell(sender, &Send { at: 1 })?;
        let first = harness.asked::<Parcel>()?;
        harness.reply(first, &Receipt { bytes: vec![7; PAST] })?;
        harness.tell(sender, &Send { at: 2 })?;
        let second = harness.asked::<Parcel>()?;
        harness.reply(second, &Receipt { bytes: vec![7; WITHIN] })?;
        harness.settle();

        let unread = Heard::No { why: Unanswered::Unreadable, at: 1 };
        let read = Heard::Receipt { len: u64::try_from(WITHIN).unwrap_or(u64::MAX), at: 2 };
        assert_eq!(harness.state(sender)?.heard, [unread, read], "{engine:?}");
    }

    Ok(())
}
