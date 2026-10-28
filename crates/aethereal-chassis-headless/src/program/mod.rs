//! The headless host as a program's host: its own section, and how it bootstraps and runs a program (ADR-0029).

#[cfg(test)]
mod tests;

use crate::{Exit, Headless, Runners};
use aethereal_actor::{Service, Services};
use aethereal_data::Plain;
use aethereal_substrate::{Builds, Host, ProgramError, Serves, Settled};
use core::num::NonZeroUsize;
use core::time::Duration;
use std::mem;
use std::sync::Arc;

/// How the headless host runs a program: its section, `running` (ADR-0028).
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
#[kind(name = "aether.headless.running")]
#[settings]
pub struct RunningSettings {
    /// The milliseconds between cycles.
    #[flag]
    pub every_millis: u64,
    /// The milliseconds `Stop` is given to finish once the program is asked to stop.
    #[flag]
    pub stop_within_millis: u64,
}

impl Default for RunningSettings {
    fn default() -> Self {
        Self { every_millis: 16, stop_within_millis: 1000 }
    }
}

/// What a native host keeps as its program bootstraps, for the runners it makes once its roots have (ADR-0029): the
/// services its roots serve its jobs, whatever they are (ADR-0034).
pub struct Serving {
    services: Services,
}

impl Serving {
    /// Nothing served yet.
    #[must_use]
    pub fn new() -> Self {
        Self { services: Services::none() }
    }

    /// Keeps `service`, which a root's bootstrap served.
    pub fn serve<S: ?Sized + Send + Sync + 'static>(&mut self, service: Service<S>) {
        self.services = service.added_to(mem::take(&mut self.services));
    }

    /// The runners on the cores an engine on `workers` workers leaves free, their jobs reaching the services served.
    #[must_use]
    pub fn runners(self, workers: NonZeroUsize) -> Runners {
        Runners::beside(workers, Arc::new(self.services))
    }
}

impl Default for Serving {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for Headless {
    type Running = RunningSettings;
    type Hosting<'host> = Serving;
    type Exit = Exit;
    type Error = ProgramError;

    /// Bootstraps the program, then runs it as [`Headless::run`] does, every `every_millis`, giving `Stop`
    /// `stop_within_millis`.
    fn run(settled: Settled<Self>) -> Result<Exit, ProgramError> {
        let every = Duration::from_millis(settled.running().every_millis);
        let stop_within = Duration::from_millis(settled.running().stop_within_millis);

        Ok(Self::build(settled)?.run(every, stop_within))
    }
}

/// Every service of every capability is kept the same way, so the host names none (ADR-0034).
impl<S: ?Sized + Send + Sync + 'static> Serves<Service<S>> for Headless {
    fn serve(serving: &mut Serving, service: Service<S>) {
        serving.serve(service);
    }
}

impl Builds for Headless {
    /// Bootstraps the program, blocking this thread on each root's bootstrap, then makes its runners with what its
    /// roots served, on the cores its workers leave free (ADR-0029): the host holding its engine on as many workers as
    /// its settings say.
    fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
        let workers = settled.engine().worker_threads();

        let (mut engine, serving) = pollster::block_on(settled.bootstrap(Serving::new()))?;
        engine.set_jobs(Arc::new(serving.runners(workers)));

        Ok(Self::new(engine, workers))
    }
}
