//! What the library's tools answer: a publish, kind by kind, and what the engine records of the moves onto every
//! kind's head, read when an agent asks (ADR-0054).

use crate::Numbered;
use aethereal_actor::{HeadMoves, Standing, Standings};
use aethereal_data::{KindName, Plain};
use aethereal_tools::spoken_name;

/// What `publish` answers: the module's hash, and each kind it makes with the number the engine gave its publish.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.publish_record")]
pub struct PublishRecord {
    /// The module's content hash, in hexadecimal.
    pub blob: String,
    /// Each kind the module makes, sorted, with the number of its publish: the head's, as `upgrades` shows it.
    pub kinds: Vec<Numbered>,
}

/// What `upgrades` answers: every kind's head with what its publish has moved, and every running guest the engine
/// found behind its kind's head.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.upgrading")]
pub struct Upgrading {
    /// Each kind code has been published for, sorted by kind.
    pub heads: Vec<HeadRecord>,
    /// Each guest, sorted by kind.
    pub guests: Vec<BehindGuest>,
}

/// A kind's head, and the actors that moved onto it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.library.head_record")]
pub struct HeadRecord {
    pub kind: KindName,
    /// The number of the publish that made it the head.
    pub publish: u64,
    /// How many actors moved onto it.
    pub moved: u64,
    /// The fields the old states of those that moved lacked, which their new code worked out.
    pub absent: Vec<String>,
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

impl Upgrading {
    /// Every head `standings` shows, with what its publish has moved, and every guest it shows behind its kind's head.
    #[must_use]
    pub fn of(standings: &Standings) -> Self {
        let heads = standings.heads.iter().map(HeadRecord::of);
        let guests = standings.behind.iter().map(|behind| BehindGuest {
            actor: spoken_name(&behind.actor),
            kind: behind.kind.clone(),
            publish: behind.publish,
            standing: GuestStanding::of(&behind.standing),
        });

        Self { heads: heads.collect(), guests: guests.collect() }
    }
}

impl HeadRecord {
    /// `head` as the agent reads it.
    fn of(head: &HeadMoves) -> Self {
        let HeadMoves { kind, publish, moved, absent } = head;

        Self { kind: kind.clone(), publish: *publish, moved: *moved, absent: absent.clone() }
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
