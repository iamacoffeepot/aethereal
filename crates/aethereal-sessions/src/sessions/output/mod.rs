//! A container's output read into the lines of its transcript (ADR-0057): its logs, followed from its start, skip what
//! the transcript already holds, and are cut into lines across the pieces they arrive in.

#[cfg(test)]
mod tests;

use super::transcript::Entry;
use crate::Std;
use aethereal_data::{Budget, Json, Plain};
use core::mem;

/// The bytes of budget each byte of a line may cost as it is checked for JSON.
const BUDGET_PER_BYTE: usize = 64;

/// The most bytes of one line kept before it is cut: past any event Claude writes.
const MOST_LINE_BYTES: usize = 16 << 20;

/// What is read of a container's stdout and stderr.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) struct Reading {
    stdout: Lines,
    stderr: Lines,
}

/// One stream's lines: the bytes still to skip, which the transcript holds already, and a line not yet whole.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
struct Lines {
    skip: u64,
    partial: Vec<u8>,
}

impl Reading {
    /// A reading that skips the first `stdout` bytes of stdout and `stderr` of stderr.
    pub const fn skipping((stdout, stderr): (u64, u64)) -> Self {
        Self {
            stdout: Lines { skip: stdout, partial: Vec::new() },
            stderr: Lines { skip: stderr, partial: Vec::new() },
        }
    }

    /// Reads `bytes`, the next of `stream`, answering the entry of each line they end.
    pub fn read(&mut self, stream: Std, bytes: &[u8]) -> Vec<Entry> {
        let lines = match stream {
            Std::Stdout => &mut self.stdout,
            Std::Stderr => &mut self.stderr,
        };

        lines.read(bytes).into_iter().map(|line| entry(stream, &line)).collect()
    }
}

impl Lines {
    /// Reads `bytes`, answering each line they end, without its newline.
    fn read(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let skipped = usize::try_from(self.skip).unwrap_or(usize::MAX).min(bytes.len());
        self.skip -= u64::try_from(skipped).unwrap_or(0);
        let mut whole = Vec::new();
        for piece in bytes[skipped..].split_inclusive(|&byte| byte == b'\n') {
            self.partial.extend_from_slice(piece);
            let is_whole = self.partial.ends_with(b"\n");
            let is_long = self.partial.len() > MOST_LINE_BYTES;
            if is_whole {
                self.partial.pop();
                whole.push(mem::take(&mut self.partial));
            } else if is_long {
                whole.push(mem::take(&mut self.partial));
            }
        }

        whole
    }
}

/// The entry a line of `stream` is: Claude's event when it is stdout's and one JSON value, and its text otherwise.
fn entry(stream: Std, line: &[u8]) -> Entry {
    let bytes = u64::try_from(line.len()).unwrap_or(u64::MAX);
    let text = String::from_utf8_lossy(line).into_owned();
    match stream {
        Std::Stdout => {
            let is_exact = text.len() == line.len();
            let mut budget = Budget::new(line.len().saturating_mul(BUDGET_PER_BYTE));
            match Json::parse(text.clone(), &mut budget) {
                Ok(json) if is_exact => Entry::Claude(json),
                Ok(_) | Err(_) => Entry::Stdout { text, bytes },
            }
        }
        Std::Stderr => Entry::Stderr { text, bytes },
    }
}
