//! How long turns take, as measured so far: what the engine plans a step's workers from (ADR-0011).

use aethereal_data::{KindId, KindName};
use std::collections::BTreeMap;

/// What a measured time is kept under: the kind of actor that took the turn, and what the turn reacted to.
///
/// How long a turn takes follows from the code that runs it and what it is
/// handed far more than from which actor of that code it is, so actors of one
/// kind share what is learned, and a new one is planned for from its first
/// step.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CostKey {
    /// The kind of actor: its name in its path.
    pub(super) actor: KindName,
    /// What its turn reacted to.
    pub(super) item: ItemKind,
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
/// step runs.
#[derive(Default)]
pub struct Tally {
    /// Under each key: the nanoseconds measured, and how many turns they were measured over.
    sums: BTreeMap<CostKey, (u64, u64)>,
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

    /// Takes a worker's tally into the averages: one move for each key it measured, by the mean of its turns.
    pub(super) fn merge(&mut self, tally: Tally) {
        for (key, (nanos, turns)) in tally.sums {
            let mean = nanos / turns.max(1);
            self.expected.entry(key).and_modify(|average| *average = toward(*average, mean)).or_insert(mean);
        }
    }
}

impl Tally {
    /// Adds a turn under `key` that took `nanos`.
    pub(super) fn add(&mut self, key: CostKey, nanos: u64) {
        let sum = self.sums.entry(key).or_default();
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
