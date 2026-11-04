//! The host's side of the program's clock: what every chassis keeps of the pacer, to take what it asks between steps,
//! say a held cycle is due when one is asked for, and say through the pacer's source when it has finished (ADR-0045).

use crate::pacer::{Paced, Pacing};
use aethereal_actor::{Render, Requests, Time};
use aethereal_substrate::{Closed, Engine, Feed};
use core::time::Duration;

/// What a chassis keeps of the pacer once the program runs. A program that lists no pacer has one that asks nothing,
/// [`Clockwork::unlisted`], and its clock runs as it always did.
pub struct Clockwork {
    ends: Ends,
    clocked: Clocked,
}

/// The chassis's end of the pacer's controller and of its source, when the program lists a pacer.
enum Ends {
    /// It lists none.
    Unlisted,
    Listed {
        requests: Requests<Pacing>,
        feed: Feed<Paced>,
    },
}

/// How the clock moves, as the host last took it from the pacer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Clocked {
    /// With the host's time: the host says when a cycle is due.
    Real,
    /// Only when advanced, and this is where the cycle asked for stands.
    Held(Owed),
}

/// A held cycle the pacer asked for: one whose `Tick` carries this many nanoseconds, or a redraw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asking {
    By(u64),
    Redraw,
}

/// Where the held cycle the pacer asked for stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owed {
    /// None is asked for.
    Nothing,
    /// One is asked for, and the host has not said it is due.
    Cycle(Asking),
    /// The host said it is due, and has not yet said it finished.
    Cycling,
}

impl Clockwork {
    /// The clockwork of a program that lists no pacer: it asks nothing, and its clock runs.
    #[must_use]
    pub const fn unlisted() -> Self {
        Self { ends: Ends::Unlisted, clocked: Clocked::Real }
    }

    /// The clockwork of the pacer that asks through `requests` and hears through `feed`, its clock running until it
    /// asks the host to hold it.
    pub(crate) const fn listed(requests: Requests<Pacing>, feed: Feed<Paced>) -> Self {
        Self { ends: Ends::Listed { requests, feed }, clocked: Clocked::Real }
    }

    /// Takes what the pacer asked since the last take, in the order its turns asked, at `now` on the host's clock: a
    /// clock set running counts the next cycle's delta from `now`, so the time it was held adds nothing. Answers
    /// whether it was set running, for a host whose frames each ask for the next, which none asked for while it was
    /// held.
    pub fn take(&mut self, engine: &mut Engine, now: Time) -> bool {
        let Ends::Listed { requests, .. } = &self.ends else {
            return false;
        };
        let was_held = !self.runs();
        for request in requests.take() {
            self.clocked = match (self.clocked, request) {
                (Clocked::Real, Pacing::Hold) => Clocked::Held(Owed::Nothing),
                (Clocked::Held(_), Pacing::Run) => {
                    engine.resume(now);
                    Clocked::Real
                }
                (Clocked::Held(_), Pacing::Cycle { delta_nanos }) => {
                    Clocked::Held(Owed::Cycle(Asking::By(delta_nanos)))
                }
                (Clocked::Held(_), Pacing::Redraw) => Clocked::Held(Owed::Cycle(Asking::Redraw)),
                (clocked, Pacing::Hold | Pacing::Run | Pacing::Cycle { .. } | Pacing::Redraw) => clocked,
            };
        }

        was_held && self.runs()
    }

    /// Whether the host says when cycles are due, of its own accord: it does unless the clock is held.
    #[must_use]
    pub const fn runs(&self) -> bool {
        matches!(self.clocked, Clocked::Real)
    }

    /// Whether a held cycle is asked for that the host has not yet said is due.
    #[must_use]
    pub const fn is_owed(&self) -> bool {
        matches!(self.clocked, Clocked::Held(Owed::Cycle(_)))
    }

    /// Says the held cycle asked for is due, its `Tick` carrying the delta the pacer asked for exactly, or the redraw
    /// asked for, the cycle from its `Render` stage. Nothing when none is asked for.
    pub fn begin(&mut self, engine: &mut Engine) {
        let Clocked::Held(Owed::Cycle(asking)) = self.clocked else {
            return;
        };
        match asking {
            Asking::By(delta_nanos) => engine.cycle_by(Duration::from_nanos(delta_nanos)),
            Asking::Redraw => engine.redraw_from::<Render>(),
        }
        self.clocked = Clocked::Held(Owed::Cycling);
    }

    /// Says through the pacer's source that the held cycle said due has finished, once the engine says it has, with
    /// the program's time then: the pacer asks for the next only once it hears this.
    ///
    /// # Errors
    ///
    /// [`Closed`] when the pacer's source has been dropped.
    pub fn settle(&mut self, engine: &Engine) -> Result<(), Closed> {
        let is_cycling = self.clocked == Clocked::Held(Owed::Cycling);
        let has_cycled = engine.has_cycled();
        let Ends::Listed { feed, .. } = &self.ends else {
            return Ok(());
        };
        let finished = is_cycling && has_cycled;
        if !finished {
            return Ok(());
        }
        self.clocked = Clocked::Held(Owed::Nothing);
        let elapsed_nanos = u64::try_from(engine.elapsed().as_nanos()).unwrap_or(u64::MAX);

        feed.push(Paced::Cycled { elapsed_nanos })
    }
}

impl Default for Clockwork {
    fn default() -> Self {
        Self::unlisted()
    }
}
