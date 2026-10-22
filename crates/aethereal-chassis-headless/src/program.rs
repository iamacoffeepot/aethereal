//! The headless host as a program's host: its own section, and how it bootstraps and runs a program (ADR-0029).

use crate::{Exit, Headless};
use aethereal_data::Plain;
use aethereal_substrate::{Builds, Host, ProgramError, Settled};
use core::time::Duration;

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

impl Host for Headless {
    type Running = RunningSettings;
    type Hosting<'host> = ();
    type Exit = Exit;

    /// Bootstraps the program, then runs it as [`Headless::run`] does, every `every_millis`, giving `Stop`
    /// `stop_within_millis`.
    fn run(settled: Settled<Self>) -> Result<Exit, ProgramError> {
        let every = Duration::from_millis(settled.running().every_millis);
        let stop_within = Duration::from_millis(settled.running().stop_within_millis);

        Ok(Self::build(settled)?.run(every, stop_within))
    }
}

impl Builds for Headless {
    /// Bootstraps the program, and the host holding its engine on as many workers as its settings say.
    fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
        let workers = settled.engine().worker_threads();

        let (engine, ()) = settled.bootstrap(())?;

        Ok(Self::new(engine, workers))
    }
}
