//! Jobs a turn runs: handed to the host once the turn has run, and answered at a bring with the context kept
//! (ADR-0018).

use super::super::*;
use super::*;
use crate::Entry;
use aethereal_actor::{
    Answer, Answered, Ending, Handles, Job, JobType, Runner, Running, RunsJob, Time, ToType, ToWithoutMail, Turn,
};
use aethereal_data::{ActorPath, Bound, Kind, KindName, PathSegment, Plain};
use std::cell::RefCell;
use std::future;

thread_local! {
    /// What this thread's actors have said they did, in order.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what an actor did.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// What has been said since this was last asked, sorted: turns of one step run in no order these tests read.
fn said() -> Vec<String> {
    let mut said = SAID.with_borrow_mut(mem::take);
    said.sort();

    said
}

/// A job that doubles its number.
#[derive(Plain)]
struct Double {
    n: u64,
}

impl Kind for Double {
    const NAME: KindName = KindName::new("aether.test.double");
}

/// What a job answers with.
#[derive(Plain)]
struct Doubled {
    n: u64,
}

impl Kind for Doubled {
    const NAME: KindName = KindName::new("aether.test.doubled");
}

impl Job for Double {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Doubled, Fault> {
        Ok(Doubled { n: self.n * 2 })
    }
}

/// A job that returns a fault.
#[derive(Plain)]
struct Fails;

impl Kind for Fails {
    const NAME: KindName = KindName::new("aether.test.fails");
}

impl Job for Fails {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Doubled, Fault> {
        Err(Fault::of_parts(KindId::of::<Self>(), Vec::new()))
    }
}

/// A job that panics.
#[derive(Plain)]
struct Panics;

impl Kind for Panics {
    const NAME: KindName = KindName::new("aether.test.panics");
}

impl Job for Panics {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Doubled, Fault> {
        panic!("a job that panics");
    }
}

/// A job that waits for good.
#[derive(Plain)]
struct Sleeps;

impl Kind for Sleeps {
    const NAME: KindName = KindName::new("aether.test.sleeps");
}

impl Job for Sleeps {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, _: &mut Running) -> Result<Doubled, Fault> {
        future::pending().await
    }
}

/// A job no runner is handed.
#[derive(Plain)]
struct Unlinked;

impl Kind for Unlinked {
    const NAME: KindName = KindName::new("aether.test.unlinked");
}

impl Job for Unlinked {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Doubled, Fault> {
        Ok(Doubled { n: 0 })
    }
}

/// What a job's asker keeps: which job it was.
#[derive(Plain)]
struct Which {
    at: u64,
}

impl Kind for Which {
    const NAME: KindName = KindName::new("aether.test.which_job");
}

/// What a worker is told to run: a job of `what`, numbered `at`.
#[derive(Plain)]
struct Go {
    what: String,
    at: u64,
}

impl Kind for Go {
    const NAME: KindName = KindName::new("aether.test.go");
}

/// An actor that doubles one as it begins, runs the job each `Go` names, and says what became of each.
#[derive(Bound)]
struct Worker;

impl Kind for Worker {
    const NAME: KindName = KindName::new("aether.test.job_worker");
}

impl Actor for Worker {
    type Config = Which;
    type Key = ();
    type Handles = (Go, Answer<Doubled, Which>);
    type BeginSends = (RunsJob<Double, Which>,);
    type EndSends = ();

    fn begin(_: Which, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let id = turn.run(&Double { n: 1 }, &Which { at: 0 })?;
        say(format!("ran 0 as {}", id.mail().count));

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Go> for Worker {
    type Sends = (
        RunsJob<Double, Which>,
        RunsJob<Fails, Which>,
        RunsJob<Panics, Which>,
        RunsJob<Sleeps, Which>,
        RunsJob<Unlinked, Which>,
    );

    fn handle(&mut self, go: Go, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let which = Which { at: go.at };
        let id = match go.what.as_str() {
            "double" => turn.run(&Double { n: 10 * go.at }, &which)?,
            "twice" => {
                let first = turn.run(&Double { n: 10 * go.at }, &which)?;
                say(format!("ran {} as {}", go.at, first.mail().count));
                turn.run(&Double { n: 10 * go.at + 1 }, &Which { at: go.at + 100 })?
            }
            "nest" => {
                let spawned = turn.spawn::<Failing>(&(), Which { at: go.at });
                say(format!("nest {:?}", spawned.err().is_some()));
                return Ok(());
            }
            "fails" => turn.run(&Fails, &which)?,
            "panics" => turn.run(&Panics, &which)?,
            "sleeps" => turn.run(&Sleeps, &which)?,
            "unlinked" => turn.run(&Unlinked, &which)?,
            _ => {
                turn.run(&Double { n: 0 }, &which)?;
                return Err(Fault::of_parts(KindId::of::<Go>(), Vec::new()));
            }
        };
        say(format!("ran {} as {}", go.at, id.mail().count));

        Ok(())
    }
}

impl Handles<Answer<Doubled, Which>> for Worker {
    type Sends = ();

    fn handle(&mut self, (done, which): (Answered<Doubled>, Which), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        match done {
            Answered::Yes(doubled) => say(format!("{} doubled {}", which.at, doubled.n)),
            Answered::No(why) => say(format!("{} {why:?}", which.at)),
        }

        Ok(())
    }
}

/// A child that runs a job as it begins, then fails: refused, it runs nothing.
#[derive(Bound)]
struct Failing;

impl Kind for Failing {
    const NAME: KindName = KindName::new("aether.test.failing_child");
}

impl Actor for Failing {
    type Config = Which;
    type Key = ();
    type Handles = (Answer<Doubled, Which>,);
    type BeginSends = (RunsJob<Unlinked, Which>,);
    type EndSends = ();

    fn begin(which: Which, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        turn.run(&Unlinked, &which)?;

        Err(Fault::of_parts(KindId::of::<Self>(), Vec::new()))
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Doubled, Which>> for Failing {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Doubled>, Which), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A root that tells the worker to run a job as it begins, from a chain of its own.
#[derive(Bound)]
struct Poker;

impl Kind for Poker {
    const NAME: KindName = KindName::new("aether.test.poker");
}

impl Actor for Poker {
    type Config = Go;
    type Key = u64;
    type Handles = ();
    type BeginSends = (ToType<Worker, Go>,);
    type EndSends = ();

    fn begin(go: Go, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let path = ActorPath::from_root(&PathSegment::new(Worker::NAME, &()));
        if let Some(worker) = turn.resolve(&path) {
            turn.send(ToWithoutMail::Held(worker), &go)?;
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A clock that never moves.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// Steps `engine` until nothing is left to run.
fn settle(engine: &mut Engine) {
    while !engine.is_idle() {
        engine.step(Time::ZERO, &Stopped);
    }
}

/// Runs jobs inline, as [`Inline`] does, and notes the kind of each it was handed.
struct Noting {
    inline: Inline,
    started: Mutex<Vec<KindId>>,
}

impl Jobs for Noting {
    fn start(&self, id: JobId, kind: KindId, job: Taken) {
        if let Ok(mut started) = self.started.lock() {
            started.push(kind);
        }
        self.inline.start(id, kind, job);
    }
}

/// An engine of four parts whose jobs `jobs` runs, with the worker made.
fn engine(jobs: Arc<dyn Jobs>) -> (Engine, Seated) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).with_jobs(jobs);
    let Ok(worker) = engine.add_root::<Worker>(&(), Which { at: 0 }) else {
        panic!("the worker begins");
    };
    let Some(seat) = engine.table().seat(worker) else {
        panic!("the worker is seated");
    };

    (engine, Seated { id: worker, seat })
}

/// Has a poker of `key` tell the worker in `engine` to run `what`, numbered `key`, and steps until it has.
fn poke(engine: &mut Engine, key: u64, what: &str) {
    let Ok(_) = engine.add_root::<Poker>(&key, Go { what: String::from(what), at: key }) else {
        panic!("the poker begins");
    };
    settle(engine);
}

#[test]
fn a_job_is_answered_at_a_bring_with_the_context_its_turn_kept() {
    // Catches a job never handed to its host, an answer handed before a bring brings it in, one handed with another
    // job's context, and a job's count not kept among what its actor sends: a later job of the actor would take it
    // again, and its answer would find the earlier job's context. The worker's `begin` runs one job, and two turns in
    // two steps after run one each.
    // Each job takes the next count of what the worker sends: none is taken twice, as its `begin`'s first, then a
    // turn's, then two of one turn.
    let (mut engine, _) = engine(Arc::new(Inline::new(vec![JobType::of::<Double>()])));
    poke(&mut engine, 1, "double");
    poke(&mut engine, 2, "twice");
    assert_eq!(said(), ["ran 0 as 1", "ran 1 as 2", "ran 2 as 3", "ran 2 as 4"], "nothing is answered before a bring");

    engine.bring();
    settle(&mut engine);

    assert_eq!(said(), ["0 doubled 2", "1 doubled 20", "102 doubled 42", "2 doubled 40"]);
}

#[test]
fn a_job_that_fails_panics_waits_or_has_no_runner_is_answered_faulted_and_logged() {
    // Catches a job's fault, a panic, a job that waits where nothing polls it again, and one of a type its host was
    // not handed, left unanswered or answered as if it had run, and a fault kept out of the actor's log.
    let types = vec![JobType::of::<Fails>(), JobType::of::<Panics>(), JobType::of::<Sleeps>()];
    let (mut engine, worker) = engine(Arc::new(Inline::new(types)));
    for (key, what) in [(1, "fails"), (2, "panics"), (3, "sleeps"), (4, "unlinked")] {
        poke(&mut engine, key, what);
    }
    engine.bring();
    settle(&mut engine);

    let faulted = ["0 Faulted", "1 Faulted", "2 Faulted", "3 Faulted", "4 Faulted"];
    assert_eq!(said().into_iter().filter(|line| line.ends_with("Faulted")).collect::<Vec<_>>(), faulted);
    let faulted: Vec<KindId> = engine
        .log(worker)
        .filter_map(|entry| match entry {
            Entry::JobFaulted(fault) => Some(fault.kind()),
            _ => None,
        })
        .collect();
    let no_runner = KindId::of::<NoRunner>();
    let why = [no_runner, KindId::of::<Fails>(), KindId::of::<Panicked>(), KindId::of::<Waits>(), no_runner];
    assert_eq!(faulted, why);
}

#[test]
fn a_turn_that_fails_runs_no_job() {
    // Catches the job of a turn that failed handed to the host, as the mail of one is never sent, and the job of a
    // child whose `begin` failed, which was never live.
    let noting = Arc::new(Noting { inline: Inline::new(Vec::new()), started: Mutex::new(Vec::new()) });
    let (mut engine, _) = engine(Arc::clone(&noting) as Arc<dyn Jobs>);
    poke(&mut engine, 1, "fail the turn");
    let (mut nesting, _) = self::engine(Arc::clone(&noting) as Arc<dyn Jobs>);
    poke(&mut nesting, 2, "nest");
    assert_eq!(said(), ["nest true", "ran 0 as 1", "ran 0 as 1"]);

    let Ok(started) = noting.started.lock() else {
        panic!("the list is not poisoned");
    };
    assert_eq!(*started, [KindId::of::<Double>(), KindId::of::<Double>()], "the `begin`s' jobs alone");
}
