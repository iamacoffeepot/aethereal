//! What crosses between a guest and its host, as fixed-width records each side reads where the other wrote them
//! (ADR-0015).
//!
//! The host writes a batch's items into the guest's inbox, each an [`ItemHead`]
//! and then its bytes: a piece of mail's, or the fault of a watched actor that
//! failed. After the call the guest's out buffer holds an
//! [`OutHead`], a [`TurnEnds`] for each turn it ran, then a [`SendRecord`] for
//! each piece of mail its turns sent, a [`LineRecord`] for each line they
//! logged, a [`SpawnRecord`] for each child they spawned, a [`StoodRecord`]
//! for each child that began, a [`WatchRecord`] for each actor they
//! watched, and a [`ClaimRecord`] for each claim they made of a resource; and, for the call as a whole, a [`PinRecord`] for each blob whose
//! pins it changed, and a [`RefRecord`] for each reference in what it sent. A send's bytes, a line's text, a key and a moved
//! child's state are not copied out: each record says where they lie in the
//! guest's memory, and the host reads them there. Sends and lines say who made
//! them, as watches do: the actor whose turn it was, or a child it spawned, in
//! its `begin`.
//!
//! A child of other code, a native kind or another blob's, the guest asks its
//! host to begin in a call out, with a [`Spawning`]; the host answers as
//! [`refusal`] writes it.
//!
//! Every number is little-endian, and every address is one in the guest's
//! 32-bit memory.

use crate::{BlobId, Chain, Claim, Claimed, Fault, Level, MailId, Refused, SeatNumber, Seated, Why};
use aethereal_data::{ActorId, ActorPath, Budget, KeyName, KindId, KindName, PathSegment, Plain, from_bytes};
use alloc::vec::Vec;

/// The head of an item in the inbox: what it is, its source's id and seat, its chain's root, sender and count, its
/// depth, and the length of the bytes after it, a context's included.
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
    /// The answer, of `kind`, to the receiver's request of the count `request`: its bytes after the head, and then the
    /// last `context` of them the context the receiver kept for it (ADR-0017).
    Answer { kind: KindId, request: u64, context: u32 },
    /// What the engine says of the receiver's request of the count `request`, answered with a kind `answer` names,
    /// which went unanswered: an [`Unanswered`](crate::Unanswered)'s bytes after the head, and then the last `context`
    /// of them the context the receiver kept for it (ADR-0017).
    Unanswered { answer: KindId, request: u64, context: u32 },
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
    pub const BYTES: usize = 64;

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
            Handed::Answer { kind, request, context } => {
                out[..8].copy_from_slice(&kind.plain_bytes());
                out[52..60].copy_from_slice(&request.to_le_bytes());
                out[60..64].copy_from_slice(&context.to_le_bytes());
                4
            }
            Handed::Unanswered { answer, request, context } => {
                out[..8].copy_from_slice(&answer.plain_bytes());
                out[52..60].copy_from_slice(&request.to_le_bytes());
                out[60..64].copy_from_slice(&context.to_le_bytes());
                5
            }
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
            4 => Handed::Answer { kind: kind_at(head, 0)?, request: u64_at(head, 52)?, context: u32_at(head, 60)? },
            5 => {
                Handed::Unanswered { answer: kind_at(head, 0)?, request: u64_at(head, 52)?, context: u32_at(head, 60)? }
            }
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
    /// [`WatchRecord`]s begin. The [`ClaimRecord`]s follow the last watch: [`Self::claims_at`] says where.
    pub turns_at: u32,
    pub sends_at: u32,
    pub lines_at: u32,
    pub spawns_at: u32,
    pub stood_at: u32,
    pub watches_at: u32,
    /// Where the call's [`PinRecord`]s begin, and how many there are.
    pub pins_at: u32,
    pub pins: u32,
    /// Where the call's [`RefRecord`]s begin, and how many there are.
    pub refs_at: u32,
    pub refs: u32,
}

/// Where some bytes lie in a guest's memory: their address, and how many there are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub at: u32,
    pub len: u32,
}

impl OutHead {
    /// How many bytes it is.
    pub const BYTES: usize = 64;

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
        out[48..52].copy_from_slice(&self.pins_at.to_le_bytes());
        out[52..56].copy_from_slice(&self.pins.to_le_bytes());
        out[56..60].copy_from_slice(&self.refs_at.to_le_bytes());
        out[60..64].copy_from_slice(&self.refs.to_le_bytes());

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
            pins_at: u32_at(head, 48)?,
            pins: u32_at(head, 52)?,
            refs_at: u32_at(head, 56)?,
            refs: u32_at(head, 60)?,
        })
    }

    /// Where the call's [`ClaimRecord`]s begin, right after its `watches` watches: how many its last turn's
    /// [`TurnEnds`] count. Claims are seldom made, so the head keeps no room for where they lie (ADR-0010).
    #[must_use]
    pub fn claims_at(&self, watches: u32) -> Option<u32> {
        let span = watches.checked_mul(u32::try_from(WatchRecord::BYTES).ok()?)?;

        self.watches_at.checked_add(span)
    }
}

/// Where one turn's records end: how many sends, lines, spawns, children stood up, watches and claims the turns up to
/// and including it asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurnEnds {
    pub sends: u32,
    pub lines: u32,
    pub spawns: u32,
    pub stood: u32,
    pub watches: u32,
    pub claims: u32,
}

impl TurnEnds {
    /// How many bytes it is.
    pub const BYTES: usize = 24;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..4].copy_from_slice(&self.sends.to_le_bytes());
        out[4..8].copy_from_slice(&self.lines.to_le_bytes());
        out[8..12].copy_from_slice(&self.spawns.to_le_bytes());
        out[12..16].copy_from_slice(&self.stood.to_le_bytes());
        out[16..20].copy_from_slice(&self.watches.to_le_bytes());
        out[20..].copy_from_slice(&self.claims.to_le_bytes());

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
            claims: u32_at(bytes, 20)?,
        })
    }
}

/// A piece of mail a turn sent: by whom, to whom, of what kind, where its bytes lie, and what it is to a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendRecord {
    pub by: Seated,
    pub to: Seated,
    pub kind: KindId,
    pub bytes: Span,
    pub request: Requesting,
}

/// What a piece of mail a guest sent is to a request, as its code says (ADR-0017). Its host checks it, and counts the
/// request itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requesting {
    /// Nothing the guest says: its host tells whether it is a reply from what its handler declares.
    Not,
    /// It asks a request, to be answered with a kind `answer` names, the context the guest keeps for it where `context`
    /// says.
    Asks { answer: KindId, context: Span },
}

impl SendRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 68;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..16].copy_from_slice(&seated_bytes(self.by));
        out[16..32].copy_from_slice(&seated_bytes(self.to));
        out[32..40].copy_from_slice(&self.kind.plain_bytes());
        out[40..44].copy_from_slice(&self.bytes.at.to_le_bytes());
        out[44..48].copy_from_slice(&self.bytes.len.to_le_bytes());
        let requesting: u32 = match self.request {
            Requesting::Not => 0,
            Requesting::Asks { answer, context } => {
                out[52..60].copy_from_slice(&answer.plain_bytes());
                out[60..64].copy_from_slice(&context.at.to_le_bytes());
                out[64..68].copy_from_slice(&context.len.to_le_bytes());
                1
            }
        };
        out[48..52].copy_from_slice(&requesting.to_le_bytes());

        out
    }

    /// The record `bytes` begin with: nothing when they are too few, or name no kind.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let span = Span { at: u32_at(bytes, 40)?, len: u32_at(bytes, 44)? };
        let request = match u32_at(bytes, 48)? {
            0 => Requesting::Not,
            1 => Requesting::Asks {
                answer: kind_at(bytes, 52)?,
                context: Span { at: u32_at(bytes, 60)?, len: u32_at(bytes, 64)? },
            },
            _ => return None,
        };

        Some(Self {
            by: seated_at(bytes, 0)?,
            to: seated_at(bytes, 16)?,
            kind: kind_at(bytes, 32)?,
            bytes: span,
            request,
        })
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

/// A child of other code a guest's turn spawns, as it asks its host to begin it: which of the call's turns spawns it,
/// counted from zero, and who; the child, by its id and the seat the host gave it; where it stands; its code; its
/// type's kind and what the guest holds that type declares, as a section writes it; and its config's bytes.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Spawning {
    pub turn: u32,
    pub by: u64,
    pub by_seat: u64,
    pub me: u64,
    pub me_seat: u64,
    pub path: ActorPath,
    pub from: Other,
    pub kind: KindName,
    pub declared: Vec<u8>,
    pub config: Vec<u8>,
    /// Where each reference in the config begins.
    pub refs: Vec<u32>,
    /// The tickets of the resources the config holds, which the host hands the child as it begins it (ADR-0010).
    pub carried: Vec<u64>,
}

/// The code of a child a guest asks its host to begin: a native kind the program keeps by name, or a blob's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Other {
    Linked,
    Blob { engine: u32, bits: u64 },
}

impl Spawning {
    /// Who spawns the child.
    #[must_use]
    pub const fn by(&self) -> Seated {
        Seated { id: ActorId::new(self.by), seat: SeatNumber::new(self.by_seat) }
    }

    /// The child.
    #[must_use]
    pub const fn me(&self) -> Seated {
        Seated { id: ActorId::new(self.me), seat: SeatNumber::new(self.me_seat) }
    }
}

/// What a host answers a [`Spawning`] with: zero when the child began, and else why it was refused, in the low 32
/// bits, with the length of the bytes of the fault its `begin` returned in the high 32 bits, for the guest to make
/// room for before it asks for them.
#[must_use]
pub fn refusal(refused: &Refused) -> u64 {
    match refused {
        Refused::KeyTaken => 1,
        Refused::TooDeep => 2,
        Refused::Began(fault) => 3 | u64::try_from(fault.bytes().len()).unwrap_or(0) << 32,
        Refused::NoRoom => 4,
        Refused::NotMade => 5,
        Refused::Waiting => 6,
    }
}

/// How a spawn whose host answered `answer`, as [`refusal`] writes it, ended: the fault of a `begin` that failed read
/// with `fault`, handed the length of its bytes, which answers with the fault's reason's kind and its bytes.
///
/// # Errors
///
/// Why the spawn was refused.
pub fn refused(answer: u64, fault: impl FnOnce(usize) -> Option<(KindId, Vec<u8>)>) -> Result<(), Refused> {
    let refused = match answer & u64::from(u32::MAX) {
        0 => return Ok(()),
        1 => Refused::KeyTaken,
        2 => Refused::TooDeep,
        3 => {
            let len = usize::try_from(answer >> 32).unwrap_or(0);
            match fault(len) {
                Some((kind, bytes)) => Refused::Began(Fault::of_parts(kind, bytes)),
                None => Refused::NotMade,
            }
        }
        4 => Refused::NoRoom,
        6 => Refused::Waiting,
        _ => Refused::NotMade,
    };

    Err(refused)
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

/// How a call changed the pins on one blob, summed over its turns: what the guest's references to it, made and
/// dropped, came to (ADR-0014).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinRecord {
    pub blob: BlobId,
    pub by: i64,
}

impl PinRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 24;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..4].copy_from_slice(&self.blob.engine().to_le_bytes());
        out[8..16].copy_from_slice(&self.blob.bits().to_le_bytes());
        out[16..].copy_from_slice(&self.by.to_le_bytes());

        out
    }

    /// The record `bytes` begin with.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let by = i64::from_le_bytes(bytes.get(16..24)?.try_into().ok()?);

        Some(Self { blob: BlobId::new(u32_at(bytes, 0)?, u64_at(bytes, 8)?), by })
    }
}

/// A reference in a piece of mail a call sent: the send it is in, as its [`SendRecord`] is numbered among the call's,
/// and where its bytes begin in the mail's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefRecord {
    pub send: u32,
    pub at: u32,
}

impl RefRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 8;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let mut out = [0; Self::BYTES];
        out[..4].copy_from_slice(&self.send.to_le_bytes());
        out[4..].copy_from_slice(&self.at.to_le_bytes());

        out
    }

    /// The record `bytes` begin with.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        Some(Self { send: u32_at(bytes, 0)?, at: u32_at(bytes, 4)? })
    }
}

/// The blob a reference written at [`Actors`](crate::Actors) or [`Within`](crate::Within) names, at the front of
/// `bytes`: its engine's number and its bits, as a reference is written (ADR-0014).
#[must_use]
pub fn blob_at(bytes: &[u8]) -> Option<BlobId> {
    Some(BlobId::new(u32_at(bytes, 0)?, u64_at(bytes, 4)?))
}

/// The bytes of an actor's state taken out of a guest, and the blob of each reference in it, as the guest writes them:
/// the state's length, the state, and then each blob's engine and bits (ADR-0014, ADR-0015).
#[must_use]
pub fn carried(bytes: &[u8]) -> Option<(&[u8], Vec<BlobId>)> {
    let len = usize::try_from(u32_at(bytes, 0)?).ok()?;
    let state = bytes.get(4..4 + len)?;
    let blobs = bytes.get(4 + len..)?.chunks(12).map(blob_at).collect::<Option<Vec<_>>>()?;

    Some((state, blobs))
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

/// A claim a turn made of a resource, or the `begin` of a child it spawned, as [`Claim`] says (ADR-0010).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimRecord(pub Claim);

impl ClaimRecord {
    /// How many bytes it is.
    pub const BYTES: usize = 32;

    /// It, as the out buffer holds it.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; Self::BYTES] {
        let Claim { by, ticket, what } = self.0;
        let (what, send) = match what {
            Claimed::Took => (0_u32, 0),
            Claimed::TookConfig => (1, 0),
            Claimed::Gave => (2, 0),
            Claimed::Carried { send } => (3, send),
            Claimed::Configured => (4, 0),
        };
        let mut out = [0; Self::BYTES];
        out[..16].copy_from_slice(&seated_bytes(by));
        out[16..24].copy_from_slice(&ticket.to_le_bytes());
        out[24..28].copy_from_slice(&what.to_le_bytes());
        out[28..].copy_from_slice(&send.to_le_bytes());

        out
    }

    /// The record `bytes` begin with: nothing when they are too few, or name no claim.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let what = match u32_at(bytes, 24)? {
            0 => Claimed::Took,
            1 => Claimed::TookConfig,
            2 => Claimed::Gave,
            3 => Claimed::Carried { send: u32_at(bytes, 28)? },
            4 => Claimed::Configured,
            _ => return None,
        };

        Some(Self(Claim { by: seated_at(bytes, 0)?, ticket: u64_at(bytes, 16)?, what }))
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
    /// Nowhere in the guest: it is of other code, which its host began and holds.
    Elsewhere,
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
            Kept::Elsewhere => 2,
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
            2 => Kept::Elsewhere,
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

/// Why a closing turn runs, as its host hands it to the guest: one number.
#[must_use]
pub const fn why_code(why: Why) -> u32 {
    match why {
        Why::Closed => 0,
        Why::Faulted => 1,
    }
}

/// Why a closing turn runs, from the number its host handed in: nothing for a number no reason has.
#[must_use]
pub const fn why_of(code: u32) -> Option<Why> {
    match code {
        0 => Some(Why::Closed),
        1 => Some(Why::Faulted),
        _ => None,
    }
}
