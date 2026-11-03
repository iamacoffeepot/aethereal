//! What each end checks of its engine as it starts: that it can read the frames it would be sent (ADR-0038).

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

/// Refuses an end whose frames of `most_bytes` could not be read under `read_limit`.
pub fn check(section: &'static str, read_limit: usize, most_bytes: u32) -> Result<(), ReadLimitTooLow> {
    let needed = usize::try_from(most_bytes).unwrap_or(usize::MAX);
    if read_limit < needed {
        return Err(ReadLimitTooLow { section, read_limit, needed });
    }

    Ok(())
}
