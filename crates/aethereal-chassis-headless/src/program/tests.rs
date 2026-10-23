//! The headless host making its runners for the job types its program lists (ADR-0029).

use crate::{Exit, Headless};
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, ChildOf, Ending, Fault, Handles, Job, On, Runner, Running, Tick, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use core::time::Duration;
use std::sync::Mutex;

/// What the counter said became of its job.
static COUNTED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A job that answers with one more than `n`.
#[derive(Plain)]
#[kind(name = "aether.test.count")]
struct Count {
    n: u64,
}

/// What a count answers with.
#[derive(Plain)]
#[kind(name = "aether.test.counted")]
struct Counted {
    n: u64,
}

impl Job for Count {
    type Done = Counted;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Counted, Fault> {
        Ok(Counted { n: self.n + 1 })
    }
}

/// What a counter keeps for its job: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.nothing_kept")]
struct NothingKept;

/// A root that runs a count as it begins, while its program bootstraps, and asks the program to stop once it is
/// answered; or, if fifty ticks go by first, gives up and asks it to stop.
#[derive(Bound)]
#[kind(name = "aether.test.counter")]
struct Counter {
    ticks: u64,
}

impl ChildOf for Counter {
    type Parent = Top;
}

impl Actor for Counter {
    type Config = NothingKept;
    type Key = ();
    type Handles = (On<Tick>, Answer<Counted, NothingKept>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: NothingKept, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();
        turn.run(&Count { n: 1 }, &NothingKept)?;

        Ok(Self { ticks: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl<H: Host> Bootstrap<H> for Counter {
    const SECTION: &'static str = "counter";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), NothingKept)
    }
}

impl Handles<On<Tick>> for Counter {
    type Sends = ();

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.ticks += 1;
        let is_long = self.ticks == 50;
        if is_long {
            COUNTED.lock().map(|mut counted| counted.push(String::from("gave up"))).ok();
            turn.quit();
        }

        Ok(())
    }
}

impl Handles<Answer<Counted, NothingKept>> for Counter {
    type Sends = ();

    fn handle(
        &mut self,
        (counted, _): (Answered<Counted>, NothingKept),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let said = match counted {
            Answered::Yes(counted) => format!("counted {}", counted.n),
            Answered::No(why) => format!("not counted: {why:?}"),
        };
        COUNTED.lock().map(|mut counted| counted.push(said)).ok();
        turn.quit();

        Ok(())
    }
}

#[test]
fn a_program_s_jobs_run_on_the_runners_its_host_makes() {
    // Catches a host that makes no runners, or makes them without the job types its program lists: the count is
    // answered faulted, or never, and the counter gives up. Its `begin` runs the count as the program bootstraps,
    // before the host has made its runners, so the count waits for them.
    let built = Headless::program().with_jobs::<(Count,)>().with_cycle::<(Tick,)>().root::<Counter>().build();
    let Ok(headless) = built else {
        panic!("the program builds");
    };

    let exit = headless.run(Duration::from_millis(5), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    let Ok(counted) = COUNTED.lock() else {
        panic!("what was said is not poisoned");
    };
    assert_eq!(*counted, ["counted 2"]);
}
