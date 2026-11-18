//! Whether an engine can carry what HTTP takes in (ADR-0017, ADR-0042): a head is mail its reader reads within the
//! engine's `read_limit`, and a body or a chunk a connection reads is a blob within its `blob_bytes`. The `Http` root
//! and a stack each check their own settings against it before they begin.

use crate::Header;
use core::error::Error;
use core::fmt;
use core::mem::size_of;

/// The fewest bytes of a head that hold a header: `a:`, and its line's end.
const LEAST_HEADER_BYTES: usize = 4;

/// The most bytes a turn reads into one piece of mail that holds a head of `most_head_bytes`: the head, and its most
/// headers, as reading a list charges each of its elements whole.
pub fn head_read_bytes(most_head_bytes: u32) -> usize {
    let head = size(most_head_bytes);
    let headers = (head / LEAST_HEADER_BYTES).saturating_mul(size_of::<Header>());

    head.saturating_add(headers)
}

/// `bytes` as a `usize`, or the most one holds.
pub fn size(bytes: u32) -> usize {
    usize::try_from(bytes).unwrap_or(usize::MAX)
}

/// Why `Http`, or a stack, did not start: mail it would send could not be read, so a request within its limits would
/// never reach its handler, nor a response its asker (ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimitTooLow {
    /// The most bytes a turn reads into one piece of mail.
    pub read_limit: usize,
    /// What the settings need of it.
    pub needed: usize,
}

impl fmt::Display for ReadLimitTooLow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { read_limit, needed } = self;
        write!(f, "the engine's `read_limit` is {read_limit}, and the HTTP settings need {needed}")
    }
}

impl Error for ReadLimitTooLow {}

/// Why `Http`, or a stack, did not start: the engine's blobs could not hold a chunk a connection reads, or a response's
/// body a client reads whole, so a request or a response within its limits could not be read (ADR-0014, ADR-0042).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobBytesTooLow {
    /// The most bytes the engine's blobs hold.
    pub blob_bytes: usize,
    /// What the settings need of it.
    pub needed: usize,
}

impl fmt::Display for BlobBytesTooLow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { blob_bytes, needed } = self;
        write!(f, "the engine's `blob_bytes` is {blob_bytes}, and the HTTP settings need {needed}")
    }
}

impl Error for BlobBytesTooLow {}
