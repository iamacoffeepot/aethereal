//! The tail of a program's stderr: its last bytes, kept as a thread of its own reads the pipe (ADR-0040).

use std::collections::VecDeque;
use std::io::{ErrorKind, Read};
use std::process::ChildStderr;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The last bytes a program wrote to its stderr, at most `most` of them, and whether the pipe has ended. A lock, as
/// the thread that reads the pipe fills it and the thread that waited on the program reads it once the program has
/// exited.
pub struct Tail {
    kept: Mutex<Kept>,
    /// Woken as the pipe ends.
    ended: Condvar,
    most: usize,
}

/// What a tail holds.
struct Kept {
    bytes: VecDeque<u8>,
    /// Whether the pipe has ended: the program, and every program it started, have closed it.
    ended: bool,
}

impl Tail {
    /// A tail that keeps at most `most` bytes, nothing read yet.
    pub fn new(most: usize) -> Self {
        Self { kept: Mutex::new(Kept { bytes: VecDeque::new(), ended: false }), ended: Condvar::new(), most }
    }

    /// Reads `stderr` until it ends, keeping its last bytes, then marks the pipe ended.
    pub fn read(&self, mut stderr: ChildStderr) {
        let mut buffer = [0; 4096];
        loop {
            let read = stderr.read(&mut buffer);
            match read {
                Ok(0) => break,
                Ok(bytes) => self.keep(&buffer[..bytes]),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        self.kept().ended = true;
        self.ended.notify_all();
    }

    /// The bytes kept, as text, a character the cut split replaced, once the pipe has ended, or once `within` has
    /// passed: what a program wrote before it exited is in the pipe and read at once, unless a program it started holds
    /// the pipe open.
    pub fn text(&self, within: Duration) -> String {
        let waited = self.ended.wait_timeout_while(self.kept(), within, |kept| !kept.ended);
        let mut kept = waited.map_or_else(|poisoned| poisoned.into_inner().0, |(kept, _)| kept);

        String::from_utf8_lossy(kept.bytes.make_contiguous()).into_owned()
    }

    /// Adds `bytes`, dropping the oldest past the most kept.
    fn keep(&self, bytes: &[u8]) {
        let mut kept = self.kept();
        kept.bytes.extend(bytes);
        let over = kept.bytes.len().saturating_sub(self.most);
        kept.bytes.drain(..over);
    }

    /// What it holds, whatever a thread that panicked holding it left.
    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
