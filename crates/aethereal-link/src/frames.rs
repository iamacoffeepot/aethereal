//! What crosses the link: each frame one plain kind's bytes, in a `LengthPrefixed` frame (ADR-0038).

use crate::Token;
use aethereal_actor::{Due, Time};
use aethereal_data::{Budget, DecodeError, EncodeError, Plain, from_bytes, to_bytes};
use aethereal_tools::{Called, ToolSheet};
use core::time::Duration;

/// The revision of the link this crate speaks. A change to [`Up`] or [`Down`] that an older side would misread is the
/// next. `2` brought [`Down::Call`]'s `within`, the time a call has left (ADR-0041), which a side of `1` misreads.
pub const REVISION: u32 = 2;

/// The port a hub listens for engines on, and an engine dials, unless set otherwise.
pub const PORT: u16 = 8892;

/// A frame from an engine to its hub.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.up")]
pub enum Up {
    /// The first frame of a connection: the revision the engine speaks, the name the hub shows the agent beside it,
    /// which is not an identity, and the token the hub minted for it, which is (ADR-0040).
    Hello { revision: u32, name: String, token: Token },
    /// Every tool the engine holds, and the revision of the set: a count the engine raises with each change.
    Sheets { revision: u64, tools: Vec<ToolSheet> },
    /// What the hub's call `call` came to.
    Called { call: u64, called: Called },
}

/// A frame from a hub to an engine.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.down")]
pub enum Down {
    /// The hub refused the engine's `Hello`, saying why. The hub then hangs up.
    Refused(Refusal),
    /// A call of `tool`, numbered by the hub, with `input` bytes of its input schema in the set of `revision`, the one
    /// the hub converted it with, and the time it has left (ADR-0041).
    Call { call: u64, revision: u64, tool: String, input: Vec<u8>, within: Within },
}

/// The time a call has left when the hub writes it (ADR-0041). Two engines' clocks are their own, so a deadline
/// crosses the link as a span: the hub writes what is left before its call's deadline, and the engine stops waiting for
/// its tool once that much has passed since it read the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.within")]
pub enum Within {
    /// No time at all: its asker waits until it is answered.
    Unbounded,
    /// This many milliseconds, rounded down, so the engine never waits past the hub's deadline.
    Millis(u64),
}

impl Within {
    /// The time left at `now` before `due`: none once it has come, and unbounded for a request never due.
    #[must_use]
    pub const fn left(due: Due, now: Time) -> Self {
        match due {
            Due::Never => Self::Unbounded,
            Due::Before(before) => Self::Millis(before.nanos().saturating_sub(now.nanos()) / NANOS_PER_MILLI),
        }
    }

    /// The deadline on this engine's clock of a call read at `now` with this time left.
    #[must_use]
    pub fn due(self, now: Time) -> Due {
        match self {
            Self::Unbounded => Due::Never,
            Self::Millis(millis) => Due::Before(now + Duration::from_millis(millis)),
        }
    }
}

/// The nanoseconds of a millisecond.
const NANOS_PER_MILLI: u64 = 1_000_000;

/// Why a hub refused an engine's `Hello`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.refusal")]
pub enum Refusal {
    /// The engine's revision is not one the hub speaks: `revision` is the one it does.
    Revision { revision: u32 },
    /// The hub holds no such token: it never minted it, it expired, or a `Hello` brought it already (ADR-0040).
    Token,
}

/// The bytes a frame's decode may allocate for each byte of the frame: a list is charged its elements' size whole,
/// which can be many times the bytes each was written in.
const ALLOCATED_PER_BYTE: usize = 64;

/// A frame as written, or that it would not fit in one.
pub enum Written {
    /// Its bytes, within the limit.
    Fits(Vec<u8>),
    /// Its bytes are past the limit.
    TooLarge,
}

/// `frame` written, when its bytes are at most `most_bytes`.
pub fn written<T: Plain>(frame: &T, most_bytes: u32) -> Result<Written, EncodeError> {
    let bytes = to_bytes(frame)?;
    let most = usize::try_from(most_bytes).unwrap_or(usize::MAX);
    let fits = bytes.len() <= most;
    let written = if fits {
        Written::Fits(bytes)
    } else {
        Written::TooLarge
    };

    Ok(written)
}

/// The frame `bytes` hold, read within the budget a frame of `most_bytes` is allowed.
pub fn read<T: Plain>(bytes: &[u8], most_bytes: u32) -> Result<T, DecodeError> {
    let most = usize::try_from(most_bytes).unwrap_or(usize::MAX);

    from_bytes(bytes, &mut Budget::new(most.saturating_mul(ALLOCATED_PER_BYTE)))
}

#[cfg(test)]
mod tests {
    use super::Within;
    use aethereal_actor::{Due, Time};

    #[test]
    fn the_time_left_is_rounded_down_and_none_once_the_deadline_has_passed() {
        // Catches the time left rounded up, so an engine would wait past the hub's deadline, and a deadline already
        // passed written as a span that wraps around to centuries.
        let now = Time::from_nanos(5_000_000);

        assert_eq!(Within::left(Due::Before(Time::from_nanos(6_999_999)), now), Within::Millis(1));
        assert_eq!(Within::left(Due::Before(Time::from_nanos(1)), now), Within::Millis(0));
        assert_eq!(Within::left(Due::Never, now), Within::Unbounded);
    }
}
