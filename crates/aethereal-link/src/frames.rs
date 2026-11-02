//! What crosses the link: each frame one plain kind's bytes, in a `LengthPrefixed` frame (ADR-0038).

use aethereal_data::{Budget, DecodeError, EncodeError, Plain, from_bytes, to_bytes};
use aethereal_tools::{Called, ToolSheet};

/// The revision of the link this crate speaks. A change to [`Up`] or [`Down`] that an older side would misread is the
/// next.
pub const REVISION: u32 = 1;

/// The port a hub listens for engines on, and an engine dials, unless set otherwise.
pub const PORT: u16 = 8892;

/// A frame from an engine to its hub.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.up")]
pub enum Up {
    /// The first frame of a connection: the revision the engine speaks, and the name the hub shows the agent beside
    /// it, which is not an identity.
    Hello { revision: u32, name: String },
    /// Every tool the engine holds, and the revision of the set: a count the engine raises with each change.
    Sheets { revision: u64, tools: Vec<ToolSheet> },
    /// What the hub's call `call` came to.
    Called { call: u64, called: Called },
}

/// A frame from a hub to an engine.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.down")]
pub enum Down {
    /// The engine's revision is not one the hub speaks: `revision` is the one it does. The hub then hangs up.
    Refused { revision: u32 },
    /// A call of `tool`, numbered by the hub, with `input` bytes of its input schema in the set of `revision`, the one
    /// the hub converted it with.
    Call { call: u64, revision: u64, tool: String, input: Vec<u8> },
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
