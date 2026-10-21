//! A program run through its lifecycle on the headless chassis.

use super::*;
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, ChildOf, Drained, Ending, Fault, Handles, Held, On, RunsJob, Sleep, Slept,
    Start, Stop, Tick, ToSender, Top, Turn,
};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_substrate::{Engine, Jobs, Setup};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

/// An actor's empty config.
#[derive(Plain)]
struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.test.nothing");
}

/// An engine of one part, running the cycle `C` on one worker.
fn headless<C: aethereal_actor::Cycle>(make: impl FnOnce(&mut Engine)) -> Headless {
    let Some(one) = NonZeroUsize::new(1) else {
        panic!("one is not zero");
    };
    let Ok(mut engine) = Engine::new(one, 1024, 0, 1 << 20, 0).with_cycle::<C>().map(Setup::ready) else {
        panic!("the cycle is in order");
    };
    make(&mut engine);

    Headless::new(engine, one)
}

/// The time `run` took, and how it ended.
fn timed(headless: Headless, every: Duration, stop_within: Duration) -> (Duration, Exit) {
    #[allow(clippy::disallowed_methods)] // The test times the host's own loop.
    let began = Instant::now();
    let exit = headless.run(every, stop_within);

    (began.elapsed(), exit)
}

/// An actor that asks the program to stop on `Start`.
#[derive(Bound)]
struct Quitter;

impl Kind for Quitter {
    const NAME: KindName = KindName::new("aether.test.quitter");
}

impl ChildOf for Quitter {
    type Parent = Top;
}

impl Actor for Quitter {
    type Config = Nothing;
    type Key = ();
    type Handles = (On<Start>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Quitter {
    type Sends = ();

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.quit();

        Ok(())
    }
}

#[test]
fn a_program_that_asks_to_stop_as_it_starts_stops_without_waiting_for_a_cycle() {
    // Catches a host that sleeps while an actor has something to run, or that never moves a lifecycle with nothing
    // to run on: the program would wait for its first cycle, or never stop.
    let headless = headless::<()>(|engine| {
        let Ok(_) = engine.add_root::<Quitter>(&(), Nothing) else {
            panic!("the quitter begins");
        };
    });
    let (took, exit) = timed(headless, Duration::from_secs(30), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

/// The deltas of the ticks a metronome was sent.
static TICKS: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// An actor that notes each `Tick`'s delta, and asks the program to stop on the third.
#[derive(Bound)]
struct Metronome;

impl Kind for Metronome {
    const NAME: KindName = KindName::new("aether.test.metronome");
}

impl ChildOf for Metronome {
    type Parent = Top;
}

impl Actor for Metronome {
    type Config = Nothing;
    type Key = ();
    type Handles = (On<Tick>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Tick>> for Metronome {
    type Sends = ();

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Ok(mut ticks) = TICKS.lock() else {
            return Ok(());
        };
        ticks.push(tick.delta_nanos);
        let is_third = ticks.len() == 3;
        if is_third {
            turn.quit();
        }

        Ok(())
    }
}

#[test]
fn a_program_cycles_on_its_timer_until_an_actor_asks_it_to_stop() {
    // Catches a host that never says a cycle is due, says it only once, or with no time between: the metronome would
    // never be sent three ticks, or their deltas after the first would be nothing.
    let headless = headless::<(Tick,)>(|engine| {
        let Ok(_) = engine.add_root::<Metronome>(&(), Nothing) else {
            panic!("the metronome begins");
        };
    });
    let (_, exit) = timed(headless, Duration::from_millis(2), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    let Ok(ticks) = TICKS.lock() else {
        panic!("the ticks are not poisoned");
    };
    assert_eq!(ticks.len(), 3, "no cycle begins once asked to stop");
    let waited = ticks[1..].iter().all(|delta| *delta >= 1_000_000);
    assert!(waited, "each cycle after the first is a timer's period after the last: {ticks:?}");
}

/// An actor that asks the program to stop on `Start`, and holds `Stop` for good.
#[derive(Bound)]
struct Holdout {
    /// The `Stop` it holds: none until it is sent.
    held: Vec<Held<Drained>>,
}

impl Kind for Holdout {
    const NAME: KindName = KindName::new("aether.test.holdout");
}

impl ChildOf for Holdout {
    type Parent = Top;
}

impl Actor for Holdout {
    type Config = Nothing;
    type Key = ();
    type Handles = (On<Start>, On<Stop>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Stop>();

        Ok(Self { held: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Holdout {
    type Sends = ();

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.quit();

        Ok(())
    }
}

impl Handles<On<Stop>> for Holdout {
    type Sends = (ToSender<Drained>,);

    fn handle(&mut self, _: Stop, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.held.push(turn.hold()?);

        Ok(())
    }
}

#[test]
fn a_stop_held_past_the_time_the_host_gives_it_ends_the_program_out_of_time() {
    // Catches a host that waits for good on a subscriber that never answers `Stop`, and one that gives up on it
    // before the time it gives has passed.
    let headless = headless::<()>(|engine| {
        let Ok(_) = engine.add_root::<Holdout>(&(), Nothing) else {
            panic!("the holdout begins");
        };
    });
    let (took, exit) = timed(headless, Duration::from_secs(30), Duration::from_millis(50));

    assert_eq!(exit, Exit::OutOfTime);
    assert!(took >= Duration::from_millis(50), "took {took:?}");
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

/// What a napper said became of its nap.
static NAPPED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

/// An actor that sleeps for twenty milliseconds on `Start`, and asks the program to stop once it has slept; or, if
/// fifty ticks go by first, gives up and asks it to stop.
#[derive(Bound)]
struct Napper {
    ticks: u64,
}

impl Kind for Napper {
    const NAME: KindName = KindName::new("aether.test.napper");
}

impl ChildOf for Napper {
    type Parent = Top;
}

impl Actor for Napper {
    type Config = Nothing;
    type Key = ();
    type Handles = (On<Start>, On<Tick>, Answer<Slept, Nothing>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Tick>();

        Ok(Self { ticks: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Napper {
    type Sends = (RunsJob<Sleep, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.run(&Sleep { until: turn.now() + Duration::from_millis(20) }, &Nothing)?;

        Ok(())
    }
}

impl Handles<On<Tick>> for Napper {
    type Sends = ();

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.ticks += 1;
        let is_long = self.ticks == 50;
        if is_long {
            NAPPED.lock().map(|mut napped| napped.push("gave up")).ok();
            turn.quit();
        }

        Ok(())
    }
}

impl Handles<Answer<Slept, Nothing>> for Napper {
    type Sends = ();

    fn handle(&mut self, (done, _): (Answered<Slept>, Nothing), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let said = match done {
            Answered::Yes(Slept) => "slept",
            Answered::No(_) => "not answered",
        };
        NAPPED.lock().map(|mut napped| napped.push(said)).ok();
        turn.quit();

        Ok(())
    }
}

#[test]
fn a_program_is_answered_for_its_jobs_while_it_runs() {
    // Catches a host that never brings in what its jobs finished as it runs: the napper is never answered, and gives
    // up once fifty ticks of five milliseconds have gone by.
    let one = NonZeroUsize::MIN;
    let runners: Arc<dyn Jobs> = Arc::new(crate::Runners::new(Vec::new(), one));
    let Ok(engine) = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(runners).with_cycle::<(Tick,)>().map(Setup::ready)
    else {
        panic!("the cycle is in order");
    };
    let mut headless = Headless::new(engine, one);
    let Ok(_) = headless.engine_mut().add_root::<Napper>(&(), Nothing) else {
        panic!("the napper begins");
    };
    let (took, exit) = timed(headless, Duration::from_millis(5), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    let Ok(napped) = NAPPED.lock() else {
        panic!("what was said is not poisoned");
    };
    assert_eq!(*napped, ["slept"]);
    assert!(took >= Duration::from_millis(20), "it slept for its time: {took:?}");
}
