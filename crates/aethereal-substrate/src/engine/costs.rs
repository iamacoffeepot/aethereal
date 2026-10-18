//! How long turns take, as measured so far: what the engine plans a step's workers from (ADR-0011).

use crate::Clock;
use aethereal_data::KindId;
use std::collections::BTreeMap;

/// What a measured time is kept under: the kind of actor that took the turn, and what the turn reacted to.
///
/// How long a turn takes follows from the code that runs it and what it is
/// handed far more than from which actor of that code it is, so actors of one
/// kind share what is learned, and a new one is planned for from its first
/// step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CostKey {
    /// The kind of actor: its type's id.
    pub(super) actor: KindId,
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
/// step runs. A step's turns are of few keys, so they are kept in a list,
/// emptied by each merge and kept for its room.
///
/// Reading the clock costs about as much as a small turn, and the costs are
/// an average, so a worker times the first turn under each key in a step and
/// every [`SAMPLE`]th after it. A key whose turns are rare is timed every step
/// it has one.
#[derive(Default)]
pub struct Tally {
    /// Under each key, what its turns this step have come to.
    sums: Vec<(CostKey, Sum)>,
    /// Where in `sums` the last turn's key was, which the next turn's key mostly is.
    last: usize,
}

/// What one key's turns in a step have come to.
#[derive(Default, Clone, Copy)]
struct Sum {
    /// How many turns there were.
    turns: u64,
    /// How many of them were timed.
    timed: u64,
    /// The nanoseconds the timed turns took.
    nanos: u64,
    /// How many turns have gone by untimed since one was timed.
    untimed: u64,
}

/// One turn in this many under a key is timed, after the first.
const SAMPLE: u64 = 8;

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

    /// How long a turn under `key` has been measured to take, in nanoseconds: nothing when it never was.
    pub(super) fn measured(&self, key: &CostKey) -> Option<u64> {
        self.expected.get(key).copied()
    }

    /// Takes a worker's tally into the averages, one move for each key it measured by the mean of its turns, and
    /// empties it.
    pub(super) fn merge(&mut self, tally: &mut Tally) {
        for (key, Sum { timed, nanos, .. }) in tally.sums.drain(..) {
            let mean = nanos / timed.max(1);
            self.expected.entry(key).and_modify(|average| *average = toward(*average, mean)).or_insert(mean);
        }
    }
}

impl Tally {
    /// Runs `turn`, a turn under `key`, and times it by `clock` when it is the first under that key or the
    /// [`SAMPLE`]th since one was timed.
    #[allow(clippy::inline_always, reason = "once for every turn: a call here costs a tenth of a turn, measured")]
    #[inline(always)]
    pub(super) fn time<R>(&mut self, key: CostKey, clock: &impl Clock, turn: impl FnOnce() -> R) -> R {
        let is_last = self.sums.get(self.last).is_some_and(|(kept, _)| *kept == key);
        let place = if is_last {
            self.last
        } else {
            let found = self.sums.iter().position(|(kept, _)| *kept == key);
            found.unwrap_or_else(|| {
                self.sums.push((key, Sum::default()));
                self.sums.len() - 1
            })
        };
        self.last = place;
        let sum = &mut self.sums[place].1;
        let is_timed = sum.turns.is_multiple_of(SAMPLE);
        sum.turns += 1;
        if !is_timed {
            return turn();
        }

        let start = clock.nanos();
        let done = turn();
        sum.nanos += clock.nanos().saturating_sub(start);
        sum.timed += 1;

        done
    }
}

impl Tally {
    /// Runs `batch`, a batch of turns whose first is under `key`, which answers how many turns it ran, and times it by
    /// `clock`, every turn of it counted under `key`, when it is the first under that key or [`SAMPLE`] turns have
    /// gone by untimed since one was.
    #[allow(clippy::inline_always, reason = "once for every batch: a call here costs a tenth of a turn, measured")]
    #[inline(always)]
    pub(super) fn time_batch(&mut self, key: CostKey, clock: &impl Clock, batch: impl FnOnce() -> u32) -> u32 {
        let is_last = self.sums.get(self.last).is_some_and(|(kept, _)| *kept == key);
        let place = if is_last {
            self.last
        } else {
            let found = self.sums.iter().position(|(kept, _)| *kept == key);
            found.unwrap_or_else(|| {
                self.sums.push((key, Sum::default()));
                self.sums.len() - 1
            })
        };
        self.last = place;
        let sum = &mut self.sums[place].1;
        let is_timed = sum.timed == 0 || sum.untimed >= SAMPLE;
        if !is_timed {
            let turns = u64::from(batch());
            sum.turns += turns;
            sum.untimed += turns;
            return u32::try_from(turns).unwrap_or(u32::MAX);
        }

        let start = clock.nanos();
        let turns = batch();
        sum.nanos += clock.nanos().saturating_sub(start);
        sum.turns += u64::from(turns);
        sum.timed += u64::from(turns);
        sum.untimed = 0;

        turns
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
