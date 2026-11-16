//! The clipboard (ADR-0024: The clipboard): the host's, read and written as plain text by jobs (ADR-0018) through a
//! service each host that has one serves (ADR-0034), so no actor stands between a screen and it, and a host with none
//! answers that it has none.
//!
//! ```text
//! copy:  turn.run(&WriteClipboard { text: blob }, &context) ──> the host's Clipboard::write ──> Copied
//! paste: turn.run(&ReadClipboard, &context) ──> the host's Clipboard::read ──> Pasted::{Text, Empty, Refused, Unavailable}
//! ```
//!
//! The text crosses as a blob both ways (ADR-0042), as a paste or a copy may be far longer than a piece of mail.

use aethereal_actor::{Bytes, Fault, Job, Runner, Running};
use aethereal_data::{Bound, Plain};
use core::future::Future;
use core::pin::Pin;

/// What a read of the clipboard comes to, as its host answers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardRead {
    /// The plain text it holds.
    Text(String),
    /// It holds no text: nothing, or something that is not text.
    Empty,
    /// The host would not let it be read, as a page whose reader refused to share it.
    Refused,
    /// The host could not reach its clipboard, as a desktop with no display to reach.
    Unavailable,
}

/// What a write of the clipboard comes to, as its host answers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardWritten {
    /// It holds the text now.
    Written,
    /// The host would not let it be written.
    Refused,
    /// The host could not reach its clipboard.
    Unavailable,
}

/// A read of the clipboard, as its host does it.
pub type ClipboardReading<'a> = Pin<Box<dyn Future<Output = ClipboardRead> + 'a>>;

/// A write of the clipboard, as its host does it.
pub type ClipboardWriting<'a> = Pin<Box<dyn Future<Output = ClipboardWritten> + 'a>>;

/// The host's clipboard (ADR-0024: The clipboard): plain text read from it and written to it. A host serves it into the
/// services its runners hand every job (ADR-0034) as it bootstraps its windows; one with no clipboard, a headless one,
/// serves none, and a read or a write there is answered unavailable.
///
/// Neither blocks the thread that polls it: the desktop's reads and writes are done on a thread of its own and wake the
/// job, and a page's are the promises of its `navigator.clipboard`.
pub trait Clipboard: Send + Sync {
    /// Reads the text it holds.
    fn read(&self) -> ClipboardReading<'_>;

    /// Writes `text` to it, in place of what it held.
    fn write(&self, text: String) -> ClipboardWriting<'_>;
}

/// Reads the host's clipboard as plain text: run by a screen whose text input pastes (ADR-0056).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.read_clipboard")]
pub struct ReadClipboard;

/// What a read of the clipboard came to: each case named, as a paste does a different thing with each.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.pasted")]
pub enum Pasted {
    /// Its plain text, as a blob of UTF-8.
    Text { text: Bytes },
    /// It holds no text.
    Empty,
    /// The host would not let it be read.
    Refused,
    /// The host has no clipboard, or could not reach it.
    Unavailable,
}

/// Writes `text`, a blob of UTF-8, to the host's clipboard: run by a screen that copies or cuts (ADR-0056).
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.window.write_clipboard")]
pub struct WriteClipboard {
    pub text: Bytes,
}

/// What a write of the clipboard came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.window.copied")]
pub enum Copied {
    /// It holds the text now.
    Copied,
    /// The host would not let it be written.
    Refused,
    /// The host has no clipboard.
    Unavailable,
}

/// It waits on the host's clipboard, so it runs where waiting is polled.
impl Job for ReadClipboard {
    type Done = Pasted;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Pasted, Fault> {
        let Some(clipboard) = job.service::<dyn Clipboard>() else {
            return Ok(Pasted::Unavailable);
        };
        let pasted = match clipboard.read().await {
            ClipboardRead::Text(text) if text.is_empty() => Pasted::Empty,
            ClipboardRead::Text(text) => Pasted::Text { text: job.put(text.into_bytes()) },
            ClipboardRead::Empty => Pasted::Empty,
            ClipboardRead::Refused => Pasted::Refused,
            ClipboardRead::Unavailable => Pasted::Unavailable,
        };

        Ok(pasted)
    }
}

/// It waits on the host's clipboard, so it runs where waiting is polled. Its text is read as UTF-8, a character that
/// does not read written as the replacement character.
impl Job for WriteClipboard {
    type Done = Copied;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Copied, Fault> {
        let text = String::from_utf8_lossy(job.read(&self.text)).into_owned();
        let Some(clipboard) = job.service::<dyn Clipboard>() else {
            return Ok(Copied::Unavailable);
        };
        let copied = match clipboard.write(text).await {
            ClipboardWritten::Written => Copied::Copied,
            ClipboardWritten::Refused => Copied::Refused,
            ClipboardWritten::Unavailable => Copied::Unavailable,
        };

        Ok(copied)
    }
}
