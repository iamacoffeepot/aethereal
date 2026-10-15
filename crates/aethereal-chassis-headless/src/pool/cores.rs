//! Which core each worker is pinned to: one per last-level cache before two share one (ADR-0011).

use std::collections::BTreeMap;
use std::fs;

/// The cores workers are pinned to, in the order they are handed out.
///
/// A step's memory stays warm in the cache of the core that uses it, so each
/// worker keeps to one core. The cores are taken one from each group sharing a
/// last-level cache before a second from any, so a few workers have the most
/// cache between them, and one hardware thread of each core before its
/// sibling. The stepping thread is not pinned: gather, settle and plan run on
/// it while every worker waits, and the host's scheduler moves it off a core
/// that something else is using.
pub(super) struct Cores {
    /// Logical CPU numbers, one per worker in turn, wrapping round when there are more workers.
    order: Vec<usize>,
}

impl Cores {
    /// The cores of this host, read from Linux's description of its caches. None are known on another host, or
    /// when that description cannot be read, and then no worker is pinned.
    pub(super) fn of_host() -> Self {
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
                return Self { order: Vec::new() };
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

        let mut order = interleaved(&firsts.into_values().collect::<Vec<_>>());
        order.extend(interleaved(&seconds.into_values().collect::<Vec<_>>()));

        Self { order }
    }

    /// Pins the calling thread, the worker numbered `worker` from one, to its core. The first core is left to the
    /// stepping thread while there are cores enough.
    pub(super) fn pin(&self, worker: usize) {
        let Some(&id) = self.order.get(worker % self.order.len().max(1)) else {
            return;
        };
        let _ = core_affinity::set_for_current(core_affinity::CoreId { id });
    }
}

/// The cores of each group, taken one from each group in turn.
fn interleaved(groups: &[Vec<usize>]) -> Vec<usize> {
    let deepest = groups.iter().map(Vec::len).max().unwrap_or(0);

    (0..deepest).flat_map(|place| groups.iter().filter_map(move |cores| cores.get(place).copied())).collect()
}
