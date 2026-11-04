//! A program run through its lifecycle on the headless chassis.

use super::*;
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, ChildOf, Controller, Drained, Ending, Fault, Given, Handle, Handles, Held, On,
    Services, Sleep, Slept, Start, Stop, Tick, ToSender, Top, Turn,
};
use aethereal_actor::{Asks, Path, path};
use aethereal_data::{Bound, Plain};
use aethereal_pace::{Advance, Advanced, Pace, Pacer, PacerConfig};
use aethereal_substrate::{Engine, Jobs};
use std::num::NonZeroUsize;
use std::sync::{Arc, mpsc};
use std::time::Instant;

/// An actor's empty config.
#[derive(Plain)]
#[kind(name = "aether.test.nothing")]
struct Nothing;

/// An engine of one part, running the cycle `C` on one worker, and what `make` answered as it set the engine up.
fn headless<C: aethereal_actor::Cycle, T>(make: impl FnOnce(&mut Engine) -> T) -> (Headless, T) {
    let Some(one) = NonZeroUsize::new(1) else {
        panic!("one is not zero");
    };
    let mut engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_cycle::<C>().ready();
    let answered = make(&mut engine);

    (Headless::new(engine, one), answered)
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
#[kind(name = "aether.test.quitter")]
struct Quitter;

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
    let (headless, ()) = headless::<(), _>(|engine| {
        let Ok(_) = engine.add_root::<Quitter>(&(), Nothing) else {
            panic!("the quitter begins");
        };
    });
    let (took, exit) = timed(headless, Duration::from_secs(30), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

/// What a metronome is made from: what it tells the test through. The run drops the engine and every actor's state
/// with it, so what the test reads afterward lies outside: the end of a controller it keeps, which is no mail, and so
/// no part of the turns the run measures.
#[derive(Bound)]
#[kind(name = "aether.test.metronome_config")]
struct MetronomeConfig {
    deltas: Given<Controller<u64>>,
}

/// An actor that tells the test each `Tick`'s delta, and asks the program to stop on the third.
#[derive(Bound)]
#[kind(name = "aether.test.metronome")]
struct Metronome {
    deltas: Handle<Controller<u64>>,
    ticks: u64,
}

impl ChildOf for Metronome {
    type Parent = Top;
}

impl Actor for Metronome {
    type Config = MetronomeConfig;
    type Key = ();
    type Handles = (On<Tick>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: MetronomeConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let deltas = turn.take(config.deltas)?;
        turn.subscribe::<Tick>();

        Ok(Self { deltas, ticks: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Tick>> for Metronome {
    type Sends = ();

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.open(&mut self.deltas)?.push(tick.delta_nanos);
        self.ticks += 1;
        let is_third = self.ticks == 3;
        if is_third {
            turn.quit();
        }

        Ok(())
    }
}

#[test]
fn a_program_cycles_on_its_timer_until_an_actor_asks_it_to_stop() {
    // Catches a host that never says a cycle is due, or says it only once: the metronome would never be sent three
    // ticks. When each is due is the timer's, tested on stated times; on the real clock only a lower bound holds, as
    // the third cycle is due two periods after the start, however a loaded machine spaces them.
    let (headless, deltas) = headless::<(Tick,), _>(|engine| {
        let (given, deltas) = engine.controller();
        let Ok(_) = engine.add_root::<Metronome>(&(), MetronomeConfig { deltas: given }) else {
            panic!("the metronome begins");
        };

        deltas
    });
    let (took, exit) = timed(headless, Duration::from_millis(2), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    assert_eq!(deltas.take().len(), 3, "no cycle begins once asked to stop");
    assert!(took >= Duration::from_millis(4), "the third cycle is two periods after the start: {took:?}");
}

/// An actor that asks the program to stop on `Start`, and holds `Stop` for good.
#[derive(Bound)]
#[kind(name = "aether.test.holdout")]
struct Holdout {
    /// The `Stop` it holds: none until it is sent.
    held: Vec<Held<Drained>>,
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
    let (headless, ()) = headless::<(), _>(|engine| {
        let Ok(_) = engine.add_root::<Holdout>(&(), Nothing) else {
            panic!("the holdout begins");
        };
    });
    let (took, exit) = timed(headless, Duration::from_secs(30), Duration::from_millis(50));

    assert_eq!(exit, Exit::OutOfTime);
    assert!(took >= Duration::from_millis(50), "took {took:?}");
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

/// What became of a napper's nap, as it tells the test.
#[derive(Debug, PartialEq, Eq)]
enum Nap {
    /// It slept.
    Slept,
    /// The sleep was not answered.
    NotAnswered,
    /// Fifty ticks went by first.
    GaveUp,
}

/// What a napper is made from: what it tells the test through, as a metronome does.
#[derive(Bound)]
#[kind(name = "aether.test.napper_config")]
struct NapperConfig {
    naps: Given<Controller<Nap>>,
}

/// An actor that sleeps for twenty milliseconds on `Start`, and asks the program to stop once it has slept; or, if
/// fifty ticks go by first, gives up and asks it to stop.
#[derive(Bound)]
#[kind(name = "aether.test.napper")]
struct Napper {
    naps: Handle<Controller<Nap>>,
    ticks: u64,
}

impl ChildOf for Napper {
    type Parent = Top;
}

impl Actor for Napper {
    type Config = NapperConfig;
    type Key = ();
    type Handles = (On<Start>, On<Tick>, Answer<Slept, Nothing>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: NapperConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let naps = turn.take(config.naps)?;
        turn.subscribe::<Start>();
        turn.subscribe::<Tick>();

        Ok(Self { naps, ticks: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Napper {
    type Sends = ();

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
            turn.open(&mut self.naps)?.push(Nap::GaveUp);
            turn.quit();
        }

        Ok(())
    }
}

impl Handles<Answer<Slept, Nothing>> for Napper {
    type Sends = ();

    fn handle(&mut self, (done, _): (Answered<Slept>, Nothing), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let nap = match done {
            Answered::Yes(Slept) => Nap::Slept,
            Answered::No(_) => Nap::NotAnswered,
        };
        turn.open(&mut self.naps)?.push(nap);
        turn.quit();

        Ok(())
    }
}

#[test]
fn a_program_is_answered_for_its_jobs_while_it_runs() {
    // Catches a host that never brings in what its jobs finished as it runs: the napper is never answered, and gives
    // up once fifty ticks of five milliseconds have gone by.
    let one = NonZeroUsize::MIN;
    let runners: Arc<dyn Jobs> = Arc::new(crate::Runners::new(one, Arc::new(Services::none())));
    let engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(runners).with_cycle::<(Tick,)>().ready();
    let mut headless = Headless::new(engine, one);
    let (given, naps) = headless.engine_mut().controller();
    let Ok(_) = headless.engine_mut().add_root::<Napper>(&(), NapperConfig { naps: given }) else {
        panic!("the napper begins");
    };
    let (took, exit) = timed(headless, Duration::from_millis(5), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    assert_eq!(naps.take(), [Nap::Slept]);
    assert!(took >= Duration::from_millis(20), "it slept for its time: {took:?}");
}

/// What a dozer is made from: what it tells the test through, as a napper does.
#[derive(Bound)]
#[kind(name = "aether.test.dozer_config")]
struct DozerConfig {
    naps: Given<Controller<Nap>>,
}

/// An actor that sleeps for twenty milliseconds on `Start`, and asks the program to stop once it has slept: it needs
/// no cycle.
#[derive(Bound)]
#[kind(name = "aether.test.dozer")]
struct Dozer {
    naps: Handle<Controller<Nap>>,
}

impl ChildOf for Dozer {
    type Parent = Top;
}

impl Actor for Dozer {
    type Config = DozerConfig;
    type Key = ();
    type Handles = (On<Start>, Answer<Slept, Nothing>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: DozerConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let naps = turn.take(config.naps)?;
        turn.subscribe::<Start>();

        Ok(Self { naps })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Dozer {
    type Sends = ();

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.run(&Sleep { until: turn.now() + Duration::from_millis(20) }, &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Slept, Nothing>> for Dozer {
    type Sends = ();

    fn handle(&mut self, (done, _): (Answered<Slept>, Nothing), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let nap = match done {
            Answered::Yes(Slept) => Nap::Slept,
            Answered::No(_) => Nap::NotAnswered,
        };
        turn.open(&mut self.naps)?.push(nap);
        turn.quit();

        Ok(())
    }
}

#[test]
fn a_program_with_no_cycle_is_woken_by_a_job_that_finishes_while_it_sleeps() {
    // Catches a host that sleeps through a finished job until its timer: with cycles thirty seconds apart, the dozer's
    // twenty-millisecond sleep would be answered only after thirty seconds.
    let one = NonZeroUsize::MIN;
    let runners: Arc<dyn Jobs> = Arc::new(crate::Runners::new(one, Arc::new(Services::none())));
    let engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(runners).ready();
    let mut headless = Headless::new(engine, one);
    let (given, naps) = headless.engine_mut().controller();
    let Ok(_) = headless.engine_mut().add_root::<Dozer>(&(), DozerConfig { naps: given }) else {
        panic!("the dozer begins");
    };
    // A host that is never woken never returns, so it runs on a thread of its own, and the test gives up on it.
    let (ran, run) = mpsc::channel();
    #[allow(clippy::disallowed_methods)] // The test runs the host's own loop on a real thread, to bound it.
    thread::spawn(move || ran.send(timed(headless, Duration::from_secs(30), Duration::from_secs(30))));
    let Ok((took, exit)) = run.recv_timeout(Duration::from_secs(10)) else {
        panic!("the program was not woken when the sleep finished");
    };

    assert_eq!(exit, Exit::Stopped);
    assert_eq!(naps.take(), [Nap::Slept]);
    assert!(took >= Duration::from_millis(20), "it slept for its time: {took:?}");
}

/// What a silent actor is asked, and what it would answer: nothing more.
#[derive(Plain)]
#[kind(name = "aether.test.hush")]
struct Hush;

/// The protocol a silent actor is asked through: a hush, answered with a hush.
struct Hushes;

impl aethereal_actor::Protocol for Hushes {
    type Kinds = (Hush,);
}

impl aethereal_actor::Carries<Hush> for Hushes {
    type Answers = (Hush,);
}

/// An actor that holds every hush it is asked, and never answers one.
#[derive(Bound)]
#[kind(name = "aether.test.silent")]
struct Silent {
    held: Vec<Held<Hush>>,
}

impl ChildOf for Silent {
    type Parent = Waiter;
}

impl Actor for Silent {
    type Config = Nothing;
    type Key = ();
    type Handles = (Hush,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { held: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Hush> for Silent {
    type Sends = (ToSender<Hush>,);

    fn handle(&mut self, _: Hush, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.held.push(turn.hold()?);

        Ok(())
    }
}

/// What became of a waiter's request, as it tells the test: why it went unanswered, or that it was answered.
#[derive(Debug, PartialEq, Eq)]
enum Waited {
    Answered,
    No(aethereal_actor::Unanswered),
}

/// What a waiter is made from: what it tells the test through.
#[derive(Bound)]
#[kind(name = "aether.test.waiter_config")]
struct WaiterConfig {
    waited: Given<Controller<Waited>>,
}

/// An actor that asks a silent actor it stands up a hush on `Start`, within twenty milliseconds, and asks the program
/// to stop once it hears what became of it: it needs no cycle.
#[derive(Bound)]
#[kind(name = "aether.test.waiter")]
struct Waiter {
    waited: Handle<Controller<Waited>>,
    silent: aethereal_actor::ActorRef<Silent>,
}

impl ChildOf for Waiter {
    type Parent = Top;
}

impl Actor for Waiter {
    type Config = WaiterConfig;
    type Key = ();
    type Handles = (On<Start>, Answer<Hush, Nothing>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: WaiterConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let waited = turn.take(config.waited)?;
        let Ok(silent) = turn.spawn(&(), Nothing) else {
            return Err(Fault::new(&Nothing)?);
        };
        turn.subscribe::<Start>();

        Ok(Self { waited, silent })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Waiter {
    type Sends = (Asks<Hushes, Hush, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let due = turn.now() + Duration::from_millis(20);
        turn.to(self.silent.protocol()).within(due).ask(&Hush, &Nothing)?;

        Ok(())
    }
}

impl Handles<Answer<Hush, Nothing>> for Waiter {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<Hush>, Nothing), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let waited = match answered {
            Answered::Yes(Hush) => Waited::Answered,
            Answered::No(why) => Waited::No(why),
        };
        turn.open(&mut self.waited)?.push(waited);
        turn.quit();

        Ok(())
    }
}

#[test]
fn a_program_with_no_cycle_is_woken_by_a_deadline_that_comes_while_it_sleeps() {
    // Catches a deadline the host sleeps through: with nothing to run and no cycle, a host the engine does not wake for
    // its earliest deadline sleeps until something else comes in, so the waiter, whose request no one answers, is never
    // told it expired.
    let one = NonZeroUsize::MIN;
    let runners: Arc<dyn Jobs> = Arc::new(crate::Runners::new(one, Arc::new(Services::none())));
    let engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(runners).ready();
    let mut headless = Headless::new(engine, one);
    let (given, waited) = headless.engine_mut().controller();
    let Ok(_) = headless.engine_mut().add_root::<Waiter>(&(), WaiterConfig { waited: given }) else {
        panic!("the waiter begins");
    };
    // A host that is never woken never returns, so it runs on a thread of its own, and the test gives up on it.
    let (ran, run) = mpsc::channel();
    #[allow(clippy::disallowed_methods)] // The test runs the host's own loop on a real thread, to bound it.
    thread::spawn(move || ran.send(timed(headless, Duration::from_secs(30), Duration::from_secs(30))));
    let Ok((took, exit)) = run.recv_timeout(Duration::from_secs(10)) else {
        panic!("the program was not woken when the deadline came");
    };

    assert_eq!(exit, Exit::Stopped);
    assert_eq!(waited.take(), [Waited::No(aethereal_actor::Unanswered::Expired)]);
    assert!(took >= Duration::from_millis(20), "it waited until the deadline: {took:?}");
}

/// What a stepper tells the test: a `Tick`'s delta, or what became of its advance.
#[derive(Debug, PartialEq, Eq)]
enum Stepped {
    Ticked(u64),
    Advanced(Advanced),
}

/// What a stepper is made from: what it tells the test through, as a metronome's.
#[derive(Bound)]
#[kind(name = "aether.test.stepper_config")]
struct StepperConfig {
    told: Given<Controller<Stepped>>,
}

/// An actor that asks the pacer, as it begins, to advance its held clock three cycles of `STEP`, tells the test each
/// `Tick`'s delta and what became of the advance, and asks the program to stop once it is answered.
#[derive(Bound)]
#[kind(name = "aether.test.stepper")]
struct Stepper {
    told: Handle<Controller<Stepped>>,
}

/// Each held cycle's delta: far longer than the host's period, so a delta measured on the host's clock reads apart.
const STEP: Duration = Duration::from_millis(200);

impl ChildOf for Stepper {
    type Parent = Top;
}

impl Actor for Stepper {
    type Config = StepperConfig;
    type Key = ();
    type Handles = (On<Tick>, Answer<Advanced, Nothing>);
    type BeginSends = (Asks<Path<Pacer>, Advance, Nothing>,);
    type EndSends = ();

    fn begin(config: StepperConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let told = turn.take(config.told)?;
        turn.subscribe::<Tick>();
        let delta_nanos = nanos(STEP);
        turn.to(path::<Pacer>(())).ask(&Advance { cycles: 3, delta_nanos }, &Nothing)?;

        Ok(Self { told })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Tick>> for Stepper {
    type Sends = ();

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.open(&mut self.told)?.push(Stepped::Ticked(tick.delta_nanos));

        Ok(())
    }
}

impl Handles<Answer<Advanced, Nothing>> for Stepper {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, _): (Answered<Advanced>, Nothing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        if let Answered::Yes(advanced) = answered {
            turn.open(&mut self.told)?.push(Stepped::Advanced(advanced));
        }
        turn.quit();

        Ok(())
    }
}

#[test]
fn a_held_clock_cycles_only_as_its_pacer_advances_it_each_tick_of_the_delta_asked() {
    // Catches a host that cycles on its timer while the clock is held, one that never says a held cycle is due or
    // never says it finished, so the advance is never answered, and a held `Tick` given the host's time between
    // cycles in place of the delta asked.
    let (headless, (told, clockwork)) = headless::<(Tick,), _>(|engine| {
        let source = engine.source();
        let controller = engine.controller();
        let (config, clockwork) = PacerConfig::paired(source, controller, Pace::Held, Duration::from_millis(250));
        let (given, told) = engine.controller();
        let begun = engine
            .add_root::<Pacer>(&(), config)
            .and_then(|_| engine.add_root::<Stepper>(&(), StepperConfig { told: given }));
        let Ok(_) = begun else {
            panic!("the pacer and the stepper begin");
        };

        (told, clockwork)
    });
    let (_, exit) = timed(headless.with_clockwork(clockwork), Duration::from_millis(1), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    let step = nanos(STEP);
    let advanced = Stepped::Advanced(Advanced::Advanced { elapsed_nanos: 3 * step });
    assert_eq!(told.take(), [Stepped::Ticked(step), Stepped::Ticked(step), Stepped::Ticked(step), advanced]);
}
