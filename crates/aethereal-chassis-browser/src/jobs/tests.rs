//! Jobs run as tasks on an event loop: here one the test pumps, on a timer whose time it sets.

use super::*;
use aethereal_actor::{
    Actor, Answer, Answered, Beginning, ChildOf, Ending, Handles, Job, Running, Seated, Services, Sleep, Slept, Time,
    Top, Turn, Unanswered,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::harness::Turned;
use aethereal_substrate::{Clock, Engine, Entry, Waits};
use std::cell::RefCell;
use std::mem;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::task::Wake;

thread_local! {
    /// The tasks put on this thread's event loop and not yet pumped. This is the fake event loop's own machinery, and
    /// no report: [`EventLoop`] puts a task on its loop through a bare `fn(Task)`, as `spawn_local` is one, which
    /// captures nothing, so the only way from it to a test's own [`Pump`] is a place it can name.
    static SPAWNED: RefCell<Vec<Task>> = const { RefCell::new(Vec::new()) };
}

/// Puts `task` on this thread's event loop.
fn spawn(task: Task) {
    SPAWNED.with_borrow_mut(|spawned| spawned.push(task));
}

/// An event loop the test pumps: each task is polled when it is put on it and whenever it is woken, and never else.
#[derive(Default)]
struct Pump {
    tasks: HashMap<usize, Task>,
    woken: Arc<Mutex<Vec<usize>>>,
    made: usize,
}

/// What wakes a task: its number, put on the list of those woken.
struct Wakes {
    task: usize,
    woken: Arc<Mutex<Vec<usize>>>,
}

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.woken.lock().unwrap_or_else(PoisonError::into_inner).push(self.task);
    }
}

impl Pump {
    /// Polls each task put on the loop since, and each woken, until none is left to poll.
    fn pump(&mut self) {
        for task in SPAWNED.with_borrow_mut(mem::take) {
            self.tasks.insert(self.made, task);
            self.woken.lock().unwrap_or_else(PoisonError::into_inner).push(self.made);
            self.made += 1;
        }
        loop {
            let woken = mem::take(&mut *self.woken.lock().unwrap_or_else(PoisonError::into_inner));
            if woken.is_empty() {
                return;
            }
            for number in woken {
                let Some(task) = self.tasks.get_mut(&number) else {
                    continue;
                };
                let waker = Waker::from(Arc::new(Wakes { task: number, woken: Arc::clone(&self.woken) }));
                let is_done = task.as_mut().poll(&mut Context::from_waker(&waker)).is_ready();
                if is_done {
                    self.tasks.remove(&number);
                }
            }
        }
    }

    /// How many tasks are on the loop, not yet done.
    fn pending(&self) -> usize {
        self.tasks.len()
    }
}

/// A timer whose time the test sets, which wakes what waits once the time it waits for is set.
#[derive(Default)]
struct Hands {
    nanos: AtomicU64,
    asked: Mutex<Vec<(Time, Waker)>>,
}

impl Hands {
    /// Sets the time to `nanos`, and wakes each wait whose time has come.
    fn set(&self, nanos: u64) {
        self.nanos.store(nanos, Ordering::Release);
        let now = self.now();
        let mut asked = self.asked.lock().unwrap_or_else(PoisonError::into_inner);
        let (come, left) = mem::take(&mut *asked).into_iter().partition(|(at, _)| *at <= now);
        *asked = left;
        drop(asked);
        for (_, waker) in come {
            waker.wake();
        }
    }
}

impl Timer for Hands {
    fn now(&self) -> Time {
        Time::from_nanos(self.nanos.load(Ordering::Acquire))
    }

    fn wake_at(&self, at: Time, waker: Waker) {
        self.asked.lock().unwrap_or_else(PoisonError::into_inner).push((at, waker));
    }
}

/// A clock the engine's steps read that never moves.
struct Still;

impl Clock for Still {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An actor's empty config, and a context that holds nothing.
#[derive(Plain)]
#[kind(name = "aether.test.nothing")]
struct Nothing;

/// A test's own count of the jobs that wait for good, served to them: how many began to run, and how many of those were
/// dropped. It says more than an `Arc`'s count could, which reads the same for a job dropped and one that never began.
#[derive(Default)]
struct Forevers {
    began: AtomicUsize,
    dropped: AtomicUsize,
}

/// What a job that waits for good holds: it notes when it is dropped.
struct Guard<'job>(&'job Forevers);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.dropped.fetch_add(1, Ordering::AcqRel);
    }
}

/// A job that waits for good, until it is dropped.
#[derive(Plain)]
#[kind(name = "aether.test.forever")]
struct Forever;

/// What a job answers with.
#[derive(Plain)]
#[kind(name = "aether.test.ran")]
struct Ran;

impl Job for Forever {
    type Done = Ran;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Ran, Fault> {
        let Some(forevers) = job.service::<Forevers>() else {
            return Ok(Ran);
        };
        forevers.began.fetch_add(1, Ordering::AcqRel);
        let _guard = Guard(forevers);
        future::pending::<()>().await;

        Ok(Ran)
    }
}

/// A job for computing that waits, which its runner never polls again.
#[derive(Plain)]
#[kind(name = "aether.test.stalls")]
struct Stalls;

impl Job for Stalls {
    type Done = Ran;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Ran, Fault> {
        future::pending::<()>().await;

        Ok(Ran)
    }
}

/// What a quitter keeps of its sleep: the job it gives up on once it wakes.
#[derive(Bound)]
#[kind(name = "aether.test.gives_up")]
struct GivesUp {
    on: JobId,
}

/// An actor that, as it begins, runs a job that waits for good and a sleep. Once the sleep wakes it gives up on the
/// job, which has been polled by then, and runs another and gives up on it in the same turn, before it is polled.
#[derive(Bound)]
#[kind(name = "aether.test.quitter")]
struct Quitter;

impl ChildOf for Quitter {
    type Parent = Top;
}

impl Actor for Quitter {
    type Config = Nothing;
    type Key = ();
    type Handles = (Answer<Ran, Nothing>, Answer<Slept, GivesUp>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let on = turn.run(&Forever, &Nothing)?;
        turn.run(&Sleep { until: Time::from_nanos(10) }, &GivesUp { on })?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What a turn was answered with, as a test reads it from the engine's record (ADR-0036).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Seen {
    Ran,
    Slept,
    /// What it asked went unanswered, for this reason.
    Unanswered(Unanswered),
}

impl Seen {
    /// What `turned` was answered with: nothing for mail no test reads.
    fn of(turned: &Turned) -> Option<Self> {
        if turned.read::<Ran>().is_ok() {
            return Some(Self::Ran);
        }
        if turned.read::<Slept>().is_ok() {
            return Some(Self::Slept);
        }

        turned.read().ok().map(Self::Unanswered)
    }
}

/// What the turns recorded since this was last asked were answered with, in order.
fn seen(engine: &mut Engine) -> Vec<Seen> {
    engine.take_turned().iter().filter_map(Seen::of).collect()
}

impl Handles<Answer<Ran, Nothing>> for Quitter {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Ran>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Slept, GivesUp>> for Quitter {
    type Sends = ();

    fn handle(
        &mut self,
        (_, gives_up): (Answered<Slept>, GivesUp),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        turn.cancel(gives_up.on);
        let unpolled = turn.run(&Forever, &Nothing)?;
        turn.cancel(unpolled);

        Ok(())
    }
}

/// Brings in what the jobs finished, and steps until no actor has anything left to run, as a frame does.
fn frame(engine: &mut Engine) {
    engine.bring();
    while !engine.is_idle() {
        engine.step(Time::ZERO, &Still);
        engine.bring();
    }
}

#[test]
fn a_stopped_job_is_dropped_whether_or_not_it_was_polled_and_a_sleep_wakes_once_its_time_comes() {
    // Catches a sleep that wakes before its time or never, and a stopped job left running: one polled and waiting,
    // whose task a stop must wake to drop it, and one stopped before its task was first polled, which must never begin
    // to run.
    let hands = Arc::new(Hands::default());
    let forevers = Arc::new(Forevers::default());
    let services = Arc::new(Services::none().with(Arc::clone(&forevers)));
    let jobs = Arc::new(EventLoop::on(Arc::clone(&hands) as Arc<dyn Timer>, services, spawn));
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_jobs(jobs).ready();
    engine.record_turns();
    let Ok(_) = engine.add_root::<Quitter>(&(), Nothing) else {
        panic!("the quitter begins");
    };
    let mut pump = Pump::default();
    frame(&mut engine);
    pump.pump();
    hands.set(5);
    pump.pump();
    frame(&mut engine);
    let early = seen(&mut engine);

    hands.set(10);
    pump.pump();
    frame(&mut engine);
    pump.pump();
    frame(&mut engine);

    assert!(early.is_empty(), "nothing is answered before the sleep's time: {early:?}");
    assert_eq!(seen(&mut engine), [Seen::Slept]);
    assert_eq!(forevers.began.load(Ordering::Acquire), 1, "the job stopped before it was polled never began");
    assert_eq!(forevers.dropped.load(Ordering::Acquire), 1, "the job stopped while it waited is dropped");
    assert_eq!(pump.pending(), 0, "no task is left on the loop");
}

/// An actor that runs a job for computing that waits.
#[derive(Bound)]
#[kind(name = "aether.test.staller")]
struct Staller;

impl ChildOf for Staller {
    type Parent = Top;
}

impl Actor for Staller {
    type Config = Nothing;
    type Key = ();
    type Handles = (Answer<Ran, Nothing>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.run(&Stalls, &Nothing)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Ran, Nothing>> for Staller {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Ran>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

#[test]
fn a_job_for_computing_runs_in_a_task_of_its_own_and_one_that_waits_is_answered_that_it_waits() {
    // Catches a job for computing run inside the step that handed it out, rather than as a task of its own, and one
    // that waits run as a job that waits is, polled again for good and never answered.
    let hands: Arc<dyn Timer> = Arc::new(Hands::default());
    let jobs = Arc::new(EventLoop::on(hands, Arc::new(Services::none()), spawn));
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_jobs(jobs).ready();
    engine.record_turns();
    let Ok(id) = engine.add_root::<Staller>(&(), Nothing) else {
        panic!("the staller begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the staller is seated");
    };
    frame(&mut engine);
    let unpumped = seen(&mut engine);

    let mut pump = Pump::default();
    pump.pump();
    frame(&mut engine);

    assert!(unpumped.is_empty(), "nothing runs before the loop runs its task: {unpumped:?}");
    assert_eq!(seen(&mut engine), [Seen::Unanswered(Unanswered::Faulted)]);
    let faulted: Vec<KindId> = engine
        .log(Seated { id, seat })
        .filter_map(|entry| match entry {
            Entry::JobFaulted(fault) => Some(fault.kind()),
            _ => None,
        })
        .collect();
    assert_eq!(faulted, [KindId::of::<Waits>()]);
}
