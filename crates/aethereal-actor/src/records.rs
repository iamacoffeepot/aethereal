//! What crosses between a guest and its host, as fixed-width records each side reads where the other wrote them
//! (ADR-0015).
//!
//! The host writes a batch's items into the guest's inbox, each an [`ItemHead`]
//! and then its bytes. After the call the guest's out buffer holds an
//! [`OutHead`], a [`TurnEnds`] for each turn it ran, then a [`SendRecord`] for
//! each piece of mail its turns sent and a [`LineRecord`] for each line they
//! logged. A send's bytes and a line's text are not copied out: each record
//! says where they lie in the guest's memory, and the host reads them there.
//!
//! Every number is little-endian, and every address is one in the guest's
//! 32-bit memory.

use crate::{Chain, Level, MailId, SeatNumber, Seated};
use aethereal_data::{ActorId, Budget, KindId, from_bytes};

/// The head of an item in the inbox: its kind's id, its source's id and seat, its chain's root, sender and count, its
/// depth, and the length of the bytes after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemHead {
    pub kind: KindId,
    pub source: Seated,
    pub chain: Chain,
    pub len: u32,
}

impl ItemHead {
    /// How many bytes it is.
    pub const BYTES: usize = 48;

    /// It, as the inbox holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..8].copy_from_slice(&self.kind.plain_bytes());
        out[8..16].copy_from_slice(&self.source.id.bits().to_le_bytes());
        out[16..24].copy_from_slice(&self.source.seat.number().to_le_bytes());
        out[24..32].copy_from_slice(&self.chain.root.sender.bits().to_le_bytes());
        out[32..40].copy_from_slice(&self.chain.root.count.to_le_bytes());
        out[40..44].copy_from_slice(&self.chain.depth.to_le_bytes());
        out[44..48].copy_from_slice(&self.len.to_le_bytes());

        out
    }

    /// The head `bytes` begin with: nothing when they are too few, or name no kind.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let head = bytes.get(..Self::BYTES)?;
        let kind = kind_at(head, 0)?;
        let source = Seated { id: ActorId::new(u64_at(head, 8)?), seat: SeatNumber::new(u64_at(head, 16)?) };
        let root = MailId { sender: ActorId::new(u64_at(head, 24)?), count: u64_at(head, 32)? };
        let chain = Chain { root, depth: u32_at(head, 40)? };

        Some(Self { kind, source, chain, len: u32_at(head, 44)? })
    }
}

/// How the last turn a guest ran of a batch ended its batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// It did not: every item of the batch had its turn.
    No,
    /// It failed, with the fault whose reason is of `kind`, its bytes where `bytes` says.
    Failed { kind: KindId, bytes: Span },
    /// It closed the actor.
    Closed,
}

/// The head of a guest's out buffer after a call: how many turns it ran, how the last stopped the batch, and where the
/// records of what the turns asked for begin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutHead {
    pub ran: u32,
    pub stopped: Stopped,
    /// Where the turns' [`TurnEnds`], [`SendRecord`]s and [`LineRecord`]s begin.
    pub turns_at: u32,
    pub sends_at: u32,
    pub lines_at: u32,
}

/// Where some bytes lie in a guest's memory: their address, and how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub at: u32,
    pub len: u32,
}

impl OutHead {
    /// How many bytes it is.
    pub const BYTES: usize = 40;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        let stopped: u32 = match self.stopped {
            Stopped::No => 0,
            Stopped::Failed { kind, bytes } => {
                out[8..16].copy_from_slice(&kind.plain_bytes());
                out[16..20].copy_from_slice(&bytes.at.to_le_bytes());
                out[20..24].copy_from_slice(&bytes.len.to_le_bytes());
                1
            }
            Stopped::Closed => 2,
        };
        out[..4].copy_from_slice(&self.ran.to_le_bytes());
        out[4..8].copy_from_slice(&stopped.to_le_bytes());
        out[24..28].copy_from_slice(&self.turns_at.to_le_bytes());
        out[28..32].copy_from_slice(&self.sends_at.to_le_bytes());
        out[32..36].copy_from_slice(&self.lines_at.to_le_bytes());

        out
    }

    /// The head `bytes` begin with: nothing when they are too few, or do not read as one.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let head = bytes.get(..Self::BYTES)?;
        let stopped = match u32_at(head, 4)? {
            0 => Stopped::No,
            1 => Stopped::Failed {
                kind: kind_at(head, 8)?,
                bytes: Span { at: u32_at(head, 16)?, len: u32_at(head, 20)? },
            },
            2 => Stopped::Closed,
            _ => return None,
        };

        Some(Self {
            ran: u32_at(head, 0)?,
            stopped,
            turns_at: u32_at(head, 24)?,
            sends_at: u32_at(head, 28)?,
            lines_at: u32_at(head, 32)?,
        })
    }
}

/// Where one turn's records end: how many sends and how many lines the turns up to and including it asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnEnds {
    pub sends: u32,
    pub lines: u32,
}

impl TurnEnds {
    /// How many bytes it is.
    pub const BYTES: usize = 8;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..4].copy_from_slice(&self.sends.to_le_bytes());
        out[4..].copy_from_slice(&self.lines.to_le_bytes());

        out
    }

    /// The record `bytes` begin with.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        Some(Self { sends: u32_at(bytes, 0)?, lines: u32_at(bytes, 4)? })
    }
}

/// A piece of mail a turn sent: to whom, of what kind, and where its bytes lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendRecord {
    pub to: Seated,
    pub kind: KindId,
    pub bytes: Span,
}

impl SendRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 32;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..8].copy_from_slice(&self.to.id.bits().to_le_bytes());
        out[8..16].copy_from_slice(&self.to.seat.number().to_le_bytes());
        out[16..24].copy_from_slice(&self.kind.plain_bytes());
        out[24..28].copy_from_slice(&self.bytes.at.to_le_bytes());
        out[28..32].copy_from_slice(&self.bytes.len.to_le_bytes());

        out
    }

    /// The record `bytes` begin with: nothing when they are too few, or name no kind.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let to = Seated { id: ActorId::new(u64_at(bytes, 0)?), seat: SeatNumber::new(u64_at(bytes, 8)?) };
        let span = Span { at: u32_at(bytes, 24)?, len: u32_at(bytes, 28)? };

        Some(Self { to, kind: kind_at(bytes, 16)?, bytes: span })
    }
}

/// A line a turn logged: at what level, and where its text lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRecord {
    pub level: Level,
    pub text: Span,
}

impl LineRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 12;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let level: u32 = match self.level {
            Level::Debug => 0,
            Level::Info => 1,
            Level::Warn => 2,
            Level::Error => 3,
        };
        let mut out = [0; Self::BYTES];
        out[..4].copy_from_slice(&level.to_le_bytes());
        out[4..8].copy_from_slice(&self.text.at.to_le_bytes());
        out[8..12].copy_from_slice(&self.text.len.to_le_bytes());

        out
    }

    /// The record `bytes` begin with.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let level = match u32_at(bytes, 0)? {
            0 => Level::Debug,
            1 => Level::Info,
            2 => Level::Warn,
            _ => Level::Error,
        };

        Some(Self { level, text: Span { at: u32_at(bytes, 4)?, len: u32_at(bytes, 8)? } })
    }
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn u64_at(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}

fn kind_at(bytes: &[u8], at: usize) -> Option<KindId> {
    let mut budget = Budget::new(8);

    from_bytes::<KindId>(bytes.get(at..at + 8)?, &mut budget).ok()
}
