//! The job that starts a program (ADR-0040).

use crate::{Invocation, Launcher, PipeEnd};
use aethereal_actor::{Fault, Given, Job, Runner, Running};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::Socket;

/// Starts the program `invocation` names through the host's launcher, as the program numbered `process`, and answers
/// with the launcher's end of its pipe (ADR-0018: `Running::hand`). Private to this crate, so no actor but
/// [`Processes`](crate::Processes) starts a program, and each has its process.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.launch")]
pub struct Launch {
    /// The number of the process it is started for, which names it to the launcher.
    pub process: u64,
    pub invocation: Invocation,
}

/// What the root keeps for a launch it runs: the number of the process it is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.process.launching")]
pub struct Launching {
    pub process: u64,
}

/// What became of a [`Launch`].
#[derive(Bound)]
#[kind(name = "aether.process.launched")]
pub enum Launched {
    /// The program is running, its stdin and stdout both closed.
    Started,
    /// The program is running, with its pipe: the launcher's end, on its way to the root.
    Piped { socket: Given<Socket> },
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
        let started = host.start(self.process, self.invocation).await;
        let launched = match started {
            Ok(PipeEnd::Closed) => Launched::Started,
            Ok(PipeEnd::Connected(socket)) => Launched::Piped { socket: job.hand(socket) },
            Err(_) => Launched::Refused,
        };

        Ok(launched)
    }
}
