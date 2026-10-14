//! How many workers a step runs on, and which actors each runs (ADR-0011).

use super::Lists;
use super::costs::KindIndex;
use aethereal_actor::{Arena, Runs};
use aethereal_data::ActorId;
use std::ops::Range;

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
    delta: isize,
    /// What each running worker runs this step, the first for the thread that steps the engine. One for each
    /// worker running once `delta` is taken.
    shares: Vec<Share>,
}

/// The actors one worker runs in a step, each with what it reacts to.
pub struct Share {
    jobs: Vec<Job>,
    /// Where the worker writes the bytes of the mail its turns send.
    arena: Arena,
    /// What the worker fills as it runs.
    lists: Lists,
}

/// One actor's turns of a step: the actor, taken out of the engine for the step, and what it runs.
pub(super) struct Job {
    pub(super) id: ActorId,
    /// The number of the actor's seat.
    pub(super) seat: usize,
    /// The kind of actor, which its turns are measured under.
    pub(super) kind: KindIndex,
    pub(super) actor: Box<dyn Runs>,
    pub(super) work: Work,
}

/// What an actor runs in a step.
pub(super) enum Work {
    /// One turn for each item the actor's part of the streams handed out in this range, in order.
    Items(Range<usize>),
    /// Its closing turn.
    Closing,
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
    /// What each running worker runs, to give each its arena.
    pub(super) fn shares_mut(&mut self) -> impl Iterator<Item = &mut Share> {
        self.shares.iter_mut()
    }
}

impl Share {
    /// The actors of the share, each with what it runs, the arena its mail is written to, and the lists it fills.
    pub(super) fn into_parts(self) -> (Vec<Job>, Arena, Lists) {
        (self.jobs, self.arena, self.lists)
    }

    /// Gives the share `arena` to write its mail to, and `lists` to fill.
    pub(super) fn write_to(&mut self, arena: Arena, lists: Lists) {
        self.arena = arena;
        self.lists = lists;
    }
}

/// The plan for jobs whose expected costs are `costs`, given in the same order, on `workers`, where the work of
/// recent steps averages `recent` nanoseconds.
pub(super) fn plan(jobs: Vec<Job>, costs: &[u64], recent: u64, workers: Workers) -> Plan {
    let work: u64 = costs.iter().sum();
    let largest = costs.iter().copied().max().unwrap_or(0);
    let running = decide(work, largest, jobs.len(), recent, workers);
    let delta = signed(running) - signed(workers.active.clamp(1, workers.max.max(1)));

    Plan { delta, shares: pack(jobs, costs, running) }
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

/// Gives each job to one of `running` shares: the most costly first, each to the share with the least so far.
pub(super) fn pack(jobs: Vec<Job>, costs: &[u64], running: usize) -> Vec<Share> {
    let mut order: Vec<(u64, Job)> = costs.iter().copied().zip(jobs).collect();
    order.sort_by(|(left, a), (right, b)| right.cmp(left).then(a.id.bits().cmp(&b.id.bits())));

    let mut shares: Vec<(u64, Share)> = (0..running.max(1))
        .map(|_| (0, Share { jobs: Vec::new(), arena: Arena::default(), lists: Lists::default() }))
        .collect();
    for (cost, job) in order {
        if let Some((load, share)) = shares.iter_mut().min_by_key(|(load, _)| *load) {
            *load += cost;
            share.jobs.push(job);
        }
    }

    shares.into_iter().map(|(_, share)| share).collect()
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
