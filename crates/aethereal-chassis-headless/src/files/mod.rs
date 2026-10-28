//! The native host's storage: a directory for each namespace, read on threads of its own (ADR-0032).

#[cfg(test)]
mod tests;

use crate::Headless;
use aethereal_actor::Service;
use aethereal_data::{FilePath, Plain};
use aethereal_files::{Files, FilesConfig, Namespace, Reading, Storage, Unread};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use crossbeam_queue::SegQueue;
use std::fs;
use std::future::Future;
use std::io::ErrorKind;
use std::mem;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle, Thread};

/// Where the native host reads each namespace from, and how many threads read: the file capability's section, `files`
/// (ADR-0028, ADR-0032).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.files.directories")]
#[settings]
pub struct FileRoots {
    /// The directory the program's assets are read from: relative to where the program runs, or absolute.
    #[flag]
    pub assets: String,
    /// How many threads read files.
    #[flag]
    pub readers: u32,
}

impl Default for FileRoots {
    fn default() -> Self {
        Self { assets: String::from("assets"), readers: 4 }
    }
}

/// The native host's storage (ADR-0032): each namespace a directory, each read done on one of threads of its own, so
/// a read never blocks the thread that polls it, and woken once its bytes are in. Dropping the last reference to it
/// stops its threads once each has finished what it was reading.
pub struct Directories {
    /// The directory of the program's assets.
    assets: PathBuf,
    shared: Arc<Shared>,
    /// The threads that read, which the reads asked wake in turn.
    readers: Vec<Thread>,
    /// Which of them the next read asked wakes.
    next: AtomicUsize,
    /// Every thread, to join once they are told to stop.
    threads: Vec<JoinHandle<()>>,
}

/// What the threads share: the reads asked and not yet taken, and whether to stop. The queue is how a read crosses from
/// the thread that polls it to one that blocks on the disk, which no turn's mail reaches.
struct Shared {
    asked: SegQueue<Asked>,
    stopping: AtomicBool,
}

/// One read asked: the file, and where its bytes go.
struct Asked {
    file: PathBuf,
    delivery: Arc<Mutex<Delivery>>,
}

/// Where a read stands, as the thread that reads it and the job that polls it both see it. A lock, as the two are on
/// different threads and either may come first.
enum Delivery {
    /// Asked, and not yet polled.
    Asked,
    /// Polled before its bytes were in: woken once they are.
    Polled(Waker),
    /// Read, its bytes or why not.
    Read(Result<Vec<u8>, Unread>),
    /// Handed to the job that polled it.
    Handed,
}

impl Directories {
    /// The storage reading the directories `roots` names, on as many threads as it says, one at the least.
    ///
    /// # Panics
    ///
    /// Panics when the host cannot make a thread.
    #[must_use]
    pub fn new(roots: &FileRoots) -> Self {
        let count = usize::try_from(roots.readers).ok().and_then(NonZeroUsize::new).unwrap_or(NonZeroUsize::MIN);
        let shared = Arc::new(Shared { asked: SegQueue::new(), stopping: AtomicBool::new(false) });
        let threads: Vec<JoinHandle<()>> = (0..count.get())
            .map(|number| {
                let shared = Arc::clone(&shared);
                #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
                let spawned =
                    thread::Builder::new().name(format!("aethereal-files-{number}")).spawn(move || read(&shared));
                let Ok(thread) = spawned else {
                    panic!("the host makes a thread to read files");
                };
                thread
            })
            .collect();
        let readers = threads.iter().map(|thread| thread.thread().clone()).collect();

        Self { assets: PathBuf::from(&roots.assets), shared, readers, next: AtomicUsize::new(0), threads }
    }

    /// The directory `namespace` is read from.
    fn root(&self, namespace: Namespace) -> &PathBuf {
        match namespace {
            Namespace::Assets => &self.assets,
        }
    }
}

impl Storage for Directories {
    fn read<'a>(&'a self, namespace: Namespace, path: &'a FilePath) -> Reading<'a> {
        let delivery = Arc::new(Mutex::new(Delivery::Asked));
        let file = self.root(namespace).join(path.as_str());
        self.shared.asked.push(Asked { file, delivery: Arc::clone(&delivery) });
        let reader = self.next.fetch_add(1, Ordering::Relaxed) % self.readers.len();
        self.readers[reader].unpark();

        Box::pin(Read { delivery })
    }
}

impl Drop for Directories {
    fn drop(&mut self) {
        self.shared.stopping.store(true, Ordering::Release);
        for reader in &self.readers {
            reader.unpark();
        }
        for thread in self.threads.drain(..) {
            // A thread that panicked has nothing left to read.
            let _ = thread.join();
        }
    }
}

/// The file capability on the native host: its storage the directories its settings name, served to the host for its
/// runners (ADR-0032).
impl Bootstrap<Headless> for Files {
    const SECTION: &'static str = "files";
    type Settings = FileRoots;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(roots: FileRoots, (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.serve(Service::<dyn Storage>::of(Arc::new(Directories::new(&roots))));
        cx.begin(&(), FilesConfig)
    }
}

/// Takes reads and does each until it is told to stop, parking whenever there is none to take.
fn read(shared: &Shared) {
    loop {
        while let Some(Asked { file, delivery }) = shared.asked.pop() {
            let read = fs::read(&file).map_err(|error| unread(error.kind()));
            let was = mem::replace(&mut *delivered(&delivery), Delivery::Read(read));
            if let Delivery::Polled(waker) = was {
                waker.wake();
            }
        }
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        thread::park();
    }
}

/// Why a file was not read, as the disk said.
fn unread(kind: ErrorKind) -> Unread {
    match kind {
        ErrorKind::NotFound | ErrorKind::IsADirectory | ErrorKind::NotADirectory => Unread::Missing,
        _ => Unread::Failed,
    }
}

/// Where a read stands, whatever a thread that panicked holding it left.
fn delivered(delivery: &Mutex<Delivery>) -> MutexGuard<'_, Delivery> {
    delivery.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A read, as the job that asked it polls it.
struct Read {
    delivery: Arc<Mutex<Delivery>>,
}

impl Future for Read {
    type Output = Result<Vec<u8>, Unread>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut delivery = delivered(&self.delivery);
        let was = mem::replace(&mut *delivery, Delivery::Handed);
        match was {
            Delivery::Read(read) => Poll::Ready(read),
            Delivery::Asked | Delivery::Polled(_) => {
                *delivery = Delivery::Polled(context.waker().clone());
                Poll::Pending
            }
            Delivery::Handed => Poll::Ready(Err(Unread::Failed)),
        }
    }
}
