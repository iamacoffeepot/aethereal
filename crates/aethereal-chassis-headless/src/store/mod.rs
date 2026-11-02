//! The native host's stockroom: the store kept in a directory, its disk worked on a thread of its own (ADR-0039).

mod disk;

use crate::Headless;
use aethereal_actor::Service;
use aethereal_store::{
    Copying, Keeping, RecordFile, Recounting, Stockroom, Store, StoreHash, StoreSettings, Uncopied, Unkept,
};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use crossbeam_queue::SegQueue;
use std::future::Future;
use std::mem;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle, Thread};

/// The native host's stockroom (ADR-0039): the store's directory, `entries/` holding each entry's bytes as `<hash>` and
/// its record as `<hash>.entry`, beside the `names` file. What it is asked is done on a thread of its own, in the order
/// asked, so the disk never blocks the thread that polls it, and woken once done. One thread is enough: the store asks
/// one thing at a time. Dropping it stops its thread once it has finished what it was doing.
pub struct StoreDirectory {
    /// The store's directory.
    root: PathBuf,
    shared: Arc<Shared>,
    /// The thread that works the disk, which each errand asked wakes.
    worker: Thread,
    /// The thread, to join once it is told to stop: one, held as a list so it can be taken out to be joined.
    threads: Vec<JoinHandle<()>>,
    /// The number the next copy's file is written under, until it is renamed to its hash. An atomic, as the store's
    /// jobs ask from the runners' threads, whichever polls them; a copy from a run before that crashed is removed by the
    /// recount, so the numbers need not outlive the process.
    next_copy: AtomicU64,
}

/// One piece of work for the disk, which delivers what it came to itself.
type Work = Box<dyn FnOnce() + Send>;

/// What the stockroom and its thread share: the errands asked and not yet taken, and whether to stop. The queue is how
/// an errand crosses from the thread that polls it to the one that blocks on the disk, which no turn's mail reaches.
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
struct Lost;

impl StoreDirectory {
    /// The stockroom keeping the store in the directory `settings` names, made as it is first used.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn new(settings: &StoreSettings) -> Self {
        let shared = Arc::new(Shared { asked: SegQueue::new(), stopping: AtomicBool::new(false) });
        let working = Arc::clone(&shared);
        #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
        let spawned = thread::Builder::new().name(String::from("aethereal-store")).spawn(move || work(&working));
        let Ok(thread) = spawned else {
            panic!("the host makes a thread to keep the store");
        };

        Self {
            root: PathBuf::from(&settings.root),
            shared,
            worker: thread.thread().clone(),
            threads: vec![thread],
            next_copy: AtomicU64::new(0),
        }
    }

    /// Asks the thread to do `errand`, and answers with what it comes to, once it has.
    fn ask<T: Send + 'static>(&self, errand: impl FnOnce() -> T + Send + 'static) -> Errand<T> {
        let delivery = Arc::new(Mutex::new(Delivery::Asked));
        let delivering = Arc::clone(&delivery);
        self.shared.asked.push(Box::new(move || deliver(&delivering, errand())));
        self.worker.unpark();

        Errand { delivery }
    }
}

impl Stockroom for StoreDirectory {
    fn copy(&self, path: String, most_bytes: u64) -> Copying<'_> {
        let root = self.root.clone();
        let number = self.next_copy.fetch_add(1, Ordering::Relaxed);
        let copying = self.ask(move || disk::copy(&root, &PathBuf::from(path), most_bytes, number));

        Box::pin(async move { copying.await.unwrap_or(Err(Uncopied::Failed)) })
    }

    fn count(&self) -> Recounting<'_> {
        let root = self.root.clone();
        let counting = self.ask(move || disk::count(&root));

        Box::pin(async move { counting.await.unwrap_or(Err(Unkept)) })
    }

    fn write(&self, records: Vec<RecordFile>, names: Vec<u8>) -> Keeping<'_> {
        let root = self.root.clone();
        let writing = self.ask(move || disk::write(&root, &records, &names));

        Box::pin(async move { writing.await.unwrap_or(Err(Unkept)) })
    }

    fn prune(&self, hashes: Vec<StoreHash>) -> Keeping<'_> {
        let root = self.root.clone();
        let pruning = self.ask(move || disk::prune(&root, &hashes));

        Box::pin(async move { pruning.await.unwrap_or(Err(Unkept)) })
    }
}

impl Drop for StoreDirectory {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        self.worker.unpark();
        for thread in self.threads.drain(..) {
            // A thread that panicked has nothing left to do.
            let _ = thread.join();
        }
    }
}

/// The store on the native host: its stockroom the directory its settings name, served to the host for its runners
/// (ADR-0039).
impl Bootstrap<Headless> for Store {
    const SECTION: &'static str = "store";
    type Settings = StoreSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(
        settings: StoreSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Headless, Self>,
    ) -> Result<(), Unbuilt> {
        cx.serve(Service::<dyn Stockroom>::of(Arc::new(StoreDirectory::new(&settings))));
        cx.begin(&(), settings.config())
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
struct Errand<T> {
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
