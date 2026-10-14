//! One worker thread: what it is told, what it answers, and how it waits between steps.

use crate::Monotonic;
use aethereal_substrate::{Clock, Done, Reading, Share};
use std::any::Any;
use std::hint;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{Receiver, RecvError, Sender, TryRecvError};

/// What the stepping thread tells a worker.
#[allow(
    clippy::large_enum_variant,
    reason = "an order is sent once for each worker in a step, and a boxed one would be made on one thread and freed \
              on another"
)]
pub(super) enum Order {
    /// Run a part of the step, then wait for the next order as `then` says.
    Run { lend: Lend, then: Waiting },
    /// Wait parked for the next order.
    Park,
    /// Answer at once, to measure how long a parked worker takes to wake.
    Ping,
}

/// How a worker waits between steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Waiting {
    /// Spinning: woken at once, at the cost of a core kept busy.
    Spin,
    /// Parked by the operating system: no core kept busy, at the cost of a wake.
    Park,
}

/// What a worker answers.
pub(super) enum Reply {
    /// Its part of the step ran, left this, and cost it this.
    Done(Done, Report),
    /// Its part of the step panicked with this.
    Panicked(Box<dyn Any + Send>),
    /// It was pinged.
    Pong,
}

/// How long a worker's share of a step kept it busy, in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Report {
    pub(super) busy_nanos: u64,
}

/// The engine as the step's workers read it, lent to one worker for its part of the step, with the clock to time
/// it by.
pub(super) struct Lend {
    reading: *const Reading<'static>,
    clock: Monotonic,
    share: Share,
}

// SAFETY: a `Lend` carries a pointer to a `Reading`, which is `Sync` (checked in `Lend::new`), so reading it from
// another thread is sound while it lives; `Pool::lend` keeps it alive until every worker it was lent to answers.
unsafe impl Send for Lend {}

impl Lend {
    /// `reading` lent for `share`, timed by `clock`.
    ///
    /// The `Reading` must outlive the worker's use of it: the caller waits for the worker's reply before it lets
    /// `reading` go.
    pub(super) fn new<'a>(reading: &'a Reading<'a>, clock: Monotonic, share: Share) -> Self
    where
        Reading<'a>: Sync,
    {
        let reading = (&raw const *reading).cast::<Reading<'static>>();

        Self { reading, clock, share }
    }
}

/// A worker's life: it waits as it was last told, runs what it is given, and ends when the stepping thread drops
/// its end of `orders`.
pub(super) fn work(orders: &Receiver<Order>, replies: &Sender<Reply>) {
    let mut waiting = Waiting::Park;
    loop {
        let order = match waiting {
            Waiting::Spin => spin(orders),
            Waiting::Park => orders.recv(),
        };
        let Ok(order) = order else {
            return;
        };

        let reply = match order {
            Order::Run { lend, then } => {
                waiting = then;
                let Lend { reading, clock, share } = lend;
                // SAFETY: the stepping thread lent this `Reading` in `Pool::lend`, which does not return, and so
                // does not let the `Reading` go, until this worker's reply arrives; the reply is sent after the
                // last use of it.
                let reading = unsafe { &*reading };
                let ran = panic::catch_unwind(AssertUnwindSafe(|| perform(reading, clock, share)));

                ran.map_or_else(Reply::Panicked, |(done, report)| Reply::Done(done, report))
            }
            Order::Park => {
                waiting = Waiting::Park;
                continue;
            }
            Order::Ping => Reply::Pong,
        };
        let is_heard = replies.send(reply).is_ok();
        if !is_heard {
            return;
        }
    }
}

/// The next order, spun for.
fn spin(orders: &Receiver<Order>) -> Result<Order, RecvError> {
    loop {
        match orders.try_recv() {
            Ok(order) => return Ok(order),
            Err(TryRecvError::Empty) => hint::spin_loop(),
            Err(TryRecvError::Disconnected) => return Err(RecvError),
        }
    }
}

/// Runs `share` against `reading`, timing its turns, and how long it kept the worker busy, by `clock`.
pub(super) fn perform(reading: &Reading<'_>, clock: Monotonic, share: Share) -> (Done, Report) {
    let start = clock.nanos();
    let done = reading.run(share, &clock);

    (done, Report { busy_nanos: clock.nanos().saturating_sub(start) })
}
