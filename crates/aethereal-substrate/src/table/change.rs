//! The one count of changes to the table (ADR-0008).

use std::array;
use std::hint::spin_loop;
use std::sync::atomic::{AtomicU64, Ordering};

/// The number of one change to the table: an actor going live, or an actor closing (ADR-0008).
///
/// Changes are numbered upward from one count, and zero is the empty table. A
/// reader that holds a number sees the table as it stood at that change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Change(pub(super) u64);

/// How many changes may be written at one moment. A writer past that many waits for one of them to finish.
const WRITERS: usize = 32;

/// What a writer's place holds while no writer is in it: later than every change.
const IDLE: u64 = u64::MAX;

/// A value on a cache line of its own, so that a thread writing it does not slow one reading its neighbour.
#[derive(Default)]
#[repr(align(64))]
struct Apart<T>(T);

/// The count the table's changes are numbered from, and the latest number a reader may take.
///
/// All of it is atomics because the table belongs to no actor: every thread
/// that runs a turn writes it and reads it, so one-mail-at-a-time does not
/// cover it.
///
/// A writer never waits for another. It says which number it is writing, in a
/// place of its own, and the latest number a reader may take is the one before
/// the lowest still being written. So a change that is written is not shown
/// while one numbered before it is still being written.
pub(super) struct Changes {
    /// How many numbers writers have taken.
    taken: Apart<AtomicU64>,
    /// The latest number whose change, and every change before it, is known to be in the table.
    latest: Apart<AtomicU64>,
    /// The writers' places: each holds a number no later than the one its writer is writing, or [`IDLE`].
    writing: [AtomicU64; WRITERS],
}

impl Default for Changes {
    fn default() -> Self {
        Self { taken: Apart::default(), latest: Apart::default(), writing: array::from_fn(|_| AtomicU64::new(IDLE)) }
    }
}

impl Changes {
    /// The latest change a reader sees: it, and every change numbered before it, is in the table.
    pub(super) fn latest(&self) -> Change {
        Change(self.latest.0.load(Ordering::SeqCst))
    }

    /// Takes the next number, has `write` put the change it numbers in the table, then shows what is written.
    ///
    /// The writer waits for no other. Its own change is shown once every
    /// change numbered before it is written, which may be after this returns:
    /// the writer of the last of those shows it. With one thread that is at
    /// once. A number whose `write` changed nothing is shown all the same.
    pub(super) fn make<T>(&self, write: impl FnOnce(Change) -> T) -> T {
        let writing = self.enter();
        let change = self.taken.0.fetch_add(1, Ordering::SeqCst) + 1;
        writing.place.store(change, Ordering::SeqCst);
        let written = write(Change(change));

        drop(writing);
        if !self.shows_alone(change) {
            self.show();
        }

        written
    }

    /// Shows `change` without reading the places, when nothing else is being written. False when it could not.
    ///
    /// The latest number being the one before `change` says every earlier
    /// change is written. No number taken after `change` says there is no
    /// later writer who, having seen this one still writing, left its own
    /// change for this one to show.
    fn shows_alone(&self, change: u64) -> bool {
        let is_next = self.latest.0.compare_exchange(change - 1, change, Ordering::SeqCst, Ordering::Relaxed).is_ok();
        let is_last = self.taken.0.load(Ordering::SeqCst) == change;

        is_next && is_last
    }

    /// Takes a free place and writes in it a number no later than the next one taken.
    ///
    /// The place is taken before the number is, so a reader of the places that
    /// has seen a number taken finds its writer here or finds it gone.
    fn enter(&self) -> Writing<'_> {
        loop {
            let next = self.taken.0.load(Ordering::SeqCst) + 1;
            #[allow(clippy::cast_possible_truncation, reason = "a remainder of the count of places fits in any usize")]
            let first = (next % WRITERS as u64) as usize;
            let places = self.writing.iter().cycle().skip(first).take(WRITERS);
            for place in places {
                if place.compare_exchange(IDLE, next, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
                    return Writing { place };
                }
            }

            spin_loop();
        }
    }

    /// Raises the latest number to the one before the lowest still being written.
    ///
    /// The count is read before the places. A writer whose number is within
    /// that count took its place before it took the number, so the places then
    /// show it or it has finished.
    fn show(&self) {
        let taken = self.taken.0.load(Ordering::SeqCst);
        let unwritten = self.writing.iter().map(|place| place.load(Ordering::SeqCst)).min().unwrap_or(IDLE);
        let written = taken.min(unwritten - 1);

        if self.latest.0.load(Ordering::SeqCst) < written {
            self.latest.0.fetch_max(written, Ordering::SeqCst);
        }
    }
}

/// A writer's hold on its place, which it leaves when its change is written, or when its write is abandoned.
struct Writing<'a> {
    place: &'a AtomicU64,
}

impl Drop for Writing<'_> {
    fn drop(&mut self) {
        self.place.store(IDLE, Ordering::SeqCst);
    }
}

// Native only: the browser host runs the engine on one thread (ADR-0001).
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;

    #[test]
    fn change_is_shown_after_every_change_before_it() {
        // Catches a reader given a number while a change before it is unwritten, and a latest number left behind
        // once that change is written.
        let changes = Changes::default();
        changes.make(|_| ());
        let (entered, is_entered) = mpsc::channel();
        let (release, is_released) = mpsc::channel::<()>();
        thread::scope(|scope| {
            let changes = &changes;
            scope.spawn(move || {
                changes.make(|change| {
                    entered.send(change).expect("the test waits for the slow writer");
                    is_released.recv().expect("the test releases the slow writer");
                });
            });
            let slow = is_entered.recv().expect("the slow writer enters");
            let later = changes.make(|change| change);

            assert_eq!((slow, later), (Change(2), Change(3)));
            assert_eq!(changes.latest(), Change(1));
            release.send(()).expect("the slow writer is waiting");
        });

        assert_eq!(changes.latest(), Change(3));
    }
}
