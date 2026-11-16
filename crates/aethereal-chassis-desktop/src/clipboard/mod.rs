//! The desktop's clipboard (ADR-0024: The clipboard): the system's, through `arboard`, read and written as plain text
//! on a thread of its own, which holds the one handle to it for the program's life, so a read that waits on another
//! program never blocks the thread that polls the job, and text copied stays offered on X11 while the program runs.

use aethereal_window::{Clipboard, ClipboardRead, ClipboardReading, ClipboardWriting, ClipboardWritten};
use arboard::Error;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::thread::{self, JoinHandle};

/// The system clipboard, served to the desktop's jobs. Dropping it stops its thread once that has done what it was
/// asked.
pub struct SystemClipboard {
    /// Where its thread is asked to read and write. A lock, as the jobs that ask are on the runners' threads and a
    /// sender is shared among them.
    asks: Mutex<Sender<Asked>>,
    /// Its thread, joined as it is dropped.
    thread: Running,
}

/// Whether its thread runs.
enum Running {
    /// It runs, until every sender is dropped.
    Thread(JoinHandle<()>),
    /// The host could not make it: every read and write is answered unavailable.
    Unmade,
}

/// One read or write asked of the thread, and where what it comes to goes.
enum Asked {
    Read(Shelf<ClipboardRead>),
    Write(String, Shelf<ClipboardWritten>),
}

/// Where what a read or a write comes to goes, shared by the thread that does it and the job that polls it. A lock, as
/// the two are on different threads and either may come first.
type Shelf<T> = Arc<Mutex<Delivery<T>>>;

/// Where a read or a write stands.
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

/// What a read or a write comes to when its thread is gone or it is polled again once handed: the host has no
/// clipboard to answer it.
trait Gone {
    const GONE: Self;
}

impl Gone for ClipboardRead {
    const GONE: Self = Self::Unavailable;
}

impl Gone for ClipboardWritten {
    const GONE: Self = Self::Unavailable;
}

impl SystemClipboard {
    /// The system clipboard, its thread started.
    #[must_use]
    pub fn new() -> Self {
        let (asks, asked) = mpsc::channel();
        #[allow(clippy::disallowed_methods)] // The native host owns the real threads (ADR-0001).
        let spawned = thread::Builder::new().name(String::from("aethereal-clipboard")).spawn(move || serve(&asked));
        let thread = spawned.map_or(Running::Unmade, Running::Thread);

        Self { asks: Mutex::new(asks), thread }
    }

    /// Hands `asked`, made with its shelf, to the thread, and answers with the delivery the job polls.
    fn ask<T: Gone>(&self, asked: impl FnOnce(Shelf<T>) -> Asked) -> Delivered<T> {
        let held = Arc::new(Mutex::new(Delivery::Asked));
        let sent = match &self.thread {
            Running::Thread(_) => {
                let sender = self.asks.lock().unwrap_or_else(PoisonError::into_inner);
                sender.send(asked(Arc::clone(&held))).is_ok()
            }
            Running::Unmade => false,
        };
        if !sent {
            *held.lock().unwrap_or_else(PoisonError::into_inner) = Delivery::Done(T::GONE);
        }

        Delivered { shelf: held }
    }
}

impl Default for SystemClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SystemClipboard {
    /// Stops its thread, which ends once every sender is gone and it has answered what it was asked.
    fn drop(&mut self) {
        let (closed, _) = mpsc::channel();
        drop(mem::replace(self.asks.get_mut().unwrap_or_else(PoisonError::into_inner), closed));
        if let Running::Thread(thread) = mem::replace(&mut self.thread, Running::Unmade) {
            thread.join().ok();
        }
    }
}

impl Clipboard for SystemClipboard {
    fn read(&self) -> ClipboardReading<'_> {
        Box::pin(self.ask(Asked::Read))
    }

    fn write(&self, text: String) -> ClipboardWriting<'_> {
        Box::pin(self.ask(|shelf| Asked::Write(text, shelf)))
    }
}

/// The thread's work: opens the system clipboard once, then does each read and write asked, in order, until every
/// sender is gone. A clipboard that does not open, as with no display to reach, answers each unavailable.
fn serve(asked: &Receiver<Asked>) {
    let mut opened = arboard::Clipboard::new();
    for ask in asked {
        match (ask, &mut opened) {
            (Asked::Read(shelf), Ok(clipboard)) => deliver(&shelf, read(clipboard.get_text())),
            (Asked::Write(text, shelf), Ok(clipboard)) => deliver(&shelf, written(&clipboard.set_text(text))),
            (Asked::Read(shelf), Err(_)) => deliver(&shelf, ClipboardRead::Unavailable),
            (Asked::Write(_, shelf), Err(_)) => deliver(&shelf, ClipboardWritten::Unavailable),
        }
    }
}

/// What a read that came to `got` comes to.
fn read(got: Result<String, Error>) -> ClipboardRead {
    match got {
        Ok(text) => ClipboardRead::Text(text),
        Err(Error::ContentNotAvailable) => ClipboardRead::Empty,
        Err(Error::ClipboardNotSupported) => ClipboardRead::Unavailable,
        Err(_) => ClipboardRead::Refused,
    }
}

/// What a write that came to `set` comes to.
fn written(set: &Result<(), Error>) -> ClipboardWritten {
    match set {
        Ok(()) => ClipboardWritten::Written,
        Err(Error::ClipboardNotSupported) => ClipboardWritten::Unavailable,
        Err(_) => ClipboardWritten::Refused,
    }
}

/// Puts `done` on `shelf`, waking the job that polled it.
fn deliver<T>(shelf: &Shelf<T>, done: T) {
    let was = mem::replace(&mut *shelf.lock().unwrap_or_else(PoisonError::into_inner), Delivery::Done(done));
    if let Delivery::Polled(waker) = was {
        waker.wake();
    }
}

/// A read or a write as its job polls it.
struct Delivered<T> {
    shelf: Shelf<T>,
}

impl<T: Gone> Future for Delivered<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<T> {
        let mut delivery = self.shelf.lock().unwrap_or_else(PoisonError::into_inner);
        match mem::replace(&mut *delivery, Delivery::Handed) {
            Delivery::Done(done) => Poll::Ready(done),
            Delivery::Asked | Delivery::Polled(_) => {
                *delivery = Delivery::Polled(context.waker().clone());
                Poll::Pending
            }
            Delivery::Handed => Poll::Ready(T::GONE),
        }
    }
}
