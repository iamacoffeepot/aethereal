//! The job that starts a program (ADR-0040).

use crate::Launcher;
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::Plain;

/// Starts `program` with `args` through the host's launcher, as the program numbered `process`. Private to this crate,
/// so no actor but [`Processes`](crate::Processes) starts a program, and each has its process.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.launch")]
pub struct Launch {
    /// The number of the process it is started for, which names it to the launcher.
    pub process: u64,
    /// Where the program is on the host's machine.
    pub program: String,
    pub args: Vec<String>,
}

/// What the root keeps for a launch it runs: the number of the process it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.launching")]
pub struct Launching {
    pub process: u64,
}

/// What became of a [`Launch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.launched")]
pub enum Launched {
    /// The program is running.
    Started,
    /// The host would not start it, or serves no launcher.
    Refused,
}

impl Job for Launch {
    type Done = Launched;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Launched, Fault> {
        let Some(host) = job.service::<dyn Launcher>() else {
            return Ok(Launched::Refused);
        };
        let started = host.start(self.process, self.program, self.args).await;
        let launched = match started {
            Ok(()) => Launched::Started,
            Err(_) => Launched::Refused,
        };

        Ok(launched)
    }
}
