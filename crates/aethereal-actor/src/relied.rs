//! What a running guest is relied on for: each kind a reference to it was proven for, what that proof allows it to be
//! answered with, and who proved it (ADR-0016, ADR-0054).
//!
//! A proof made at run time, a cast or a guard proving a sender, asks for what it relies on as an effect,
//! [`What::Relied`](crate::What::Relied), when the guest it proves was not already relied on for it as the step began,
//! through [`Table::rely`](crate::Table::rely), as a spawn by name will (ADR-0059). A proof made at build time relies on
//! nothing. The engine keeps each live guest's reliances beside its entry in the table, and they only grow while it
//! runs.

use crate::BlobId;
use crate::effect::{Effect, What};
use crate::seat::Seated;
use aethereal_data::{ActorId, Budget, KindId, from_bytes};
use alloc::vec::Vec;

/// One kind a protocol takes, and what a reference proven for it allows that kind to be answered with: what a proof
/// relies on its target for, as the protocol names it, fixed when the program is built (ADR-0016).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Relying {
    pub kind: KindId,
    pub answer: AnsweredWith,
}

/// What a proof allows a kind to be answered with: a handler answers with one kind or none (ADR-0059).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsweredWith {
    /// Nothing: the kind is told, as every kind of a tuple of kinds is.
    Nothing,
    /// The kind of this id.
    Kind(KindId),
}

/// Who proved a reference: whose code holds it, and moves with it (ADR-0054).
///
/// A module is named by its blob's id written out in its two parts, so the case's tag lies in the padding between
/// them, a prover takes sixteen bytes, and an effect that asks for a reliance is no wider than the widest other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Prover {
    /// The program's own code, a native actor's, which moves only with a rebuild.
    Program,
    /// A guest's module: the blob of its code, the one [`BlobId::new`] makes of `engine` and `bits`.
    Module { engine: u32, bits: u64 },
}

/// One thing a guest is relied on for: a kind a reference to it was proven for, an answer the proof allows, and who
/// proved it (ADR-0054). A kind proven again by the same prover with another answer is a second reliance beside the
/// first, so the two together keep the answers both proofs allow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reliance {
    pub kind: KindId,
    pub answer: AnsweredWith,
    pub prover: Prover,
}

/// What one live guest is relied on for, each reliance once, in one order whatever order they were folded in, so the
/// same turns always leave the same reliances (ADR-0011).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Reliances {
    /// Each reliance, ordered by kind, then prover, then answer.
    held: Vec<Reliance>,
}

/// What a table holds of the actor a proof relies on, as the step began (ADR-0054).
#[derive(Debug, Clone, Copy)]
pub enum Relied<'t> {
    /// A native actor: its code is the program's own and never moves, so a proof of it relies on nothing.
    Native,
    /// A live guest, relied on for these.
    Guest(&'t Reliances),
    /// An actor the table did not hold as the step began: stood up since, or ended. A proof of it asks for every
    /// reliance, and the fold keeps one only for a live guest.
    Unheld,
}

impl Relying {
    /// How many bytes [`Self::to_bytes`] writes.
    pub const BYTES: usize = 17;

    /// How many bytes the most a protocol lists, 64 kinds, take written one after another: what a guest writes a
    /// proof's into on its stack, for its host to read.
    pub const ROOM: usize = 64 * Self::BYTES;

    /// Writes each of `relying` into `room`, one after another as [`Self::to_bytes`] writes it, as many as it has room
    /// for: answers with how many bytes they take.
    pub fn write_all(relying: &[Self], room: &mut [u8; Self::ROOM]) -> usize {
        let mut len = 0;
        for (slot, relying) in room.chunks_exact_mut(Self::BYTES).zip(relying) {
            slot.copy_from_slice(&relying.to_bytes());
            len += Self::BYTES;
        }

        len
    }

    /// Each one `written` holds, one after another as [`Self::to_bytes`] writes them, read where they lie: none when
    /// one does not read back, or bytes are left over.
    #[must_use]
    pub fn read_all(written: &[u8]) -> Option<impl Iterator<Item = Self> + Clone + '_> {
        let chunks = written.chunks_exact(Self::BYTES);
        let is_whole = chunks.remainder().is_empty();
        let reads = is_whole && chunks.clone().all(|chunk| Self::read(chunk).is_some());

        reads.then(|| chunks.filter_map(Self::read))
    }

    /// A kind told, as every kind of a tuple of kinds is.
    #[must_use]
    pub const fn told(kind: KindId) -> Self {
        Self { kind, answer: AnsweredWith::Nothing }
    }

    /// The kind `kind`, answered with the one kind of `answers`, or with nothing when it holds none, as a named
    /// protocol's `Carries` lists them.
    ///
    /// # Panics
    ///
    /// When `answers` holds more than one kind, which a handler cannot answer with: in a constant, the build stops.
    #[must_use]
    pub const fn answered(kind: KindId, answers: &[KindId]) -> Self {
        let answer = match answers {
            [] => AnsweredWith::Nothing,
            [answer] => AnsweredWith::Kind(*answer),
            [..] => panic!("a kind of a protocol is answered with one kind or none"),
        };

        Self { kind, answer }
    }

    /// The reliance it is, proven by `prover`.
    #[must_use]
    pub const fn by(self, prover: Prover) -> Reliance {
        Reliance { kind: self.kind, answer: self.answer, prover }
    }

    /// Its bytes, as a guest hands them to its host: the kind's id, a byte that is one when it is answered, and the
    /// answer's id, or eight zeros for none.
    #[must_use]
    pub const fn to_bytes(self) -> [u8; Self::BYTES] {
        let mut bytes = [0u8; Self::BYTES];
        let kind = self.kind.plain_bytes();
        let mut index = 0;
        while index < 8 {
            bytes[index] = kind[index];
            index += 1;
        }
        if let AnsweredWith::Kind(answer) = self.answer {
            bytes[8] = 1;
            let answer = answer.plain_bytes();
            let mut index = 0;
            while index < 8 {
                bytes[9 + index] = answer[index];
                index += 1;
            }
        }

        bytes
    }

    /// It, from what [`Self::to_bytes`] wrote: none when they do not read back.
    #[must_use]
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let kind = kind_at(bytes.get(..8)?)?;
        let answer = match bytes.get(8)? {
            0 => AnsweredWith::Nothing,
            1 => AnsweredWith::Kind(kind_at(bytes.get(9..Self::BYTES)?)?),
            _ => return None,
        };

        Some(Self { kind, answer })
    }
}

/// The 64 bits of `kind`, as its plain bytes hold them: what reliances are ordered by.
const fn bits(kind: KindId) -> u64 {
    u64::from_le_bytes(kind.plain_bytes())
}

/// The id whose plain bytes are `bytes`: none when they do not read as one.
fn kind_at(bytes: &[u8]) -> Option<KindId> {
    from_bytes::<KindId>(bytes, &mut Budget::new(8)).ok()
}

impl Prover {
    /// The guest's module whose code is the blob `blob`.
    #[must_use]
    pub const fn module(blob: BlobId) -> Self {
        Self::Module { engine: blob.engine(), bits: blob.bits() }
    }
}

impl Reliance {
    /// What it is ordered by among a guest's reliances: its kind, its prover, and its answer, none first.
    fn order(&self) -> (u64, Prover, u64, bool) {
        let (answer, is_answered) = match self.answer {
            AnsweredWith::Nothing => (0, false),
            AnsweredWith::Kind(answer) => (bits(answer), true),
        };

        (bits(self.kind), self.prover, answer, is_answered)
    }

    /// The effect by which the actor `by` asks that the guest `on` stands for be relied on for it.
    #[must_use]
    pub const fn asked(self, by: Seated, on: ActorId) -> Effect {
        Effect::new(by, What::Relied { on, reliance: self })
    }
}

impl Reliances {
    /// Whether it holds `reliance`.
    #[must_use]
    pub fn holds(&self, reliance: &Reliance) -> bool {
        self.held.binary_search_by_key(&reliance.order(), Reliance::order).is_ok()
    }

    /// Adds `reliance`, when it does not hold it already.
    pub fn add(&mut self, reliance: Reliance) {
        if let Err(at) = self.held.binary_search_by_key(&reliance.order(), Reliance::order) {
            self.held.insert(at, reliance);
        }
    }

    /// Each reliance, ordered by kind, then prover, then answer.
    #[must_use]
    pub fn all(&self) -> &[Reliance] {
        &self.held
    }
}

impl Relied<'_> {
    /// Each of `relying`, proven by `prover`, that a proof must ask for: none of a native actor, each a guest is not
    /// relied on for yet, and every one of an actor the table did not hold.
    pub fn lacks(self, prover: Prover, relying: impl IntoIterator<Item = Relying>) -> impl Iterator<Item = Reliance> {
        relying.into_iter().map(move |relying| relying.by(prover)).filter(move |reliance| !self.covers(reliance))
    }

    /// Asks, into `effects`, as the turn of `by`, for each of `relying`, proven by `prover`, that the actor `on` stands
    /// for lacks, as [`Self::lacks`] finds them (ADR-0054).
    pub fn ask(
        self,
        (by, prover): (Seated, Prover),
        on: ActorId,
        relying: impl IntoIterator<Item = Relying>,
        effects: &mut Vec<Effect>,
    ) {
        // Most proofs are of a native actor, which is relied on for nothing, so nothing of the proof is read.
        if matches!(self, Self::Native) {
            return;
        }

        effects.extend(self.lacks(prover, relying).map(|reliance| reliance.asked(by, on)));
    }

    /// Whether a proof need not ask for `reliance`: of a native actor, or of a guest that holds it.
    fn covers(self, reliance: &Reliance) -> bool {
        match self {
            Self::Native => true,
            Self::Guest(reliances) => reliances.holds(reliance),
            Self::Unheld => false,
        }
    }
}
