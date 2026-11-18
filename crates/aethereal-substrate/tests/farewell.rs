//! A guard checks the type its mail was sent as, stamped at the send: so what a child tells its parent from its closing
//! turn reaches the parent's guarded handler, though the child has left the table by the time it is delivered
//! (ADR-0020, ADR-0058).

use aethereal_actor::guard::{Child, Parent};
use aethereal_actor::{ActorRef, Answered, Ending, Fault, Speaks, Turn, actor};
use aethereal_data::Plain;
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Unheard};
use std::num::NonZeroUsize;

/// Which of the elder's handlers a youngster says farewell to as it ends: each is guarded another way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Through {
    /// The handler guarded by `Child<Youngster>`.
    Child,
    /// The handler guarded by `ActorRef<Youngster>`.
    Type,
    /// The handler guarded by `Speaks<(Leave,)>`.
    Protocol,
}

/// A farewell to the elder's handler guarded by `Child<Youngster>`.
#[derive(Plain)]
#[kind(name = "aether.test.farewell.to_a_parent")]
struct ToAParent;

/// A farewell to the elder's handler guarded by `ActorRef<Youngster>`.
#[derive(Plain)]
#[kind(name = "aether.test.farewell.from_a_type")]
struct FromAType;

/// A farewell to the elder's handler guarded by `Speaks<(Leave,)>`.
#[derive(Plain)]
#[kind(name = "aether.test.farewell.from_a_speaker")]
struct FromASpeaker;

/// A word to the elder to raise a youngster that says farewell through `farewell`, and to tell it to leave.
#[derive(Plain)]
#[kind(name = "aether.test.farewell.raise")]
struct Raise {
    farewell: Through,
}

/// A word to a youngster to close.
#[derive(Plain)]
#[kind(name = "aether.test.farewell.leave")]
struct Leave;

/// A question to the elder: which farewells has it heard?
#[derive(Plain)]
#[kind(name = "aether.test.farewell.heard")]
struct Heard;

/// What the elder answers a question of what it heard with: each farewell its handlers took, oldest first.
#[derive(Plain)]
#[kind(name = "aether.test.farewell.farewells")]
struct Farewells {
    heard: Vec<Through>,
}

/// A root that raises youngsters, and keeps each farewell its guarded handlers take.
#[derive(Plain)]
#[kind(name = "aether.test.farewell.elder")]
struct Elder {
    heard: Vec<Through>,
}

#[actor(spawns(Youngster))]
impl Elder {
    fn begin(elder: Self) -> Result<Self, Fault> {
        Ok(elder)
    }

    #[tell]
    fn raise(&mut self, raise: Raise, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if let Ok(youngster) = turn.spawn::<Youngster>(&(), Youngster { farewell: raise.farewell }) {
            turn.to(youngster).send(&Leave)?;
        }

        Ok(())
    }

    #[tell]
    fn to_a_parent(&mut self, _: ToAParent, _: &mut Turn<'_, Self, Child<Youngster>>) -> Result<(), Fault> {
        self.heard.push(Through::Child);

        Ok(())
    }

    #[tell]
    fn from_a_type(&mut self, _: FromAType, _: &mut Turn<'_, Self, ActorRef<Youngster>>) -> Result<(), Fault> {
        self.heard.push(Through::Type);

        Ok(())
    }

    #[tell]
    fn from_a_speaker(&mut self, _: FromASpeaker, _: &mut Turn<'_, Self, Speaks<(Leave,)>>) -> Result<(), Fault> {
        self.heard.push(Through::Protocol);

        Ok(())
    }

    #[request]
    fn heard(&mut self, _: Heard) -> Result<Farewells, Fault> {
        Ok(Farewells { heard: self.heard.clone() })
    }
}

/// A child of the elder that closes when its parent tells it to leave, and says farewell from its closing turn.
#[derive(Plain)]
#[kind(name = "aether.test.farewell.youngster")]
struct Youngster {
    farewell: Through,
}

#[actor(parent = Elder)]
impl Youngster {
    fn begin(youngster: Self) -> Result<Self, Fault> {
        Ok(youngster)
    }

    #[tell]
    fn leave(&mut self, _: Leave, turn: &mut Turn<'_, Self, Parent>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }

    fn end(self, turn: &mut Ending<'_, Self>) -> Result<(), Fault> {
        let parent = turn.parent();
        let mut to_parent = turn.to(parent);
        match self.farewell {
            Through::Child => to_parent.send(&ToAParent)?,
            Through::Type => to_parent.send(&FromAType)?,
            Through::Protocol => to_parent.send(&FromASpeaker)?,
        };

        Ok(())
    }
}

/// The farewells the elder heard once a youngster that says farewell through `farewell` has left, and its closing
/// turn's mail has been handled: on an engine that fuses no mail, so each piece is handled in the step after it was
/// sent, by which time the youngster has left the table.
fn heard_through(farewell: Through) -> Result<Vec<Through>, Unheard> {
    let Ok(setup) =
        Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 20, 0).add_root::<Elder>(&(), Elder { heard: Vec::new() })
    else {
        panic!("the elder begins");
    };
    let mut harness: Harness<Bench> = Harness::bare(setup.ready())?;
    let elder = harness.root::<Elder>();
    harness.tell(elder, &Raise { farewell })?;
    harness.settle();

    let asked = harness.ask(elder, &Heard)?;
    let heard = match harness.answer(asked)? {
        Answered::Yes(farewells) => farewells.heard,
        Answered::No(why) => panic!("the elder answers what it heard: {why:?}"),
    };

    Ok(heard)
}

#[test]
fn a_farewell_from_a_closing_turn_reaches_a_handler_guarded_by_child() -> Result<(), Unheard> {
    // Catches a guard that reads the sender's type from the table, which no longer holds a child once its closing turn
    // has run, so the farewell is refused and dropped with a log line.
    assert_eq!(heard_through(Through::Child)?, [Through::Child]);
    Ok(())
}

#[test]
fn a_farewell_from_a_closing_turn_reaches_a_handler_guarded_by_the_senders_type() -> Result<(), Unheard> {
    // Catches `ActorRef<T>` checked against the table, where a sender that has ended is of no type.
    assert_eq!(heard_through(Through::Type)?, [Through::Type]);
    Ok(())
}

#[test]
fn a_farewell_from_a_closing_turn_reaches_a_handler_guarded_by_a_protocol_it_speaks() -> Result<(), Unheard> {
    // Catches `Speaks<P>` checked against the table, where a sender that has ended lists no kind.
    assert_eq!(heard_through(Through::Protocol)?, [Through::Protocol]);
    Ok(())
}
