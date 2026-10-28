//! The native host's runner for waiting: one thread that polls each job it holds whenever it is woken, and the timer
//! those jobs wait on (ADR-0018).

use crate::Monotonic;
use aethereal_actor::{Driven, Fault, JobId, JobType, Services, Time, Timer};
use aethereal_data::{Kind, KindId};
use aethereal_substrate::{Clock, Finish, Panicked, Taken};
use crossbeam_queue::SegQueue;
use std::collections::{BTreeMap, HashMap};
use std::panic::{self, AssertUnwindSafe};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, JoinHandle, Thread};
use std::time::Duration;

/// One thread that runs the jobs that wait, so a job waiting on time holds no thread a computation could use. Each is
/// polled when it is handed out and whenever its waker wakes, and the thread parks until the next is woken or the
/// next time a job waits for comes.
///
/// What it is handed, asked to stop, and woken for comes to it on queues no one waits on.
pub(super) struct Io {
    shared: Arc<Shared>,
    thread: Thread,
}

/// What the thread shares with the engine's side, and with the wakers of its jobs.
struct Shared {
    /// The services the host serves its jobs (ADR-0034).
    services: Arc<Services>,
    handed: SegQueue<(JobId, JobType, Taken)>,
    stopped: SegQueue<JobId>,
    /// The jobs woken, by their keys.
    woken: Arc<SegQueue<Key>>,
    timer: Arc<Timers>,
    stopping: AtomicBool,
}

/// A job's key among those the thread holds: its actor's id and its count.
type Key = (u64, u64);

/// The host's timer, as the thread's jobs wait on it: what they ask to be woken at comes to the thread on a queue.
struct Timers {
    clock: Monotonic,
    asked: SegQueue<(Time, Waker)>,
}

impl Timer for Timers {
    fn now(&self) -> Time {
        Time::from_nanos(self.clock.nanos())
    }

    fn wake_at(&self, at: Time, waker: Waker) {
        self.asked.push((at, waker));
    }
}

/// A job the thread holds: its future, where its outcome goes, and the waker it is polled with.
struct Held {
    running: Driven,
    finish: Finish,
    waker: Waker,
}

/// What wakes a job: its key, put on the queue of those woken, and the thread unparked to poll it.
struct Wakes {
    key: Key,
    woken: Arc<SegQueue<Key>>,
    thread: Thread,
}

impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.woken.push(self.key);
        self.thread.unpark();
    }
}

impl Io {
    /// The runner of the jobs that wait, its timer on `clock`, its jobs reaching the host's `services`, and its thread,
    /// to join once it is told to stop.
    pub(super) fn new(clock: Monotonic, services: Arc<Services>) -> (Self, JoinHandle<()>) {
        let timer = Arc::new(Timers { clock, asked: SegQueue::new() });
        let shared = Arc::new(Shared {
            services,
            handed: SegQueue::new(),
            stopped: SegQueue::new(),
            woken: Arc::new(SegQueue::new()),
            timer,
            stopping: AtomicBool::new(false),
        });
        let running = Arc::clone(&shared);
        #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
        let spawned = thread::Builder::new().name(String::from("aethereal-io")).spawn(move || run(&running));
        let Ok(joined) = spawned else {
            panic!("the host makes a thread for jobs that wait");
        };

        (Self { shared, thread: joined.thread().clone() }, joined)
    }

    /// Hands it the job `id`, of the type `of`.
    pub(super) fn start(&self, id: JobId, of: JobType, job: Taken) {
        self.shared.handed.push((id, of, job));
        self.thread.unpark();
    }

    /// Drops the job `id`, when it holds it: its future is dropped, and it goes unanswered.
    pub(super) fn stop(&self, id: JobId) {
        self.shared.stopped.push(id);
        self.thread.unpark();
    }

    /// Tells the thread to stop, dropping the jobs it holds, which go unanswered.
    pub(super) fn end(&self) {
        self.shared.stopping.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

/// The key of the job `id`.
const fn key(id: JobId) -> Key {
    let mail = id.mail();

    (mail.sender.bits(), mail.count)
}

/// Runs the jobs it is handed, polling each when it is woken, until it is told to stop.
fn run(shared: &Arc<Shared>) {
    let mut held: HashMap<Key, Held> = HashMap::new();
    // What the jobs wait for, by the time and the order they asked, so two at one time are both kept.
    let mut waits: BTreeMap<(Time, u64), Waker> = BTreeMap::new();
    let mut asked = 0;
    loop {
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        while let Some((id, of, job)) = shared.handed.pop() {
            hold(shared, &mut held, id, of, job);
        }
        while let Some(id) = shared.stopped.pop() {
            drop(held.remove(&key(id)));
        }
        while let Some(key) = shared.woken.pop() {
            poll(&mut held, key);
        }
        while let Some((at, waker)) = shared.timer.asked.pop() {
            asked += 1;
            waits.insert((at, asked), waker);
        }
        let now = shared.timer.now();
        while let Some(entry) = waits.first_entry() {
            let has_come = entry.key().0 <= now;
            if !has_come {
                break;
            }
            entry.remove().wake();
        }
        let is_woken = !shared.woken.is_empty();
        if is_woken {
            continue;
        }
        // Anything handed, stopped, or woken from now unparks this thread, and then this returns at once.
        match waits.first_key_value() {
            Some(((at, _), _)) => thread::park_timeout(Duration::from_nanos(at.nanos().saturating_sub(now.nanos()))),
            None => thread::park(),
        }
    }
}

/// Holds the job `id` of the type `of`, to be polled first now.
fn hold(shared: &Arc<Shared>, held: &mut HashMap<Key, Held>, id: JobId, of: JobType, job: Taken) {
    let Taken { bytes, lent, limit, finish } = job;
    let timer: Arc<dyn Timer> = Arc::clone(&shared.timer) as Arc<dyn Timer>;
    let running = of.drive(bytes, lent, limit, timer, Arc::clone(&shared.services));
    let waker =
        Waker::from(Arc::new(Wakes { key: key(id), woken: Arc::clone(&shared.woken), thread: thread::current() }));
    held.insert(key(id), Held { running, finish, waker });
    shared.woken.push(key(id));
}

/// Polls the job of `key`, when it is held, answering it once it is ready or has panicked.
fn poll(held: &mut HashMap<Key, Held>, key: Key) {
    let Some(job) = held.get_mut(&key) else {
        return;
    };
    let mut context = Context::from_waker(&job.waker);
    let polled = panic::catch_unwind(AssertUnwindSafe(|| job.running.as_mut().poll(&mut context)));
    let outcome = match polled {
        Ok(Poll::Pending) => return,
        Ok(Poll::Ready(outcome)) => outcome,
        Err(_) => Err(fault::<Panicked>()),
    };
    if let Some(job) = held.remove(&key) {
        job.finish.done(outcome);
    }
}

/// The fault whose reason is `K`, which holds nothing.
fn fault<K: Kind>() -> Fault {
    Fault::of_parts(KindId::of::<K>(), Vec::new())
}
