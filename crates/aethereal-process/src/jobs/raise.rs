//! The job that signals a program's group (ADR-0040).

use crate::{Launcher, Signal};
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::Plain;

/// Sends `signal` to the group of the program numbered `process`, through the host's launcher, which signals it only
/// while it runs. Private to this crate, so no actor but the program's [`Process`](crate::Process) signals it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.raise")]
pub struct Raise {
    pub process: u64,
    pub signal: Signal,
}

/// What an actor keeps for a signal it sends: nothing, as what follows is told by the program's exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.raising")]
pub struct Raising;

/// What a [`Raise`] answers with, once the signal is sent, or passed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.raised")]
pub struct Raised;

impl Job for Raise {
    type Done = Raised;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Raised, Fault> {
        if let Some(launcher) = job.service::<dyn Launcher>() {
            launcher.signal(self.process, self.signal);
        }

        Ok(Raised)
    }
}
