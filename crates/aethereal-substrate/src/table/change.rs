//! The one count of changes to the table (ADR-0008).

use std::hint::spin_loop;
use std::sync::atomic::{AtomicU64, Ordering};

/// The number of one change to the table: an actor going live, or an actor closing (ADR-0008).
///
/// Changes are numbered upward from one count, and zero is the empty table. A
/// reader that holds a number sees the table as it stood at that change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Change(pub(super) u64);

/// The count the table's changes are numbered from, and the latest number a reader may take.
///
/// Both are atomics because the table belongs to no actor: every thread that
/// runs a turn writes it and reads it, so one-mail-at-a-time does not cover it.
#[derive(Default)]
pub(super) struct Changes {
    /// How many numbers writers have taken.
    taken: AtomicU64,
    /// The latest number whose change, and every change before it, is in the table.
    latest: AtomicU64,
}

impl Changes {
    /// The latest change a reader sees: it, and every change numbered before it, is in the table.
    pub(super) fn latest(&self) -> Change {
        Change(self.latest.load(Ordering::Acquire))
    }

    /// Takes the next number, has `write` put the change it numbers in the table, then shows it to readers.
    ///
    /// Numbers are shown in order, so a writer waits here for the writers that
    /// took the numbers before its own; each of those is past its `write` or in
    /// it, and none waits on a reader. With one thread there is never one to
    /// wait for. A number whose `write` changed nothing is shown all the same.
    pub(super) fn make<T>(&self, write: impl FnOnce(Change) -> T) -> T {
        let before = self.taken.fetch_add(1, Ordering::Relaxed);
        let change = before + 1;
        let written = write(Change(change));

        // Acquire on the way in, release on the way out: a reader that loads this number is after every earlier
        // writer's entry as well as this one's.
        while self.latest.compare_exchange_weak(before, change, Ordering::AcqRel, Ordering::Relaxed).is_err() {
            spin_loop();
        }

        written
    }
}
