//! What splitting a step among the workers costs, rehearsed on an engine of the host's own (ADR-0011).

use crate::pool::Pool;
use crate::{Monotonic, Stepped, step};
use aethereal_substrate::{Engine, SplitCost, Workers};

/// Steps a rehearsal runs whole before anything is timed, for what its actors' turns take to be measured.
const WARMING: usize = 8;
/// Steps run untimed on each count of workers before it is timed, for the workers to be woken.
const SETTLING: usize = 3;
/// Steps timed on each count of workers, of which the median is taken.
const TIMED: usize = 15;

/// What splitting a step of `engine` among `pool`'s workers costs: a rehearsal of it is run whole, split between two
/// workers, and split among as many as it can be, and what each split took beyond the step run whole, but for the
/// time its turns saved, is what the split costs. The workers are parked after, and the steps rehearsed are not
/// counted among those measured.
pub fn split_cost(engine: &Engine, pool: &mut Pool, clock: Monotonic) -> SplitCost {
    let max = pool.workers(SplitCost::NONE).max;
    if max < 2 {
        return SplitCost::NONE;
    }

    let mut rehearsal = engine.rehearsal();
    let on = |count| Workers { active: count, max: count, wake_nanos: 0, split: SplitCost::NONE };
    for _ in 0..WARMING {
        step(&mut rehearsal, pool, clock, on(1));
    }
    let whole = timed(&mut rehearsal, pool, clock, on(1));
    let two = timed(&mut rehearsal, pool, clock, on(2));
    let most = timed(&mut rehearsal, pool, clock, on(max));
    pool.park();
    pool.forget_measured();

    let two_nanos = two.beyond(&whole);
    let past_two = u64::try_from(most.sharing.saturating_sub(2)).unwrap_or(0);
    let added_nanos = most.beyond(&whole).saturating_sub(two_nanos).checked_div(past_two).unwrap_or(0);

    SplitCost { two_nanos, added_nanos }
}

/// The median of [`TIMED`] steps of `rehearsal` on `pool`, planned for `workers`, after [`SETTLING`] untimed.
fn timed(rehearsal: &mut Engine, pool: &mut Pool, clock: Monotonic, workers: Workers) -> Stepped {
    for _ in 0..SETTLING {
        step(rehearsal, pool, clock, workers);
    }
    let mut steps: Vec<Stepped> = (0..TIMED).map(|_| step(rehearsal, pool, clock, workers)).collect();
    steps.sort_unstable_by_key(|stepped| stepped.nanos);

    steps.swap_remove(TIMED / 2)
}

impl Stepped {
    /// What this step took beyond `whole`, the same step run whole, but for the time its turns saved: what it cost
    /// to split.
    fn beyond(&self, whole: &Self) -> u64 {
        let saved = whole.expected_nanos.saturating_sub(self.expected_nanos);

        self.nanos.saturating_add(saved).saturating_sub(whole.nanos)
    }
}
