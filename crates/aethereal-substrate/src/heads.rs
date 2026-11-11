//! The view of each kind's head: the blob last published for it, numbered by the engine, and the actors that moved
//! onto it; and each guest found behind its kind's head, with what became of asking it (ADR-0009, ADR-0011,
//! ADR-0054).

use crate::{ByActor, Context, View};
use aethereal_actor::{ActorName, Arena, Behind, Bytes, Effect, HeadMoves, Standing, Standings, Upgraded, What};
use aethereal_data::{ActorId, KindName};
use std::collections::{BTreeSet, HashMap};

/// The view of each kind's head, folded from `Publish`, and of each guest behind its kind's head, folded from what
/// the engine records as it asks one whether to move (ADR-0054).
///
/// Every head is kept under one key, so the publishes of a step are folded
/// in one part, in the order they were asked: only the actor that holds the
/// engine's one publisher asks one, so the order is its own. Each publish is
/// numbered as it is folded, one more than the last, and the number is the
/// head's. A head keeps a pin on its blob, so the blob is kept while it is
/// the head, and let go of once another blob is published for the kind.
///
/// What became of asking each guest is kept under the same key: an actor's
/// asks are recorded by the worker that runs its part, or between steps, so
/// they arrive in the order they were made. A guest is kept from when the
/// engine finds it behind its kind's head until it moves or its life is over.
pub struct Heads;

/// One part of the view: every head and every guest behind one, in the part their key falls in, and none in the
/// others.
#[derive(Default)]
pub struct Shard {
    /// The head of each kind code has been published for.
    heads: HashMap<KindName, Head>,
    /// How many publishes have been folded: the number of the last, none before the first.
    published: u64,
    /// Each guest found behind its kind's head, by its id, until it moves or its life is over.
    behind: ByActor<Found>,
}

/// A kind's head: the blob last published for it, that publish's number, and the actors that moved onto it
/// (ADR-0054).
#[derive(Debug)]
pub struct Head {
    blob: Bytes,
    number: u64,
    /// How many actors have moved onto it.
    moved: u64,
    /// The fields the old states of those that moved lacked.
    absent: BTreeSet<String>,
}

/// A guest found behind its kind's head, and what became of asking it about that head.
struct Found {
    actor: ActorId,
    kind: KindName,
    /// The number of the publish that made the head.
    publish: u64,
    standing: Standing,
}

/// What the view keeps of a publish, of an ask whether to move onto a head, or of an actor whose life is over.
pub enum Record {
    /// The blob, with a pin of the view's own, published as the kind's head under the publish numbered `number`.
    Published { kind: KindName, blob: Bytes, number: u64 },
    /// The actor, of the kind, was found behind the head the publish numbered `publish` made, and this became of it.
    Asked { actor: ActorId, kind: KindName, publish: u64, became: Became },
    /// The actor's closing turn has run.
    Over(ActorId),
}

/// What became of a guest found behind its kind's head (ADR-0054).
pub enum Became {
    /// It had mail as the head moved, and is asked before its next batch.
    Unasked,
    /// It answered `Later`.
    Later,
    /// It moved, and its old state lacked these fields.
    Moved { absent: Vec<String> },
    /// Its move failed, for this reason, and it is parked.
    Parked { reason: String },
    /// A call into its instance broke, and it ended.
    Ended,
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

impl Became {
    /// What became of the actor `upgraded` says of, when it was asked: nothing when it was not.
    #[must_use]
    pub fn of(upgraded: &Upgraded) -> Option<Self> {
        let became = match upgraded {
            Upgraded::Unasked => return None,
            Upgraded::Later => Self::Later,
            Upgraded::Moved { absent, .. } => Self::Moved { absent: absent.clone() },
            Upgraded::Parked { reason, .. } => Self::Parked { reason: reason.clone() },
            Upgraded::BrokeAsking { .. } | Upgraded::BrokeMoving { .. } => Self::Ended,
        };

        Some(became)
    }
}

/// The key and record of what became of the actor `actor`, of the kind `kind`, found behind the head the publish
/// numbered `publish` made.
#[must_use]
pub const fn asked(actor: ActorId, kind: KindName, publish: u64, became: Became) -> (u64, Record) {
    (HEADS, Record::Asked { actor, kind, publish, became })
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

    /// Each head with what its publish moved, sorted by kind, and each guest found behind its kind's head that `name`
    /// names, sorted by kind and then by the order of their ids: `name` names a live actor, and passes over one that is
    /// not (ADR-0054).
    pub fn standings(&self, mut name: impl FnMut(ActorId) -> Option<ActorName>) -> Standings {
        let mut heads: Vec<HeadMoves> = self
            .heads
            .iter()
            .map(|(kind, head)| HeadMoves {
                kind: kind.clone(),
                publish: head.number,
                moved: head.moved,
                absent: head.absent.iter().cloned().collect(),
            })
            .collect();
        heads.sort_by(|a, b| a.kind.cmp(&b.kind));

        let mut found: Vec<&Found> = self.behind.values().collect();
        found.sort_by(|a, b| (&a.kind, a.actor.bits()).cmp(&(&b.kind, b.actor.bits())));
        let behind = found
            .into_iter()
            .filter_map(|found| {
                let actor = name(found.actor)?;
                let (kind, publish, standing) = (found.kind.clone(), found.publish, found.standing.clone());

                Some(Behind { actor, kind, publish, standing })
            })
            .collect();

        Standings { heads, behind }
    }

    /// Takes in what became of `actor`, of the kind `kind`, found behind the head the publish numbered `publish`
    /// made: a `Later` about the same head counts one more batch, a move counts on the head it moved onto while that
    /// is still the kind's head, and a move or an ending lets the actor go.
    fn ask(&mut self, actor: ActorId, kind: KindName, publish: u64, became: Became) {
        let standing = match became {
            Became::Unasked => Standing::Unasked,
            Became::Later => {
                let batches = match self.behind.get(&actor.bits()) {
                    Some(Found { publish: was, standing: Standing::Later { batches }, .. }) if *was == publish => {
                        batches.saturating_add(1)
                    }
                    _ => 1,
                };
                Standing::Later { batches }
            }
            Became::Parked { reason } => Standing::Parked { reason },
            Became::Moved { absent } => {
                self.behind.remove(&actor.bits());
                if let Some(head) = self.heads.get_mut(&kind)
                    && head.number == publish
                {
                    head.moved += 1;
                    head.absent.extend(absent);
                }
                return;
            }
            Became::Ended => {
                self.behind.remove(&actor.bits());
                return;
            }
        };

        self.behind.insert(actor.bits(), Found { actor, kind, publish, standing });
    }
}

impl View for Heads {
    type Shard = Shard;
    type Record = Record;

    fn takes<'a>(effect: &'a Effect, _: &'a Context) -> impl Iterator<Item = (u64, Record)> + 'a {
        let record = match effect.what() {
            What::Publish { kind, blob, number } => {
                let published = Record::Published { kind: kind.clone(), blob: blob.clone(), number: *number };
                Some((HEADS, published))
            }
            What::Ended(_) => Some((HEADS, Record::Over(effect.seated().id))),
            _ => None,
        };

        record.into_iter()
    }

    /// A publish makes the record's blob the kind's head, under the number it was asked with, which no actor has moved
    /// onto yet: the pin on the blob it replaces is dropped. An ask is taken in as [`Shard::ask`] says, and a life that
    /// is over lets its actor go.
    fn fold(shard: &mut Shard, _: u64, record: Record, _: &[Arena]) {
        match record {
            Record::Published { kind, blob, number } => {
                shard.published = number;
                let head = Head { blob, number, moved: 0, absent: BTreeSet::new() };
                shard.heads.insert(kind, head);
            }
            Record::Asked { actor, kind, publish, became } => shard.ask(actor, kind, publish, became),
            Record::Over(actor) => {
                shard.behind.remove(&actor.bits());
            }
        }
    }
}
