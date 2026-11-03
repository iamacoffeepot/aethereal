//! What each end checks of its engine as it starts: that it can read the frames it would be sent, and hold each as the
//! blob a connection puts it in (ADR-0038, ADR-0042).

use aethereal_substrate::Unbuilt;
use core::error::Error;
use core::fmt;

/// Why an end of the link did not start: a frame it would read could not be read, and would be dropped unread
/// (ADR-0017), so what a peer sent within the end's own limits would be lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadLimitTooLow {
    /// The section whose `most_bytes` asked for more.
    pub section: &'static str,
    /// The most bytes a turn reads into one piece of mail.
    pub read_limit: usize,
    /// What the section's settings need of it.
    pub needed: usize,
}

impl fmt::Display for ReadLimitTooLow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { section, read_limit, needed } = self;
        write!(f, "the engine's `read_limit` is {read_limit}, and `{section}`'s settings need {needed}")
    }
}

impl Error for ReadLimitTooLow {}

/// Why an end of the link did not start: the engine's blobs could not hold a frame it would read, which its connection
/// puts as a blob, so the connection would hang up on it (ADR-0014, ADR-0042).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobBytesTooLow {
    /// The section whose `most_bytes` asked for more.
    pub section: &'static str,
    /// The most bytes the engine's blobs hold.
    pub blob_bytes: usize,
    /// What the section's settings need of it.
    pub needed: usize,
}

impl fmt::Display for BlobBytesTooLow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { section, blob_bytes, needed } = self;
        write!(f, "the engine's `blob_bytes` is {blob_bytes}, and `{section}`'s settings need {needed}")
    }
}

impl Error for BlobBytesTooLow {}

/// Refuses an end whose frames of `most_bytes` could not be read under `read_limit`, as the tools it is sent are mail,
/// or held under `blob_bytes`, as each frame is a blob.
pub fn check(section: &'static str, read_limit: usize, blob_bytes: usize, most_bytes: u32) -> Result<(), Unbuilt> {
    let needed = usize::try_from(most_bytes).unwrap_or(usize::MAX);
    if read_limit < needed {
        return Err(Unbuilt::Failed(Box::new(ReadLimitTooLow { section, read_limit, needed })));
    }
    if blob_bytes < needed {
        return Err(Unbuilt::Failed(Box::new(BlobBytesTooLow { section, blob_bytes, needed })));
    }

    Ok(())
}
