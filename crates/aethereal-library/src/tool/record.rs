//! What the library's tools answer: a publish's record, kind by kind, and the guests behind their kinds' heads, each
//! read from what the engine records of the moves onto heads (ADR-0054).

use crate::Numbered;
use aethereal_actor::{Behind, HeadMoves, Standing, Standings};
use aethereal_data::{KindName, Plain};
use aethereal_tools::spoken_name;
use core::cmp::Ordering;

/// What `publish` answers: the module's hash, and each kind it makes with what its publish has come to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.publish_record")]
pub struct PublishRecord {
    /// The module's content hash, in hexadecimal.
    pub blob: String,
    /// Each kind the module makes, sorted.
    pub kinds: Vec<KindRecord>,
}

/// One kind a publish made the head of, and what that came to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.kind_record")]
pub struct KindRecord {
    pub kind: KindName,
    /// The number of the publish.
    pub publish: u64,
    pub moves: KindMoves,
}

/// What a publish of a kind has come to.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.kind_moves")]
pub enum KindMoves {
    /// It is the kind's head: who has moved onto it, who waits, who is parked, and who was still to be asked.
    Head(HeadRecord),
    /// Another publish has made the kind's head since: its number.
    Replaced { publish: u64 },
}

/// Who has moved onto a kind's head, and who has not.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.head_record")]
pub struct HeadRecord {
    /// How many actors moved onto it.
    pub moved: u64,
    /// The fields the old states of those that moved lacked, which their new code worked out.
    pub absent: Vec<String>,
    /// The actors that answered `Later`.
    pub waiting: Vec<WaitingGuest>,
    /// The actors whose move failed, which run on their old code until the head moves again.
    pub parked: Vec<ParkedGuest>,
    /// The actors still to be asked, before their next batch, when the wait ended.
    pub unasked: Vec<String>,
}

/// An actor that answered `Later`, by path, and before how many batches it did.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.waiting_guest")]
pub struct WaitingGuest {
    pub actor: String,
    pub batches: u32,
}

/// An actor whose move failed, by path, and why.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.parked_guest")]
pub struct ParkedGuest {
    pub actor: String,
    pub reason: String,
}

/// What `upgrades` answers: every running guest the engine found behind its kind's head.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.upgrading")]
pub struct Upgrading {
    /// Each guest, sorted by kind.
    pub guests: Vec<BehindGuest>,
}

/// A running guest behind its kind's head.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.behind_guest")]
pub struct BehindGuest {
    /// Its path.
    pub actor: String,
    pub kind: KindName,
    /// The number of the publish whose head it is behind, or failed to move onto.
    pub publish: u64,
    pub standing: GuestStanding,
}

/// Where a guest behind its kind's head stands.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.guest_standing")]
pub enum GuestStanding {
    /// It is still to be asked, before its next batch.
    Unasked,
    /// It answered `Later` before this many batches.
    Later { batches: u32 },
    /// Its move failed, for this reason: it runs on its old code until its kind's head moves again.
    Parked { reason: String },
}

impl KindRecord {
    /// What the publish `numbered` has come to, as `standings` shows it: none while the engine has not yet folded it.
    #[must_use]
    pub fn of(numbered: &Numbered, standings: &Standings) -> Option<Self> {
        let Numbered { kind, publish } = numbered;
        let head = standings.head(kind)?;
        let moves = match head.publish.cmp(publish) {
            Ordering::Greater => KindMoves::Replaced { publish: head.publish },
            Ordering::Equal => KindMoves::Head(HeadRecord::of(numbered, head, standings)),
            Ordering::Less => return None,
        };

        Some(Self { kind: kind.clone(), publish: *publish, moves })
    }

    /// Whether every running actor of its kind has moved, is parked, or has answered `Later`: none is still to be
    /// asked, or another publish has made the head since.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        match &self.moves {
            KindMoves::Head(record) => record.unasked.is_empty(),
            KindMoves::Replaced { .. } => true,
        }
    }
}

impl HeadRecord {
    /// Who has moved onto `head`, the head the publish `numbered` made, and who has not, as `standings` shows them.
    fn of(Numbered { kind, publish }: &Numbered, head: &HeadMoves, standings: &Standings) -> Self {
        let mut record = Self {
            moved: head.moved,
            absent: head.absent.clone(),
            waiting: Vec::new(),
            parked: Vec::new(),
            unasked: Vec::new(),
        };
        let behind = standings.behind_of(kind).filter(|behind| behind.publish == *publish);
        for Behind { actor, standing, .. } in behind {
            let actor = spoken_name(actor);
            match standing {
                Standing::Unasked => record.unasked.push(actor),
                Standing::Later { batches } => record.waiting.push(WaitingGuest { actor, batches: *batches }),
                Standing::Parked { reason } => record.parked.push(ParkedGuest { actor, reason: reason.clone() }),
            }
        }

        record
    }
}

impl Upgrading {
    /// Every guest `standings` shows behind its kind's head.
    #[must_use]
    pub fn of(standings: &Standings) -> Self {
        let guests = standings.behind.iter().map(|behind| BehindGuest {
            actor: spoken_name(&behind.actor),
            kind: behind.kind.clone(),
            publish: behind.publish,
            standing: GuestStanding::of(&behind.standing),
        });

        Self { guests: guests.collect() }
    }
}

impl GuestStanding {
    /// `standing` as the agent reads it.
    fn of(standing: &Standing) -> Self {
        match standing {
            Standing::Unasked => Self::Unasked,
            Standing::Later { batches } => Self::Later { batches: *batches },
            Standing::Parked { reason } => Self::Parked { reason: reason.clone() },
        }
    }
}
