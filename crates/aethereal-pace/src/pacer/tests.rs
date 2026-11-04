//! The pacer's tests: a held clock is advanced one cycle at a time, each of the delta asked, and each advance is
//! answered once its last cycle has finished, in the order asked; an advance the clock cannot run is refused; and a
//! clock set running, or a program stopping, refuses the advances held.

use super::*;
use aethereal_actor::{ActorRef, Actors, Tick, Time};
use aethereal_data::{DecodeError, Encode, Kind, KindId};
use aethereal_substrate::harness::{Arrival, Askable, Bench, Harness, Steps, Unheard};
use aethereal_substrate::{Engine, Lifecycle};
use core::time::Duration;
use std::num::NonZeroUsize;

/// The most the tests' engine gives one `Tick`, in nanoseconds.
const LIMIT: u64 = 1000;

/// What reached the outside, as a test reads it from the queue (ADR-0036): an answer to what it asked, or mail of any
/// other kind, which no test expects.
#[derive(Debug, Clone, PartialEq)]
enum Noted {
    Set(PaceSet),
    Advanced(Advanced),
    Other(KindId),
}

impl Noted {
    /// What `arrival` is.
    fn of(arrival: &Arrival) -> Result<Self, DecodeError> {
        if let Some(set) = arrival.read()? {
            return Ok(Self::Set(set));
        }
        let advanced = arrival.read()?.map_or_else(|| Self::Other(arrival.kind()), Self::Advanced);

        Ok(advanced)
    }
}

/// A harness over a ready engine whose cycle is `Tick` alone, with the pacer, its clock moving as `pace` says as the
/// program starts, and the clockwork its host keeps.
fn paced(pace: Pace) -> Result<(Harness<Bench>, Clockwork), Unheard> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<(Tick,)>();
    let source = setup.source::<Paced>();
    let controller = setup.controller::<Pacing>();
    let (config, clockwork) = PacerConfig::paired(source, controller, pace, Duration::from_nanos(LIMIT));
    let mut harness = Harness::bare(setup.add_root::<Pacer>(&(), config)?.ready())?;
    harness.host().engine().start();

    Ok((harness, clockwork))
}

/// Asks `mail` of the pacer: what comes of it is read in the order it arrives, through [`clocked`].
fn ask<K>(harness: &mut Harness<Bench>, mail: &K) -> Result<(), Unheard>
where
    K: Kind + Encode<Actors>,
    ActorRef<Pacer>: Askable<K>,
{
    let pacer = harness.root::<Pacer>();
    let _ = harness.ask(pacer, mail)?;

    Ok(())
}

/// Steps until no actor has anything to run, then does as a host does between steps: takes what the pacer asked,
/// says a held cycle asked for is due, and says when it has finished. Does so `times` times, which is more than the
/// cycles asked for need, and answers with what reached the outside, in the order it arrived.
fn clocked(harness: &mut Harness<Bench>, clockwork: &mut Clockwork, times: usize) -> Result<Vec<Noted>, Unheard> {
    let mut noted = Vec::new();
    for _ in 0..times {
        harness.settle();
        let engine = harness.host().engine();
        let _ = clockwork.take(engine, Time::ZERO);
        clockwork.begin(engine);
        let _ = clockwork.settle(engine);
        for arrival in harness.arrived() {
            noted.push(Noted::of(&arrival)?);
        }
    }

    Ok(noted)
}

/// The program's time, as the engine counts it, in nanoseconds.
fn elapsed(harness: &mut Harness<Bench>) -> u128 {
    harness.host().engine().elapsed().as_nanos()
}

#[test]
fn a_held_clock_runs_each_advance_s_cycles_in_turn_and_answers_once_the_last_has_finished() -> Result<(), Unheard> {
    // Catches an advance answered before its last cycle has run, a cycle asked of the host before the last finished, so
    // two run as one, a delta other than the one asked, advances answered out of the order asked, and a clock that
    // starts held running cycles of its own.
    let (mut harness, mut clockwork) = paced(Pace::Held)?;
    assert!(clocked(&mut harness, &mut clockwork, 1)?.is_empty());
    assert!(!clockwork.runs(), "the clock starts held");

    ask(&mut harness, &Advance { cycles: 3, delta_nanos: 100 })?;
    ask(&mut harness, &Advance { cycles: 2, delta_nanos: 7 })?;
    assert_eq!(
        clocked(&mut harness, &mut clockwork, 3)?,
        [],
        "three cycles have run, one a pass, and the third has not been heard"
    );
    assert_eq!(elapsed(&mut harness), 300);
    let noted = clocked(&mut harness, &mut clockwork, 8)?;
    assert_eq!(
        noted,
        [
            Noted::Advanced(Advanced::Advanced { elapsed_nanos: 300 }),
            Noted::Advanced(Advanced::Advanced { elapsed_nanos: 314 })
        ]
    );
    assert_eq!(elapsed(&mut harness), 314, "no cycle of the host's own ran between them");

    Ok(())
}

#[test]
fn an_advance_the_clock_cannot_run_is_refused() -> Result<(), Unheard> {
    // Catches a running clock frozen by an advance, an advance of no cycles or no time held forever, and a delta past
    // the engine's limit handed to actors written for less.
    let (mut harness, mut clockwork) = paced(Pace::Real)?;
    ask(&mut harness, &Advance { cycles: 1, delta_nanos: 10 })?;
    assert_eq!(clocked(&mut harness, &mut clockwork, 1)?, [Noted::Advanced(Advanced::Refused(Unadvanced::Running))]);
    assert!(clockwork.runs());

    ask(&mut harness, &SetPace { pace: Pace::Held })?;
    ask(&mut harness, &Advance { cycles: 0, delta_nanos: 10 })?;
    ask(&mut harness, &Advance { cycles: 1, delta_nanos: 0 })?;
    ask(&mut harness, &Advance { cycles: 1, delta_nanos: LIMIT + 1 })?;
    ask(&mut harness, &Advance { cycles: 1, delta_nanos: LIMIT })?;
    let noted = clocked(&mut harness, &mut clockwork, 4)?;
    assert_eq!(
        noted,
        [
            Noted::Set(PaceSet { was: Pace::Real }),
            Noted::Advanced(Advanced::Refused(Unadvanced::NoCycles)),
            Noted::Advanced(Advanced::Refused(Unadvanced::NoTime)),
            Noted::Advanced(Advanced::Refused(Unadvanced::PastLimit { limit_nanos: LIMIT })),
            Noted::Advanced(Advanced::Advanced { elapsed_nanos: LIMIT }),
        ]
    );

    Ok(())
}

#[test]
fn a_clock_set_running_or_a_program_stopping_refuses_the_advances_held() -> Result<(), Unheard> {
    // Catches advances left held forever once no held cycle will run, a clock set running whose host still waits on a
    // held cycle, and one set to the pace it has answered as though it had another.
    let (mut harness, mut clockwork) = paced(Pace::Held)?;
    ask(&mut harness, &Advance { cycles: 50, delta_nanos: 10 })?;
    ask(&mut harness, &Advance { cycles: 1, delta_nanos: 10 })?;
    clocked(&mut harness, &mut clockwork, 2)?;
    ask(&mut harness, &SetPace { pace: Pace::Real })?;
    ask(&mut harness, &SetPace { pace: Pace::Real })?;
    let noted = clocked(&mut harness, &mut clockwork, 2)?;
    assert_eq!(
        noted,
        [
            Noted::Advanced(Advanced::Refused(Unadvanced::Running)),
            Noted::Advanced(Advanced::Refused(Unadvanced::Running)),
            Noted::Set(PaceSet { was: Pace::Held }),
            Noted::Set(PaceSet { was: Pace::Real }),
        ]
    );
    assert!(clockwork.runs(), "the host's own cycles are due again");

    ask(&mut harness, &SetPace { pace: Pace::Held })?;
    ask(&mut harness, &Advance { cycles: 50, delta_nanos: 10 })?;
    clocked(&mut harness, &mut clockwork, 2)?;
    harness.host().engine().quit();
    let noted = clocked(&mut harness, &mut clockwork, 4)?;
    assert_eq!(noted, [Noted::Advanced(Advanced::Refused(Unadvanced::Stopping))]);
    assert_eq!(harness.host().engine().lifecycle(), Lifecycle::Stopped);

    Ok(())
}
