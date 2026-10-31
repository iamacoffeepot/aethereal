//! The headless host making its runners for the job types its program lists (ADR-0029).

use crate::Headless;
use crate::waiting::{self, BUDGET};
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, ChildOf, Ending, Fault, Handles, Job, Runner, Running, Tick, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::harness::{Harness, Turned, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};

/// A job that answers with one more than `n`.
#[derive(Plain)]
#[kind(name = "aether.test.count")]
struct Count {
    n: u64,
}

/// What a count answers with.
#[derive(Debug, PartialEq, Eq, Plain)]
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

/// A root that runs a count as it begins, while its program bootstraps: the test reads its answer from the turn it
/// was handed.
#[derive(Bound)]
#[kind(name = "aether.test.counter")]
struct Counter;

impl ChildOf for Counter {
    type Parent = Top;
}

impl Actor for Counter {
    type Config = NothingKept;
    type Key = ();
    type Handles = (Answer<Counted, NothingKept>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: NothingKept, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.run(&Count { n: 1 }, &NothingKept)?;

        Ok(Self)
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

impl Handles<Answer<Counted, NothingKept>> for Counter {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Counted>, NothingKept), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

#[test]
fn a_program_s_jobs_run_on_the_runners_its_host_makes() -> Result<(), Unheard> {
    // Catches a host that makes no runners, or makes them without the job types its program lists: the count is
    // answered faulted, or never, and the test's wait is spent. Its `begin` runs the count as the program bootstraps,
    // before the host has made its runners, so the count waits for them.
    let mut harness = Harness::of(Headless::program().with_cycle::<(Tick,)>().root::<Counter>())?;
    let mut turned = Vec::new();

    waiting::until(BUDGET, || {
        turned.extend(harness.step_turns());
        !turned.is_empty()
    });

    let counted: Vec<Result<Counted, Unheard>> = turned.iter().map(Turned::read).collect();
    assert_eq!(counted, [Ok(Counted { n: 2 })]);

    Ok(())
}
