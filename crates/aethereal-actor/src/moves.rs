//! What a turn reads of guests moving onto their kinds' heads: what each head's publish has moved, and each guest the
//! engine found behind its kind's head (ADR-0054).

use crate::ActorName;
use aethereal_data::KindName;
use alloc::string::String;
use alloc::vec::Vec;

/// What a turn reads of the moves onto kinds' heads, as the heads view stood when the step began (ADR-0054).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Moves {
    /// The table the turn read through reads no heads view: a guest's, as it reads no log.
    Unread,
    /// The heads, and the guests behind them.
    Read(Standings),
}

/// Each kind's head with what its publish has moved, and each live guest the engine found behind its kind's head
/// (ADR-0054).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standings {
    /// Each kind code has been published for: its head's publish, and what that publish has moved.
    pub heads: Vec<HeadMoves>,
    /// Each live guest the engine found behind its kind's head, asked about the head or still to be.
    pub behind: Vec<Behind>,
}

/// A kind's head, and the actors that moved onto it (ADR-0054).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadMoves {
    pub kind: KindName,
    /// The number of the publish that made it the head.
    pub publish: u64,
    /// How many actors have moved onto it.
    pub moved: u64,
    /// The fields the old states of those that moved lacked, each once, sorted.
    pub absent: Vec<String>,
}

/// A live guest that runs other code than its kind's head (ADR-0054).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Behind {
    pub actor: ActorName,
    pub kind: KindName,
    /// The number of the publish whose head it was last found behind.
    pub publish: u64,
    /// What became of it, asked about that head.
    pub standing: Standing,
}

/// What became of a guest behind its kind's head, asked whether to move onto it (ADR-0054).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// It is still to be asked: it had mail as the head moved, and is asked before its next batch.
    Unasked,
    /// It answered `Later` before each of this many batches.
    Later { batches: u32 },
    /// It answered `Now` and its move failed, for `reason`: it runs on its old code until its kind's head moves on.
    Parked { reason: String },
}

impl Standings {
    /// The head of the kind `kind`, when code has been published for it.
    #[must_use]
    pub fn head(&self, kind: &KindName) -> Option<&HeadMoves> {
        self.heads.iter().find(|head| head.kind == *kind)
    }

    /// The guests of the kind `kind` behind its head.
    pub fn behind_of<'a>(&'a self, kind: &'a KindName) -> impl Iterator<Item = &'a Behind> + 'a {
        self.behind.iter().filter(move |behind| behind.kind == *kind)
    }
}
