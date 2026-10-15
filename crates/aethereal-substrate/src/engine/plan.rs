//! How many workers a step runs on, and which parts of the engine each runs (ADR-0011).

use super::{Lists, Parcel};
use aethereal_actor::Arena;

/// The workers a host has, as the engine plans a step for them (ADR-0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Workers {
    /// How many are running now, the thread that steps the engine among them: at least one.
    pub active: usize,
    /// How many there are, running or parked, that thread among them: at least one.
    pub max: usize,
    /// What waking a parked worker costs, as the host measured it, in nanoseconds.
    pub wake_nanos: u64,
}

impl Workers {
    /// One worker, the thread that steps the engine: a pumped host's whole engine (ADR-0001).
    pub const ONE: Self = Self { active: 1, max: 1, wake_nanos: 0 };
}

/// A step's plan: how the count of running workers changes, and what each runs (ADR-0011).
pub struct Plan {
    /// Workers to wake when above zero, to park when below: never more than are parked, and never so many that
    /// none runs.
    pub(super) delta: isize,
    /// What each running worker runs this step, the first for the thread that steps the engine. One for each
    /// worker running once `delta` is taken.
    pub(super) shares: Vec<Share>,
}

/// The parts of the engine one worker runs in a step, each taken out of the engine for the step with the actors in
/// it and what they react to.
pub struct Share {
    pub(super) parcels: Vec<Parcel>,
    /// Where the worker writes the bytes of the mail its turns send.
    pub(super) arena: Arena,
    /// What the worker fills as it runs.
    pub(super) lists: Lists,
}

impl Plan {
    /// Workers to wake when above zero, to park when below.
    #[must_use]
    pub const fn delta(&self) -> isize {
        self.delta
    }

    /// What each running worker runs, the first for the thread that steps the engine.
    #[must_use]
    pub fn into_shares(self) -> Vec<Share> {
        self.shares
    }
}

/// Plans a step whose parts with something to run are `parts`, each with what its turns are expected to cost, on
/// `workers`, given the expected work of `recent` steps: how the count of running workers changes, and how many run.
pub(super) fn plan(parts: &[(usize, u64)], recent: u64, workers: Workers) -> (isize, usize) {
    let work: u64 = parts.iter().map(|(_, cost)| cost).sum();
    let largest = parts.iter().map(|(_, cost)| *cost).max().unwrap_or(0);
    let running = decide(work, largest, parts.len(), recent, workers);
    let delta = signed(running) - signed(workers.active.clamp(1, workers.max.max(1)));

    (delta, running)
}

/// How many workers run a step whose expected work is `work` nanoseconds, the largest actor's `largest`, over
/// `actors` actors, where the work of recent steps averages `recent`.
///
/// One actor's turns run on one worker, so no more workers are useful than the work over the largest actor's, or
/// than there are actors. A parked worker is woken only when the step it shortens saves more than the wake costs;
/// a running one is parked only when recent steps would not have used it either.
pub(super) fn decide(work: u64, largest: u64, actors: usize, recent: u64, workers: Workers) -> usize {
    let max = workers.max.max(1);
    let active = workers.active.clamp(1, max);
    let useful = |work: u64| useful(work, largest, actors, max);
    let wanted = useful(work);

    if wanted > active {
        let saved = takes(work, largest, active).saturating_sub(takes(work, largest, wanted));
        let pays = saved > workers.wake_nanos;
        if pays {
            wanted
        } else {
            active
        }
    } else {
        wanted.max(useful(recent).min(active))
    }
}

/// How many workers shorten a step of `work` nanoseconds whose largest actor's is `largest`, over `actors`
/// actors, of `max` workers: at least one.
fn useful(work: u64, largest: u64, actors: usize, max: usize) -> usize {
    let by_work = work.div_ceil(largest.max(1));
    let by_work = usize::try_from(by_work).unwrap_or(usize::MAX);

    by_work.min(actors).min(max).max(1)
}

/// How long a step of `work` nanoseconds whose largest actor's is `largest` takes on `workers` workers, at best.
fn takes(work: u64, largest: u64, workers: usize) -> u64 {
    let workers = u64::try_from(workers.max(1)).unwrap_or(u64::MAX);

    work.div_ceil(workers).max(largest)
}

/// What packing parts into shares works in, kept from one step to the next for its room.
#[derive(Default)]
pub(super) struct Packing {
    /// The parts to pack, each with what it is expected to cost.
    pub(super) parts: Vec<(usize, u64)>,
    /// The parts in the order they are given out.
    order: Vec<(u64, usize)>,
    /// Each share's load and the parts given it: the first `running` of them, as the last [`Packing::pack`] made.
    pub(super) shares: Vec<(u64, Vec<usize>)>,
}

impl Packing {
    /// Gives each of [`Packing::parts`] to one of `running` shares: the most costly first, each to the share with the
    /// least so far, and of parts that cost the same, the lower first. Each share then runs its parts in the order of
    /// their numbers.
    pub(super) fn pack(&mut self, running: usize) {
        let running = running.max(1);
        self.order.clear();
        self.order.extend(self.parts.iter().map(|(part, cost)| (*cost, *part)));
        self.order.sort_unstable_by(|(left, a), (right, b)| right.cmp(left).then(a.cmp(b)));

        if self.shares.len() < running {
            self.shares.resize_with(running, Default::default);
        }
        for (load, parts) in &mut self.shares {
            *load = 0;
            parts.clear();
        }
        for (cost, part) in &self.order {
            if let Some((load, share)) = self.shares[..running].iter_mut().min_by_key(|(load, _)| *load) {
                *load += cost;
                share.push(*part);
            }
        }
        for (_, share) in &mut self.shares[..running] {
            share.sort_unstable();
        }
    }
}

/// `count` as a signed count.
fn signed(count: usize) -> isize {
    isize::try_from(count).unwrap_or(isize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four workers, one running, with waking costing `wake_nanos`.
    const fn four(active: usize, wake_nanos: u64) -> Workers {
        Workers { active, max: 4, wake_nanos }
    }

    #[test]
    fn no_more_workers_than_the_largest_actor_lets_shorten() {
        // Catches workers woken past the point where one actor's turns are the whole step.
        assert_eq!(decide(1_000, 500, 100, 0, four(1, 0)), 2);
        assert_eq!(decide(1_000, 100, 3, 0, four(1, 0)), 3);
        assert_eq!(decide(1_000, 10, 100, 0, four(1, 0)), 4);
    }

    #[test]
    fn a_wake_that_saves_less_than_it_costs_is_not_made() {
        // Catches a wake made for a step it shortens by less than the wake takes.
        assert_eq!(decide(1_000, 10, 100, 0, four(1, 800)), 1);
        assert_eq!(decide(1_000, 10, 100, 0, four(1, 700)), 4);
    }

    #[test]
    fn a_worker_recent_steps_used_is_kept_running() {
        // Catches parking on one light step a worker the steps before it needed, and keeping one none needed.
        assert_eq!(decide(10, 10, 100, 1_000, four(4, 0)), 4);
        assert_eq!(decide(10, 10, 100, 20, four(4, 0)), 2);
    }
}
