//! Jobs run on the native host's threads for computing.

use super::*;
use crate::Monotonic;
use crate::waiting::{self, BUDGET};
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, ChildOf, Ending, Fault, Handles, Job, Runner, Running, Seated, Services, Sleep,
    Slept, Time, Top, Turn, Unanswered,
};
use aethereal_data::{ActorId, Bound, Plain};
use aethereal_substrate::harness::{Turned, Unheard};
use aethereal_substrate::{Clock, Engine};
use std::future;
use std::time::Duration;

/// A job that says whether it ran on one of the host's threads for jobs.
#[derive(Plain)]
#[kind(name = "aether.test.where")]
struct Where {
    at: u64,
}

/// What a job that says where it ran answers with.
#[derive(Debug, PartialEq, Eq, Plain)]
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

/// An actor that runs three jobs as it begins: the test reads each answer from the turn it was handed.
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

    fn handle(&mut self, _: (Answered<Ran>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

#[test]
fn each_job_runs_on_a_thread_of_its_own_and_those_past_its_threads_wait_their_turn() -> Result<(), Unheard> {
    // Catches a job run on the engine's thread, one never answered, and one past what the threads let be in flight
    // left waiting for good. One thread runs three jobs, so two wait in the engine. Dropping the runners stops their
    // thread, or this never returns.
    let one = NonZeroUsize::MIN;
    let runners = Arc::new(Runners::new(one, Arc::new(Services::none())));
    let mut engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(Arc::clone(&runners) as Arc<dyn Jobs>).ready();
    engine.record_turns();
    let Ok(_) = engine.add_root::<Asker>(&(), Nothing) else {
        panic!("the asker begins");
    };
    let clock = Monotonic::new();
    let mut turned = Vec::new();

    waiting::until(BUDGET, || {
        engine.bring();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }
        turned.extend(engine.take_turned());
        turned.len() == 3
    });
    drop(engine);
    drop(runners);

    let mut ran = turned.iter().map(Turned::read).collect::<Result<Vec<Ran>, Unheard>>()?;
    ran.sort_by_key(|ran| ran.at);
    let on_job_threads = [0, 1, 2].map(|at| Ran { at, on_a_job_thread: true });
    assert_eq!(ran, on_job_threads);

    Ok(())
}

/// The test's own word on the job that waits for good, served to it: whether it was dropped. It says more than an
/// `Arc`'s count could, which reads the same for a job dropped and one that never began.
struct Dropped(AtomicBool);

/// What a job that waits for good holds: it notes when it is dropped.
struct Guard<'job>(&'job Dropped);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.0.store(true, Ordering::Release);
    }
}

/// A job that waits for good, until it is dropped.
#[derive(Plain)]
#[kind(name = "aether.test.forever")]
struct Forever;

impl Job for Forever {
    type Done = Ran;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Ran, Fault> {
        let Some(dropped) = job.service::<Dropped>() else {
            return Ok(Ran { at: 0, on_a_job_thread: true });
        };
        let _guard = Guard(dropped);
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

/// What became of a sleeper's jobs, as it keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Did {
    /// The sleep `order` woke it, at a time past its own or not.
    Woke { order: u64, after_its_time: bool },
    /// A sleep went unanswered.
    NeverWoke(Unanswered),
    /// The job that waits for good was answered.
    Finished,
}

/// An actor that runs a job that waits for good and two sleeps as it begins, the later run first, and gives up on the
/// job once the later sleep wakes.
#[derive(Bound)]
#[kind(name = "aether.test.sleeper")]
struct Sleeper {
    /// What became of its jobs, in order: only it reads the time a sleep woke it at.
    did: Vec<Did>,
}

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

        Ok(Self { did: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Ran, Nothing>> for Sleeper {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Ran>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.did.push(Did::Finished);

        Ok(())
    }
}

impl Handles<Answer<Slept, Woke>> for Sleeper {
    type Sends = ();

    fn handle(&mut self, (done, woke): (Answered<Slept>, Woke), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let did = match done {
            Answered::Yes(Slept) => Did::Woke { order: woke.order, after_its_time: turn.now() >= woke.until },
            Answered::No(why) => Did::NeverWoke(why),
        };
        self.did.push(did);
        if woke.order == 2 {
            turn.cancel(woke.on);
        }

        Ok(())
    }
}

/// The seat of the actor `id` names in `engine`.
fn seated(engine: &Engine, id: ActorId) -> Seated {
    let Some(seat) = engine.table().seat(id) else {
        panic!("the actor is seated");
    };

    Seated { id, seat }
}

#[test]
fn a_sleep_wakes_once_its_time_has_come_and_a_job_given_up_on_is_dropped() {
    // Catches a sleep answered before its time, or never; two sleeps answered out of the order of their times; a
    // timer on another clock than the steps' time; and a job that waits, cancelled, left running. The job that waits
    // for good is given up on once the later sleep wakes.
    let dropped = Arc::new(Dropped(AtomicBool::new(false)));
    let services = Services::none().with(Arc::clone(&dropped));
    let runners = Arc::new(Runners::new(NonZeroUsize::MIN, Arc::new(services)));
    let one = NonZeroUsize::MIN;
    let mut engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(Arc::clone(&runners) as Arc<dyn Jobs>).ready();
    let clock = Monotonic::new();
    let now = || Time::from_nanos(clock.nanos());
    // The root begins at the time of the last step.
    engine.step(now(), &clock);
    let Ok(sleeper) = engine.add_root::<Sleeper>(&(), Nothing) else {
        panic!("the sleeper begins");
    };
    let sleeper = seated(&engine, sleeper);
    let mut did = Vec::new();

    let mut is_dropped = false;
    waiting::until(BUDGET, || {
        engine.bring();
        while !engine.is_idle() {
            engine.step(now(), &clock);
        }
        did = engine.state::<Sleeper>(sleeper).map(|state| state.did.clone()).unwrap_or_default();
        is_dropped = dropped.0.load(Ordering::Acquire);
        did.len() == 2 && is_dropped
    });
    drop(engine);
    drop(runners);

    assert_eq!(did, [Did::Woke { order: 1, after_its_time: true }, Did::Woke { order: 2, after_its_time: true }]);
    assert!(is_dropped, "the job given up on is dropped");
}
