//! The chassis's tests: a frame runs its cycle through to its end, brings in what the page pushed before it, and answers
//! each window asked to open, to close, or to be shown in a mode `NoHost`.

use super::*;
use aethereal_actor::{
    Actor, Actors, Answer, Answered, Beginning, ChildOf, Ending, Fault, Given, Handle, Handles, On, Render, Seated,
    Services, Sleep, Slept, Source, Tick, Top, Turn, Unanswered,
};
use aethereal_data::{Bound, Encode, Kind, KindId, Plain};
use aethereal_substrate::harness::{Turned, Unheard};
use aethereal_substrate::{Inline, Jobs};
use aethereal_window::{
    CloseWindow, Closed, Heard, Logical, Mode, ModeSet, OpenWindow, Opened, Opening, SetMode, Surface, Unclosed,
    Unopened, Unset, Windowing, Windows,
};
use std::num::NonZeroUsize;
use std::sync::Arc;

/// A clock that reads the same time always: the tests' frames take no time.
struct Still;

impl Clock for Still {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An actor's empty config.
#[derive(Plain)]
#[kind(name = "aether.test.nothing")]
struct Nothing;

/// What a turn of the painter was handed, as a test reads it from the engine's record (ADR-0036).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Painted {
    Tick,
    Render,
    Slept,
    /// Its sleep went unanswered, for this reason.
    Unanswered(Unanswered),
}

impl Painted {
    /// What `turned` was handed: nothing for mail no test reads.
    fn of(turned: &Turned) -> Option<Self> {
        if turned.read::<Tick>().is_ok() {
            return Some(Self::Tick);
        }
        if turned.read::<Render>().is_ok() {
            return Some(Self::Render);
        }
        if turned.read::<Slept>().is_ok() {
            return Some(Self::Slept);
        }

        turned.read().ok().map(Self::Unanswered)
    }
}

/// What the turns `browser` recorded since this was last asked were handed, in order.
fn painted<C>(browser: &mut Browser<C>) -> Vec<Painted> {
    browser.engine.take_turned().iter().filter_map(Painted::of).collect()
}

/// An actor that sleeps until now on each `Tick`, and is sent each `Render`.
#[derive(Bound)]
#[kind(name = "aether.test.painter")]
struct Painter;

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
    type Sends = ();

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.run(&Sleep { until: turn.now() }, &Nothing)?;

        Ok(())
    }
}

impl Handles<On<Render>> for Painter {
    type Sends = ();

    fn handle(&mut self, _: Render, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Slept, Nothing>> for Painter {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Slept>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

#[test]
fn a_frame_runs_its_cycle_through_to_its_end_and_is_answered_for_the_jobs_it_ran() {
    // Catches a frame that never says a cycle is due, steps once rather than until nothing is left to run, or brings
    // in what its jobs finished only before its first step: a frame would end before its `Render`, or before the
    // sleep its `Tick` ran was answered, and the next would begin with what the last left.
    let one = NonZeroUsize::MIN;
    let jobs: Arc<dyn Jobs> = Arc::new(Inline::new(Arc::new(Services::none())));
    let mut engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(jobs).with_cycle::<(Tick, Render)>().ready();
    let Ok(_) = engine.add_root::<Painter>(&(), Nothing) else {
        panic!("the painter begins");
    };
    engine.record_turns();
    let mut browser = Browser::new(engine, Still, Serving::Nothing, Hosted::Nothing);

    browser.frame(16.0);
    let first = painted(&mut browser);
    browser.frame(32.0);
    let second = painted(&mut browser);

    // A stage does not wait for the jobs it ran (ADR-0020), so the answer comes after the render, in the same frame.
    let one_frame = [Painted::Tick, Painted::Render, Painted::Slept];
    assert_eq!(first, one_frame);
    assert_eq!(second, one_frame);
}

/// The pointer moved: what the page pushes onto a pointer's source.
#[derive(Plain)]
#[kind(name = "aether.test.moved")]
struct Moved {
    x: u64,
}

/// The source a pointer is handed.
#[derive(Bound)]
#[kind(name = "aether.test.pointer_config")]
struct PointerConfig {
    source: Given<Source<Moved>>,
}

/// An actor that holds a source of moves, and is brought each.
#[derive(Bound)]
#[kind(name = "aether.test.pointer")]
struct Pointer {
    source: Handle<Source<Moved>>,
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

    fn handle(&mut self, _: Moved, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

#[test]
fn a_frame_brings_in_what_the_page_pushed_before_it_though_its_cycle_runs_no_stage() {
    // Catches a frame that brings in what was pushed only after a step: with no stage to run, the engine is idle as
    // the frame begins, takes no step, and the move waits for a frame that never brings it.
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>().ready();
    let (source, feed) = engine.source::<Moved>();
    let Ok(_) = engine.add_root::<Pointer>(&(), PointerConfig { source }) else {
        panic!("the pointer begins");
    };
    engine.record_turns();
    let mut browser = Browser::new(engine, Still, Serving::Nothing, Hosted::Nothing);
    browser.frame(48.0);

    let Ok(()) = feed.push(Moved { x: 7 }) else {
        panic!("the pointer holds its source");
    };
    browser.frame(64.0);

    let turned = browser.engine.take_turned();
    let brought: Vec<u64> = turned.iter().filter_map(|turn| turn.read::<Moved>().ok()).map(|moved| moved.x).collect();
    assert_eq!(brought, [7]);
}

#[test]
fn an_animation_frame_s_timestamp_is_its_time_in_nanoseconds() {
    // Catches the frame's timestamp, in milliseconds, read in the wrong unit, which would run every actor's time
    // a thousand times too fast or too slow, and one from before the page opened read as a time far in the future.
    assert_eq!(frame_time(16.5), Time::from_nanos(16_500_000));
    assert_eq!(frame_time(-1.0), Time::ZERO);
}

/// Asks `mail` of `windows` from the outside `by`, to be answered with an `A`.
fn ask<K: Kind + Encode<Actors>, A: Kind>(browser: &mut Browser<Still>, by: Seated, windows: Seated, mail: &K) {
    let Ok(_) = browser.engine.ask_outside(by, windows, mail, KindId::of::<A>()) else {
        panic!("what the test asks encodes");
    };
}

#[test]
fn a_window_asked_to_open_close_or_show_in_a_mode_is_answered_no_host_within_the_frame() -> Result<(), Unheard> {
    // Catches a page that never takes what the window root asks, so an actor that asks for a window waits on an
    // answer that never comes and a program written for both hosts cannot fall back; and a canvas asked to close, or
    // to be shown fullscreen, answered as done, or refused as not open, though the page still shows it as it was.
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>();
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let canvas = opening.open(Surface { width: 640, height: 480, scale: 1.0 }, Mode::Windowed, true);
    let (windows, hosting) = opening.config();
    let mut engine = setup.ready();
    let Ok(id) = engine.add_root::<Windows>(&(), windows) else {
        panic!("the window root begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the window root is seated");
    };
    let windows = Seated { id, seat };
    let (outside, _arrivals) = engine.seat_outside()?;
    let mut browser = Browser::new(engine, Still, Serving::Nothing, Hosted::Windows(hosting));

    let open = OpenWindow { title: String::from("another"), size: Logical { width: 640.0, height: 480.0 } };
    ask::<_, Opened>(&mut browser, outside, windows, &open);
    let fullscreen = SetMode { window: canvas.window(), mode: Mode::Fullscreen };
    ask::<_, ModeSet>(&mut browser, outside, windows, &fullscreen);
    ask::<_, Closed>(&mut browser, outside, windows, &CloseWindow { window: canvas.window() });
    browser.frame(16.0);

    let turned = browser.engine.take_turned();
    let answers: Vec<&Turned> = turned.iter().filter(|turn| turn.actor == outside).collect();
    let [opened, moded, closed] = answers[..] else {
        panic!("three questions are answered once each: {}", answers.len());
    };
    assert_eq!(opened.read::<Opened>()?, Opened::Refused(Unopened::NoHost));
    assert_eq!(moded.read::<ModeSet>()?, ModeSet::Refused(Unset::NoHost));
    assert_eq!(closed.read::<Closed>()?, Closed::Refused(Unclosed::NoHost));

    Ok(())
}
