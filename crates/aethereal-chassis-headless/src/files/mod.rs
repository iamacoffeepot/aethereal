//! The native host's storage: a directory for each namespace, read and written on threads of its own (ADR-0032,
//! ADR-0039).

#[cfg(test)]
mod tests;

use crate::Headless;
use aethereal_actor::{Service, Spares};
use aethereal_data::{FilePath, Plain};
use aethereal_files::{Files, FilesConfig, Namespace, Read, Reading, Storage, Unread, Unwritten, Writing};
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use crossbeam_queue::SegQueue;
use std::fs::{self, File};
use std::future::{self, Future};
use std::io::{self, ErrorKind, Read as _, Write as _};
use std::mem;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle, Thread};

/// Where the native host reads and writes each namespace, and how many threads do it: the file capability's section,
/// `files` (ADR-0028, ADR-0032, ADR-0039).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.files.directories")]
#[settings]
pub struct FileRoots {
    /// The directory the program's assets are read from: relative to where the program runs, or absolute.
    #[flag]
    pub assets: String,
    /// The directory the program's saves are read from and written to: relative to where the program runs, or
    /// absolute.
    #[flag]
    pub save: String,
    /// How many threads read and write files.
    #[flag]
    pub readers: u32,
}

impl Default for FileRoots {
    fn default() -> Self {
        Self { assets: String::from("assets"), save: String::from("save"), readers: 4 }
    }
}

/// The native host's storage (ADR-0032, ADR-0039): each namespace a directory, each read and write done on one of
/// threads of its own, so neither blocks the thread that polls it, and woken once it is done. A write goes to a
/// sibling of its file, synced, and renamed over the file, so a reader sees the file as it was or as it is written,
/// never half of it. Dropping the last reference to it stops its threads once each has finished what it was doing.
pub struct Directories {
    /// The directory of the program's assets.
    assets: PathBuf,
    /// The directory of the program's saves.
    save: PathBuf,
    shared: Arc<Shared>,
    /// The threads that read and write, which the work asked wakes in turn.
    readers: Vec<Thread>,
    /// The number of the next read or write asked, which names the thread it wakes and a write's sibling.
    next: AtomicUsize,
    /// Every thread, to join once they are told to stop.
    threads: Vec<JoinHandle<()>>,
}

/// What the threads share: the reads and writes asked and not yet taken, and whether to stop. The queue is how each
/// crosses from the thread that polls it to one that blocks on the disk, which no turn's mail reaches.
struct Shared {
    asked: SegQueue<Asked>,
    stopping: AtomicBool,
}

/// Where what a read or a write comes to goes, shared by the thread that does it and the job that polls it.
type Shelf<T> = Arc<Mutex<Delivery<T>>>;

/// One piece of work asked of the threads, and where what it comes to goes.
enum Asked {
    /// Read the whole of `file` into a buffer taken from `lent`.
    Read { file: PathBuf, lent: Spares, delivery: Shelf<Read> },
    /// Write `bytes` as the whole of `file`, by way of `sibling`.
    Write { file: PathBuf, sibling: PathBuf, bytes: Vec<u8>, delivery: Shelf<Result<(), Unwritten>> },
}

/// Where a read or a write stands, as the thread that does it and the job that polls it both see it. A lock, as the
/// two are on different threads and either may come first.
enum Delivery<T> {
    /// Asked, and not yet polled.
    Asked,
    /// Polled before it was done: woken once it is.
    Polled(Waker),
    /// Done, with what it came to.
    Done(T),
    /// Handed to the job that polled it.
    Handed,
}

/// What a read or a write comes to, with what one polled again once handed answers.
trait Outcome {
    /// What a delivery polled once it was handed answers: a failure, as it has nothing left to give.
    const HANDED: Self;
}

impl Outcome for Read {
    const HANDED: Self = Self { bytes: Err(Unread::Failed), lent: Spares::new() };
}

impl Outcome for Result<(), Unwritten> {
    const HANDED: Self = Err(Unwritten::Failed);
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
                    thread::Builder::new().name(format!("aethereal-files-{number}")).spawn(move || work(&shared));
                let Ok(thread) = spawned else {
                    panic!("the host makes a thread to read files");
                };
                thread
            })
            .collect();
        let readers = threads.iter().map(|thread| thread.thread().clone()).collect();

        Self {
            assets: PathBuf::from(&roots.assets),
            save: PathBuf::from(&roots.save),
            shared,
            readers,
            next: AtomicUsize::new(0),
            threads,
        }
    }

    /// The directory `namespace` is read from.
    fn root(&self, namespace: Namespace) -> &PathBuf {
        match namespace {
            Namespace::Assets => &self.assets,
            Namespace::Save => &self.save,
        }
    }

    /// Hands the work `asked` makes, given its number, to a thread, and answers with its delivery as the job polls it.
    fn ask<T>(&self, asked: impl FnOnce(usize, Shelf<T>) -> Asked) -> Delivered<T> {
        let delivery = Arc::new(Mutex::new(Delivery::Asked));
        let number = self.next.fetch_add(1, Ordering::Relaxed);
        self.shared.asked.push(asked(number, Arc::clone(&delivery)));
        self.readers[number % self.readers.len()].unpark();

        Delivered { delivery }
    }
}

impl Storage for Directories {
    fn read<'a>(&'a self, namespace: Namespace, path: &'a FilePath, lent: Spares) -> Reading<'a> {
        let file = self.root(namespace).join(path.as_str());

        Box::pin(self.ask(|_, delivery| Asked::Read { file, lent, delivery }))
    }

    fn write<'a>(&'a self, namespace: Namespace, path: &'a FilePath, bytes: Vec<u8>) -> Writing<'a> {
        if !namespace.is_written() {
            return Box::pin(future::ready(Err(Unwritten::ReadOnly)));
        }
        let file = self.root(namespace).join(path.as_str());

        Box::pin(self.ask(|number, delivery| Asked::Write { sibling: sibling(&file, number), file, bytes, delivery }))
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

/// Takes reads and writes and does each until it is told to stop, parking whenever there is none to take.
fn work(shared: &Shared) {
    loop {
        while let Some(asked) = shared.asked.pop() {
            match asked {
                Asked::Read { file, mut lent, delivery } => {
                    let bytes = read_whole(&file, &mut lent).map_err(|error| unread(error.kind()));
                    deliver(&delivery, Read { bytes, lent });
                }
                Asked::Write { file, sibling, bytes, delivery } => {
                    deliver(&delivery, write_whole(&file, &sibling, &bytes).map_err(|_| Unwritten::Failed));
                }
            }
        }
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        thread::park();
    }
}

/// Hands `done` to the job polling `delivery`, waking it when it polled first.
fn deliver<T>(delivery: &Mutex<Delivery<T>>, done: T) {
    let was = mem::replace(&mut *delivered(delivery), Delivery::Done(done));
    if let Delivery::Polled(waker) = was {
        waker.wake();
    }
}

/// The sibling of `file` a write numbered `number` goes to before it is renamed over `file`: in the same directory, so
/// the rename replaces the file in one step, and named for this process and the write, so two writes never share one.
fn sibling(file: &Path, number: usize) -> PathBuf {
    let name = file.file_name().map_or_else(Default::default, |name| name.to_string_lossy().into_owned());

    file.with_file_name(format!(".{name}.{}-{number}.saving", process::id()))
}

/// Reads the whole of `file` into a buffer taken from `lent` for its size, as `fs::read` reads one into a new buffer:
/// the size is a hint, and a file that grows as it is read grows the buffer. A buffer taken for a read that failed is
/// kept in `lent` again.
fn read_whole(file: &Path, lent: &mut Spares) -> io::Result<Vec<u8>> {
    let mut opened = File::open(file)?;
    // A size the host cannot hold is no hint: the read fails as it runs out of room.
    let size = opened.metadata().map_or(0, |metadata| usize::try_from(metadata.len()).unwrap_or(0));
    let mut bytes = lent.take(size);
    let read = opened.read_to_end(&mut bytes);
    if let Err(error) = read {
        lent.keep(bytes, usize::MAX);
        return Err(error);
    }

    Ok(bytes)
}

/// Writes `bytes` as the whole of `file`: makes the directories it is in, writes `sibling` and syncs it, and renames it
/// over `file`. A sibling that was not renamed is removed.
fn write_whole(file: &Path, sibling: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = file.parent().ok_or_else(|| io::Error::from(ErrorKind::InvalidInput))?;
    fs::create_dir_all(directory)?;
    let written = write_synced(sibling, bytes).and_then(|()| fs::rename(sibling, file));
    if written.is_err() {
        // The sibling may not have been made; either way, nothing of it is left.
        let _ = fs::remove_file(sibling);
    }
    written?;
    // The directory is synced too, so the rename outlasts a crash; a host that is not Unix keeps it with the file.
    #[cfg(unix)]
    File::open(directory)?.sync_all()?;

    Ok(())
}

/// Writes `bytes` as a new file at `file`, and syncs it to the disk before it is renamed into place.
fn write_synced(file: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut made = File::options().write(true).create_new(true).open(file)?;
    made.write_all(bytes)?;

    made.sync_all()
}

/// Why a file was not read, as the disk said.
fn unread(kind: ErrorKind) -> Unread {
    match kind {
        ErrorKind::NotFound | ErrorKind::IsADirectory | ErrorKind::NotADirectory => Unread::Missing,
        _ => Unread::Failed,
    }
}

/// Where a read or a write stands, whatever a thread that panicked holding it left.
fn delivered<T>(delivery: &Mutex<Delivery<T>>) -> MutexGuard<'_, Delivery<T>> {
    delivery.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A read or a write, as the job that asked it polls it.
struct Delivered<T> {
    delivery: Shelf<T>,
}

impl<T: Outcome> Future for Delivered<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        let mut delivery = delivered(&self.delivery);
        let was = mem::replace(&mut *delivery, Delivery::Handed);
        match was {
            Delivery::Done(done) => Poll::Ready(done),
            Delivery::Asked | Delivery::Polled(_) => {
                *delivery = Delivery::Polled(context.waker().clone());
                Poll::Pending
            }
            Delivery::Handed => Poll::Ready(T::HANDED),
        }
    }
}
