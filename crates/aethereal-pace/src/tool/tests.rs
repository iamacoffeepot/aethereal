//! The clock's tools' tests: each call is asked of the pacer as the tool it names, and answered with what the pacer
//! answered. A program that lists the tool and no pacer before it does not build (ADR-0022: `NeedsIn`).

use super::*;
use crate::pacer::{Pace, PacerConfig, Pacing, Unadvanced};
use aethereal_actor::Tick;
use aethereal_data::Plain;
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Unheard};
use core::time::Duration;
use std::num::NonZeroUsize;

/// A harness over an engine with the pacer, its clock running, and the tool.
fn tooled() -> Result<Harness<Bench>, Unheard> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<(Tick,)>();
    let source = setup.source::<crate::Paced>();
    let controller = setup.controller::<Pacing>();
    let (config, _clockwork) = PacerConfig::paired(source, controller, Pace::Real, Duration::from_millis(100));
    let engine = setup.add_root::<Pacer>(&(), config)?.add_root::<PaceTool>(&(), PaceToolConfig)?.ready();

    Harness::bare(engine)
}

/// Calls `tool` with `input`, and answers what it answered: the bytes of its value, or why it refused.
fn call<K: Plain>(harness: &mut Harness<Bench>, tool: &str, input: &K) -> Result<Result<Vec<u8>, String>, Unheard> {
    let input = harness.put(to_bytes(input)?)?;
    let worker = PaceTool::worker(harness.root::<PaceTool>());
    let asked = harness.ask(worker, &Call { tool: String::from(tool), input })?;
    let called = match harness.answer(asked)? {
        Answered::Yes(Called::Output(output)) => Ok(harness.read(&output).map(<[u8]>::to_vec).unwrap_or_default()),
        Answered::Yes(Called::Refused(why)) => Err(why),
        Answered::Yes(Called::Image { .. }) | Answered::No(_) => Err(String::from("not an answer the tool gives")),
    };

    Ok(called)
}

#[test]
fn each_call_is_asked_of_the_pacer_as_named_and_answered_with_its_answer() -> Result<(), Unheard> {
    // Catches a call asked of the pacer as the other tool, the pacer's answer not passed back, and a name the worker
    // does not serve left unanswered.
    let mut harness = tooled()?;
    let set = call(&mut harness, "set_pace", &SetPace { pace: Pace::Held })?;
    assert_eq!(set, Ok(to_bytes(&PaceSet { was: Pace::Real })?));
    let advanced = call(&mut harness, "advance", &Advance { cycles: 0, delta_nanos: 10 })?;
    assert_eq!(advanced, Ok(to_bytes(&Advanced::Refused(Unadvanced::NoCycles))?));
    assert!(call(&mut harness, "rewind", &SetPace { pace: Pace::Real })?.is_err());

    Ok(())
}
