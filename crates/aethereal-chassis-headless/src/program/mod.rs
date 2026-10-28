//! The headless host as a program's host: its own section, and how it bootstraps and runs a program (ADR-0029).

#[cfg(test)]
mod tests;

use crate::{Exit, Headless, Runners};
use aethereal_actor::{NoDialer, NoStorage, Storage};
use aethereal_data::Plain;
use aethereal_files::FileHosting;
use aethereal_substrate::{Builds, Host, ProgramError, Serves, Settled};
use core::num::NonZeroUsize;
use core::time::Duration;
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
/// storage a root serves it. With no root serving it, its storage is [`NoStorage`], and every read is unavailable.
pub struct Serving {
    storage: Arc<dyn Storage>,
}

impl Serving {
    /// Nothing served yet.
    #[must_use]
    pub fn new() -> Self {
        Self { storage: Arc::new(NoStorage) }
    }

    /// Keeps the storage the file capability's bootstrap served (ADR-0032).
    pub fn files(&mut self, files: FileHosting) {
        self.storage = files.into_storage();
    }

    /// The runners on the cores an engine on `workers` workers leaves free, their jobs reading files from the storage
    /// served.
    #[must_use]
    pub fn runners(self, workers: NonZeroUsize) -> Runners {
        Runners::beside(workers, self.storage, Arc::new(NoDialer))
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

impl Serves<FileHosting> for Headless {
    fn serve(serving: &mut Serving, files: FileHosting) {
        serving.files(files);
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
