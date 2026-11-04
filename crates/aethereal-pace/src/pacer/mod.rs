//! The pacer: the root that keeps how the program's clock moves, asks its host through its controller to hold it, let
//! it run, or run one held cycle, and answers each advance once its host has said, through the root's source, that
//! the last cycle it asked for has finished (ADR-0045).

mod asked;
#[cfg(test)]
mod tests;

pub use asked::{Advance, Advanced, Pace, PaceSet, SetPace, Unadvanced};

use crate::host::Clockwork;
use aethereal_actor::Requests;
use aethereal_actor::{
    Actor, Beginning, Carries, ChildOf, Controller, Ending, Fault, Given, Handle, Handles, Held, Level, On, Source,
    Stop, ToSender, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::Feed;
use core::time::Duration;
use std::collections::VecDeque;

/// What the pacer's source brings in: that the held cycle it last asked for has finished, every stage of it, and the
/// program's time once it had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.pace.paced")]
pub enum Paced {
    Cycled { elapsed_nanos: u64 },
}

/// What the pacer asks its host through its controller, in the order its turns asked: to hold the clock, so no cycle
/// is due of the host's own accord; to let it run, counting the next cycle's delta from when the host takes this; or
/// to run one held cycle whose `Tick` carries `delta_nanos` exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pacing {
    Hold,
    Run,
    Cycle { delta_nanos: u64 },
}

/// What the pacer is made from: the source its host feeds, the controller it asks its host through, how the clock
/// moves as the program starts, and the most the engine gives one `Tick`, past which a held delta is refused.
#[derive(Bound)]
#[kind(name = "aether.pace.config")]
pub struct PacerConfig {
    source: Given<Source<Paced>>,
    control: Given<Controller<Pacing>>,
    pace: Pace,
    limit_nanos: u64,
}

impl PacerConfig {
    /// The pacer's config, its source the one the engine made, `(source, feed)`, as
    /// [`Setup::source`](aethereal_substrate::Setup::source) answers, and its controller `(control, requests)`, as
    /// [`Setup::controller`](aethereal_substrate::Setup::controller) answers, its clock moving as `pace` says as the
    /// program starts and no held delta past `limit`; and the clockwork its host keeps of it.
    #[must_use]
    pub fn paired(
        (source, feed): (Given<Source<Paced>>, Feed<Paced>),
        (control, requests): (Given<Controller<Pacing>>, Requests<Pacing>),
        pace: Pace,
        limit: Duration,
    ) -> (Self, Clockwork) {
        let limit_nanos = u64::try_from(limit.as_nanos()).unwrap_or(u64::MAX);

        (Self { source, control, pace, limit_nanos }, Clockwork::listed(requests, feed))
    }
}

/// The pacer: it holds the source its host says a held cycle finished through and the controller it asks its host
/// through, and keeps how the clock moves, with the advances it holds while held, answered in the order asked.
#[derive(Bound)]
#[kind(name = "aether.pace")]
pub struct Pacer {
    /// Held so what the host brings comes here; nothing opens it.
    source: Handle<Source<Paced>>,
    control: Handle<Controller<Pacing>>,
    limit_nanos: u64,
    clock: Clock,
}

/// How the clock moves, as the pacer keeps it.
#[derive(Bound)]
enum Clock {
    /// With the host's time.
    Real,
    /// Only when advanced: the advances held, in the order asked. The first one's cycle is the one the host was asked
    /// to run, and the host is asked for the next once it has said that one finished.
    Held(VecDeque<Advancing>),
    /// The program is stopping: no cycle runs again, and every advance is refused.
    Stopping,
}

/// An advance held: the request, the cycles it has left to run, the one the host runs now among them, and the delta of
/// each.
#[derive(Bound)]
struct Advancing {
    held: Held<Advanced>,
    left: u32,
    delta_nanos: u64,
}

/// The pacer is a root, made by the chassis whose clock it holds.
impl ChildOf for Pacer {
    type Parent = Top;
}

/// What asking the pacer at its typed path, `path::<Pacer>(())`, gets.
impl Carries<SetPace> for Pacer {
    type Answers = (PaceSet,);
}

impl Carries<Advance> for Pacer {
    type Answers = (Advanced,);
}

impl Actor for Pacer {
    type Config = PacerConfig;
    type Key = ();
    type Handles = (Paced, SetPace, Advance, On<Stop>);
    type BeginSends = ();
    type EndSends = ();

    /// Takes the source and the controller, asks the host to hold the clock when the program starts held, and
    /// subscribes to `Stop`, after which no cycle runs.
    fn begin(config: PacerConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let source = turn.take(config.source)?;
        let mut control = turn.take(config.control)?;
        turn.subscribe::<Stop>();
        let clock = match config.pace {
            Pace::Real => Clock::Real,
            Pace::Held => {
                turn.open(&mut control)?.push(Pacing::Hold);
                Clock::Held(VecDeque::new())
            }
        };

        Ok(Self { source, control, limit_nanos: config.limit_nanos, clock })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Sets the clock as asked, and answers how it moved before. A clock set running while advances are held refuses
/// them, as they would run against the host's own cycles. Setting the pace it has asks the host nothing.
impl Handles<SetPace> for Pacer {
    type Sends = (ToSender<PaceSet>,);

    fn handle(&mut self, set: SetPace, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let was = self.pace();
        match (&mut self.clock, set.pace) {
            (Clock::Real, Pace::Held) => {
                self.clock = Clock::Held(VecDeque::new());
                turn.open(&mut self.control)?.push(Pacing::Hold);
            }
            (Clock::Held(advancing), Pace::Real) => {
                for advance in advancing.drain(..) {
                    turn.answer(advance.held, &Advanced::Refused(Unadvanced::Running))?;
                }
                self.clock = Clock::Real;
                turn.open(&mut self.control)?.push(Pacing::Run);
            }
            (Clock::Real, Pace::Real) | (Clock::Held(_), Pace::Held) | (Clock::Stopping, _) => {}
        }
        turn.reply(&PaceSet { was })?;

        Ok(())
    }
}

/// Holds the advance, and asks the host for its first cycle when no other advance is under way: answered once its last
/// cycle has finished. One the clock cannot run is refused at once.
impl Handles<Advance> for Pacer {
    type Sends = (ToSender<Advanced>,);

    fn handle(&mut self, advance: Advance, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Advance { cycles, delta_nanos } = advance;
        let refused = match &self.clock {
            Clock::Real => Some(Unadvanced::Running),
            Clock::Stopping => Some(Unadvanced::Stopping),
            Clock::Held(_) if cycles == 0 => Some(Unadvanced::NoCycles),
            Clock::Held(_) if delta_nanos == 0 => Some(Unadvanced::NoTime),
            Clock::Held(_) if delta_nanos > self.limit_nanos => {
                Some(Unadvanced::PastLimit { limit_nanos: self.limit_nanos })
            }
            Clock::Held(_) => None,
        };
        if let Some(why) = refused {
            turn.reply(&Advanced::Refused(why))?;
            return Ok(());
        }
        let Clock::Held(advancing) = &mut self.clock else {
            return Ok(());
        };
        let held = turn.hold()?;
        let is_first = advancing.is_empty();
        advancing.push_back(Advancing { held, left: cycles, delta_nanos });
        if is_first {
            // Pushed last, so a turn that fails before it asks the host for nothing (ADR-0019).
            turn.open(&mut self.control)?.push(Pacing::Cycle { delta_nanos });
        }

        Ok(())
    }
}

/// What the source brought, from this actor to itself (ADR-0010): that the held cycle last asked for finished. It
/// counts against the first advance held, answers that advance once it has none left, and asks the host for the next
/// cycle, of that advance or the one after. What another actor sends is not the host's, and is not acted on.
impl Handles<Paced> for Pacer {
    type Sends = ();

    fn handle(&mut self, paced: Paced, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let me = turn.me();
        let is_brought = turn.sender() == me.into();
        if !is_brought {
            turn.log(Level::Warn, "what another actor said of a cycle was not acted on: only the host's is");
            return Ok(());
        }
        let Paced::Cycled { elapsed_nanos } = paced;
        let Clock::Held(advancing) = &mut self.clock else {
            // Set running, or stopping, as the cycle ran: its advance has been answered already.
            return Ok(());
        };
        let Some(first) = advancing.front_mut() else {
            turn.log(Level::Warn, "the host ran a held cycle that no advance asked for");
            return Ok(());
        };
        first.left -= 1;
        if first.left == 0 {
            let Some(done) = advancing.pop_front() else {
                return Ok(());
            };
            turn.answer(done.held, &Advanced::Advanced { elapsed_nanos })?;
        }
        if let Some(next) = advancing.front() {
            let delta_nanos = next.delta_nanos;
            turn.open(&mut self.control)?.push(Pacing::Cycle { delta_nanos });
        }

        Ok(())
    }
}

/// The program is stopping: every advance held is refused, and so is every one asked from now on.
impl Handles<On<Stop>> for Pacer {
    type Sends = ();

    fn handle(&mut self, _: Stop, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if let Clock::Held(advancing) = &mut self.clock {
            for advance in advancing.drain(..) {
                turn.answer(advance.held, &Advanced::Refused(Unadvanced::Stopping))?;
            }
        }
        self.clock = Clock::Stopping;

        Ok(())
    }
}

impl Pacer {
    /// How the clock moves: a stopping program's as it last did not matter, so it reads as held, which no cycle of
    /// the host's own ends.
    const fn pace(&self) -> Pace {
        match self.clock {
            Clock::Real => Pace::Real,
            Clock::Held(_) | Clock::Stopping => Pace::Held,
        }
    }
}
