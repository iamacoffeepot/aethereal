//! Jobs run on the native host's threads for computing.

use super::*;
use crate::Monotonic;
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, Ending, Fault, Handles, Job, Runner, Running, RunsJob, Time, Turn,
};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_substrate::Engine;
use std::cell::RefCell;
use std::mem;

thread_local! {
    /// What this thread's actors have said they were answered with, in order.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// A job that says whether it ran on one of the host's threads for jobs.
#[derive(Plain)]
struct Where {
    at: u64,
}

impl Kind for Where {
    const NAME: KindName = KindName::new("aether.test.where");
}

/// What a job that says where it ran answers with.
#[derive(Plain)]
struct Ran {
    at: u64,
    on_a_job_thread: bool,
}

impl Kind for Ran {
    const NAME: KindName = KindName::new("aether.test.ran");
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
struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.test.nothing");
}

/// An actor that runs three jobs as it begins, and says what each was answered with.
#[derive(Bound)]
struct Asker;

impl Kind for Asker {
    const NAME: KindName = KindName::new("aether.test.asker");
}

impl Actor for Asker {
    type Config = Nothing;
    type Key = ();
    type Handles = (Answer<Ran, Nothing>,);
    type BeginSends = (RunsJob<Where, Nothing>,);
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        for at in 0..3 {
            turn.run(&Where { at }, &Nothing)?;
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Ran, Nothing>> for Asker {
    type Sends = ();

    fn handle(&mut self, (done, _): (Answered<Ran>, Nothing), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
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
    let runners = Arc::new(Runners::new(vec![JobType::of::<Where>()], one));
    let mut engine = Engine::new(one, 1024, 0, 1 << 20, 0).with_jobs(Arc::clone(&runners) as Arc<dyn Jobs>);
    let Ok(_) = engine.add_root::<Asker>(&(), Nothing) else {
        panic!("the asker begins");
    };
    let clock = Monotonic::new();
    let mut said = Vec::new();

    // A bounded number of brings, each after the threads have had a chance to run: far more than three jobs need.
    for _ in 0..1_000_000 {
        engine.bring();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }
        said.extend(SAID.with_borrow_mut(mem::take));
        if said.len() == 3 {
            break;
        }
        thread::yield_now();
    }
    drop(engine);
    drop(runners);

    said.sort();
    assert_eq!(said, ["0 on a job thread: true", "1 on a job thread: true", "2 on a job thread: true"]);
}
