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
    /// What running a step on several workers costs beyond running it on one, as the host measured it.
    pub split: SplitCost,
}

impl Workers {
    /// One worker, the thread that steps the engine: a pumped host's whole engine (ADR-0001).
    pub const ONE: Self = Self { active: 1, max: 1, wake_nanos: 0, split: SplitCost::NONE };
}

/// What running a step on several workers costs beyond running it on one, as a host measured it (ADR-0011): taking
/// its parts out to the workers, gathering them back, and folding them on the workers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SplitCost {
    /// What splitting a step between two workers costs, in nanoseconds.
    pub two_nanos: u64,
    /// What each worker past two adds to it, in nanoseconds.
    pub added_nanos: u64,
}

impl SplitCost {
    /// A split that costs nothing.
    pub const NONE: Self = Self { two_nanos: 0, added_nanos: 0 };

    /// What splitting a step among `workers` costs, in nanoseconds: nothing for a step run whole, on one.
    #[must_use]
    pub fn among(self, workers: usize) -> u64 {
        let past_two = u64::try_from(workers.saturating_sub(2)).unwrap_or(u64::MAX);
        let splits = workers > 1;

        if splits {
            self.two_nanos.saturating_add(self.added_nanos.saturating_mul(past_two))
        } else {
            0
        }
    }
}

/// A step's plan: how the count of running workers changes, and what each runs (ADR-0011).
pub struct Plan {
    /// Workers to wake when above zero, to park when below: never more than are parked, and never so many that
    /// none runs.
    pub(super) delta: isize,
    /// How the step's work is split among the workers running once `delta` is taken.
    pub(super) split: Split,
    /// How long the plan expects the step's turns to take on those workers, in nanoseconds.
    pub(super) expected_nanos: u64,
}

/// How a step's work is split among the workers that run it (ADR-0011).
#[allow(clippy::large_enum_variant, reason = "made and moved once a step, where a box would allocate every step")]
pub enum Split {
    /// One worker runs it, the thread that steps the engine, by [`super::Engine::run_whole`]: no part is taken out.
    Whole(Whole),
    /// Each running worker runs one share, the first for the thread that steps the engine.
    Shares(Vec<Share>),
}

/// What the one worker of a step run whole writes to: its arena and its lists. The parts it runs stay in the engine.
pub struct Whole {
    /// Where the worker writes the bytes of the mail its turns send.
    pub(super) arena: Arena,
    /// What the worker fills as it runs.
    pub(super) lists: Lists,
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

    /// How long the plan expects the step's turns to take on the workers it runs on, in nanoseconds: for a host
    /// timing a split against a step run whole to take out what the split saved.
    #[must_use]
    pub const fn expected_nanos(&self) -> u64 {
        self.expected_nanos
    }

    /// How the step's work is split among the workers that run it.
    #[must_use]
    pub fn into_split(self) -> Split {
        self.split
    }
}

/// Plans a step whose parts with something to run are `parts`, each with what its turns are expected to cost, on
/// `workers`, given the expected work of `recent` steps: how the count of running workers changes, how many run,
/// and how long their turns are expected to take.
pub(super) fn plan(parts: &[(usize, u64)], recent: u64, workers: Workers) -> (isize, usize, u64) {
    let work: u64 = parts.iter().map(|(_, cost)| cost).sum();
    let largest = parts.iter().map(|(_, cost)| *cost).max().unwrap_or(0);
    let Decided { awake, sharing } = decide(work, largest, parts.len(), recent, workers);
    let delta = signed(awake) - signed(workers.active.clamp(1, workers.max.max(1)));

    (delta, sharing, takes(work, largest, sharing))
}

/// How many workers a step keeps awake, and how many of them it is split among (ADR-0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Decided {
    /// The workers running once the step's wakes and parks are made: at least one.
    pub(super) awake: usize,
    /// The workers the step is split among, of those awake: one when it runs whole.
    pub(super) sharing: usize,
}

/// How many workers run a step whose expected work is `work` nanoseconds, the largest actor's `largest`, over
/// `actors` actors, where the work of recent steps averages `recent`.
///
/// One actor's turns run on one worker, so no more workers are useful than the work over the largest actor's, or
/// than there are actors. Of those, the step is split among the count it takes least time on, with what the split
/// costs, or run whole when that is least. A parked worker is woken only when the step it shortens saves more than
/// the wake costs; a running one is parked only when recent steps would not have used it either, and a worker kept
/// running through a step that does not use it is there for the steps after it.
pub(super) fn decide(work: u64, largest: u64, actors: usize, recent: u64, workers: Workers) -> Decided {
    let max = workers.max.max(1);
    let active = workers.active.clamp(1, max);
    let useful = |work: u64| useful(work, largest, actors, max);
    let wanted = useful(work);
    let on = |count: usize| takes(work, largest, count).saturating_add(workers.split.among(count));
    // Of at most `most` workers, the count the step takes least time on: one, with nothing to weigh, when one is all.
    let quickest = |most: usize| {
        let weighs = most > 1;
        if weighs {
            (1..=most).min_by_key(|count| (on(*count), *count)).unwrap_or(1)
        } else {
            1
        }
    };

    let best = quickest(wanted);
    let wakes = best > active;
    let awake = if wakes {
        let saved = on(quickest(active)).saturating_sub(on(best));
        let pays = saved > workers.wake_nanos;
        if pays {
            best
        } else {
            active
        }
    } else {
        wanted.max(useful(recent)).min(active)
    };

    Decided { awake, sharing: quickest(awake) }
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
pub(super) struct Packing {
    /// The parts to pack, each with what it is expected to cost.
    pub(super) parts: Vec<(usize, u64)>,
    /// The parts in the order they are given out.
    order: Vec<(u64, usize)>,
    /// Each share's load and the parts given it: the first `running` of them, as the last [`Packing::pack`] made.
    pub(super) shares: Vec<(u64, Vec<usize>)>,
    /// The share each part was last given to run, by part number: where its memory is warm. A part that has not run
    /// is at home in the share its number falls in.
    homes: Vec<usize>,
}

impl Packing {
    /// Packing for an engine of `parts` parts.
    pub(super) fn new(parts: usize) -> Self {
        Self { parts: Vec::new(), order: Vec::new(), shares: Vec::new(), homes: (0..parts).collect() }
    }

    /// Gives each of [`Packing::parts`] to one of `running` shares, the most costly first, and of parts that cost the
    /// same, the lower first: to the share it last ran in while that share stays within the step's target, and
    /// otherwise to the share with the least so far. The target is an even split of the step's work, or its most
    /// costly part when that is more, so no share is given more than a share the least-loaded rule would give. Each
    /// share then runs its parts in the order of their numbers.
    ///
    /// A part's memory is warm in the cache of the worker that last ran it, so keeping it there saves what moving it
    /// costs, which grows with the distance between the two workers' caches.
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
        let total: u64 = self.order.iter().map(|(cost, _)| cost).sum();
        let largest = self.order.first().map_or(0, |(cost, _)| *cost);
        let target = total.div_ceil(u64::try_from(running).unwrap_or(u64::MAX)).max(largest);
        for &(cost, part) in &self.order {
            let home = self.homes[part] % running;
            let fits = self.shares[home].0 + cost <= target;
            let least = (0..running).min_by_key(|share| self.shares[*share].0).unwrap_or(home);
            let given = if fits {
                home
            } else {
                least
            };
            let (load, share) = &mut self.shares[given];
            *load += cost;
            share.push(part);
            self.homes[part] = given;
        }
        for (_, share) in &mut self.shares[..running] {
            share.sort_unstable();
        }
    }

    /// Gives each of [`Packing::parts`] to the one of `folding` shares it last ran in, to be folded where its memory is
    /// warm. Each share then folds its parts in the order of their numbers.
    pub(super) fn pack_at_home(&mut self, folding: usize) {
        let folding = folding.max(1);
        if self.shares.len() < folding {
            self.shares.resize_with(folding, Default::default);
        }
        for (load, parts) in &mut self.shares {
            *load = 0;
            parts.clear();
        }
        for &(part, records) in &self.parts {
            let (load, share) = &mut self.shares[self.homes[part] % folding];
            *load += records;
            share.push(part);
        }
        for (_, share) in &mut self.shares[..folding] {
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

    /// Four workers, `active` running, with waking costing `wake_nanos` and splitting nothing.
    const fn four(active: usize, wake_nanos: u64) -> Workers {
        Workers { active, max: 4, wake_nanos, split: SplitCost::NONE }
    }

    #[test]
    fn no_more_workers_than_the_largest_actor_lets_shorten() {
        // Catches workers woken past the point where one actor's turns are the whole step.
        assert_eq!(decide(1_000, 500, 100, 0, four(1, 0)).sharing, 2);
        assert_eq!(decide(1_000, 100, 3, 0, four(1, 0)).sharing, 3);
        assert_eq!(decide(1_000, 10, 100, 0, four(1, 0)).sharing, 4);
    }

    #[test]
    fn a_wake_that_saves_less_than_it_costs_is_not_made() {
        // Catches a wake made for a step it shortens by less than the wake takes.
        assert_eq!(decide(1_000, 10, 100, 0, four(1, 800)).awake, 1);
        assert_eq!(decide(1_000, 10, 100, 0, four(1, 700)).awake, 4);
    }

    #[test]
    fn a_step_runs_whole_unless_splitting_it_saves_more_than_the_split_costs() {
        // Catches a step split when what the split costs is more than it saves, whether its workers are running or
        // parked, a step kept whole when the split pays, and workers parked because one step ran whole.
        let splitting = |active, two_nanos| Workers {
            active,
            max: 4,
            wake_nanos: 0,
            split: SplitCost { two_nanos, added_nanos: 0 },
        };
        assert_eq!(decide(1_000, 10, 100, 0, splitting(4, 800)), Decided { awake: 4, sharing: 1 });
        assert_eq!(decide(1_000, 10, 100, 0, splitting(1, 800)), Decided { awake: 1, sharing: 1 });
        assert_eq!(decide(1_000, 10, 100, 0, splitting(4, 700)), Decided { awake: 4, sharing: 4 });
        assert_eq!(decide(1_000, 10, 100, 0, splitting(1, 700)), Decided { awake: 4, sharing: 4 });
    }

    #[test]
    fn a_step_is_split_among_the_count_it_takes_least_time_on() {
        // Catches a step split among every useful worker when each worker past two costs more than it saves.
        let costly =
            Workers { active: 4, max: 4, wake_nanos: 0, split: SplitCost { two_nanos: 100, added_nanos: 300 } };
        assert_eq!(decide(1_000, 10, 100, 0, costly), Decided { awake: 4, sharing: 2 });
    }

    #[test]
    fn a_part_stays_where_it_last_ran_while_that_share_has_room() {
        // Catches a part moved off the worker that last ran it though that worker had room, which loses its warm
        // cache, and a share loaded past an even split because the homes of costly parts all point at it.
        let mut packing = Packing::new(4);
        let shares = |packing: &Packing| packing.shares[..2].iter().map(|(_, parts)| parts.clone()).collect::<Vec<_>>();
        packing.parts = vec![(1, 100)];
        packing.pack(2);
        assert_eq!(shares(&packing), [vec![], vec![1]]);

        packing.parts = vec![(0, 300), (1, 10), (2, 300), (3, 10)];
        packing.pack(2);
        assert_eq!(shares(&packing), [vec![0, 3], vec![1, 2]]);
        let loads: Vec<u64> = packing.shares[..2].iter().map(|(load, _)| *load).collect();
        assert_eq!(loads, [310, 310]);

        packing.parts = vec![(2, 100)];
        packing.pack(2);
        assert_eq!(shares(&packing), [vec![], vec![2]]);
    }

    #[test]
    fn a_worker_recent_steps_used_is_kept_running() {
        // Catches parking on one light step a worker the steps before it needed, and keeping one none needed.
        assert_eq!(decide(10, 10, 100, 1_000, four(4, 0)).awake, 4);
        assert_eq!(decide(10, 10, 100, 20, four(4, 0)).awake, 2);
    }
}
