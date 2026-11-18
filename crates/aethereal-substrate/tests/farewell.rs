//! A guard checks the type its mail was sent as, stamped at the send: so what a child tells its parent from its closing
//! turn reaches the parent's guarded handler, though the child has left the table by the time it is delivered, and what
//! a guard refuses is logged as of that type (ADR-0020, ADR-0058).

use aethereal_actor::guard::{Child, Parent};
use aethereal_actor::{ActorRef, Answered, Declared, Ending, Entry, Fault, Level, Speaks, Turn, actor};
use aethereal_data::Plain;
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Steps, Unheard};
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
    /// The handler guarded by `ActorRef<Elder>`, which refuses a youngster.
    Elders,
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

/// A farewell to the elder's handler guarded by `ActorRef<Elder>`, which takes it only from an elder.
#[derive(Plain)]
#[kind(name = "aether.test.farewell.to_elders")]
struct ToElders;

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

    #[tell]
    fn to_elders(&mut self, _: ToElders, _: &mut Turn<'_, Self, ActorRef<Elder>>) -> Result<(), Fault> {
        self.heard.push(Through::Elders);

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
        match self.farewell {
            Through::Child => {
                turn.to(parent).send(&ToAParent)?;
            }
            Through::Type => {
                turn.to(parent).send(&FromAType)?;
            }
            Through::Protocol => {
                turn.to(parent).send(&FromASpeaker)?;
            }
            // Through a reference that does not name the elder's type, so no check at the call refuses what its guard
            // will (ADR-0059).
            Through::Elders => {
                if let Some(elder) = turn.cast::<(ToElders,)>(parent) {
                    turn.to(elder).send(&ToElders)?;
                }
            }
        }

        Ok(())
    }
}

/// The elder, once a youngster that says farewell through `farewell` has left, and its closing turn's mail has been
/// handled: on an engine that fuses no mail, so each piece is handled in the step after it was sent, by which time the
/// youngster has left the table.
fn farewelled(farewell: Through) -> Result<(Harness<Bench>, ActorRef<Elder>), Unheard> {
    let Ok(setup) =
        Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 20, 0).add_root::<Elder>(&(), Elder { heard: Vec::new() })
    else {
        panic!("the elder begins");
    };
    let mut harness: Harness<Bench> = Harness::bare(setup.ready())?;
    let elder = harness.root::<Elder>();
    harness.tell(elder, &Raise { farewell })?;
    harness.settle();

    Ok((harness, elder))
}

/// The farewells the elder heard once a youngster that says farewell through `farewell` has left.
fn heard_through(farewell: Through) -> Result<Vec<Through>, Unheard> {
    let (mut harness, elder) = farewelled(farewell)?;

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

#[test]
fn a_refused_farewell_from_a_closing_turn_is_logged_as_the_type_that_sent_it() -> Result<(), Unheard> {
    // Catches a refusal that names its sender's type from the table, which no longer holds a youngster once its closing
    // turn has run, so the line calls it of an unknown type.
    let (mut harness, elder) = farewelled(Through::Elders)?;
    let youngster = Declared::of::<Youngster>().name().as_str();
    let logged = harness.host().engine().log(elder.seated());
    let warned: Vec<String> = logged
        .filter_map(|entry| match entry {
            Entry::Line { level: Level::Warn, text } => Some(text.clone()),
            _ => None,
        })
        .collect();

    assert!(warned.iter().any(|line| line.contains(youngster)), "{warned:?}");
    Ok(())
}
