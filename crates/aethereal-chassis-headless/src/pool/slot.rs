//! Where the stepping thread leaves each worker its orders and finds its answers: a count each side spins or parks on,
//! with no channel between them (ADR-0011).

use super::worker::{Order, Reply, Waiting};
use std::collections::VecDeque;
use std::hint;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::{self, Thread};

/// Every worker's slot, and how many answers they have given between them.
pub(super) struct Board {
    /// One slot for each worker, by its number less one: the stepping thread has none.
    slots: Vec<Slot>,
    /// How many answers the workers have given, ever: what the stepping thread waits on.
    answered: AtomicUsize,
    /// The stepping thread, woken by each answer when it parked to wait for them.
    stepping: Thread,
}

/// One worker's orders, and its answers.
struct Slot {
    /// The orders it has not yet taken, oldest first.
    orders: Mutex<VecDeque<Order>>,
    /// How many orders wait in `orders`: what the worker spins or parks on.
    waiting: AtomicUsize,
    /// What it has answered that the stepping thread has not yet taken.
    answers: Mutex<Vec<Reply>>,
}

impl Board {
    /// A board for `workers` workers, each with nothing to do, made on the stepping thread.
    pub(super) fn new(workers: usize) -> Self {
        let slot =
            || Slot { orders: Mutex::new(VecDeque::new()), waiting: AtomicUsize::new(0), answers: Mutex::default() };

        Self {
            slots: (0..workers).map(|_| slot()).collect(),
            answered: AtomicUsize::new(0),
            stepping: thread::current(),
        }
    }

    /// Leaves `order` for the worker `worker`, which runs on `thread`, and wakes it if it is parked.
    pub(super) fn post(&self, worker: usize, thread: &Thread, order: Order) {
        let slot = &self.slots[worker];
        locked(&slot.orders).push_back(order);
        slot.waiting.fetch_add(1, Ordering::Release);
        thread.unpark();
    }

    /// The next order for the worker `worker`, the thread that calls this, waited for as `waiting` says.
    pub(super) fn take(&self, worker: usize, waiting: Waiting) -> Order {
        let slot = &self.slots[worker];
        loop {
            let has_order = slot.waiting.load(Ordering::Acquire) > 0;
            if has_order && let Some(order) = locked(&slot.orders).pop_front() {
                slot.waiting.fetch_sub(1, Ordering::Relaxed);

                return order;
            }
            match waiting {
                Waiting::Spin => hint::spin_loop(),
                Waiting::Park => thread::park(),
            }
        }
    }

    /// Leaves the worker `worker`'s answer `reply` for the stepping thread.
    pub(super) fn answer(&self, worker: usize, reply: Reply) {
        locked(&self.slots[worker].answers).push(reply);
        self.answered.fetch_add(1, Ordering::Release);
        self.stepping.unpark();
    }

    /// How many answers the workers have given, ever.
    pub(super) fn answered(&self) -> usize {
        self.answered.load(Ordering::Acquire)
    }

    /// Waits until the workers have given `answers` answers, ever: spinning while `spinning` says to go on, which
    /// it is asked between spins, then parked, woken by each answer.
    pub(super) fn wait_for(&self, answers: usize, mut spinning: impl FnMut() -> bool) {
        while self.answered() < answers {
            if spinning() {
                hint::spin_loop();
            } else {
                thread::park();
            }
        }
    }

    /// Moves the worker `worker`'s answers into `into`, oldest first.
    pub(super) fn answers(&self, worker: usize, into: &mut Vec<Reply>) {
        into.append(&mut locked(&self.slots[worker].answers));
    }
}

/// `mutex` locked. A thread that panicked while it held one left nothing half done: every lock here is held only to
/// push or pop.
fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
