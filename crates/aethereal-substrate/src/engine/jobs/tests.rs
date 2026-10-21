//! Jobs a turn runs: handed to the host once the turn has run, and answered at a bring with the context kept
//! (ADR-0018).

use super::super::*;
use super::*;
use crate::Entry;
use aethereal_actor::{
    Actor, Answer, Answered, BlobRef, ChildOf, Ending, Handles, Job, JobType, Runner, Running, Time, ToType,
    ToWithoutMail, Top, Turn,
};
use aethereal_data::{ActorPath, Bound, Kind, KindName, PathSegment, Plain};
use std::cell::RefCell;
use std::future;
use std::sync::Mutex;
use std::task::Context as Polled;

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

/// A job that rings at once: the stand-in for a timer.
#[derive(Plain)]
struct Ring;

impl Kind for Ring {
    const NAME: KindName = KindName::new("aether.test.ring");
}

/// What a ring answers with.
#[derive(Plain)]
struct Rang;

impl Kind for Rang {
    const NAME: KindName = KindName::new("aether.test.rang");
}

impl Job for Ring {
    type Done = Rang;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, _: &mut Running) -> Result<Rang, Fault> {
        Ok(Rang)
    }
}

/// A job that sums the bytes of the blobs it refers to: one twice, and another.
#[derive(Bound)]
struct Measure {
    blob: BlobRef,
    again: BlobRef,
    other: BlobRef,
}

impl Kind for Measure {
    const NAME: KindName = KindName::new("aether.test.measure");
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
struct Hangs {
    blob: BlobRef,
}

impl Kind for Hangs {
    const NAME: KindName = KindName::new("aether.test.hangs");
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
struct Shrink {
    blob: BlobRef,
    big: bool,
}

impl Kind for Shrink {
    const NAME: KindName = KindName::new("aether.test.shrink");
}

/// What a shrink answers with.
#[derive(Bound)]
struct Shrunk {
    first: BlobRef,
    kept: BlobRef,
    second: BlobRef,
}

impl Kind for Shrunk {
    const NAME: KindName = KindName::new("aether.test.shrunk");
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

/// What a job's asker keeps to give up on another job: the job it gives up on.
#[derive(Bound)]
struct GiveUp {
    on: JobId,
}

impl Kind for GiveUp {
    const NAME: KindName = KindName::new("aether.test.give_up");
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

/// An actor that doubles one as it begins, runs the job each `Go` names, and says what became of each. It keeps the
/// jobs it was told to hold, to cancel later.
#[derive(Bound)]
struct Worker {
    kept: Vec<JobId>,
}

impl Kind for Worker {
    const NAME: KindName = KindName::new("aether.test.job_worker");
}

impl ChildOf for Worker {
    type Parent = Top;
}

impl Actor for Worker {
    type Config = Which;
    type Key = ();
    type Handles = (Go, Answer<Doubled, Which>, Answer<Rang, GiveUp>, Answer<Shrunk, Which>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Which, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let id = turn.run(&Double { n: 1 }, &Which { at: 0 })?;
        say(format!("ran 0 as {}", id.mail().count));

        Ok(Self { kept: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Go> for Worker {
    type Sends = ();

    fn handle(&mut self, go: Go, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let which = Which { at: go.at };
        let id = match go.what.as_str() {
            "double" => turn.run(&Double { n: 10 * go.at }, &which)?,
            "twice" => {
                let first = turn.run(&Double { n: 10 * go.at }, &which)?;
                say(format!("ran {} as {}", go.at, first.mail().count));
                turn.run(&Double { n: 10 * go.at + 1 }, &Which { at: go.at + 100 })?
            }
            "cancel" => {
                let id = turn.run(&Double { n: 10 * go.at }, &which)?;
                turn.cancel(id);
                id
            }
            "give up" => {
                let sleeping = turn.run(&Sleeps, &which)?;
                turn.run(&Ring, &GiveUp { on: sleeping })?;
                sleeping
            }
            "hold" => {
                let id = turn.run(&Sleeps, &which)?;
                self.kept.push(id);
                id
            }
            "measure" => {
                let (Ok(blob), Ok(other)) = (turn.put(vec![1, 2, 3, 4]), turn.put(vec![100])) else {
                    return Err(Fault::of_parts(KindId::of::<Measure>(), Vec::new()));
                };
                let id = turn.run(&Measure { again: blob.clone(), blob, other }, &which)?;
                self.kept.push(id);
                id
            }
            "hang" => {
                let Ok(blob) = turn.put(vec![1]) else {
                    return Err(Fault::of_parts(KindId::of::<Hangs>(), Vec::new()));
                };
                turn.run(&Hangs { blob }, &which)?
            }
            "shrink" | "shrink big" => {
                let Ok(blob) = turn.put(vec![1, 2, 3, 4]) else {
                    return Err(Fault::of_parts(KindId::of::<Shrink>(), Vec::new()));
                };
                turn.run(&Shrink { blob, big: go.what == "shrink big" }, &which)?
            }
            "close" => {
                turn.close();
                return Ok(());
            }
            "let go" => {
                for id in self.kept.drain(..) {
                    turn.cancel(id);
                }
                return Ok(());
            }
            "let go and fail" => {
                for id in self.kept.drain(..) {
                    turn.cancel(id);
                }
                return Err(Fault::of_parts(KindId::of::<Go>(), Vec::new()));
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

    fn handle(&mut self, (done, which): (Answered<Doubled>, Which), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        match done {
            Answered::Yes(doubled) => say(format!("{} doubled {}", which.at, doubled.n)),
            Answered::No(why) => say(format!("{} {why:?}", which.at)),
        }

        Ok(())
    }
}

impl Handles<Answer<Rang, GiveUp>> for Worker {
    type Sends = ();

    fn handle(&mut self, (_, give_up): (Answered<Rang>, GiveUp), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        say(format!("gave up on {}", give_up.on.mail().count));
        turn.cancel(give_up.on);

        Ok(())
    }
}

impl Handles<Answer<Shrunk, Which>> for Worker {
    type Sends = ();

    fn handle(&mut self, (done, which): (Answered<Shrunk>, Which), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        match done {
            Answered::Yes(Shrunk { first, kept, second }) => {
                let (first, kept, second) = (turn.read(&first), turn.read(&kept), turn.read(&second));
                say(format!("{} put {first:?} {second:?} kept {kept:?}", which.at));
            }
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
struct Poker;

impl Kind for Poker {
    const NAME: KindName = KindName::new("aether.test.poker");
}

impl ChildOf for Poker {
    type Parent = Top;
}

impl Actor for Poker {
    type Config = Go;
    type Key = u64;
    type Handles = ();
    type BeginSends = (ToType<Worker, Go>,);
    type EndSends = ();

    fn begin(go: Go, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let path = ActorPath::from_root(&PathSegment::new(Worker::NAME, &()));
        if let Some(worker) = turn.resolve(&path) {
            turn.send(ToWithoutMail::Held(worker), &go)?;
        }

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
    fn start(&self, id: JobId, kind: KindId, job: Taken) {
        if let Ok(mut started) = self.started.lock() {
            started.push(kind);
        }
        self.inline.start(id, kind, job);
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
    held: Mutex<Vec<(JobId, KindId, Taken)>>,
    stopped: Mutex<Vec<JobId>>,
    /// The most jobs it lets be in flight.
    most: usize,
}

impl Holding {
    /// Runs [`Double`]s and [`Ring`]s, and holds [`Sleeps`] and [`Measure`]s.
    fn new() -> Self {
        Self {
            inline: Inline::new(vec![JobType::of::<Double>(), JobType::of::<Ring>(), JobType::of::<Measure>()]),
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
        for (id, kind, job) in held.drain(..) {
            let sleeps = kind == KindId::of::<Sleeps>();
            if sleeps {
                job.finish.done(Produced::of(&Doubled { n: 7 }).map_err(|_| fault::<Waits>()));
            } else {
                self.inline.start(id, kind, job);
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
    fn take(&self) -> Vec<(JobId, KindId, Taken)> {
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
    fn start(&self, id: JobId, kind: KindId, job: Taken) {
        let sleeps = kind == KindId::of::<Sleeps>();
        let measures = kind == KindId::of::<Measure>();
        let hangs = kind == KindId::of::<Hangs>();
        let is_held = sleeps || measures || hangs;
        if !is_held {
            self.inline.start(id, kind, job);
            return;
        }
        if let Ok(mut held) = self.held.lock() {
            held.push((id, kind, job));
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

#[test]
fn a_cancelled_job_is_never_answered_and_its_host_is_asked_to_stop() {
    // Catches a cancelled job's context kept, so its answer runs a turn, whether it finished before the cancel (one
    // cancelled in the turn that ran it, which `Inline` finishes at once) or after (one held, given up on from the
    // handler of another job's answer, through an id kept in that job's context, and one held and let go of from a
    // turn of plain mail that does nothing else); and a host never asked to stop a job. A job held and not cancelled
    // is answered when it is let go of.
    let holding = Arc::new(Holding::new());
    let (mut engine, _) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
    poke(&mut engine, 1, "cancel");
    poke(&mut engine, 2, "give up");
    poke(&mut engine, 3, "hold");
    poke(&mut engine, 4, "let go");
    poke(&mut engine, 5, "sleeps");
    engine.bring();
    settle(&mut engine);
    let ran = ["ran 0 as 1", "ran 1 as 2", "ran 2 as 3", "ran 3 as 5", "ran 5 as 6"];
    assert_eq!(said(), [&["0 doubled 2", "gave up on 3"][..], &ran].concat());

    holding.let_go();
    engine.bring();
    settle(&mut engine);

    assert_eq!(said(), ["5 doubled 7"], "the jobs given up on and let go of are not answered");
    assert_eq!(holding.stopped(), [2, 5, 3], "as they were cancelled: the job given up on after the bring");
}

#[test]
fn a_turn_that_fails_cancels_nothing() {
    // Catches the cancels of a failed turn sent to the host, as the mail of one is never sent.
    let holding = Arc::new(Holding::new());
    let (mut engine, _) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
    poke(&mut engine, 1, "hold");
    poke(&mut engine, 2, "let go and fail");

    assert_eq!(said(), ["ran 0 as 1", "ran 1 as 2"]);
    assert_eq!(holding.stopped(), Vec::<u64>::new());
}

#[test]
fn a_job_reads_a_copy_of_each_blob_it_refers_to() {
    // Catches a blob a job refers to not handed out with it, so it reads no bytes, and a reference read out of the
    // job refused for holding no pin, so the job faults, and a reference read as another blob's bytes. It refers to one
    // blob twice, and another once.
    let types = vec![JobType::of::<Double>(), JobType::of::<Measure>()];
    let (mut engine, _) = engine(Arc::new(Inline::new(types)));
    poke(&mut engine, 1, "measure");
    engine.bring();
    settle(&mut engine);

    assert_eq!(said(), ["0 doubled 2", "1 doubled 120", "ran 0 as 1", "ran 1 as 2"]);
}

#[test]
fn a_job_keeps_the_blobs_it_refers_to_until_it_is_answered_cancelled_or_its_actor_ends() {
    // Catches a blob freed while a job that refers to it is in flight, once the turn that ran it let go of its own
    // reference, and one kept once the job is answered, cancelled, or its actor has ended. Each of three workers runs a
    // job of two blobs, held by its host: the first's is answered, the second's cancelled, and the third worker closes.
    // Catches too the changes to pins the runner made left on the engine's queue, which would grow for good.
    let mut held = Vec::new();
    for (key, end) in [(1, "answered"), (2, "let go"), (3, "close")] {
        let holding = Arc::new(Holding::new());
        let (mut engine, _) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
        poke(&mut engine, key, "measure");
        let in_flight = engine.blobs.held().0;
        if end == "answered" {
            holding.let_go();
            engine.bring();
            settle(&mut engine);
        } else {
            poke(&mut engine, key + 10, end);
        }
        held.push((in_flight, engine.blobs.held().0, engine.job_pins.len()));
    }
    said();

    assert_eq!(held, [(2, 0, 0), (2, 0, 0), (2, 0, 0)], "the runner's own pins are summed, and leave nothing queued");
}

#[test]
fn a_job_changes_its_engines_pins_on_the_engines_queue_as_it_runs_and_as_it_is_dropped() {
    // Catches a host's thread that runs a job changing pins where unbuffered changes go, behind a lock, as the job reads
    // its references or drops them, whether it finishes or is dropped while it waits; a change lost on the way; and a
    // thread's own buffer not put back once a job has run on it. Both jobs run on this thread, with a buffer open.
    let holding = Arc::new(Holding::new());
    let (mut engine, _) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
    poke(&mut engine, 1, "measure");
    poke(&mut engine, 2, "hang");
    let Ok(probe) = engine.put_blob(vec![9]) else {
        panic!("a blob is put");
    };
    // The put's own pin was noted where unbuffered changes go.
    pins::take_shared(engine.number.0, &mut Vec::new());
    let mut context = Polled::from_waker(Waker::noop());
    pins::open(engine.number.0, Vec::new());
    for (_, kind, Taken { bytes, lent, limit, .. }) in holding.take() {
        let measures = kind == KindId::of::<Measure>();
        let of = if measures {
            JobType::of::<Measure>()
        } else {
            JobType::of::<Hangs>()
        };
        let mut running = of.drive(bytes, lent, limit, Arc::new(Never));
        let polled = running.as_mut().poll(&mut context);
        assert_eq!(polled.is_ready(), measures, "the measure finishes, and the hang waits");
    }
    let copy = probe.clone();
    let own = pins::close();
    said();

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
    let types = vec![JobType::of::<Double>(), JobType::of::<Shrink>()];
    let (mut engine, _) = engine(Arc::new(Inline::new(types)));
    poke(&mut engine, 1, "shrink");
    engine.bring();
    settle(&mut engine);

    assert_eq!(said(), ["0 doubled 2", "1 put [4, 3, 2, 1] [4] kept [1, 2, 3, 4]", "ran 0 as 1", "ran 1 as 2"]);
    // The answer's mail keeps all three until the step after it was handled, when its arena is emptied.
    let handled = engine.blobs.held().0;
    engine.step(Time::ZERO, &Stopped);
    assert_eq!((handled, engine.blobs.held().0), (3, 0), "every blob is freed once its answer has gone");
}

#[test]
fn a_job_whose_blob_does_not_fit_is_answered_faulted_and_puts_none() {
    // Catches a blob past the budget for blobs put anyway, or the job answered as if it fit, and the blob it put
    // before it kept once the job is answered.
    let types = vec![JobType::of::<Double>(), JobType::of::<Shrink>()];
    let (mut engine, worker) = engine(Arc::new(Inline::new(types)));
    poke(&mut engine, 1, "shrink big");
    engine.bring();
    settle(&mut engine);

    assert_eq!(said(), ["0 doubled 2", "1 Faulted", "ran 0 as 1", "ran 1 as 2"]);
    let faulted = engine
        .log(worker)
        .any(|entry| matches!(entry, Entry::JobFaulted(fault) if fault.kind() == KindId::of::<NoRoom>()));
    assert!(faulted, "the fault is the budget's");
    assert_eq!(engine.blobs.held().0, 0);
}

#[test]
fn past_the_jobs_its_host_lets_be_in_flight_a_job_waits_in_order_until_one_finishes() {
    // Catches a job past what the host lets be in flight handed out anyway, one that waits for good once a job in
    // flight finishes, jobs that wait handed out in another order than they were run, and a job answered still
    // counted in flight. The host lets one be in flight, and holds each.
    let holding = Arc::new(Holding { most: 1, ..Holding::new() });
    let (mut engine, _) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
    for key in 1..=3 {
        poke(&mut engine, key, "hold");
    }
    let mut holds = vec![holding.holding()];

    for _ in 0..3 {
        holding.let_go();
        engine.bring();
        settle(&mut engine);
        holds.push(holding.holding());
    }

    assert_eq!(holds, [vec![2], vec![3], vec![4], vec![]], "each is handed out as the one before it finishes");
    let answered = ["1 doubled 7", "2 doubled 7", "3 doubled 7"];
    assert_eq!(
        said(),
        [&["0 doubled 2"][..], &answered, &["ran 0 as 1", "ran 1 as 2", "ran 2 as 3", "ran 3 as 4"]].concat()
    );
}

#[test]
fn a_waiting_job_cancelled_or_of_an_actor_that_ended_is_never_handed_out() {
    // Catches a job cancelled while it waits handed out later, or the host asked to stop one it was never handed, and
    // the waiting job of an actor that has ended handed out, to be answered to no one. The host lets one be in flight.
    let mut handed = Vec::new();
    for end in ["let go", "close"] {
        let holding = Arc::new(Holding { most: 1, ..Holding::new() });
        let (mut engine, _) = engine(Arc::clone(&holding) as Arc<dyn Jobs>);
        poke(&mut engine, 1, "hold");
        poke(&mut engine, 2, "hold");
        poke(&mut engine, 3, end);
        holding.let_go();
        engine.bring();
        settle(&mut engine);
        handed.push((holding.holds(), holding.stopped()));
    }
    said();

    assert_eq!(handed, [(0, vec![2]), (0, Vec::new())], "the first is stopped, and the second never handed out");
}
