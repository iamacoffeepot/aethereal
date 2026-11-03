//! Deadlines as the engine keeps them, answers them, and hands them on to the requests made for them (ADR-0041).

use crate::Engine;
use crate::harness::{Bench, Harness, Unheard};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, Carries, ChildOf, Due, Ending, Fault, Handles, Held,
    Protocol, ProtocolRef, Sleep, Slept, Time, ToSender, Top, Turn, Unanswered,
};
use aethereal_data::{Bound, Plain};
use core::time::Duration;
use std::num::NonZeroUsize;

/// What a relay or a mute is asked: a query, which says what a relay does with it once it holds it.
#[derive(Plain)]
#[kind(name = "aether.test.due.query")]
struct Query {
    onward: Onward,
}

/// What a query is answered with.
#[derive(Plain)]
#[kind(name = "aether.test.due.reply")]
struct Reply;

/// What a relay does with a query it holds, in the turn that holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Onward {
    /// It asks its mute a query, stating no deadline.
    Asks,
    /// It runs a sleep until long after any deadline of these tests.
    Runs,
    /// Nothing, until it is told to [`Forward`] it.
    Waits,
}

/// The protocol relays and mutes are asked through: a query, answered with a reply.
struct Queries;

impl Protocol for Queries {
    type Kinds = (Query,);
}

impl Carries<Query> for Queries {
    type Answers = (Reply,);
}

/// What became of a request, as its asker keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Heard {
    Replied,
    No(Unanswered),
}

impl Heard {
    /// `answered`, kept.
    const fn of(answered: &Answered<Reply>) -> Self {
        match answered {
            Answered::Yes(Reply) => Self::Replied,
            Answered::No(why) => Self::No(*why),
        }
    }
}

/// The engine's word that a request's deadline came first.
const EXPIRED: Heard = Heard::No(Unanswered::Expired);

/// What every actor of these tests keeps for a request of its own: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.due.pending")]
struct Pending;

/// What an actor of these tests is made from: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.due.blank")]
struct Blank;

/// Tells a mute to answer every query it holds.
#[derive(Plain)]
#[kind(name = "aether.test.due.release")]
struct Release;

/// Tells a relay to ask its mute, within each one's deadline, a query for each query it holds.
#[derive(Plain)]
#[kind(name = "aether.test.due.forward")]
struct Forward;

/// An actor that holds each query it is asked, keeping when each is due, and answers each once it is released.
#[derive(Bound)]
#[kind(name = "aether.test.due.mute")]
struct Mute {
    held: Vec<Held<Reply>>,
    /// When each query it held is due, in the order held.
    dues: Vec<Due>,
}

impl ChildOf for Mute {
    type Parent = Anywhere;
}

impl Actor for Mute {
    type Config = Blank;
    type Key = u64;
    type Handles = (Query, Release);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Blank, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { held: Vec::new(), dues: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Query> for Mute {
    type Sends = (ToSender<Reply>,);

    fn handle(&mut self, _: Query, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        self.dues.push(held.due());
        self.held.push(held);

        Ok(())
    }
}

impl Handles<Release> for Mute {
    type Sends = ();

    fn handle(&mut self, _: Release, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        for held in self.held.drain(..) {
            turn.answer(held, &Reply)?;
        }

        Ok(())
    }
}

/// An actor that holds each query it is asked and does with it what the query says, answering what it holds once its
/// own query to its mute is answered, as a hop of a call does.
#[derive(Bound)]
#[kind(name = "aether.test.due.relay")]
struct Relay {
    mute: ActorRef<Mute>,
    held: Vec<Held<Reply>>,
    /// When each query it held is due, in the order held.
    dues: Vec<Due>,
    /// What became of each query it asked its mute, in the order heard.
    heard: Vec<Heard>,
    /// How many of the sleeps it ran were answered, however.
    slept: u64,
}

impl ChildOf for Relay {
    type Parent = Anywhere;
}

impl Actor for Relay {
    type Config = Blank;
    type Key = ();
    type Handles = (Query, Forward, Answer<Reply, Pending>, Answer<Slept, Pending>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Blank, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let Ok(mute) = turn.spawn(&1, Blank) else {
            return Err(Fault::new(&Blank)?);
        };

        Ok(Self { mute, held: Vec::new(), dues: Vec::new(), heard: Vec::new(), slept: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Query> for Relay {
    type Sends = (ToSender<Reply>, Asks<Queries, Query, Pending>);

    fn handle(&mut self, query: Query, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        self.dues.push(held.due());
        self.held.push(held);
        match query.onward {
            Onward::Asks => {
                turn.to(self.mute.protocol()).ask(&Query { onward: Onward::Waits }, &Pending)?;
            }
            Onward::Runs => {
                turn.run(&Sleep { until: Time::from_nanos(u64::MAX) }, &Pending)?;
            }
            Onward::Waits => {}
        }

        Ok(())
    }
}

impl Handles<Forward> for Relay {
    type Sends = (Asks<Queries, Query, Pending>,);

    fn handle(&mut self, _: Forward, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        for held in &self.held {
            turn.to(self.mute.protocol()).within(held.due()).ask(&Query { onward: Onward::Waits }, &Pending)?;
        }

        Ok(())
    }
}

impl Handles<Answer<Reply, Pending>> for Relay {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Pending): (Answered<Reply>, Pending),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.heard.push(Heard::of(&answered));
        // It answers what it holds whatever it heard, as a hop frees its slot: past its deadline, the engine drops it.
        if !self.held.is_empty() {
            turn.answer(self.held.remove(0), &Reply)?;
        }

        Ok(())
    }
}

impl Handles<Answer<Slept, Pending>> for Relay {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Slept>, Pending), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.slept += 1;

        Ok(())
    }
}

/// Whom an asker asks: its relay, or its own mute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Whom {
    Relay,
    Mute,
}

/// What deadline an asker states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Stating {
    /// None: its ask inherits what its turn's item gives, which for the plain mail an order is, is nothing.
    Nothing,
    /// This time.
    Within(Time),
}

/// What a test tells an asker: to ask `whom`, stating as `stating` says, a query a relay does `onward` with.
#[derive(Plain)]
#[kind(name = "aether.test.due.order")]
struct Order {
    whom: Whom,
    stating: Stating,
    onward: Onward,
}

/// The root of these tests: it spawns a relay and a mute of its own as it begins, and asks one of them as each
/// [`Order`] says, keeping what became of each request.
#[derive(Bound)]
#[kind(name = "aether.test.due.asker")]
struct Asker {
    relay: ActorRef<Relay>,
    mute: ActorRef<Mute>,
    /// What became of each of its requests, in the order heard.
    heard: Vec<Heard>,
}

impl ChildOf for Asker {
    type Parent = Top;
}

impl Actor for Asker {
    type Config = Blank;
    type Key = ();
    type Handles = (Order, Answer<Reply, Pending>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Blank, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let (Ok(relay), Ok(mute)) = (turn.spawn(&(), Blank), turn.spawn(&0, Blank)) else {
            return Err(Fault::new(&Blank)?);
        };

        Ok(Self { relay, mute, heard: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Order> for Asker {
    type Sends = (Asks<Queries, Query, Pending>,);

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let to: ProtocolRef<Queries> = match order.whom {
            Whom::Relay => self.relay.protocol(),
            Whom::Mute => self.mute.protocol(),
        };
        let query = Query { onward: order.onward };
        match order.stating {
            Stating::Nothing => {
                turn.to(to).ask(&query, &Pending)?;
            }
            Stating::Within(at) => {
                turn.to(to).within(at).ask(&query, &Pending)?;
            }
        }

        Ok(())
    }
}

impl Handles<Answer<Reply, Pending>> for Asker {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Pending): (Answered<Reply>, Pending),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.heard.push(Heard::of(&answered));

        Ok(())
    }
}

/// Each engine the tests run in: how many parts, and how many fused hops each part runs at most in a step. In one part
/// an asker, its relay, and the mutes fuse, so a request may be handled in the step that asked it.
const ENGINES: [(usize, usize); 4] = [(1, 0), (1, 64), (4, 0), (4, 64)];

/// How long after the start the deadline these tests state is.
const WITHIN: Duration = Duration::from_millis(10);

/// The deadline these tests state.
const DUE: Time = Time::from_nanos(10_000_000);

/// How long after the start a nanosecond before that deadline is.
const BEFORE: Duration = Duration::from_nanos(9_999_999);

/// A harness over an engine in `parts` parts that each run at most `hops` fused hops a step, holding an asker, with
/// its relay and mutes stood up, at the start of its clock.
fn asking((parts, hops): (usize, usize)) -> Result<(Harness<Bench>, ActorRef<Asker>), Unheard> {
    let Some(parts) = NonZeroUsize::new(parts) else {
        panic!("an engine has parts");
    };
    let mut harness = Harness::bare(Engine::new(parts, 1024, 1 << 20, 1 << 20, hops).ready())?;
    harness.add_root::<Asker>(&(), Blank)?;
    harness.settle();
    let asker = harness.root::<Asker>();

    Ok((harness, asker))
}

/// What the asker has heard.
fn asker_heard(harness: &mut Harness<Bench>, asker: ActorRef<Asker>) -> Result<Vec<Heard>, Unheard> {
    Ok(harness.state(asker)?.heard.clone())
}

/// The asker's relay.
fn relay_of(harness: &mut Harness<Bench>, asker: ActorRef<Asker>) -> Result<ActorRef<Relay>, Unheard> {
    Ok(harness.state(asker)?.relay)
}

/// When each query the relay's mute held is due.
fn relays_mute_dues(harness: &mut Harness<Bench>, asker: ActorRef<Asker>) -> Result<Vec<Due>, Unheard> {
    let relay = relay_of(harness, asker)?;
    let mute = harness.state(relay)?.mute;

    Ok(harness.state(mute)?.dues.clone())
}

#[test]
fn a_request_unanswered_by_its_deadline_is_answered_expired_then_and_not_before() -> Result<(), Unheard> {
    // Catches a deadline never kept, one kept a step early, and the asker told some other reason.
    for engine in ENGINES {
        let (mut harness, asker) = asking(engine)?;
        let order = Order { whom: Whom::Mute, stating: Stating::Within(DUE), onward: Onward::Waits };
        harness.tell(asker, &order)?;
        harness.settle();

        harness.advance(BEFORE);
        harness.step();
        let before = asker_heard(&mut harness, asker)?;
        harness.advance(Duration::from_nanos(1));
        harness.settle();
        let at = asker_heard(&mut harness, asker)?;

        assert_eq!(before, [], "{engine:?}: a nanosecond before its deadline it is still waited for");
        assert_eq!(at, [EXPIRED], "{engine:?}: at its deadline it is answered `Expired`");
    }

    Ok(())
}

#[test]
fn an_answer_after_its_requests_deadline_is_dropped_and_the_asker_hears_once() -> Result<(), Unheard> {
    // Catches an expired request still filed, so the answer that comes later runs its asker's handler a second time,
    // with a context already handed over.
    for engine in ENGINES {
        let (mut harness, asker) = asking(engine)?;
        let order = Order { whom: Whom::Mute, stating: Stating::Within(DUE), onward: Onward::Waits };
        harness.tell(asker, &order)?;
        harness.settle();
        harness.advance(WITHIN);
        harness.settle();

        let mute = harness.state(asker)?.mute;
        harness.tell(mute, &Release)?;
        harness.settle();

        assert_eq!(asker_heard(&mut harness, asker)?, [EXPIRED], "{engine:?}");
        assert!(harness.state(mute)?.held.is_empty(), "{engine:?}: the mute answered what it held");
    }

    Ok(())
}

#[test]
fn a_request_made_for_a_request_inherits_its_deadline_and_expires_with_it() -> Result<(), Unheard> {
    // Catches an ask from a turn that handles a request left due never: the relay's query to its mute would hold its
    // slot for good once its own asker gave up, as `Mcp`'s did (#355).
    for engine in ENGINES {
        let (mut harness, asker) = asking(engine)?;
        let order = Order { whom: Whom::Relay, stating: Stating::Within(DUE), onward: Onward::Asks };
        harness.tell(asker, &order)?;
        harness.settle();
        let relay = relay_of(&mut harness, asker)?;
        let held = harness.state(relay)?.dues.clone();
        let mute_dues = relays_mute_dues(&mut harness, asker)?;

        harness.advance(WITHIN);
        harness.settle();

        assert_eq!(held, [Due::At(DUE)], "{engine:?}: the relay holds a request due when its asker said");
        assert_eq!(mute_dues, [Due::At(DUE)], "{engine:?}: what it asks for it is due then too");
        assert_eq!(asker_heard(&mut harness, asker)?, [EXPIRED], "{engine:?}");
        assert_eq!(harness.state(relay)?.heard, [EXPIRED], "{engine:?}: the relay's own ask expires with it");
    }

    Ok(())
}

#[test]
fn an_ask_from_a_turn_of_plain_mail_inherits_no_deadline() -> Result<(), Unheard> {
    // Catches a deadline inherited from something other than a request: an order is plain mail, so what the asker
    // asks for it, and what its relay asks in turn, are due never.
    for engine in ENGINES {
        let (mut harness, asker) = asking(engine)?;
        let order = Order { whom: Whom::Relay, stating: Stating::Nothing, onward: Onward::Asks };
        harness.tell(asker, &order)?;
        harness.settle();
        let relay = relay_of(&mut harness, asker)?;

        assert_eq!(harness.state(relay)?.dues, [Due::Never], "{engine:?}");
        assert_eq!(relays_mute_dues(&mut harness, asker)?, [Due::Never], "{engine:?}");
    }

    Ok(())
}

#[test]
fn a_request_due_never_waits_for_its_answer_however_late() -> Result<(), Unheard> {
    // Catches a request with no deadline expired anyway, by a clock run far on, or answered other than by its
    // receiver.
    for engine in ENGINES {
        let (mut harness, asker) = asking(engine)?;
        let order = Order { whom: Whom::Relay, stating: Stating::Nothing, onward: Onward::Asks };
        harness.tell(asker, &order)?;
        harness.settle();
        harness.advance(Duration::from_secs(1 << 20));
        harness.step();
        let waiting = asker_heard(&mut harness, asker)?;

        let relay = relay_of(&mut harness, asker)?;
        let mute = harness.state(relay)?.mute;
        harness.tell(mute, &Release)?;
        harness.settle();

        assert_eq!(waiting, [], "{engine:?}: nothing came while nothing answered it");
        assert_eq!(harness.state(relay)?.heard, [Heard::Replied], "{engine:?}");
        assert_eq!(asker_heard(&mut harness, asker)?, [Heard::Replied], "{engine:?}");
    }

    Ok(())
}

#[test]
fn a_later_turn_asks_for_a_held_request_within_its_deadline() -> Result<(), Unheard> {
    // Catches a held request that forgets its deadline, so a holder that asks for it in a later turn, a turn of plain
    // mail that inherits nothing, cannot give up when its asker does.
    for engine in ENGINES {
        let (mut harness, asker) = asking(engine)?;
        let order = Order { whom: Whom::Relay, stating: Stating::Within(DUE), onward: Onward::Waits };
        harness.tell(asker, &order)?;
        harness.settle();
        let relay = relay_of(&mut harness, asker)?;
        harness.tell(relay, &Forward)?;
        harness.settle();
        let mute_dues = relays_mute_dues(&mut harness, asker)?;

        harness.advance(WITHIN);
        harness.settle();

        assert_eq!(mute_dues, [Due::At(DUE)], "{engine:?}");
        assert_eq!(harness.state(relay)?.heard, [EXPIRED], "{engine:?}");
        assert_eq!(asker_heard(&mut harness, asker)?, [EXPIRED], "{engine:?}");
    }

    Ok(())
}

#[test]
fn a_job_run_for_a_request_inherits_no_deadline() -> Result<(), Unheard> {
    // Catches a job given the deadline of the request its turn handles, which the engine would answer `Expired` while
    // its host's work runs on: the engine here has no runner, so the sleep never runs, and only a deadline answers it.
    for engine in ENGINES {
        let (mut harness, asker) = asking(engine)?;
        let order = Order { whom: Whom::Relay, stating: Stating::Within(DUE), onward: Onward::Runs };
        harness.tell(asker, &order)?;
        harness.settle();
        harness.advance(WITHIN);
        harness.settle();

        let relay = relay_of(&mut harness, asker)?;
        assert_eq!(asker_heard(&mut harness, asker)?, [EXPIRED], "{engine:?}: the deadline came");
        assert_eq!(harness.state(relay)?.slept, 0, "{engine:?}: and the sleep was not answered for it");
    }

    Ok(())
}
