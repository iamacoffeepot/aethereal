//! The store's index: its entries, names, and holds, and how a check-in is fitted into its budget (ADR-0039).

use crate::record::{Record, Recorded};
use crate::{Binary, Entry, Held, Pinned, StoreHash, StoreName, Stored};
use aethereal_actor::ErasedActorRef;
use aethereal_data::Bound;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

/// One entry an actor holds.
#[derive(Bound)]
struct Holding {
    holder: ErasedActorRef,
    hash: StoreHash,
}

/// What a check-in came to, once fitted into the budget.
pub enum Admitted {
    /// It is in, and these entries were pruned to make room.
    Stored {
        /// Whether its bytes were not already in the store.
        fresh: bool,
        /// The entries pruned, least recently used first.
        pruned: Vec<StoreHash>,
    },
    /// It does not fit, even with every entry it may prune gone: nothing changed.
    Full,
}

/// How many hexadecimal digits of its hash name the launch copy of an entry no name points at.
const HASH_NAME_DIGITS: usize = 12;

/// What a launch found in the index.
pub enum Launchable {
    /// The entry is held for the asker.
    Held {
        /// The entry.
        hash: StoreHash,
        /// The name its launch copy is given.
        name: String,
    },
    /// No entry of that name or hash: nothing is held.
    NotStored,
}

/// The store's index, rebuilt from the records as it begins: each entry's record by its hash, each name's hash, and
/// the holds, which are never written.
#[derive(Bound)]
pub struct Shelves {
    entries: BTreeMap<StoreHash, Record>,
    names: BTreeMap<StoreName, StoreHash>,
    /// Each entry each live actor holds, once each.
    holds: Vec<Holding>,
    /// The count the next check-in or hold takes.
    next_count: u64,
}

impl Shelves {
    /// The index of `entries` and `names`, as a recount found them: a name pointing at no entry is dropped, and counts
    /// carry on from the highest a record holds.
    pub fn rebuilt(entries: Vec<Recorded>, mut names: BTreeMap<StoreName, StoreHash>) -> Self {
        let entries: BTreeMap<StoreHash, Record> =
            entries.into_iter().map(|Recorded { hash, record }| (hash, record)).collect();
        names.retain(|_, hash| entries.contains_key(hash));
        let next_count = entries.values().map(Record::last_count).max().map_or(0, |last| last + 1);

        Self { entries, names, holds: Vec::new(), next_count }
    }

    /// Takes the next count.
    fn count(&mut self) -> u64 {
        let count = self.next_count;
        self.next_count += 1;

        count
    }

    /// Every entry, the last checked in first.
    pub fn stored(&self) -> Stored {
        let mut listed: Vec<(&StoreHash, &Record)> = self.entries.iter().collect();
        listed.sort_by_key(|(_, record)| Reverse(record.checked_in));
        let entries = listed
            .into_iter()
            .map(|(hash, record)| Entry {
                hash: *hash,
                names: self.names_of(*hash),
                bytes: record.bytes,
                pinned: record.pinned,
                held: self.is_held(*hash),
            })
            .collect();

        Stored { entries }
    }

    /// The names pointing at `hash`, in order.
    fn names_of(&self, hash: StoreHash) -> Vec<StoreName> {
        self.names.iter().filter(|(_, named)| **named == hash).map(|(name, _)| name.clone()).collect()
    }

    /// Whether a name points at `hash`.
    fn is_named(&self, hash: StoreHash) -> bool {
        self.names.values().any(|named| *named == hash)
    }

    /// Whether an actor holds `hash`.
    fn is_held(&self, hash: StoreHash) -> bool {
        self.holds.iter().any(|holding| holding.hash == hash)
    }

    /// Whether `hash` may be pruned: no name points at it, and it is neither pinned nor held.
    fn is_prunable(&self, hash: StoreHash, record: &Record) -> bool {
        let is_kept = record.pinned || self.is_named(hash) || self.is_held(hash);

        !is_kept
    }

    /// Pins `hash`, or clears its pin, as `pinned` says.
    pub fn set_pin(&mut self, hash: StoreHash, pinned: bool) -> Pinned {
        let Some(record) = self.entries.get_mut(&hash) else {
            return Pinned::NotStored;
        };
        record.pinned = pinned;

        Pinned::Done
    }

    /// Holds `hash` for `holder` until it ends, counting it as a use.
    pub fn hold(&mut self, hash: StoreHash, holder: ErasedActorRef) -> Held {
        let count = self.next_count;
        let Some(record) = self.entries.get_mut(&hash) else {
            return Held::NotStored;
        };
        record.used = count;
        self.next_count += 1;
        let is_holding = self.holds.iter().any(|holding| holding.holder == holder && holding.hash == hash);
        if !is_holding {
            self.holds.push(Holding { holder, hash });
        }

        Held::Done
    }

    /// Holds the entry `binary` names for `holder` until it ends, counting it as a use, as [`Shelves::hold`] does, and
    /// names its launch copy: by the name asked, or for a hash, the first name pointing at it in the order of their
    /// bytes, or its first 12 hexadecimal digits when none does.
    pub fn launch(&mut self, binary: &Binary, holder: ErasedActorRef) -> Launchable {
        let found = match binary {
            Binary::Named(name) => self.names.get(name).map(|hash| (*hash, name.to_string())),
            Binary::Hash(hash) => Some((*hash, self.launch_name(*hash))),
        };
        let Some((hash, name)) = found else {
            return Launchable::NotStored;
        };
        match self.hold(hash, holder) {
            Held::Done => Launchable::Held { hash, name },
            Held::NotStored => Launchable::NotStored,
        }
    }

    /// The name of a launch copy of `hash` asked by its hash: the first name pointing at it, or its first 12
    /// hexadecimal digits when none does.
    fn launch_name(&self, hash: StoreHash) -> String {
        let named = self.names.iter().find(|(_, named)| **named == hash);
        match named {
            Some((name, _)) => name.to_string(),
            None => hash.to_string().chars().take(HASH_NAME_DIGITS).collect(),
        }
    }

    /// Lets go of every hold of `holder`, which has ended.
    pub fn ended(&mut self, holder: ErasedActorRef) {
        self.holds.retain(|holding| holding.holder != holder);
    }

    /// Fits the check-in of `bytes` bytes of `hash` under `name` within `most_bytes`: it takes the next count, its
    /// name moves to it, and the entries it may prune go, the least recently used first, until the store is within
    /// its budget. Bytes already in the store always fit, as they add none; new bytes that do not fit change nothing.
    pub fn admit(&mut self, hash: StoreHash, bytes: u64, name: StoreName, most_bytes: u64) -> Admitted {
        let fresh = !self.entries.contains_key(&hash);
        let count = self.count();
        let pinned = self.entries.get(&hash).is_some_and(|record| record.pinned);
        let record = Record { bytes, checked_in: count, used: count, pinned };
        self.entries.insert(hash, record);
        let moved_from = self.names.insert(name.clone(), hash);
        let pruned = self.least_used_beyond(most_bytes);
        let total = self.total_bytes() - pruned.iter().map(|(_, bytes)| bytes).sum::<u64>();
        let overflows = total > most_bytes;
        let refused = fresh && overflows;
        if refused {
            self.entries.remove(&hash);
            match moved_from {
                Some(previous) => self.names.insert(name, previous),
                None => self.names.remove(&name),
            };
            return Admitted::Full;
        }
        for (gone, _) in &pruned {
            self.entries.remove(gone);
        }

        Admitted::Stored { fresh, pruned: pruned.into_iter().map(|(hash, _)| hash).collect() }
    }

    /// The prunable entries, least recently used first, with their sizes, that bring the store within `most_bytes`
    /// once gone: every prunable entry when even that does not.
    fn least_used_beyond(&self, most_bytes: u64) -> Vec<(StoreHash, u64)> {
        let mut prunable: Vec<(&StoreHash, &Record)> =
            self.entries.iter().filter(|(hash, record)| self.is_prunable(**hash, record)).collect();
        prunable.sort_by_key(|(_, record)| record.used);
        let mut total = self.total_bytes();
        let mut chosen = Vec::new();
        for (hash, record) in prunable {
            if total <= most_bytes {
                break;
            }
            total -= record.bytes;
            chosen.push((*hash, record.bytes));
        }

        chosen
    }

    /// How many bytes every entry holds together.
    fn total_bytes(&self) -> u64 {
        self.entries.values().map(|record| record.bytes).sum()
    }

    /// The records of `hashes` still in the store, to write.
    pub fn records(&self, hashes: &BTreeSet<StoreHash>) -> Vec<Recorded> {
        hashes
            .iter()
            .filter_map(|hash| self.entries.get(hash).map(|record| Recorded { hash: *hash, record: *record }))
            .collect()
    }

    /// Every name, and its hash, to write.
    pub fn names(&self) -> BTreeMap<StoreName, StoreHash> {
        self.names.clone()
    }
}
