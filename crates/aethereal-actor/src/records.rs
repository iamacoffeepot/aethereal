//! What crosses between a guest and its host, as fixed-width records each side reads where the other wrote them
//! (ADR-0015).
//!
//! The host writes a batch's items into the guest's inbox, each an [`ItemHead`]
//! and then its bytes: a piece of mail's, or the fault of a watched actor that
//! failed. After the call the guest's out buffer holds an
//! [`OutHead`], a [`TurnEnds`] for each turn it ran, then a [`SendRecord`] for
//! each piece of mail its turns sent, a [`LineRecord`] for each line they
//! logged, a [`SpawnRecord`] for each child they spawned, a [`StoodRecord`]
//! for each child that began, and a [`WatchRecord`] for each actor they
//! watched. A send's bytes, a line's text, a key and a moved
//! child's state are not copied out: each record says where they lie in the
//! guest's memory, and the host reads them there. Sends and lines say who made
//! them, as watches do: the actor whose turn it was, or a child it spawned, in
//! its `begin`.
//!
//! Every number is little-endian, and every address is one in the guest's
//! 32-bit memory.

use crate::{Chain, Level, MailId, SeatNumber, Seated};
use aethereal_data::{ActorId, ActorPath, Budget, KeyName, KindId, KindName, PathSegment, Plain, from_bytes};

/// The head of an item in the inbox: what it is, its source's id and seat, its chain's root, sender and count, its
/// depth, and the length of the bytes after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemHead {
    pub handed: Handed,
    pub source: Seated,
    pub chain: Chain,
    pub len: u32,
}

/// What an item in the inbox is: a piece of mail, or the ending of an actor its receiver watches, which is its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handed {
    /// Mail of `kind`, its bytes after the head.
    Mail { kind: KindId },
    /// The watched actor ended so.
    Ended(Gone),
}

/// How a watched actor ended, as its ending is handed into the inbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gone {
    /// It asked to close.
    Closed,
    /// A turn of its own failed with the fault whose reason is of `kind`, its bytes after the head.
    Failed { kind: KindId },
    /// It had ended before the step the watch was asked in.
    Earlier,
}

impl ItemHead {
    /// How many bytes it is.
    pub const BYTES: usize = 52;

    /// It, as the inbox holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        let handed: u32 = match self.handed {
            Handed::Mail { kind } => {
                out[..8].copy_from_slice(&kind.plain_bytes());
                0
            }
            Handed::Ended(Gone::Closed) => 1,
            Handed::Ended(Gone::Failed { kind }) => {
                out[..8].copy_from_slice(&kind.plain_bytes());
                2
            }
            Handed::Ended(Gone::Earlier) => 3,
        };
        out[48..52].copy_from_slice(&handed.to_le_bytes());
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
        let handed = match u32_at(head, 48)? {
            0 => Handed::Mail { kind: kind_at(head, 0)? },
            1 => Handed::Ended(Gone::Closed),
            2 => Handed::Ended(Gone::Failed { kind: kind_at(head, 0)? }),
            3 => Handed::Ended(Gone::Earlier),
            _ => return None,
        };
        let source = Seated { id: ActorId::new(u64_at(head, 8)?), seat: SeatNumber::new(u64_at(head, 16)?) };
        let root = MailId { sender: ActorId::new(u64_at(head, 24)?), count: u64_at(head, 32)? };
        let chain = Chain { root, depth: u32_at(head, 40)? };

        Some(Self { handed, source, chain, len: u32_at(head, 44)? })
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
    /// Where the turns' [`TurnEnds`], [`SendRecord`]s, [`LineRecord`]s, [`SpawnRecord`]s, [`StoodRecord`]s and
    /// [`WatchRecord`]s begin.
    pub turns_at: u32,
    pub sends_at: u32,
    pub lines_at: u32,
    pub spawns_at: u32,
    pub stood_at: u32,
    pub watches_at: u32,
}

/// Where some bytes lie in a guest's memory: their address, and how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub at: u32,
    pub len: u32,
}

impl OutHead {
    /// How many bytes it is.
    pub const BYTES: usize = 48;

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
        out[36..40].copy_from_slice(&self.spawns_at.to_le_bytes());
        out[40..44].copy_from_slice(&self.stood_at.to_le_bytes());
        out[44..48].copy_from_slice(&self.watches_at.to_le_bytes());

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
            spawns_at: u32_at(head, 36)?,
            stood_at: u32_at(head, 40)?,
            watches_at: u32_at(head, 44)?,
        })
    }
}

/// Where one turn's records end: how many sends, lines, spawns, children stood up and watches the turns up to and
/// including it asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurnEnds {
    pub sends: u32,
    pub lines: u32,
    pub spawns: u32,
    pub stood: u32,
    pub watches: u32,
}

impl TurnEnds {
    /// How many bytes it is.
    pub const BYTES: usize = 20;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..4].copy_from_slice(&self.sends.to_le_bytes());
        out[4..8].copy_from_slice(&self.lines.to_le_bytes());
        out[8..12].copy_from_slice(&self.spawns.to_le_bytes());
        out[12..16].copy_from_slice(&self.stood.to_le_bytes());
        out[16..].copy_from_slice(&self.watches.to_le_bytes());

        out
    }

    /// The record `bytes` begin with.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        Some(Self {
            sends: u32_at(bytes, 0)?,
            lines: u32_at(bytes, 4)?,
            spawns: u32_at(bytes, 8)?,
            stood: u32_at(bytes, 12)?,
            watches: u32_at(bytes, 16)?,
        })
    }
}

/// A piece of mail a turn sent: by whom, to whom, of what kind, and where its bytes lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendRecord {
    pub by: Seated,
    pub to: Seated,
    pub kind: KindId,
    pub bytes: Span,
}

impl SendRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 48;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..16].copy_from_slice(&seated_bytes(self.by));
        out[16..32].copy_from_slice(&seated_bytes(self.to));
        out[32..40].copy_from_slice(&self.kind.plain_bytes());
        out[40..44].copy_from_slice(&self.bytes.at.to_le_bytes());
        out[44..48].copy_from_slice(&self.bytes.len.to_le_bytes());

        out
    }

    /// The record `bytes` begin with: nothing when they are too few, or name no kind.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let span = Span { at: u32_at(bytes, 40)?, len: u32_at(bytes, 44)? };

        Some(Self { by: seated_at(bytes, 0)?, to: seated_at(bytes, 16)?, kind: kind_at(bytes, 32)?, bytes: span })
    }
}

/// The segment of an actor's path, as it crosses between a guest and its host: its type's name, and its key's when it
/// is keyed.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Naming {
    Singleton { kind: KindName },
    Keyed { kind: KindName, key: KeyName },
}

impl Naming {
    /// The naming of `segment`.
    #[must_use]
    pub fn of(segment: &PathSegment) -> Self {
        let kind = segment.kind().clone();
        match segment.key() {
            None => Self::Singleton { kind },
            Some(key) => Self::Keyed { kind, key: key.clone() },
        }
    }

    /// The segment it names.
    #[must_use]
    pub fn segment(&self) -> PathSegment {
        match self {
            Self::Singleton { kind } => PathSegment::new(kind.clone(), &()),
            Self::Keyed { kind, key } => PathSegment::instanced(kind.clone(), key.clone()),
        }
    }
}

/// Where an actor a guest is handed stands, as its host writes it before the actor's config to begin it, or as the
/// guest writes it before the actor's state to take it out: its path, and what is above it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Born {
    pub path: ActorPath,
    pub parent: Parent,
}

/// What is above an actor a guest is handed: nothing for a root, or the actor that spawned it, by its id and seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Parent {
    Chassis,
    Beneath { id: u64, seat: u64 },
}

/// A child a turn spawned, as the spawn is asked for: by whom, of what type, under what key, and in what seat. A
/// child whose `begin` failed has no record: its spawn was taken back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnRecord {
    pub by: Seated,
    pub kind: KindId,
    pub key: Keying,
    pub seat: SeatNumber,
}

/// The key a child was spawned under: none for a singleton, or the name that lies where it says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keying {
    Singleton,
    Keyed(Span),
}

impl SpawnRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 48;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..16].copy_from_slice(&seated_bytes(self.by));
        out[16..24].copy_from_slice(&self.kind.plain_bytes());
        out[24..32].copy_from_slice(&self.seat.number().to_le_bytes());
        let keyed: u32 = match self.key {
            Keying::Singleton => 0,
            Keying::Keyed(span) => {
                out[36..40].copy_from_slice(&span.at.to_le_bytes());
                out[40..44].copy_from_slice(&span.len.to_le_bytes());
                1
            }
        };
        out[32..36].copy_from_slice(&keyed.to_le_bytes());

        out
    }

    /// The record `bytes` begin with: nothing when they are too few, or do not read as one.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let key = match u32_at(bytes, 32)? {
            0 => Keying::Singleton,
            1 => Keying::Keyed(Span { at: u32_at(bytes, 36)?, len: u32_at(bytes, 40)? }),
            _ => return None,
        };

        Some(Self {
            by: seated_at(bytes, 0)?,
            kind: kind_at(bytes, 16)?,
            key,
            seat: SeatNumber::new(u64_at(bytes, 24)?),
        })
    }
}

/// An actor a turn watched: by whom, the actor whose turn it was or a child it spawned, in its `begin`, and which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchRecord {
    pub by: Seated,
    pub of: Seated,
}

impl WatchRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 32;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..16].copy_from_slice(&seated_bytes(self.by));
        out[16..].copy_from_slice(&seated_bytes(self.of));

        out
    }

    /// The record `bytes` begin with: nothing when they are too few.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        Some(Self { by: seated_at(bytes, 0)?, of: seated_at(bytes, 16)? })
    }
}

/// A child that began, in the order its `begin` finished: who it is, who spawned it, and where its state is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoodRecord {
    pub me: Seated,
    pub above: Seated,
    pub held: Kept,
}

/// Where a child's state is held once it began.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept {
    /// In the instance it began in, which is its part's, under the handle the guest knows it by.
    Here { handle: u32 },
    /// Taken out, as it sits in another part: its path and state, written at `Within`, lie where `state` says, for its
    /// host to take into its own part's instance.
    Out { state: Span },
}

impl StoodRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 48;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..16].copy_from_slice(&seated_bytes(self.me));
        out[16..32].copy_from_slice(&seated_bytes(self.above));
        let kept: u32 = match self.held {
            Kept::Here { handle } => {
                out[36..40].copy_from_slice(&handle.to_le_bytes());
                0
            }
            Kept::Out { state } => {
                out[36..40].copy_from_slice(&state.at.to_le_bytes());
                out[40..44].copy_from_slice(&state.len.to_le_bytes());
                1
            }
        };
        out[32..36].copy_from_slice(&kept.to_le_bytes());

        out
    }

    /// The record `bytes` begin with: nothing when they are too few, or do not read as one.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let held = match u32_at(bytes, 32)? {
            0 => Kept::Here { handle: u32_at(bytes, 36)? },
            1 => Kept::Out { state: Span { at: u32_at(bytes, 36)?, len: u32_at(bytes, 40)? } },
            _ => return None,
        };

        Some(Self { me: seated_at(bytes, 0)?, above: seated_at(bytes, 16)?, held })
    }
}

/// A line a turn logged: by whom, at what level, and where its text lies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRecord {
    pub by: Seated,
    pub level: Level,
    pub text: Span,
}

impl LineRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 28;

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
        out[..16].copy_from_slice(&seated_bytes(self.by));
        out[16..20].copy_from_slice(&level.to_le_bytes());
        out[20..24].copy_from_slice(&self.text.at.to_le_bytes());
        out[24..28].copy_from_slice(&self.text.len.to_le_bytes());

        out
    }

    /// The record `bytes` begin with.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let level = match u32_at(bytes, 16)? {
            0 => Level::Debug,
            1 => Level::Info,
            2 => Level::Warn,
            _ => Level::Error,
        };

        Some(Self { by: seated_at(bytes, 0)?, level, text: Span { at: u32_at(bytes, 20)?, len: u32_at(bytes, 24)? } })
    }
}

/// An actor and its seat, as a record holds them: its id, then its seat's number.
fn seated_bytes(seated: Seated) -> [u8; 16] {
    let mut out = [0; 16];
    out[..8].copy_from_slice(&seated.id.bits().to_le_bytes());
    out[8..].copy_from_slice(&seated.seat.number().to_le_bytes());

    out
}

fn seated_at(bytes: &[u8], at: usize) -> Option<Seated> {
    Some(Seated { id: ActorId::new(u64_at(bytes, at)?), seat: SeatNumber::new(u64_at(bytes, at + 8)?) })
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
