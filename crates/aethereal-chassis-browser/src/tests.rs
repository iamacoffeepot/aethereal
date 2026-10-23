//! The chassis's tests: a frame runs its cycle through to its end, brings in what the page pushed before it, and answers
//! each window asked to open, to close, or to be shown in a mode `NoHost`.

use super::*;
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Given, Handle, Handles, On, Path, Render, Sleep,
    Slept, Source, Tick, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::{Inline, Jobs, Setup};
use aethereal_window::{
    CloseWindow, Closed, Heard, Logical, Mode, ModeSet, OpenWindow, Opened, Opening, SetMode, Surface, Unclosed,
    Unopened, Unset, WindowId, Windowing, Windows,
};
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
#[kind(name = "aether.test.nothing")]
struct Nothing;

/// What the painter was sent, in order.
static PAINTED: Mutex<Vec<&str>> = Mutex::new(Vec::new());

/// Notes what the painter was sent.
fn note(what: &'static str) {
    PAINTED.lock().map(|mut painted| painted.push(what)).ok();
}

/// An actor that notes each `Tick` and `Render` it is sent, and sleeps until now on each `Tick`, noting the answer.
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
    let mut engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(jobs).with_cycle::<(Tick, Render)>().ready();
    let Ok(_) = engine.add_root::<Painter>(&(), Nothing) else {
        panic!("the painter begins");
    };
    let mut browser = Browser::new(engine, Still, Serving::Nothing, Hosted::Nothing);

    browser.frame(16.0);
    let Ok(first) = PAINTED.lock().map(|painted| painted.clone()) else {
        panic!("what was painted is not poisoned");
    };
    browser.frame(32.0);
    let Ok(both) = PAINTED.lock().map(|painted| painted.clone()) else {
        panic!("what was painted is not poisoned");
    };

    // A stage does not wait for the jobs it ran (ADR-0020), so the answer comes after the render, in the same frame.
    assert_eq!(first, ["tick", "render", "slept"]);
    assert_eq!(both, ["tick", "render", "slept", "tick", "render", "slept"]);
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

/// Where the pointer was brought, in order.
static MOVED: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// An actor that holds a source of moves, and notes each it is brought.
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

    fn handle(&mut self, moved: Moved, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        MOVED.lock().map(|mut brought| brought.push(moved.x)).ok();

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
    let mut browser = Browser::new(engine, Still, Serving::Nothing, Hosted::Nothing);
    browser.frame(48.0);

    let Ok(()) = feed.push(Moved { x: 7 }) else {
        panic!("the pointer holds its source");
    };
    browser.frame(64.0);

    let Ok(brought) = MOVED.lock().map(|brought| brought.clone()) else {
        panic!("what was brought is not poisoned");
    };
    assert_eq!(brought, [7]);
}

#[test]
fn an_animation_frame_s_timestamp_is_its_time_in_nanoseconds() {
    // Catches the frame's timestamp, in milliseconds, read in the wrong unit, which would run every actor's time
    // a thousand times too fast or too slow, and one from before the page opened read as a time far in the future.
    assert_eq!(frame_time(16.5), Time::from_nanos(16_500_000));
    assert_eq!(frame_time(-1.0), Time::ZERO);
}

/// What the asker was answered, in order.
static ANSWERED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The window the asker asks to close.
#[derive(Plain)]
#[kind(name = "aether.test.closes")]
struct Closes {
    window: WindowId,
}

/// An actor that asks the window root for a window, to show the one it is given fullscreen, and to close it, as it
/// begins, and notes each answer.
#[derive(Bound)]
#[kind(name = "aether.test.asker")]
struct Asker;

impl ChildOf for Asker {
    type Parent = Top;
}

impl Actor for Asker {
    type Config = Closes;
    type Key = ();
    type Handles = (Answer<Opened, Nothing>, Answer<ModeSet, Nothing>, Answer<Closed, Nothing>);
    type BeginSends = (
        Asks<Path<Windows>, OpenWindow, Nothing>,
        Asks<Path<Windows>, SetMode, Nothing>,
        Asks<Path<Windows>, CloseWindow, Nothing>,
    );
    type EndSends = ();

    fn begin(closes: Closes, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let open = OpenWindow { title: String::from("another"), size: Logical { width: 640.0, height: 480.0 } };
        let fullscreen = SetMode { window: closes.window, mode: Mode::Fullscreen };
        turn.to(path::<Windows>(()))
            .ask(&open, &Nothing)?
            .ask(&fullscreen, &Nothing)?
            .ask(&CloseWindow { window: closes.window }, &Nothing)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Opened, Nothing>> for Asker {
    type Sends = ();

    fn handle(&mut self, (opened, _): (Answered<Opened>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        ANSWERED.lock().map(|mut answered| answered.push(format!("{opened:?}"))).ok();

        Ok(())
    }
}

impl Handles<Answer<ModeSet, Nothing>> for Asker {
    type Sends = ();

    fn handle(&mut self, (set, _): (Answered<ModeSet>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        ANSWERED.lock().map(|mut answered| answered.push(format!("{set:?}"))).ok();

        Ok(())
    }
}

impl Handles<Answer<Closed, Nothing>> for Asker {
    type Sends = ();

    fn handle(&mut self, (closed, _): (Answered<Closed>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        ANSWERED.lock().map(|mut answered| answered.push(format!("{closed:?}"))).ok();

        Ok(())
    }
}

#[test]
fn a_window_asked_to_open_close_or_show_in_a_mode_is_answered_no_host_within_the_frame() {
    // Catches a page that never takes what the window root asks, so an actor that asks for a window waits on an
    // answer that never comes and a program written for both hosts cannot fall back; and a canvas asked to close, or
    // to be shown fullscreen, answered as done, or refused as not open, though the page still shows it as it was.
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>();
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let canvas = opening.open(Surface { width: 640, height: 480, scale: 1.0 }, Mode::Windowed, true);
    let (windows, hosting) = opening.config();
    let Ok(engine) = setup
        .add_root::<Windows>(&(), windows)
        .and_then(|setup| setup.add_root::<Asker>(&(), Closes { window: canvas.window() }))
        .map(Setup::ready)
    else {
        panic!("the window root and the asker begin");
    };
    let mut browser = Browser::new(engine, Still, Serving::Nothing, Hosted::Windows(hosting));
    browser.frame(16.0);

    let Ok(answered) = ANSWERED.lock().map(|answered| answered.clone()) else {
        panic!("what was answered is not poisoned");
    };
    let expected = [
        format!("{:?}", Answered::Yes(Opened::Refused(Unopened::NoHost))),
        format!("{:?}", Answered::Yes(ModeSet::Refused(Unset::NoHost))),
        format!("{:?}", Answered::Yes(Closed::Refused(Unclosed::NoHost))),
    ];
    assert_eq!(answered, expected);
}
