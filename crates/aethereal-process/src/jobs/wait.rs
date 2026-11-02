//! The job that waits for a program to exit (ADR-0040).

use crate::{ExitStatus, Exited, Launcher};
use aethereal_actor::{Fault, Job, Runner, Running};
use aethereal_data::Plain;

/// Waits for the program numbered `process` to exit, through the host's launcher, which waits on a thread of its own
/// and wakes it. Private to this crate, so no actor but the program's [`Process`](crate::Process) takes its exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.wait")]
pub struct Wait {
    pub process: u64,
}

/// What a process keeps for the wait it runs: nothing, as it runs one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.waiting")]
pub struct Waiting;

/// What became of a [`Wait`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.waited")]
pub enum Waited {
    /// The program exited: how, and the tail of its stderr.
    Exited { status: ExitStatus, stderr: String },
    /// The host serves no launcher, or its launcher could not tell the exit.
    Lost,
}

impl Job for Wait {
    type Done = Waited;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Waited, Fault> {
        let Some(launcher) = job.service::<dyn Launcher>() else {
            return Ok(Waited::Lost);
        };
        let exited = launcher.wait(self.process).await;
        let waited = match exited {
            Ok(Exited { status, stderr }) => Waited::Exited { status, stderr },
            Err(_) => Waited::Lost,
        };

        Ok(waited)
    }
}
