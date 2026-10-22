//! Time a job waits for: the host's timer, which a running job is handed, and the job that only waits (ADR-0018).

use crate::{Fault, Job, Runner, Running, Time};
use aethereal_data::Plain;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

/// The host's timer, on the clock its steps' times are read from, which a running job waits on (ADR-0018): the
/// reactor's natively, and `setTimeout` in a browser.
pub trait Timer: Send + Sync {
    /// The time now.
    fn now(&self) -> Time;

    /// Wakes `waker` at `at`, or as soon as it can after.
    fn wake_at(&self, at: Time, waker: Waker);
}

/// The timer of a host with no thread to wait on: every time past its first moment is one it never reaches, so a job
/// that waits for one waits for good.
pub struct Never;

impl Timer for Never {
    fn now(&self) -> Time {
        Time::ZERO
    }

    fn wake_at(&self, _: Time, _: Waker) {}
}

/// A running job's wait for a time on its host's timer: ready once the time has come.
#[must_use = "a wait does nothing unless it is awaited"]
pub struct Until<'a> {
    at: Time,
    timer: &'a dyn Timer,
}

impl<'a> Until<'a> {
    /// The wait for `at` on `timer`.
    pub(crate) const fn new(at: Time, timer: &'a dyn Timer) -> Self {
        Self { at, timer }
    }
}

impl Future for Until<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        let has_come = self.timer.now() >= self.at;
        if has_come {
            return Poll::Ready(());
        }
        self.timer.wake_at(self.at, context.waker().clone());

        Poll::Pending
    }
}

/// A job that waits until `until` on its host's timer, and does nothing else: the time a turn measures against, as
/// for when to stop waiting for another job (ADR-0018). Every host runs it.
///
/// ```ignore
/// let fetch = turn.run(&Fetch { url }, &Loading { tile })?;
/// turn.run(&Sleep { until: turn.now() + Duration::from_secs(5) }, &GiveUp { on: fetch })?;
/// // in the handler of Answer<Slept, GiveUp>: turn.cancel(on)
/// ```
#[derive(Plain)]
#[kind(name = "aether.job.sleep")]
pub struct Sleep {
    pub until: Time,
}

/// What a sleep answers with, once its time has come.
#[derive(Plain)]
#[kind(name = "aether.job.slept")]
pub struct Slept;

impl Job for Sleep {
    type Done = Slept;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Slept, Fault> {
        job.until(self.until).await;

        Ok(Slept)
    }
}
