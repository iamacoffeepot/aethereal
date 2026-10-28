//! Jobs run on the native host's threads for computing.

use super::*;
use crate::Monotonic;
use crate::waiting::{self, BUDGET};
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, ChildOf, Ending, Fault, Handles, Job, Runner, Running, Services, Sleep, Slept,
    Time, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::{Clock, Engine};
use std::cell::RefCell;
use std::future;
use std::mem;
use std::time::Duration;

thread_local! {
    /// What this thread's actors have said they were answered with, in order.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// A job that says whether it ran on one of the host's threads for jobs.
#[derive(Plain)]
#[kind(name = "aether.test.where")]
struct Where {
    at: u64,
}

/// What a job that says where it ran answers with.
#[derive(Plain)]
#[kind(name = "aether.test.ran")]
struct Ran {
    at: u64,
    on_a_job_thread: bool,
}

impl Job for Where {
    type Done = Ran;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Ran, Fault> {
        let on_a_job_thread = thread::current().name().is_some_and(|name| name.starts_with("aethereal-job-"));

        Ok(Ran { at: self.at, on_a_job_thread })
    }
}

/// What an asker keeps: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.nothing")]
struct Nothing;

/// An actor that runs three jobs as it begins, and says what each was answered with.
#[derive(Bound)]
#[kind(name = "aether.test.asker")]
struct Asker;

impl ChildOf for Asker {
    type Parent = Top;
}

impl Actor for Asker {
    type Config = Nothing;
    type Key = ();
    type Handles = (Answer<Ran, Nothing>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for at in 0..3 {
            turn.run(&Where { at }, &Nothing)?;
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Ran, Nothing>> for Asker {
    type Sends = ();

    fn handle(&mut self, (done, _): (Answered<Ran>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let line = match done {
            Answered::Yes(Ran { at, on_a_job_thread }) => format!("{at} on a job thread: {on_a_job_thread}"),
            Answered::No(why) => format!("{why:?}"),
        };
        SAID.with_borrow_mut(|said| said.push(line));

        Ok(())
    }
}

#[test]
fn each_job_runs_on_a_thread_of_its_own_and_those_past_its_threads_wait_their_turn() {
    // Catches a job run on the engine's thread, one never answered, and one past what the threads let be in flight
    // left waiting for good. One thread runs three jobs, so two wait in the engine. Dropping the runners stops their
    // thread, or this never returns.
    let one = NonZeroUsize::MIN;
    let runners = Arc::new(Runners::new(one, Arc::new(Services::none())));
    let mut engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(Arc::clone(&runners) as Arc<dyn Jobs>).ready();
    let Ok(_) = engine.add_root::<Asker>(&(), Nothing) else {
        panic!("the asker begins");
    };
    let clock = Monotonic::new();
    let mut said = Vec::new();

    waiting::until(BUDGET, || {
        engine.bring();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }
        said.extend(SAID.with_borrow_mut(mem::take));
        said.len() == 3
    });
    drop(engine);
    drop(runners);

    said.sort();
    assert_eq!(said, ["0 on a job thread: true", "1 on a job thread: true", "2 on a job thread: true"]);
}

/// Whether the job that waits for good has been dropped.
static DROPPED: AtomicBool = AtomicBool::new(false);

/// What a job that waits for good holds: it notes when it is dropped.
struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        DROPPED.store(true, Ordering::Release);
    }
}

/// A job that waits for good, until it is dropped.
#[derive(Plain)]
#[kind(name = "aether.test.forever")]
struct Forever;

impl Job for Forever {
    type Done = Ran;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, _: &mut Running) -> Result<Ran, Fault> {
        let _guard = Guard;
        future::pending::<()>().await;

        Ok(Ran { at: 0, on_a_job_thread: true })
    }
}

/// What a sleeper keeps of each sleep: which it is, the time it sleeps until, and the job it gives up on.
#[derive(Bound)]
#[kind(name = "aether.test.woke")]
struct Woke {
    order: u64,
    until: Time,
    on: JobId,
}

/// An actor that runs a job that waits for good and two sleeps as it begins, the later run first, and gives up on the
/// job once the later sleep wakes.
#[derive(Bound)]
#[kind(name = "aether.test.sleeper")]
struct Sleeper;

impl ChildOf for Sleeper {
    type Parent = Top;
}

impl Actor for Sleeper {
    type Config = Nothing;
    type Key = ();
    type Handles = (Answer<Ran, Nothing>, Answer<Slept, Woke>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let on = turn.run(&Forever, &Nothing)?;
        for (order, millis) in [(2, 30), (1, 10)] {
            let until = turn.now() + Duration::from_millis(millis);
            turn.run(&Sleep { until }, &Woke { order, until, on })?;
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Ran, Nothing>> for Sleeper {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Ran>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        SAID.with_borrow_mut(|said| said.push(String::from("the job that waits for good finished")));

        Ok(())
    }
}

impl Handles<Answer<Slept, Woke>> for Sleeper {
    type Sends = ();

    fn handle(&mut self, (done, woke): (Answered<Slept>, Woke), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let line = match done {
            Answered::Yes(Slept) => format!("woke {} after its time: {}", woke.order, turn.now() >= woke.until),
            Answered::No(why) => format!("{why:?}"),
        };
        SAID.with_borrow_mut(|said| said.push(line));
        if woke.order == 2 {
            turn.cancel(woke.on);
        }

        Ok(())
    }
}

#[test]
fn a_sleep_wakes_once_its_time_has_come_and_a_job_given_up_on_is_dropped() {
    // Catches a sleep answered before its time, or never; two sleeps answered out of the order of their times; a
    // timer on another clock than the steps' time; and a job that waits, cancelled, left running. The job that waits
    // for good is given up on once the later sleep wakes.
    let runners = Arc::new(Runners::new(NonZeroUsize::MIN, Arc::new(Services::none())));
    let one = NonZeroUsize::MIN;
    let mut engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(Arc::clone(&runners) as Arc<dyn Jobs>).ready();
    let clock = Monotonic::new();
    let now = || Time::from_nanos(clock.nanos());
    // The root begins at the time of the last step.
    engine.step(now(), &clock);
    let Ok(_) = engine.add_root::<Sleeper>(&(), Nothing) else {
        panic!("the sleeper begins");
    };
    let mut said = Vec::new();

    let mut is_dropped = false;
    waiting::until(BUDGET, || {
        engine.bring();
        while !engine.is_idle() {
            engine.step(now(), &clock);
        }
        said.extend(SAID.with_borrow_mut(mem::take));
        is_dropped = DROPPED.load(Ordering::Acquire);
        said.len() == 2 && is_dropped
    });
    drop(engine);
    drop(runners);

    assert_eq!(said, ["woke 1 after its time: true", "woke 2 after its time: true"]);
    assert!(is_dropped, "the job given up on is dropped");
}
