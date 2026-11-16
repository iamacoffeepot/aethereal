//! A thread of its own that a host's service does its blocking work on, one errand at a time, in the order asked,
//! waking the job that polls each once it is done (ADR-0034): what the store's stockroom keeps its disk work on, so it
//! never blocks the thread that polls it.

use crossbeam_queue::SegQueue;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle, Thread};

/// One thread that does what it is asked in order. Dropping it stops its thread once it has finished what it was
/// doing.
pub struct Errands {
    shared: Arc<Shared>,
    /// The thread that works, which each errand asked wakes.
    worker: Thread,
    /// The thread, to join once it is told to stop: one, held as a list so it can be taken out to be joined.
    threads: Vec<JoinHandle<()>>,
}

/// One piece of work for the thread, which delivers what it came to itself.
type Work = Box<dyn FnOnce() + Send>;

/// What the errands and their thread share: the errands asked and not yet taken, and whether to stop. The queue is how
/// an errand crosses from the thread that polls it to the one that blocks, which no turn's mail reaches.
struct Shared {
    asked: SegQueue<Work>,
    stopping: AtomicBool,
}

/// Where an errand stands, as the thread that does it and the job that polls it both see it. A lock, as the two are on
/// different threads and either may come first.
enum Delivery<T> {
    /// Asked, and not yet polled.
    Asked,
    /// Polled before it was done: woken once it is.
    Polled(Waker),
    /// Done, and what it came to.
    Done(T),
    /// Handed to the job that polled it.
    Handed,
}

/// What came of an errand whose thread never delivered it, or that was polled again once handed.
pub struct Lost;

impl Errands {
    /// A thread named `name`, parked until it is asked something.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn new(name: &str) -> Self {
        let shared = Arc::new(Shared { asked: SegQueue::new(), stopping: AtomicBool::new(false) });
        let working = Arc::clone(&shared);
        #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
        let spawned = thread::Builder::new().name(String::from(name)).spawn(move || work(&working));
        let Ok(thread) = spawned else {
            panic!("the host makes a thread for `{name}`");
        };

        Self { shared, worker: thread.thread().clone(), threads: vec![thread] }
    }

    /// Asks the thread to do `errand`, and answers with what it comes to, once it has.
    pub fn ask<T: Send + 'static>(&self, errand: impl FnOnce() -> T + Send + 'static) -> Errand<T> {
        let delivery = Arc::new(Mutex::new(Delivery::Asked));
        let delivering = Arc::clone(&delivery);
        self.shared.asked.push(Box::new(move || deliver(&delivering, errand())));
        self.worker.unpark();

        Errand { delivery }
    }
}

impl Drop for Errands {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        self.worker.unpark();
        for thread in self.threads.drain(..) {
            // A thread that panicked has nothing left to do.
            let _ = thread.join();
        }
    }
}

/// Does each errand until it is told to stop, parking whenever there is none to take.
fn work(shared: &Shared) {
    loop {
        while let Some(work) = shared.asked.pop() {
            work();
        }
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        thread::park();
    }
}

/// Hands `done` to the errand `delivery` tracks, waking the job that polled it.
fn deliver<T>(delivery: &Mutex<Delivery<T>>, done: T) {
    let was = mem::replace(&mut *delivered(delivery), Delivery::Done(done));
    if let Delivery::Polled(waker) = was {
        waker.wake();
    }
}

/// Where an errand stands, whatever a thread that panicked holding it left.
fn delivered<T>(delivery: &Mutex<Delivery<T>>) -> MutexGuard<'_, Delivery<T>> {
    delivery.lock().unwrap_or_else(PoisonError::into_inner)
}

/// An errand, as the job that asked it polls it.
pub struct Errand<T> {
    delivery: Arc<Mutex<Delivery<T>>>,
}

impl<T> Future for Errand<T> {
    type Output = Result<T, Lost>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut delivery = delivered(&self.delivery);
        let was = mem::replace(&mut *delivery, Delivery::Handed);
        match was {
            Delivery::Done(done) => Poll::Ready(Ok(done)),
            Delivery::Asked | Delivery::Polled(_) => {
                *delivery = Delivery::Polled(context.waker().clone());
                Poll::Pending
            }
            Delivery::Handed => Poll::Ready(Err(Lost)),
        }
    }
}
