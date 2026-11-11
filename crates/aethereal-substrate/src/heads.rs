//! The view of each kind's head: the blob last published for it, numbered by the engine (ADR-0009, ADR-0011,
//! ADR-0054).

use crate::{Context, View};
use aethereal_actor::{Arena, Bytes, Effect, What};
use aethereal_data::KindName;
use std::collections::HashMap;

/// The view of each kind's head, folded from `Publish` (ADR-0054).
///
/// Every head is kept under one key, so the publishes of a step are folded
/// in one part, in the order they were asked: only the actor that holds the
/// engine's one publisher asks one, so the order is its own. Each publish is
/// numbered as it is folded, one more than the last, and the number is the
/// head's. A head keeps a pin on its blob, so the blob is kept while it is
/// the head, and let go of once another blob is published for the kind.
pub struct Heads;

/// One part of the view: every head, in the part their key falls in, and none in the others.
#[derive(Default)]
pub struct Shard {
    /// The head of each kind code has been published for.
    heads: HashMap<KindName, Head>,
    /// How many publishes have been folded: the number of the last, none before the first.
    published: u64,
}

/// A kind's head: the blob last published for it, and that publish's number (ADR-0054).
#[derive(Debug)]
pub struct Head {
    blob: Bytes,
    number: u64,
}

/// What the view keeps of a publish: the kind, and a pin of its own on the blob.
pub struct Record {
    kind: KindName,
    blob: Bytes,
}

/// The key every head is kept under.
pub const HEADS: u64 = 0;

impl Head {
    /// The blob last published for the kind.
    #[must_use]
    pub const fn blob(&self) -> &Bytes {
        &self.blob
    }

    /// The number of the publish that made it the head, from one, counted over every kind (ADR-0054).
    #[must_use]
    pub const fn number(&self) -> u64 {
        self.number
    }
}

impl Shard {
    /// The head of the kind `kind`, as of the last fold: nothing when no code has been published for it.
    #[must_use]
    pub fn head(&self, kind: &KindName) -> Option<&Head> {
        self.heads.get(kind)
    }

    /// How many publishes have been folded: the number of the last, nought before the first.
    #[must_use]
    pub const fn published(&self) -> u64 {
        self.published
    }

    /// Each kind whose head a publish numbered past `after` made, as of the last fold (ADR-0054).
    pub fn since(&self, after: u64) -> impl Iterator<Item = &KindName> {
        self.heads.iter().filter(move |(_, head)| head.number > after).map(|(kind, _)| kind)
    }
}

impl View for Heads {
    type Shard = Shard;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        let record = match effect.what() {
            What::Publish { kind, blob } => Some((HEADS, Record { kind: kind.clone(), blob: blob.clone() })),
            _ => None,
        };

        record.into_iter()
    }

    /// The kind's head is the record's blob, under the next number: the pin on the blob it replaces is dropped.
    fn fold(shard: &mut Shard, _: u64, Record { kind, blob }: Record, _: &[Arena]) {
        shard.published += 1;
        shard.heads.insert(kind, Head { blob, number: shard.published });
    }
}
