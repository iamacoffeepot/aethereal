//! A program's stderr as it is kept: its head and its tail, each bounded, every over-long line cut, kept as a thread
//! of its own reads the pipe (ADR-0040).

#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::io::{ErrorKind, Read};
use std::process::ChildStderr;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The most bytes kept of the first a program writes to its stderr: where the first panic's `panicked at` line is.
pub const HEAD_BYTES: usize = 1024;
/// The most bytes kept of the last a program writes to its stderr: how it ended.
pub const TAIL_BYTES: usize = 512;
/// The most characters of one line kept: the rest of a longer line, such as a backtrace frame's generic type names, is
/// left out, and the line ends in [`CUT`].
pub const LONGEST_LINE_CHARACTERS: usize = 200;
/// What ends a line cut at [`LONGEST_LINE_CHARACTERS`].
const CUT: &str = "…";

/// What a program wrote to its stderr, as it is kept, and whether the pipe has ended. A lock, as the thread that reads
/// the pipe fills it and the thread that waited on the program reads it once the program has exited.
#[derive(Default)]
pub struct HeadAndTail {
    kept: Mutex<Kept>,
    /// Woken as the pipe ends.
    ended: Condvar,
}

/// What it holds. The bytes are those of the stderr with every over-long line cut: the head the first
/// [`HEAD_BYTES`] of them, the tail the last [`TAIL_BYTES`] of those that came after.
struct Kept {
    line: Line,
    head: Vec<u8>,
    tail: VecDeque<u8>,
    /// What has passed out of the front of the tail, left out.
    passed: Passed,
    /// Whether the pipe has ended: the program, and every program it started, have closed it.
    ended: bool,
}

/// Where the line being read stands.
#[derive(Clone, Copy)]
enum Line {
    /// Kept, `characters` of it so far.
    Kept { characters: usize },
    /// Cut: the rest of it, until its newline, is left out.
    Cut,
}

/// The bytes that have passed out of the front of the tail: how many, how many newlines among them, and whether the
/// last of them was one, so the tail begins a line.
#[derive(Clone, Copy)]
struct Passed {
    bytes: usize,
    newlines: usize,
    ends_line: bool,
}

impl HeadAndTail {
    /// Reads `stderr` until it ends, keeping its head and its tail, then marks the pipe ended.
    pub fn read(&self, mut stderr: ChildStderr) {
        let mut buffer = [0; 4096];
        loop {
            let read = stderr.read(&mut buffer);
            match read {
                Ok(0) => break,
                Ok(bytes) => self.kept().keep(&buffer[..bytes]),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        self.kept().ended = true;
        self.ended.notify_all();
    }

    /// The stderr as text, once the pipe has ended, or once `within` has passed: what a program wrote before it exited
    /// is in the pipe and read at once, unless a program it started holds the pipe open.
    pub fn text(&self, within: Duration) -> String {
        let waited = self.ended.wait_timeout_while(self.kept(), within, |kept| !kept.ended);
        let kept = waited.map_or_else(|poisoned| poisoned.into_inner().0, |(kept, _)| kept);

        kept.text()
    }

    /// What it holds, whatever a thread that panicked holding it left.
    fn kept(&self) -> MutexGuard<'_, Kept> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for Kept {
    /// Nothing read yet.
    fn default() -> Self {
        Self {
            line: Line::Kept { characters: 0 },
            head: Vec::new(),
            tail: VecDeque::new(),
            passed: Passed { bytes: 0, newlines: 0, ends_line: true },
            ended: false,
        }
    }
}

impl Kept {
    /// Adds `bytes` read from the pipe, a line cut once it is [`LONGEST_LINE_CHARACTERS`] long.
    fn keep(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if byte == b'\n' {
                self.line = Line::Kept { characters: 0 };
                self.add(byte);
                continue;
            }
            let Line::Kept { characters } = self.line else {
                continue;
            };
            let starts_character = !is_continuation(byte);
            let is_full = characters == LONGEST_LINE_CHARACTERS;
            let cuts = starts_character && is_full;
            if cuts {
                self.line = Line::Cut;
                CUT.bytes().for_each(|cut| self.add(cut));
                continue;
            }
            self.line = Line::Kept { characters: characters + usize::from(starts_character) };
            self.add(byte);
        }
    }

    /// Adds one byte of a kept line: to the head while it has room, to the tail after, the tail's first byte passing
    /// once it is full.
    fn add(&mut self, byte: u8) {
        if self.head.len() < HEAD_BYTES {
            self.head.push(byte);
            return;
        }
        self.tail.push_back(byte);
        if self.tail.len() > TAIL_BYTES {
            let passed = self.tail.pop_front().unwrap_or_default();
            let ends_line = passed == b'\n';
            self.passed.bytes += 1;
            self.passed.newlines += usize::from(ends_line);
            self.passed.ends_line = ends_line;
        }
    }

    /// The stderr as text. When all of it was kept, the whole of it; otherwise the head up to its last line's end, a
    /// line saying how many lines and bytes were left out, and the tail from its first whole line. A character the
    /// program's own bytes cut short is replaced.
    fn text(&self) -> String {
        let tail = Vec::from(self.tail.clone());
        let is_whole = self.passed.bytes == 0;
        if is_whole {
            let mut whole = self.head.clone();
            whole.extend(tail);
            return String::from_utf8_lossy(&whole).into_owned();
        }

        let head_end = self.head.iter().rposition(|&byte| byte == b'\n').map_or(0, |newline| newline + 1);
        let tail_start = if self.passed.ends_line {
            0
        } else {
            line_start(&tail)
        };
        let left_out = &self.head[head_end..];
        let cut_off = &tail[..tail_start];
        let bytes = left_out.len() + self.passed.bytes + cut_off.len();
        let newlines = newlines(left_out) + self.passed.newlines + newlines(cut_off);
        let marker = format!("{CUT} {newlines} lines, {bytes} bytes, left out {CUT}\n");

        let mut text = String::from_utf8_lossy(&self.head[..head_end]).into_owned();
        text.push_str(&marker);
        text.push_str(&String::from_utf8_lossy(&tail[tail_start..]));

        text
    }
}

/// Where the first whole line of `bytes`, whose front was cut, begins: after its first newline, or, with none, at its
/// first character's start.
fn line_start(bytes: &[u8]) -> usize {
    let first_character = bytes.iter().position(|&byte| !is_continuation(byte)).unwrap_or(bytes.len());

    bytes.iter().position(|&byte| byte == b'\n').map_or(first_character, |newline| newline + 1)
}

/// How many newlines `bytes` hold.
#[allow(clippy::naive_bytecount)] // A few kilobytes, counted once as a program exits: no crate is added for it.
fn newlines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&byte| byte == b'\n').count()
}

/// Whether `byte` continues a UTF-8 character rather than starting one.
const fn is_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}
