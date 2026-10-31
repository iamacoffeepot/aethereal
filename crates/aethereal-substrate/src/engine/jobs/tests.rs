//! Jobs a turn runs: handed to the host once the turn has run, and answered at a bring with the context kept
//! (ADR-0018).

use super::super::*;
use super::*;
use crate::Entry;
use aethereal_actor::{
    Actor, Answer, Answered, BlobRef, ChildOf, Ending, Given, Handles, Job, JobType, Path, Resource, Runner, Running,
    Services, Tells, Time, Top, Turn, Unanswered, path,
};
use aethereal_data::{Bound, Plain};
use std::future;
use std::sync::Mutex;
use std::task::Context as Polled;

/// What became of a job, as a worker keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Done {
    /// It answered with this number.
    Doubled(u64),
    /// It went unanswered for this.
    No(Unanswered),
    /// A shrink answered with these blobs' bytes: the two it put, and the one it was handed.
    Put { first: Vec<u8>, second: Vec<u8>, kept: Vec<u8> },
    /// A light answered with lanterns of these lights.
    Lit(u64, u64),
}

/// A job that doubles its number.
#[derive(Plain)]
#[kind(name = "aether.test.double")]
struct Double {
    n: u64,
}

/// What a job answers with.
#[derive(Plain)]
#[kind(name = "aether.test.doubled")]
struct Doubled {
    n: u64,
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
#[kind(name = "aether.test.fails")]
struct Fails;

impl Job for Fails {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Doubled, Fault> {
        Err(Fault::of_parts(KindId::of::<Self>(), Vec::new()))
    }
}

/// A job that panics.
#[derive(Plain)]
#[kind(name = "aether.test.panics")]
struct Panics;

impl Job for Panics {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Doubled, Fault> {
        panic!("a job that panics");
    }
}

/// A job that waits for good.
#[derive(Plain)]
#[kind(name = "aether.test.sleeps")]
struct Sleeps;

impl Job for Sleeps {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, _: &mut Running) -> Result<Doubled, Fault> {
        future::pending().await
    }
}

/// A job that rings at once: the stand-in for a timer.
#[derive(Plain)]
#[kind(name = "aether.test.ring")]
struct Ring;

/// What a ring answers with.
#[derive(Plain)]
#[kind(name = "aether.test.rang")]
struct Rang;

impl Job for Ring {
    type Done = Rang;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, _: &mut Running) -> Result<Rang, Fault> {
        Ok(Rang)
    }
}

/// A job that sums the bytes of the blobs it refers to: one twice, and another.
#[derive(Bound)]
#[kind(name = "aether.test.measure")]
struct Measure {
    blob: BlobRef,
    again: BlobRef,
    other: BlobRef,
}

impl Job for Measure {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<Doubled, Fault> {
        let sum = |blob: &BlobRef| job.read(blob).iter().map(|&byte| u64::from(byte)).sum::<u64>();

        Ok(Doubled { n: sum(&self.blob) + sum(&self.again) + sum(&self.other) })
    }
}

/// A job that reads the blob it refers to, then waits for good.
#[derive(Bound)]
#[kind(name = "aether.test.hangs")]
struct Hangs {
    blob: BlobRef,
}

impl Job for Hangs {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Doubled, Fault> {
        let read = job.read(&self.blob).len();
        future::pending::<()>().await;

        Ok(Doubled { n: u64::try_from(read).unwrap_or(0) })
    }
}

/// A job that puts the bytes of the blob it refers to backwards, and then the count of what it put, read back, or past
/// any budget when told to, and answers with references to both and to the blob it was handed.
#[derive(Bound)]
#[kind(name = "aether.test.shrink")]
struct Shrink {
    blob: BlobRef,
    big: bool,
}

/// What a shrink answers with.
#[derive(Bound)]
#[kind(name = "aether.test.shrunk")]
struct Shrunk {
    first: BlobRef,
    kept: BlobRef,
    second: BlobRef,
}

impl Job for Shrink {
    type Done = Shrunk;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<Shrunk, Fault> {
        let read = job.read(&self.blob).to_vec();
        let first = job.put(read.into_iter().rev().collect());
        let count = vec![u8::try_from(job.read(&first).len()).unwrap_or(u8::MAX)];
        let second = if self.big {
            job.put(vec![0; 2 << 20])
        } else {
            job.put(count)
        };

        Ok(Shrunk { first, kept: self.blob, second })
    }
}

/// A resource a job makes: a value that is not data.
struct Lantern {
    light: u64,
}

impl Resource for Lantern {}

/// A job that makes three lanterns and answers with the second and the third, in the other order: the first it hands
/// and does not answer with.
#[derive(Plain)]
#[kind(name = "aether.test.light")]
struct Light;

/// What a light answers with.
#[derive(Bound)]
#[kind(name = "aether.test.lit")]
struct Lit {
    first: Given<Lantern>,
    second: Given<Lantern>,
}

impl Job for Light {
    type Done = Lit;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Lit, Fault> {
        let _unheld = job.hand(Lantern { light: 1 });
        let second = job.hand(Lantern { light: 2 });
        let first = job.hand(Lantern { light: 3 });

        Ok(Lit { first, second })
    }
}

/// A job no program lists: a native turn's job carries its own code.
#[derive(Plain)]
#[kind(name = "aether.test.unlinked")]
struct Unlinked;

impl Job for Unlinked {
    type Done = Doubled;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Doubled, Fault> {
        Ok(Doubled { n: 0 })
    }
}

/// What a job's asker keeps: which job it was.
#[derive(Plain)]
#[kind(name = "aether.test.which_job")]
struct Which {
    at: u64,
}

/// What a job's asker keeps to give up on another job: the job it gives up on.
#[derive(Bound)]
#[kind(name = "aether.test.give_up")]
struct GiveUp {
    on: JobId,
}

/// What a worker is told to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum What {
    /// Runs a [`Double`] of ten times its place.
    Double,
    /// Runs a [`Double`] of ten times its place, and another of one more, a hundred places on.
    Twice,
    /// Runs a [`Double`] and cancels it in the same turn.
    Cancel,
    /// Runs a [`Sleeps`], and a [`Ring`] that gives up on it.
    GiveUp,
    /// Runs a [`Sleeps`] and keeps it, to cancel later.
    Hold,
    /// Puts two blobs, runs a [`Measure`] of them, and keeps it, to cancel later.
    Measure,
    /// Puts a blob and runs a [`Hangs`] of it.
    Hang,
    /// Puts a blob and runs a [`Shrink`] of it.
    Shrink,
    /// Puts a blob and runs a [`Shrink`] of it past any budget.
    ShrinkBig,
    /// Closes the worker.
    Close,
    /// Cancels each job it kept.
    LetGo,
    /// Cancels each job it kept, and fails its turn.
    LetGoAndFail,
    /// Spawns a [`Failing`] child.
    Nest,
    /// Runs a [`Light`].
    Light,
    /// Runs a [`Fails`].
    Fails,
    /// Runs a [`Panics`].
    Panics,
    /// Runs a [`Sleeps`].
    Sleeps,
    /// Runs an [`Unlinked`].
    Unlinked,
    /// Runs a job, and fails its turn.
    FailTheTurn,
}

/// How a job a test holds comes to an end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Finishes {
    /// Its host lets it go, and it is answered.
    Answered,
    /// Its worker is told this.
    Told(What),
}

/// What a worker is told to run: `what`, numbered `at`.
#[derive(Plain)]
#[kind(name = "aether.test.go")]
struct Go {
    what: What,
    at: u64,
}

/// An actor that doubles one as it begins, runs the job each `Go` names, and keeps what became of each. It keeps the
/// jobs it was told to hold, to cancel later.
#[derive(Bound)]
#[kind(name = "aether.test.job_worker")]
struct Worker {
    kept: Vec<JobId>,
    /// The place of each job it ran, and the count it ran as, in order.
    ran: Vec<(u64, u64)>,
    /// The place of each job that was answered, and what it came to, in the order they were.
    done: Vec<(u64, Done)>,
    /// The count of each job it gave up on, in order.
    gave_up: Vec<u64>,
    /// Whether each child it nested was refused, in order.
    refused: Vec<bool>,
}

impl ChildOf for Worker {
    type Parent = Top;
}

impl Actor for Worker {
    type Config = Which;
    type Key = ();
    type Handles = (Go, Answer<Doubled, Which>, Answer<Rang, GiveUp>, Answer<Shrunk, Which>, Answer<Lit, Which>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Which, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let id = turn.run(&Double { n: 1 }, &Which { at: 0 })?;

        Ok(Self {
            kept: Vec::new(),
            ran: vec![(0, id.mail().count)],
            done: Vec::new(),
            gave_up: Vec::new(),
            refused: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Go> for Worker {
    type Sends = ();

    fn handle(&mut self, go: Go, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let which = Which { at: go.at };
        let id = match go.what {
            What::Double => turn.run(&Double { n: 10 * go.at }, &which)?,
            What::Twice => {
                let first = turn.run(&Double { n: 10 * go.at }, &which)?;
                self.ran.push((go.at, first.mail().count));
                turn.run(&Double { n: 10 * go.at + 1 }, &Which { at: go.at + 100 })?
            }
            What::Cancel => {
                let id = turn.run(&Double { n: 10 * go.at }, &which)?;
                turn.cancel(id);
                id
            }
            What::GiveUp => {
                let sleeping = turn.run(&Sleeps, &which)?;
                turn.run(&Ring, &GiveUp { on: sleeping })?;
                sleeping
            }
            What::Hold => {
                let id = turn.run(&Sleeps, &which)?;
                self.kept.push(id);
                id
            }
            What::Measure => {
                let (Ok(blob), Ok(other)) = (turn.put(vec![1, 2, 3, 4]), turn.put(vec![100])) else {
                    return Err(Fault::of_parts(KindId::of::<Measure>(), Vec::new()));
                };
                let id = turn.run(&Measure { again: blob.clone(), blob, other }, &which)?;
                self.kept.push(id);
                id
            }
            What::Hang => {
                let Ok(blob) = turn.put(vec![1]) else {
                    return Err(Fault::of_parts(KindId::of::<Hangs>(), Vec::new()));
                };
                turn.run(&Hangs { blob }, &which)?
            }
            What::Shrink | What::ShrinkBig => {
                let Ok(blob) = turn.put(vec![1, 2, 3, 4]) else {
                    return Err(Fault::of_parts(KindId::of::<Shrink>(), Vec::new()));
                };
                turn.run(&Shrink { blob, big: go.what == What::ShrinkBig }, &which)?
            }
            What::Close => {
                turn.close();
                return Ok(());
            }
            What::LetGo => {
                for id in self.kept.drain(..) {
                    turn.cancel(id);
                }
                return Ok(());
            }
            What::LetGoAndFail => {
                for id in self.kept.drain(..) {
                    turn.cancel(id);
                }
                return Err(Fault::of_parts(KindId::of::<Go>(), Vec::new()));
            }
            What::Nest => {
                let spawned = turn.spawn::<Failing>(&(), Which { at: go.at });
                self.refused.push(spawned.is_err());
                return Ok(());
            }
            What::Light => turn.run(&Light, &which)?,
            What::Fails => turn.run(&Fails, &which)?,
            What::Panics => turn.run(&Panics, &which)?,
            What::Sleeps => turn.run(&Sleeps, &which)?,
            What::Unlinked => turn.run(&Unlinked, &which)?,
            What::FailTheTurn => {
                turn.run(&Double { n: 0 }, &which)?;
                return Err(Fault::of_parts(KindId::of::<Go>(), Vec::new()));
            }
        };
        self.ran.push((go.at, id.mail().count));

        Ok(())
    }
}

impl Handles<Answer<Doubled, Which>> for Worker {
    type Sends = ();

    fn handle(&mut self, (done, which): (Answered<Doubled>, Which), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let done = match done {
            Answered::Yes(doubled) => Done::Doubled(doubled.n),
            Answered::No(why) => Done::No(why),
        };
        self.done.push((which.at, done));

        Ok(())
    }
}

impl Handles<Answer<Rang, GiveUp>> for Worker {
    type Sends = ();

    fn handle(&mut self, (_, give_up): (Answered<Rang>, GiveUp), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.gave_up.push(give_up.on.mail().count);
        turn.cancel(give_up.on);

        Ok(())
    }
}

impl Handles<Answer<Shrunk, Which>> for Worker {
    type Sends = ();

    fn handle(&mut self, (done, which): (Answered<Shrunk>, Which), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let done = match done {
            Answered::Yes(Shrunk { first, kept, second }) => {
                let (first, kept, second) = (turn.read(&first), turn.read(&kept), turn.read(&second));

                Done::Put { first: first.to_vec(), second: second.to_vec(), kept: kept.to_vec() }
            }
            Answered::No(why) => Done::No(why),
        };
        self.done.push((which.at, done));

        Ok(())
    }
}

impl Handles<Answer<Lit, Which>> for Worker {
    type Sends = ();

    fn handle(&mut self, (done, which): (Answered<Lit>, Which), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let done = match done {
            Answered::Yes(Lit { first, second }) => {
                let (mut first, mut second) = (turn.take(first)?, turn.take(second)?);

                Done::Lit(turn.open(&mut first)?.light, turn.open(&mut second)?.light)
            }
            Answered::No(why) => Done::No(why),
        };
        self.done.push((which.at, done));

        Ok(())
    }
}

/// A child that runs a job as it begins, then fails: refused, it runs nothing.
#[derive(Bound)]
#[kind(name = "aether.test.failing_child")]
struct Failing;

impl ChildOf for Failing {
    type Parent = Worker;
}

impl Actor for Failing {
    type Config = Which;
    type Key = ();
    type Handles = (Answer<Doubled, Which>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(which: Which, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.run(&Unlinked, &which)?;

        Err(Fault::of_parts(KindId::of::<Self>(), Vec::new()))
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Doubled, Which>> for Failing {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Doubled>, Which), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A root that tells the worker to run a job as it begins, from a chain of its own.
#[derive(Bound)]
#[kind(name = "aether.test.poker")]
struct Poker;

impl ChildOf for Poker {
    type Parent = Top;
}

impl Actor for Poker {
    type Config = Go;
    type Key = u64;
    type Handles = ();
    type BeginSends = (Tells<Path<Worker>, Go>,);
    type EndSends = ();

    fn begin(go: Go, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Worker>(())).send(&go)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
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
    fn start(&self, id: JobId, of: JobType, job: Taken) {
        if let Ok(mut started) = self.started.lock() {
            started.push(of.kind());
        }
        self.inline.start(id, of, job);
    }

    fn stop(&self, _: JobId) {}

    fn most(&self) -> usize {
        usize::MAX
    }
}

/// Runs jobs inline, as [`Inline`] does, but holds each [`Sleeps`] and [`Measure`] until it is let go of, and notes
/// each job it is asked to stop.
struct Holding {
    inline: Inline,
    held: Mutex<Vec<(JobId, JobType, Taken)>>,
    stopped: Mutex<Vec<JobId>>,
    /// The most jobs it lets be in flight.
    most: usize,
}

impl Holding {
    /// Runs every job but [`Sleeps`], [`Measure`]s, and [`Hangs`], which it holds.
    fn new() -> Self {
        Self {
            inline: Inline::new(Arc::new(Services::none())),
            held: Mutex::default(),
            stopped: Mutex::default(),
            most: usize::MAX,
        }
    }

    /// Finishes each job it holds: a sleep as doubling to seven, and a measure by running it.
    fn let_go(&self) {
        let Ok(mut held) = self.held.lock() else {
            panic!("the list is not poisoned");
        };
        for (id, of, job) in held.drain(..) {
            let sleeps = of.kind() == KindId::of::<Sleeps>();
            if sleeps {
                job.finish.done(Produced::of(&Doubled { n: 7 }).map_err(|_| fault::<Waits>()));
            } else {
                self.inline.start(id, of, job);
            }
        }
    }

    /// How many jobs it holds.
    fn holds(&self) -> usize {
        self.held.lock().map_or(0, |held| held.len())
    }

    /// The jobs it holds, by their counts.
    fn holding(&self) -> Vec<u64> {
        self.held.lock().map_or_else(|_| Vec::new(), |held| held.iter().map(|(id, ..)| id.mail().count).collect())
    }

    /// Takes every job it holds.
    fn take(&self) -> Vec<(JobId, JobType, Taken)> {
        let Ok(mut held) = self.held.lock() else {
            panic!("the list is not poisoned");
        };

        mem::take(&mut *held)
    }

    /// The jobs it was asked to stop, by their counts.
    fn stopped(&self) -> Vec<u64> {
        let Ok(stopped) = self.stopped.lock() else {
            panic!("the list is not poisoned");
        };

        stopped.iter().map(|id| id.mail().count).collect()
    }
}

impl Jobs for Holding {
    fn start(&self, id: JobId, of: JobType, job: Taken) {
        let sleeps = of.kind() == KindId::of::<Sleeps>();
        let measures = of.kind() == KindId::of::<Measure>();
        let hangs = of.kind() == KindId::of::<Hangs>();
        let is_held = sleeps || measures || hangs;
        if !is_held {
            self.inline.start(id, of, job);
            return;
        }
        if let Ok(mut held) = self.held.lock() {
            held.push((id, of, job));
        }
    }

    fn stop(&self, id: JobId) {
        if let Ok(mut stopped) = self.stopped.lock() {
            stopped.push(id);
        }
    }

    fn most(&self) -> usize {
        self.most
    }
}

/// An engine of four parts whose jobs `jobs` runs, with the worker made.
fn engine(jobs: Arc<dyn Jobs>) -> (Engine, Seated) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).with_jobs(jobs).ready();
    let Ok(worker) = engine.add_root::<Worker>(&(), Which { at: 0 }) else {
        panic!("the worker begins");
    };
    let Some(seat) = engine.table().seat(worker) else {
        panic!("the worker is seated");
    };

    (engine, Seated { id: worker, seat })
}

/// Has a poker of `key` tell the worker in `engine` to run `what`, numbered `key`, and steps until it has.
fn poke(engine: &mut Engine, key: u64, what: What) {
    let Ok(_) = engine.add_root::<Poker>(&key, Go { what, at: key }) else {
        panic!("the poker begins");
    };
    settle(engine);
}

/// The worker's state, as of the last step.
fn worker(engine: &Engine, worker: Seated) -> &Worker {
    let Some(state) = engine.state(worker) else {
        panic!("the worker is live");
    };

    state
}

/// What the worker's jobs came to, by their places: turns of one step run in no order these tests read.
fn done(engine: &Engine, seated: Seated) -> Vec<(u64, Done)> {
    let mut done = worker(engine, seated).done.clone();
    done.sort_by_key(|(at, _)| *at);

    done
}

/// A job that answered with the number `n`, at the place `at`.
const fn doubled(at: u64, n: u64) -> (u64, Done) {
    (at, Done::Doubled(n))
}

/// A job that faulted, at the place `at`.
const fn faulted(at: u64) -> (u64, Done) {
    (at, Done::No(Unanswered::Faulted))
}

#[test]
fn a_job_is_answered_at_a_bring_with_the_context_its_turn_kept() {
    // Catches a job never handed to its host, an answer handed before a bring brings it in, one handed with another
    // job's context, and a job's count not kept among what its actor sends: a later job of the actor would take it
    // again, and its answer would find the earlier job's context. The worker's `begin` runs one job, and two turns in
    // two steps after run one each.
    // Each job takes the next count of what the worker sends: none is taken twice, as its `begin`'s first, then a
    // turn's, then two of one turn.
    let (mut engine, seated) = engine(Arc::new(Inline::new(Arc::new(Services::none()))));
    poke(&mut engine, 1, What::Double);
    poke(&mut engine, 2, What::Twice);
    assert_eq!(worker(&engine, seated).ran, [(0, 1), (1, 2), (2, 3), (2, 4)]);
    assert_eq!(done(&engine, seated), [], "nothing is answered before a bring");

    engine.bring();
    settle(&mut engine);

    assert_eq!(done(&engine, seated), [doubled(0, 2), doubled(1, 20), doubled(2, 40), doubled(102, 42)]);
}

#[test]
fn a_job_run_before_its_host_makes_its_runner_waits_for_it() {
    // Catches a job a root's `begin` runs as its program bootstraps, before the host has made its runners, handed to
    // no runner and lost, or answered faulted: a host makes its runners only once its roots have bootstrapped.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let Ok(id) = engine.add_root::<Worker>(&(), Which { at: 0 }) else {
        panic!("the worker begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the worker is seated");
    };
    let seated = Seated { id, seat };
    engine.bring();
    settle(&mut engine);
    assert_eq!(worker(&engine, seated).ran, [(0, 1)]);
    assert_eq!(done(&engine, seated), [], "nothing runs the job before there is a runner");

    engine.set_jobs(Arc::new(Inline::new(Arc::new(Services::none()))));
    engine.bring();
    settle(&mut engine);

    assert_eq!(done(&engine, seated), [doubled(0, 2)]);
}

#[test]
fn a_job_that_fails_panics_or_waits_is_answered_faulted_and_logged() {
    // Catches a job's fault, a panic, and a job that waits where nothing polls it again, left unanswered or answered
    // as if it had run, and a fault kept out of the actor's log.
    let (mut engine, seated) = engine(Arc::new(Inline::new(Arc::new(Services::none()))));
    for (key, what) in [(1, What::Fails), (2, What::Panics), (3, What::Sleeps)] {
        poke(&mut engine, key, what);
    }
    engine.bring();
    settle(&mut engine);

    assert_eq!(done(&engine, seated), [doubled(0, 2), faulted(1), faulted(2), faulted(3)]);
    let faulted: Vec<KindId> = engine
        .log(seated)
        .filter_map(|entry| match entry {
            Entry::JobFaulted(fault) => Some(fault.kind()),
            _ => None,
        })
        .collect();
    assert_eq!(faulted, [KindId::of::<Fails>(), KindId::of::<Panicked>(), KindId::of::<Waits>()]);
}

#[test]
fn a_native_turn_s_job_runs_with_its_own_code_though_no_program_lists_its_type() {
    // Catches a native turn's job looked up among the job types the engine lets guests run, as a guest's is, and
    // answered that no runner runs it: an engine that lists none runs it, and its runner is handed its type.
    let noting = Arc::new(Noting { inline: Inline::new(Arc::new(Services::none())), started: Mutex::new(Vec::new()) });
    let (mut engine, seated) = engine(Arc::clone(&noting) as Arc<dyn Jobs>);
    poke(&mut engine, 4, What::Unlinked);
    engine.bring();
    settle(&mut engine);

    assert_eq!(done(&engine, seated), [doubled(0, 2), doubled(4, 0)]);
    assert_eq!(worker(&engine, seated).ran, [(0, 1), (4, 2)]);
    let started = noting.started.lock().map_or_else(|_| Vec::new(), |started| started.clone());
    assert_eq!(started, [KindId::of::<Double>(), KindId::of::<Unlinked>()]);
}

#[test]
fn a_turn_that_fails_runs_no_job() {
    // Catches the job of a turn that failed handed to the host, as the mail of one is never sent, and the job of a
    // child whose `begin` failed, which was never live.
    let noting = Arc::new(Noting { inline: Inline::new(Arc::new(Services::none())), started: Mutex::new(Vec::new()) });
    let (mut engine, seated) = engine(Arc::clone(&noting) as Arc<dyn Jobs>);
    // The turn that fails ends the worker, so what it ran is read before.
    assert_eq!(worker(&engine, seated).ran, [(0, 1)]);
    poke(&mut engine, 1, What::FailTheTurn);
    assert!(!engine.table().is_live(seated.id), "the turn failed");
    let (mut nesting, nester) = self::engine(Arc::clone(&noting) as Arc<dyn Jobs>);
    poke(&mut nesting, 2, What::Nest);
    assert_eq!(worker(&nesting, nester).ran, [(0, 1)]);
    assert_eq!(worker(&nesting, nester).refused, [true]);

    let Ok(started) = noting.started.lock() else {
        panic!("the list is not poisoned");
    };
    assert_eq!(*started, [KindId::of::<Double>(), KindId::of::<Double>()], "the `begin`s' jobs alone");
}

#[test]
fn a_cancelled_job_is_never_answered_and_its_host_is_asked_to_stop() {
    // Catches a cancelled job's context kept, so its answer runs a turn, whether it finished before the cancel (one
    // cancelled in the turn that ran it, which `Inline` finishes at once) or after (one held, given up on from the
    // handler of another job's answer, through an id kept in that job's context, and one held and let go of from a
    // turn of plain mail that does nothing else); and a host never asked to stop a job. A job held and not cancelled
    // is answered when it is let go of.
    let holding = Arc::new(Holding::new());
    let (mut engine, seated) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
    poke(&mut engine, 1, What::Cancel);
    poke(&mut engine, 2, What::GiveUp);
    poke(&mut engine, 3, What::Hold);
    poke(&mut engine, 4, What::LetGo);
    poke(&mut engine, 5, What::Sleeps);
    engine.bring();
    settle(&mut engine);
    assert_eq!(worker(&engine, seated).ran, [(0, 1), (1, 2), (2, 3), (3, 5), (5, 6)]);
    assert_eq!(worker(&engine, seated).gave_up, [3]);
    assert_eq!(done(&engine, seated), [doubled(0, 2)]);

    holding.let_go();
    engine.bring();
    settle(&mut engine);

    let answered = [doubled(0, 2), doubled(5, 7)];
    assert_eq!(done(&engine, seated), answered, "the jobs given up on and let go of are not answered");
    assert_eq!(holding.stopped(), [2, 5, 3], "as they were cancelled: the job given up on after the bring");
}

#[test]
fn a_turn_that_fails_cancels_nothing() {
    // Catches the cancels of a failed turn sent to the host, as the mail of one is never sent.
    let holding = Arc::new(Holding::new());
    let (mut engine, seated) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
    poke(&mut engine, 1, What::Hold);
    // The turn that fails ends the worker, so what it ran is read before.
    assert_eq!(worker(&engine, seated).ran, [(0, 1), (1, 2)]);
    poke(&mut engine, 2, What::LetGoAndFail);

    assert!(!engine.table().is_live(seated.id), "the turn failed");
    assert_eq!(holding.stopped(), Vec::<u64>::new());
}

#[test]
fn a_job_reads_a_copy_of_each_blob_it_refers_to() {
    // Catches a blob a job refers to not handed out with it, so it reads no bytes, and a reference read out of the
    // job refused for holding no pin, so the job faults, and a reference read as another blob's bytes. It refers to one
    // blob twice, and another once.
    let (mut engine, seated) = engine(Arc::new(Inline::new(Arc::new(Services::none()))));
    poke(&mut engine, 1, What::Measure);
    engine.bring();
    settle(&mut engine);

    assert_eq!(done(&engine, seated), [doubled(0, 2), doubled(1, 120)]);
    assert_eq!(worker(&engine, seated).ran, [(0, 1), (1, 2)]);
}

#[test]
fn a_job_keeps_the_blobs_it_refers_to_until_it_is_answered_cancelled_or_its_actor_ends() {
    // Catches a blob freed while a job that refers to it is in flight, once the turn that ran it let go of its own
    // reference, and one kept once the job is answered, cancelled, or its actor has ended. Each of three workers runs a
    // job of two blobs, held by its host: the first's is answered, the second's cancelled, and the third worker closes.
    // Catches too the changes to pins the runner made left on the engine's queue, which would grow for good.
    let mut held = Vec::new();
    let ends = [(1, Finishes::Answered), (2, Finishes::Told(What::LetGo)), (3, Finishes::Told(What::Close))];
    for (key, end) in ends {
        let holding = Arc::new(Holding::new());
        let (mut engine, _) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
        poke(&mut engine, key, What::Measure);
        let in_flight = engine.blobs.held().0;
        match end {
            Finishes::Answered => {
                holding.let_go();
                engine.bring();
                settle(&mut engine);
            }
            Finishes::Told(what) => poke(&mut engine, key + 10, what),
        }
        held.push((in_flight, engine.blobs.held().0, engine.job_pins.len()));
    }

    assert_eq!(held, [(2, 0, 0), (2, 0, 0), (2, 0, 0)], "the runner's own pins are summed, and leave nothing queued");
}

#[test]
fn a_job_changes_its_engines_pins_on_the_engines_queue_as_it_runs_and_as_it_is_dropped() {
    // Catches a host's thread that runs a job changing pins where unbuffered changes go, behind a lock, as the job reads
    // its references or drops them, whether it finishes or is dropped while it waits; a change lost on the way; and a
    // thread's own buffer not put back once a job has run on it. Both jobs run on this thread, with a buffer open.
    let holding = Arc::new(Holding::new());
    let (mut engine, _) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
    poke(&mut engine, 1, What::Measure);
    poke(&mut engine, 2, What::Hang);
    let Ok(probe) = engine.put_blob(vec![9]) else {
        panic!("a blob is put");
    };
    // The put's own pin was noted where unbuffered changes go.
    pins::take_shared(engine.number.0, &mut Vec::new());
    let mut context = Polled::from_waker(Waker::noop());
    pins::open(engine.number.0, Vec::new());
    for (_, of, Taken { bytes, lent, limit, .. }) in holding.take() {
        let measures = of.kind() == KindId::of::<Measure>();
        let mut running = of.drive(bytes, lent, limit, Arc::new(Never), Arc::new(Services::none()));
        let polled = running.as_mut().poll(&mut context);
        assert_eq!(polled.is_ready(), measures, "the measure finishes, and the hang waits");
    }
    let copy = probe.clone();
    let own = pins::close();

    let mut shared = Vec::new();
    pins::take_shared(engine.number.0, &mut shared);
    let mut queued = Vec::new();
    pins::take_queued(&engine.job_pins, &mut queued);
    let net: i64 = queued.iter().map(|(_, by)| by).sum();
    assert_eq!((shared.len(), queued.len(), net), (0, 8, 0), "three references read and dropped, and one");
    assert_eq!(own, [(probe.id(), 1)], "a change after the jobs is in the thread's own buffer");
    drop(copy);
}

#[test]
fn a_job_answers_with_the_blobs_it_put_once_the_engine_has_put_them() {
    // Catches a blob a job put not put in the store, a reference to one left naming the job's own number, so it reads
    // nothing, two puts' references swapped, a blob the job was handed and answers with read with no pin of the
    // answer's own, and a blob kept once nothing refers to it. The job puts two, and answers with the one it was
    // handed between them.
    let (mut engine, seated) = engine(Arc::new(Inline::new(Arc::new(Services::none()))));
    poke(&mut engine, 1, What::Shrink);
    engine.bring();
    settle(&mut engine);

    let put = Done::Put { first: vec![4, 3, 2, 1], second: vec![4], kept: vec![1, 2, 3, 4] };
    assert_eq!(done(&engine, seated), [doubled(0, 2), (1, put)]);
    assert_eq!(worker(&engine, seated).ran, [(0, 1), (1, 2)]);
    // The answer's mail keeps all three until the step after it was handled, when its arena is emptied.
    let handled = engine.blobs.held().0;
    engine.step(Time::ZERO, &Stopped);
    assert_eq!((handled, engine.blobs.held().0), (3, 0), "every blob is freed once its answer has gone");
}

#[test]
fn a_job_whose_blob_does_not_fit_is_answered_faulted_and_puts_none() {
    // Catches a blob past the budget for blobs put anyway, or the job answered as if it fit, and the blob it put
    // before it kept once the job is answered.
    let (mut engine, seated) = engine(Arc::new(Inline::new(Arc::new(Services::none()))));
    poke(&mut engine, 1, What::ShrinkBig);
    engine.bring();
    settle(&mut engine);

    assert_eq!(done(&engine, seated), [doubled(0, 2), faulted(1)]);
    assert_eq!(worker(&engine, seated).ran, [(0, 1), (1, 2)]);
    let faulted = engine
        .log(seated)
        .any(|entry| matches!(entry, Entry::JobFaulted(fault) if fault.kind() == KindId::of::<NoRoom>()));
    assert!(faulted, "the fault is the budget's");
    assert_eq!(engine.blobs.held().0, 0);
}

#[test]
fn a_job_answers_with_the_resources_it_handed_and_its_actor_takes_them() {
    // Catches a ticket left naming the job's own number, so the take finds nothing; two tickets written at each
    // other's place, or both at one, so the lanterns are swapped or one is lost; a resource kept for mail other than
    // the answer that holds it, so the take is refused; and a resource handed and not answered with carried anyway.
    let (mut engine, seated) = engine(Arc::new(Inline::new(Arc::new(Services::none()))));
    poke(&mut engine, 1, What::Light);
    engine.bring();
    settle(&mut engine);

    assert_eq!(done(&engine, seated), [doubled(0, 2), (1, Done::Lit(3, 2))]);
    assert_eq!(worker(&engine, seated).ran, [(0, 1), (1, 2)]);
    let dropped = engine.log(seated).any(|entry| matches!(entry, Entry::Dropped(_)));
    assert!(!dropped, "the lantern not answered with never reached the worker, so nothing of it is dropped there");
}

#[test]
fn past_the_jobs_its_host_lets_be_in_flight_a_job_waits_in_order_until_one_finishes() {
    // Catches a job past what the host lets be in flight handed out anyway, one that waits for good once a job in
    // flight finishes, jobs that wait handed out in another order than they were run, and a job answered still
    // counted in flight. The host lets one be in flight, and holds each.
    let holding = Arc::new(Holding { most: 1, ..Holding::new() });
    let (mut engine, seated) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
    for key in 1..=3 {
        poke(&mut engine, key, What::Hold);
    }
    let mut holds = vec![holding.holding()];

    for _ in 0..3 {
        holding.let_go();
        engine.bring();
        settle(&mut engine);
        holds.push(holding.holding());
    }

    assert_eq!(holds, [vec![2], vec![3], vec![4], vec![]], "each is handed out as the one before it finishes");
    assert_eq!(done(&engine, seated), [doubled(0, 2), doubled(1, 7), doubled(2, 7), doubled(3, 7)]);
    assert_eq!(worker(&engine, seated).ran, [(0, 1), (1, 2), (2, 3), (3, 4)]);
}

#[test]
fn a_waiting_job_cancelled_or_of_an_actor_that_ended_is_never_handed_out() {
    // Catches a job cancelled while it waits handed out later, or the host asked to stop one it was never handed, and
    // the waiting job of an actor that has ended handed out, to be answered to no one. The host lets one be in flight.
    let mut handed = Vec::new();
    for end in [What::LetGo, What::Close] {
        let holding = Arc::new(Holding { most: 1, ..Holding::new() });
        let (mut engine, _) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
        poke(&mut engine, 1, What::Hold);
        poke(&mut engine, 2, What::Hold);
        poke(&mut engine, 3, end);
        holding.let_go();
        engine.bring();
        settle(&mut engine);
        handed.push((holding.holds(), holding.stopped()));
    }

    assert_eq!(handed, [(0, vec![2]), (0, Vec::new())], "the first is stopped, and the second never handed out");
}
