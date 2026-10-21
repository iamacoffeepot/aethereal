//! The chassis's tests: a frame runs its cycle through to its end, and brings in what the page pushed before it.

use super::*;
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, ChildOf, Ending, Fault, Given, Handle, Handles, On, Render, RunsJob, Sleep,
    Slept, Source, Tick, Top, Turn,
};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_substrate::{Inline, Jobs, Setup};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// A clock that reads the same time always: the tests' frames take no time.
struct Still;

impl Clock for Still {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An actor's empty config.
#[derive(Plain)]
struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.test.nothing");
}

/// What the painter was sent, in order.
static PAINTED: Mutex<Vec<&str>> = Mutex::new(Vec::new());

/// Notes what the painter was sent.
fn note(what: &'static str) {
    PAINTED.lock().map(|mut painted| painted.push(what)).ok();
}

/// An actor that notes each `Tick` and `Render` it is sent, and sleeps until now on each `Tick`, noting the answer.
#[derive(Bound)]
struct Painter;

impl Kind for Painter {
    const NAME: KindName = KindName::new("aether.test.painter");
}

impl ChildOf for Painter {
    type Parent = Top;
}

impl Actor for Painter {
    type Config = Nothing;
    type Key = ();
    type Handles = (On<Tick>, On<Render>, Answer<Slept, Nothing>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();
        turn.subscribe::<Render>();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Tick>> for Painter {
    type Sends = (RunsJob<Sleep, Nothing>,);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        note("tick");
        turn.run(&Sleep { until: turn.now() }, &Nothing)?;

        Ok(())
    }
}

impl Handles<On<Render>> for Painter {
    type Sends = ();

    fn handle(&mut self, _: Render, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        note("render");

        Ok(())
    }
}

impl Handles<Answer<Slept, Nothing>> for Painter {
    type Sends = ();

    fn handle(&mut self, (done, _): (Answered<Slept>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let said = match done {
            Answered::Yes(Slept) => "slept",
            Answered::No(_) => "not answered",
        };
        note(said);

        Ok(())
    }
}

#[test]
fn a_frame_runs_its_cycle_through_to_its_end_and_is_answered_for_the_jobs_it_ran() {
    // Catches a frame that never says a cycle is due, steps once rather than until nothing is left to run, or brings
    // in what its jobs finished only before its first step: a frame would end before its `Render`, or before the
    // sleep its `Tick` ran was answered, and the next would begin with what the last left.
    let one = NonZeroUsize::MIN;
    let jobs: Arc<dyn Jobs> = Arc::new(Inline::new(Vec::new()));
    let Ok(mut engine) =
        Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(jobs).with_cycle::<(Tick, Render)>().map(Setup::ready)
    else {
        panic!("the cycle is in order");
    };
    let Ok(_) = engine.add_root::<Painter>(&(), Nothing) else {
        panic!("the painter begins");
    };
    let mut browser = Browser::new(engine, Still);

    browser.frame();
    let Ok(first) = PAINTED.lock().map(|painted| painted.clone()) else {
        panic!("what was painted is not poisoned");
    };
    browser.frame();
    let Ok(both) = PAINTED.lock().map(|painted| painted.clone()) else {
        panic!("what was painted is not poisoned");
    };

    // A stage does not wait for the jobs it ran (ADR-0020), so the answer comes after the render, in the same frame.
    assert_eq!(first, ["tick", "render", "slept"]);
    assert_eq!(both, ["tick", "render", "slept", "tick", "render", "slept"]);
}

/// The pointer moved: what the page pushes onto a pointer's source.
#[derive(Plain)]
struct Moved {
    x: u64,
}

impl Kind for Moved {
    const NAME: KindName = KindName::new("aether.test.moved");
}

/// The source a pointer is handed.
#[derive(Bound)]
struct PointerConfig {
    source: Given<Source<Moved>>,
}

impl Kind for PointerConfig {
    const NAME: KindName = KindName::new("aether.test.pointer_config");
}

/// Where the pointer was brought, in order.
static MOVED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// An actor that holds a source of moves, and notes each it is brought.
#[derive(Bound)]
struct Pointer {
    source: Handle<Source<Moved>>,
}

impl Kind for Pointer {
    const NAME: KindName = KindName::new("aether.test.pointer");
}

impl ChildOf for Pointer {
    type Parent = Top;
}

impl Actor for Pointer {
    type Config = PointerConfig;
    type Key = ();
    type Handles = (Moved,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: PointerConfig, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let source = turn.take(config.source)?;

        Ok(Self { source })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Moved> for Pointer {
    type Sends = ();

    fn handle(&mut self, moved: Moved, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        MOVED.lock().map(|mut brought| brought.push(moved.x)).ok();

        Ok(())
    }
}

#[test]
fn a_frame_brings_in_what_the_page_pushed_before_it_though_its_cycle_runs_no_stage() {
    // Catches a frame that brings in what was pushed only after a step: with no stage to run, the engine is idle as
    // the frame begins, takes no step, and the move waits for a frame that never brings it.
    let Ok(mut engine) = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>().map(Setup::ready)
    else {
        panic!("the cycle is in order");
    };
    let (source, feed) = engine.source::<Moved>();
    let Ok(_) = engine.add_root::<Pointer>(&(), PointerConfig { source }) else {
        panic!("the pointer begins");
    };
    let mut browser = Browser::new(engine, Still);
    browser.frame();

    let Ok(()) = feed.push(Moved { x: 7 }) else {
        panic!("the pointer holds its source");
    };
    browser.frame();

    let Ok(brought) = MOVED.lock().map(|brought| brought.clone()) else {
        panic!("what was brought is not poisoned");
    };
    assert_eq!(brought, [7]);
}
