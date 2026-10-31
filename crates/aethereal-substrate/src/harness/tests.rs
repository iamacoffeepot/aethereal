//! The outside asked of, asking, and hearing, over a bare engine (ADR-0036).

use super::{Harness, Ran, Unheard};
use crate::Engine;
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Protocol, ProtocolRef, ToAny,
    ToSender, ToType, Top, Turn, Unanswered,
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
fn what_arrived_is_read_whole_in_the_order_it_came_and_is_gone() -> Result<(), Unheard> {
    // Catches arrivals handed over from the back of the queue, which would turn the order of an answer among events
    // around, and a queue left holding them, which would hand each out twice.
    let mut harness = harness();
    let report = harness.protocol::<Listening>();
    harness.add_root::<Relay>(&(), Report { to: report })?;
    let (doubler, relay) = (harness.root::<Doubler>(), harness.root::<Relay>());

    let _ = harness.ask(doubler, &Ping(4))?;
    harness.settle();
    harness.tell(relay, &Note(7))?;
    harness.settle();

    let arrived = harness.arrived();
    let [answer, told] = &arrived[..] else {
        panic!("two pieces of mail came: {}", arrived.len());
    };
    assert_eq!(answer.read::<Pong>(), Ok(Some(Pong(8))));
    assert_eq!(told.read::<Note>(), Ok(Some(Note(7))));
    assert_eq!(harness.arrived().len(), 0);

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

/// A hop down from `Pass(n)` to `Pass(0)`.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.outside.pass")]
struct Pass(u32);

/// A root that keeps what it is passed, passes one less on to itself, and fails on `Pass(0)`.
#[derive(Plain)]
#[kind(name = "aether.test.outside.passer")]
struct Passer {
    seen: Vec<u32>,
}

impl ChildOf for Passer {
    type Parent = Top;
}

impl Actor for Passer {
    type Config = Self;
    type Key = ();
    type Handles = (Pass,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(passer: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(passer)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Pass> for Passer {
    type Sends = (ToType<Self, Pass>,);

    fn handle(&mut self, Pass(left): Pass, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.seen.push(left);
        if left == 0 {
            return Err(Fault::new(&Pass(left))?);
        }
        turn.to(turn.me()).send(&Pass(left - 1))?;

        Ok(())
    }
}

#[test]
fn a_step_is_read_turn_by_turn_with_its_hops_and_its_failure() -> Result<(), Unheard> {
    // Catches a hop not told from a turn of a waiting item, and a failed turn left out of the record, which is what a
    // report by mail could not show.
    let mut harness = harness_of(NonZeroUsize::MIN, 64);
    harness.add_root::<Passer>(&(), Passer { seen: Vec::new() })?;
    let passer = harness.root::<Passer>();
    harness.tell(passer, &Pass(2))?;

    let turns = harness.step_turns();

    assert_eq!(turns.len(), 3);
    assert_eq!(turns[0].read::<Pass>()?, Pass(2));
    assert_eq!(turns[0].ran, Ran::Waiting);
    assert!(turns[1].is_hop());
    assert_eq!(turns[2].read::<Pass>()?, Pass(0));
    assert!(turns[2].failed());
    assert!(!turns[1].failed());

    Ok(())
}

#[test]
fn a_closing_turn_is_recorded() -> Result<(), Unheard> {
    // Catches a closing turn left out of the record, so a test could not see an actor end.
    let mut harness = harness_of(NonZeroUsize::MIN, 0);
    harness.add_root::<Passer>(&(), Passer { seen: Vec::new() })?;
    let passer = harness.root::<Passer>();
    harness.tell(passer, &Pass(0))?;

    assert!(harness.step_turns()[0].failed());

    assert_eq!(harness.step_turns()[0].ran, Ran::Closing);

    Ok(())
}

#[test]
fn an_actor_s_state_is_read_between_steps_and_recording_sends_nothing() -> Result<(), Unheard> {
    // Catches a read of the wrong seat, and a record that counts as a send, which would change what a test measures.
    let mut harness = harness_of(NonZeroUsize::MIN, 0);
    harness.add_root::<Passer>(&(), Passer { seen: Vec::new() })?;
    let passer = harness.root::<Passer>();
    harness.tell(passer, &Pass(1))?;

    let _ = harness.step_turns();

    assert_eq!(harness.state(passer)?.seen, [1]);
    let sent = harness.host().engine.sent(passer.seated());
    assert_eq!(sent, 1, "the one piece it passed on, and nothing for the record");

    Ok(())
}

#[test]
fn a_tuple_of_requests_is_answered_each_in_its_own_type_whatever_order_they_settle_in() -> Result<(), Unheard> {
    // Catches answers read in the order they arrived rather than the order given, one wait spending the budget of the
    // next, and an unanswered request read as an answer: the silent request settles in the same step as the others.
    let mut harness = harness();
    let doubler = harness.root::<Doubler>();

    let silent = harness.ask(doubler, &Silence)?;
    let ping = harness.ask(doubler, &Ping(4))?;

    let answers = harness.answers((ping, silent))?;

    assert_eq!(answers, (Answered::Yes(Pong(8)), Answered::No(Unanswered::Unreplied)));
    assert_eq!(harness.rest(), Vec::new());

    Ok(())
}

#[test]
fn a_list_of_requests_is_answered_in_the_order_given() -> Result<(), Unheard> {
    // Catches a list read in the order its answers arrived.
    let mut harness = harness();
    let doubler = harness.root::<Doubler>();
    let mut asked = Vec::new();
    for value in [3, 1, 2] {
        asked.push(harness.ask(doubler, &Ping(value))?);
    }
    asked.reverse();

    let answers = harness.answers(asked)?;

    assert_eq!(answers, [Answered::Yes(Pong(4)), Answered::Yes(Pong(2)), Answered::Yes(Pong(6))]);

    Ok(())
}

#[test]
fn settling_answers_with_every_turn_of_every_step_in_order() -> Result<(), Unheard> {
    // Catches turns of earlier steps thrown away, and turns recorded before the settle reported as part of it.
    let mut harness = harness();
    harness.add_root::<Passer>(&(), Passer { seen: Vec::new() })?;
    let passer = harness.root::<Passer>();
    harness.tell(passer, &Pass(3))?;
    harness.step();

    let turns = harness.settle_turns();

    let handed: Vec<Pass> = turns.iter().filter_map(|turn| turn.read().ok()).collect();
    assert_eq!(handed, [Pass(2), Pass(1), Pass(0)]);
    assert_eq!(turns.last().map(|turn| turn.ran), Some(Ran::Closing));
    assert!(turns.iter().map(|turn| turn.step).is_sorted());

    Ok(())
}

/// A root that casts whoever tells it a `Note` to the protocol that takes one, and tells them back through the cast.
#[derive(Plain)]
#[kind(name = "aether.test.outside.caster")]
struct Caster;

impl ChildOf for Caster {
    type Parent = Top;
}

impl Actor for Caster {
    type Config = Self;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(caster: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(caster)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Caster {
    type Sends = (ToAny<(Note,), Note>,);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if let Some(sender) = turn.cast(turn.sender()) {
            turn.to(sender).send(&note)?;
        }

        Ok(())
    }
}

#[test]
fn the_outside_passes_a_cast_as_it_takes_every_kind() -> Result<(), Unheard> {
    // Catches a subscriber's cast refused for the outside, which lists no kind, so a test could not subscribe.
    let mut harness = harness();
    harness.add_root::<Caster>(&(), Caster)?;
    let caster = harness.root::<Caster>();

    harness.tell(caster, &Note(9))?;

    assert_eq!(harness.heard::<Note>(), Ok(Note(9)));

    Ok(())
}
