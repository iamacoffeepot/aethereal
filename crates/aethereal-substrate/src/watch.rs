//! The view of who watches each actor (ADR-0009, ADR-0011).

use crate::{Context, View};
use aethereal_actor::{Effect, What};
use std::collections::{BTreeMap, BTreeSet};

/// The view of who watches each actor, keyed by the id of the actor watched (ADR-0011).
///
/// Many actors write one key, each adding itself, and a set comes out the
/// same in whatever order they do. Once the watched actor's closing turn has
/// run, its key says so, and keeps its watchers.
pub struct Watchers;

/// One watched actor's entry.
#[derive(Default)]
pub struct Watched {
    /// The actors that watch it.
    by: BTreeSet<u64>,
    /// Whether its life is over.
    life: Watching,
}

/// Whether a watched actor's life is over.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Watching {
    /// Its closing turn has not run.
    #[default]
    Running,
    /// Its closing turn has run.
    Over,
}

/// What the view keeps of an effect.
pub enum Record {
    /// The actor whose bits these are watches the key's actor.
    Watch { by: u64 },
    /// The key's actor's closing turn has run.
    Over,
}

impl Watched {
    /// The actors that watch it.
    pub const fn by(&self) -> &BTreeSet<u64> {
        &self.by
    }

    /// Whether its closing turn has run.
    pub fn is_over(&self) -> bool {
        self.life == Watching::Over
    }
}

impl View for Watchers {
    type Shard = BTreeMap<u64, Watched>;
    type Record = Record;
    type Reads<'a> = ();

    fn takes<'a>(effect: &'a Effect, _: &'a Context, (): ()) -> impl Iterator<Item = (u64, Record)> + 'a {
        let by = effect.by().bits();
        let record = match effect.what() {
            What::Watch { of } => Some((of.id().bits(), Record::Watch { by })),
            What::Ended(_) => Some((by, Record::Over)),
            _ => None,
        };

        record.into_iter()
    }

    fn fold(shard: &mut BTreeMap<u64, Watched>, key: u64, record: Record) {
        let watched = shard.entry(key).or_default();
        match record {
            Record::Watch { by } => {
                watched.by.insert(by);
            }
            Record::Over => watched.life = Watching::Over,
        }
    }
}
