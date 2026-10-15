//! How long turns take, as measured so far: what the engine plans a step's workers from (ADR-0011).

use aethereal_data::{KindId, KindName};
use std::collections::BTreeMap;

/// What a measured time is kept under: the kind of actor that took the turn, and what the turn reacted to.
///
/// How long a turn takes follows from the code that runs it and what it is
/// handed far more than from which actor of that code it is, so actors of one
/// kind share what is learned, and a new one is planned for from its first
/// step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CostKey {
    /// The kind of actor.
    pub(super) actor: KindIndex,
    /// What its turn reacted to.
    pub(super) item: ItemKind,
}

/// A kind of actor as the engine numbers it: given when the first actor of the kind is seated, so a turn's cost
/// is found by number and never by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct KindIndex(u32);

/// The number given to each kind of actor the engine has seated.
pub(super) struct Kinds {
    numbered: BTreeMap<KindName, KindIndex>,
}

impl Kinds {
    /// No kind numbered yet.
    pub(super) const fn new() -> Self {
        Self { numbered: BTreeMap::new() }
    }

    /// The number of `kind`, given now when it has none.
    pub(super) fn number(&mut self, kind: &KindName) -> KindIndex {
        let next = KindIndex(u32::try_from(self.numbered.len()).unwrap_or(u32::MAX));

        *self.numbered.entry(kind.clone()).or_insert(next)
    }
}

/// What a turn reacted to, as far as its cost goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ItemKind {
    /// A piece of mail of this kind.
    Mail(KindId),
    /// The ending of an actor it watches.
    Ended,
    /// Its own closing turn.
    Closing,
}

/// The expected time of a turn under each key, as a moving average weighted to the most recent (ADR-0011).
///
/// It is kept beside the steps, and not as a view: the same turns take
/// different times from run to run, and every view is the same on every run.
pub(super) struct Costs {
    /// Nanoseconds a turn is expected to take, under each key a turn has been measured under.
    expected: BTreeMap<CostKey, u64>,
}

/// One worker's measurements of a step, summed under each key, merged into the engine's costs between steps.
///
/// A worker adds to its own tally only, so no measurement is shared while the
/// step runs. A step's turns are of few keys, so they are kept in a list,
/// emptied by each merge and kept for its room.
#[derive(Default)]
pub struct Tally {
    /// Under each key: the nanoseconds measured, and how many turns they were measured over.
    sums: Vec<(CostKey, (u64, u64))>,
}

/// What one new measurement moves an average by: an eighth of the way to it.
const WEIGHT: u64 = 8;

/// What a turn under a key never measured is expected to take: enough that items count where nothing is known.
const UNKNOWN: u64 = 1;

impl Costs {
    /// No turn measured yet.
    pub(super) const fn new() -> Self {
        Self { expected: BTreeMap::new() }
    }

    /// How long a turn under `key` is expected to take, in nanoseconds.
    pub(super) fn expect(&self, key: &CostKey) -> u64 {
        self.expected.get(key).copied().unwrap_or(UNKNOWN)
    }

    /// Takes a worker's tally into the averages, one move for each key it measured by the mean of its turns, and
    /// empties it.
    pub(super) fn merge(&mut self, tally: &mut Tally) {
        for (key, (nanos, turns)) in tally.sums.drain(..) {
            let mean = nanos / turns.max(1);
            self.expected.entry(key).and_modify(|average| *average = toward(*average, mean)).or_insert(mean);
        }
    }
}

impl Tally {
    /// Adds a turn under `key` that took `nanos`.
    pub(super) fn add(&mut self, key: CostKey, nanos: u64) {
        let place = self.sums.iter().position(|(kept, _)| *kept == key);
        let place = place.unwrap_or_else(|| {
            self.sums.push((key, (0, 0)));
            self.sums.len() - 1
        });
        let sum = &mut self.sums[place].1;
        sum.0 += nanos;
        sum.1 += 1;
    }
}

/// `average` moved an eighth of the way to `sample`.
pub(super) const fn toward(average: u64, sample: u64) -> u64 {
    if sample >= average {
        average + (sample - average) / WEIGHT
    } else {
        average - (average - sample) / WEIGHT
    }
}
