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
/// it on any of the group's cores after it sleeps through the workers' run.
pub(super) struct Cores {
    /// Logical CPU numbers, one per worker in turn, wrapping round when there are more workers.
    order: Vec<usize>,
    /// Logical CPU numbers sharing a last-level cache with the first core, both hardware threads of each.
    group: Vec<usize>,
}

impl Cores {
    /// The cores of this host, read from Linux's description of its caches. None are known on another host, or
    /// when that description cannot be read, and then no thread is pinned.
    pub(super) fn of_host() -> Self {
        let none = Self { order: Vec::new(), group: Vec::new() };
        let ids = core_affinity::get_core_ids().unwrap_or_default();
        // Each core's first hardware thread, then its others, by the cache they share.
        let mut firsts: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut seconds: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        let mut seen_siblings = Vec::new();
        for id in ids.iter().map(|core| core.id) {
            let cpu = format!("/sys/devices/system/cpu/cpu{id}");
            let (Ok(cache), Ok(siblings)) = (
                fs::read_to_string(format!("{cpu}/cache/index3/shared_cpu_list")),
                fs::read_to_string(format!("{cpu}/topology/thread_siblings_list")),
            ) else {
                return none;
            };
            let is_sibling = seen_siblings.contains(&siblings);
            let groups = if is_sibling {
                &mut seconds
            } else {
                &mut firsts
            };
            groups.entry(cache.trim().to_owned()).or_default().push(id);
            seen_siblings.push(siblings);
        }
        let Some(first_cache) = firsts.keys().next().cloned() else {
            return none;
        };

        let group =
            [&firsts, &seconds].into_iter().filter_map(|groups| groups.get(&first_cache)).flatten().copied().collect();
        let mut order: Vec<usize> = firsts.into_values().flatten().collect();
        order.extend(seconds.into_values().flatten());

        Self { order, group }
    }

    /// Pins the calling thread, the worker numbered `worker` from one, to its core. The first core is left to the
    /// stepping thread while there are cores enough.
    pub(super) fn pin(&self, worker: usize) {
        let Some(&id) = self.order.get(worker % self.order.len().max(1)) else {
            return;
        };
        let _ = core_affinity::set_for_current(core_affinity::CoreId { id });
    }

    /// Keeps the calling thread, the stepping thread, to the first core's group until the answer is dropped.
    pub(super) fn keep_stepping(&self) -> Kept {
        Kept::to(&self.group)
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
