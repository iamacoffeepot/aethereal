//! Which core each worker is pinned to, and where the stepping thread is kept: one cache's group filled before the
//! next (ADR-0011).

use std::collections::BTreeMap;
use std::fs;
#[cfg(target_os = "linux")]
use std::mem;

/// The cores workers are pinned to, in the order they are handed out, and the group the stepping thread keeps to.
///
/// A step's memory stays warm in the cache of the core that uses it, so each
/// worker keeps to one core. A step's shares and its records pass between the
/// stepping thread and every worker it runs on, so the cores are taken from
/// one group sharing a last-level cache until it is full before any from the
/// next, and one hardware thread of each core before its sibling. The stepping
/// thread keeps to the group of the first core, where the first workers are,
/// and is not pinned to one core within it, so the host's scheduler can wake
/// it on any of the group's cores after it sleeps through the workers' run,
/// the cores the workers are pinned to among them; but not on the other
/// hardware thread of a worker's core, where it would share the core with a
/// worker spinning between steps. It always keeps the first core, which no
/// worker takes while there are cores enough, so it is never kept only to
/// cores where workers spin.
pub(super) struct Cores {
    /// Logical CPU numbers, one per worker in turn, wrapping round when there are more workers.
    order: Vec<usize>,
    /// Logical CPU numbers sharing a last-level cache with the first core, both hardware threads of each.
    group: Vec<usize>,
    /// The core of each logical CPU, as the hardware threads it shares with.
    core_of: BTreeMap<usize, String>,
}

impl Cores {
    /// The cores of this host, read from Linux's description of its caches. None are known on another host, or
    /// when that description cannot be read, and then no thread is pinned.
    pub(super) fn of_host() -> Self {
        let none = Self { order: Vec::new(), group: Vec::new(), core_of: BTreeMap::new() };
        let ids = core_affinity::get_core_ids().unwrap_or_default();
        // Each core's first hardware thread, then its others, by the cache they share.
        let mut firsts: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut seconds: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut core_of = BTreeMap::new();
        for id in ids.iter().map(|core| core.id) {
            let cpu = format!("/sys/devices/system/cpu/cpu{id}");
            let (Ok(cache), Ok(siblings)) = (
                fs::read_to_string(format!("{cpu}/cache/index3/shared_cpu_list")),
                fs::read_to_string(format!("{cpu}/topology/thread_siblings_list")),
            ) else {
                return none;
            };
            let siblings = siblings.trim().to_owned();
            let is_sibling = core_of.values().any(|core| *core == siblings);
            let groups = if is_sibling {
                &mut seconds
            } else {
                &mut firsts
            };
            groups.entry(cache.trim().to_owned()).or_default().push(id);
            core_of.insert(id, siblings);
        }
        let Some(first_cache) = firsts.keys().next().cloned() else {
            return none;
        };

        let group =
            [&firsts, &seconds].into_iter().filter_map(|groups| groups.get(&first_cache)).flatten().copied().collect();
        let mut order: Vec<usize> = firsts.into_values().flatten().collect();
        order.extend(seconds.into_values().flatten());

        Self { order, group, core_of }
    }

    /// The logical CPU the worker numbered `worker` from one is pinned to, when any is known.
    fn of_worker(&self, worker: usize) -> Option<usize> {
        self.order.get(worker % self.order.len().max(1)).copied()
    }

    /// Pins the calling thread, the worker numbered `worker` from one, to its core. The first core is left to the
    /// stepping thread while there are cores enough.
    pub(super) fn pin(&self, worker: usize) {
        let Some(id) = self.of_worker(worker) else {
            return;
        };
        let _ = core_affinity::set_for_current(core_affinity::CoreId { id });
    }

    /// Keeps the calling thread, the stepping thread of a pool of `workers` workers, to the first core's group until
    /// the answer is dropped: to the first core, which [`Self::pin`] leaves to it, and to each of the group's hardware
    /// threads but those beside one another worker is pinned to, on the same core.
    pub(super) fn keep_stepping(&self, workers: usize) -> Kept {
        Kept::to(&self.stepping(workers))
    }

    /// The hardware threads [`Self::keep_stepping`] keeps the stepping thread of `workers` workers to.
    fn stepping(&self, workers: usize) -> Vec<usize> {
        let pinned: Vec<usize> = (1..workers).filter_map(|worker| self.of_worker(worker)).collect();
        let taken: Vec<&String> = pinned.iter().filter_map(|id| self.core_of.get(id)).collect();
        let own = self.order.first();
        self.group
            .iter()
            .copied()
            .filter(|id| own == Some(id) || pinned.contains(id) || !self.is_beside(*id, &taken))
            .collect()
    }

    /// Whether the hardware thread `id` is on one of the cores `taken`.
    fn is_beside(&self, id: usize, taken: &[&String]) -> bool {
        self.core_of.get(&id).is_some_and(|core| taken.contains(&core))
    }
}

/// Where the stepping thread was put for as long as a pool lives.
///
/// It is let back onto the cores it could run on before when dropped, so a
/// second pool made on the same thread reads every core of the host, and the
/// thread runs where it did once the engine stops.
pub(super) enum Kept {
    /// Kept to a group of cores, from the cores it could run on before.
    #[cfg(target_os = "linux")]
    Moved { before: libc::cpu_set_t },
    /// Left where it was: no group is known, or the cores it could run on could not be read.
    Left,
}

impl Kept {
    /// Keeps the calling thread to `group`, or leaves it where it is when the group is empty.
    #[cfg(target_os = "linux")]
    fn to(group: &[usize]) -> Self {
        let size = size_of::<libc::cpu_set_t>();
        // SAFETY: a `cpu_set_t` is a plain array of bits, for which all zeroes is the empty set.
        let mut before: libc::cpu_set_t = unsafe { mem::zeroed() };
        // SAFETY: the set is `size` bytes, and thread 0 is the calling thread.
        let read = unsafe { libc::sched_getaffinity(0, size, &raw mut before) };
        let is_known = read == 0;
        if !is_known || group.is_empty() {
            return Self::Left;
        }

        // SAFETY: as for `before`.
        let mut kept: libc::cpu_set_t = unsafe { mem::zeroed() };
        for &cpu in group.iter().filter(|&&cpu| cpu < 8 * size) {
            // SAFETY: `cpu` is within the set, so the bit it sets is one of the set's own.
            unsafe { libc::CPU_SET(cpu, &mut kept) };
        }
        // SAFETY: as for the read.
        let _ = unsafe { libc::sched_setaffinity(0, size, &raw const kept) };

        Self::Moved { before }
    }

    /// Leaves the calling thread where it is, on a host with no group of cores to keep it to.
    #[cfg(not(target_os = "linux"))]
    const fn to(_group: &[usize]) -> Self {
        Self::Left
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        #[cfg(target_os = "linux")]
        if let Self::Moved { before } = self {
            // SAFETY: the set is as large as the size given, and thread 0 is the calling thread, the one kept.
            let _ = unsafe { libc::sched_setaffinity(0, size_of::<libc::cpu_set_t>(), &raw const *before) };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::Cores;

    /// A host of `cores` cores of two hardware threads each, sharing one cache, numbered as Linux numbers them: each
    /// core's first threads, then their seconds.
    fn one_cache(cores: usize) -> Cores {
        let order: Vec<usize> = (0..2 * cores).collect();
        let core_of: BTreeMap<usize, String> =
            order.iter().map(|id| (*id, format!("{},{}", id % cores, id % cores + cores))).collect();

        Cores { group: order.clone(), order, core_of }
    }

    #[test]
    fn the_stepping_thread_keeps_its_own_core_when_a_worker_takes_the_thread_beside_it() {
        let cores = one_cache(8);

        // Workers 1 to 15 take every thread but the first, its sibling among them.
        let kept = cores.stepping(16);

        assert!(kept.contains(&0), "kept {kept:?}");
    }

    #[test]
    fn the_stepping_thread_keeps_off_the_threads_beside_the_workers() {
        let cores = one_cache(8);

        // Workers 1 to 3 take threads 1 to 3, whose siblings are 9 to 11.
        let kept = cores.stepping(4);

        assert_eq!(kept, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 12, 13, 14, 15]);
    }
}
