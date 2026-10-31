//! The outside asked of, asking, and hearing, over a bare engine (ADR-0036).

use super::{Harness, Unheard};
use crate::Engine;
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Protocol, ProtocolRef, ToAny,
    ToSender, Top, Turn, Unanswered,
};
use aethereal_data::{Bound, Plain};
use std::num::NonZeroUsize;

#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.outside.ping")]
struct Ping(u32);

#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.outside.pong")]
struct Pong(u32);

#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.outside.note")]
struct Note(u32);

#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.outside.silence")]
struct Silence;

/// What the relay asks and tells: the outside, as it takes every kind.
struct Listening;

impl Protocol for Listening {
    type Kinds = (Ping, Note);
}

impl Carries<Ping> for Listening {
    type Answers = (Pong,);
}

impl Carries<Note> for Listening {
    type Answers = ();
}

/// A root that doubles what it is asked, and stays silent or fails when told so.
#[derive(Plain)]
#[kind(name = "aether.test.outside.doubler")]
struct Doubler;

impl ChildOf for Doubler {
    type Parent = Top;
}

impl Actor for Doubler {
    type Config = Self;
    type Key = ();
    type Handles = (Ping, Silence);
    type BeginSends = ();
    type EndSends = ();

    fn begin(doubler: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(doubler)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Ping> for Doubler {
    type Answers = (Pong,);
}

impl Carries<Silence> for Doubler {
    type Answers = (Pong,);
}

impl Handles<Ping> for Doubler {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, Ping(value): Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(turn.reply(&Pong(value * 2))?)
    }
}

impl Handles<Silence> for Doubler {
    type Sends = ();

    fn handle(&mut self, _: Silence, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The config of a [`Relay`]: where it reports.
#[derive(Bound)]
#[kind(name = "aether.test.outside.relay_config")]
struct Report {
    to: ProtocolRef<Listening>,
}

/// What a relay keeps for a request it asks: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.outside.waiting")]
struct Waiting;

/// A root that notes what it is told, and asks the outside what it is told to.
#[derive(Bound)]
#[kind(name = "aether.test.outside.relay")]
struct Relay {
    report: ProtocolRef<Listening>,
}

impl ChildOf for Relay {
    type Parent = Top;
}

impl Actor for Relay {
    type Config = Report;
    type Key = ();
    type Handles = (Note, Ping, Answer<Pong, Waiting>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(report: Report, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { report: report.to })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Relay {
    type Sends = (ToAny<Listening, Note>,);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(turn.to(self.report).send(&note).map(|_| ())?)
    }
}

impl Handles<Ping> for Relay {
    type Sends = (Asks<Listening, Ping, Waiting>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(turn.to(self.report).ask(&ping, &Waiting).map(|_| ())?)
    }
}

impl Handles<Answer<Pong, Waiting>> for Relay {
    type Sends = (ToAny<Listening, Note>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Pong>, Waiting),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let note = match answered {
            Answered::Yes(Pong(value)) => Note(value),
            Answered::No(_) => Note(0),
        };

        Ok(turn.to(self.report).send(&note).map(|_| ())?)
    }
}

/// A harness over a one-part engine holding a [`Doubler`].
fn harness() -> Harness<super::Bench> {
    harness_of(NonZeroUsize::MIN, 0)
}

/// A harness over an engine of `parts` parts that fuses `hops` hops, holding a [`Doubler`].
fn harness_of(parts: NonZeroUsize, hops: usize) -> Harness<super::Bench> {
    let Ok(setup) = Engine::new(parts, 1024, 1 << 20, 1 << 20, hops).add_root::<Doubler>(&(), Doubler) else {
        panic!("the doubler is made");
    };

    Harness::bare(setup.ready()).expect("the outside is seated")
}

#[test]
fn an_answer_comes_back_typed() -> Result<(), Unheard> {
    let mut harness = harness();
    let doubler = harness.root::<Doubler>();

    let asked = harness.ask(doubler, &Ping(21))?;

    assert_eq!(harness.answer(asked), Ok(Answered::Yes(Pong(42))));
    assert_eq!(harness.rest(), Vec::new());

    Ok(())
}

#[test]
fn two_requests_are_read_in_either_order() -> Result<(), Unheard> {
    let mut harness = harness();
    let doubler = harness.root::<Doubler>();

    let first = harness.ask(doubler, &Ping(1))?;
    let second = harness.ask(doubler, &Ping(2))?;

    assert_eq!(harness.answer(second), Ok(Answered::Yes(Pong(4))));
    assert_eq!(harness.answer(first), Ok(Answered::Yes(Pong(2))));

    Ok(())
}

#[test]
fn a_handler_that_stays_silent_reads_as_the_asker_would() -> Result<(), Unheard> {
    let mut harness = harness();
    let doubler = harness.root::<Doubler>();

    let asked = harness.ask(doubler, &Silence)?;

    assert_eq!(harness.answer(asked), Ok(Answered::No(Unanswered::Unreplied)));

    Ok(())
}

#[test]
fn what_an_actor_tells_is_heard_and_nothing_else_came() -> Result<(), Unheard> {
    let mut harness = harness();
    let report = harness.protocol::<Listening>();
    harness.add_root::<Relay>(&(), Report { to: report })?;
    let relay = harness.root::<Relay>();

    harness.tell(relay, &Note(7))?;

    assert_eq!(harness.heard::<Note>(), Ok(Note(7)));
    harness.settle();
    assert_eq!(harness.rest(), Vec::new());

    Ok(())
}

#[test]
fn a_request_of_the_outside_is_held_and_replied_to() -> Result<(), Unheard> {
    let mut harness = harness();
    let report = harness.protocol::<Listening>();
    harness.add_root::<Relay>(&(), Report { to: report })?;
    let relay = harness.root::<Relay>();
    harness.tell(relay, &Ping(5))?;

    let held = harness.asked::<Ping>()?;
    assert_eq!(held.request, Ping(5));
    harness.reply(held, &Pong(50))?;

    assert_eq!(harness.heard::<Note>(), Ok(Note(50)));

    Ok(())
}

#[test]
fn a_request_of_the_outside_can_be_let_go_unanswered() -> Result<(), Unheard> {
    let mut harness = harness();
    let report = harness.protocol::<Listening>();
    harness.add_root::<Relay>(&(), Report { to: report })?;
    let relay = harness.root::<Relay>();
    harness.tell(relay, &Ping(5))?;

    let held = harness.asked::<Ping>()?;
    harness.unanswered(held, Unanswered::Failed)?;

    assert_eq!(harness.heard::<Note>(), Ok(Note(0)));

    Ok(())
}

#[test]
fn a_reply_of_the_wrong_kind_is_refused() -> Result<(), Unheard> {
    let mut harness = harness();
    let report = harness.protocol::<Listening>();
    harness.add_root::<Relay>(&(), Report { to: report })?;
    let relay = harness.root::<Relay>();
    harness.tell(relay, &Ping(5))?;

    let held = harness.asked::<Ping>()?;

    assert!(matches!(harness.reply(held, &Note(1)), Err(Unheard::OtherAnswer { .. })));

    Ok(())
}

#[test]
fn waiting_on_what_never_comes_names_what_came_instead() -> Result<(), Unheard> {
    let mut harness = harness();
    let report = harness.protocol::<Listening>();
    harness.add_root::<Relay>(&(), Report { to: report })?;
    let relay = harness.root::<Relay>();
    harness.tell(relay, &Note(1))?;

    assert!(matches!(harness.heard::<Pong>(), Err(Unheard::OtherKind { .. })));
    assert!(matches!(harness.heard::<Pong>(), Err(Unheard::OtherKind { .. })));
    assert_eq!(harness.heard::<Note>(), Ok(Note(1)));
    assert!(matches!(harness.heard::<Note>(), Err(Unheard::Spent { .. })));

    Ok(())
}

#[test]
fn the_outside_is_asked_across_parts_and_through_hops() -> Result<(), Unheard> {
    let mut harness = harness_of(NonZeroUsize::new(4).expect("four is not nought"), 64);
    let report = harness.protocol::<Listening>();
    harness.add_root::<Relay>(&(), Report { to: report })?;
    let doubler = harness.root::<Doubler>();
    let relay = harness.root::<Relay>();

    let asked = harness.ask(doubler, &Ping(3))?;
    harness.tell(relay, &Ping(4))?;

    let held = harness.asked::<Ping>()?;
    harness.reply(held, &Pong(40))?;
    assert_eq!(harness.heard::<Note>(), Ok(Note(40)));
    assert_eq!(harness.answer(asked), Ok(Answered::Yes(Pong(6))));

    Ok(())
}
