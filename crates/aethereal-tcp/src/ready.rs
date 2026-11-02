//! The job that waits for a socket to be ready: run by the actor that owns the socket, each time a call on it would
//! wait (ADR-0033).

use crate::{Interest, Readiness};
use aethereal_actor::{Fault, Job, JobId, Runner, Running};
use aethereal_data::{Bound, Plain};

/// Waits through the host's poller until the socket whose descriptor is `socket` is ready for `interest`, once. The
/// job names the socket and does not own it: the actor that ran it keeps the socket (ADR-0018). It is private to this
/// crate, so no actor but the socket's owner names a descriptor.
#[derive(Plain)]
#[kind(name = "aether.tcp.ready")]
pub struct Ready {
    socket: u64,
    interest: Interest,
}

impl Ready {
    pub const fn new(socket: u64, interest: Interest) -> Self {
        Self { socket, interest }
    }
}

/// Whether the actor that owns a socket waits for it to be ready for one interest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
pub enum Wait {
    /// It does not: no call it would make now waits on the socket.
    Not,
    /// The [`Ready`] it ran, not yet answered.
    On(JobId),
}

/// What a wait for a socket came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.readied")]
pub enum Readied {
    /// The socket is ready, or may be: the call made now says which.
    Ready,
    /// The host serves no poller, so the socket can never be waited on.
    Unavailable,
}

/// It waits on the host's poller, which the thread that runs it sleeps in.
impl Job for Ready {
    type Done = Readied;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Readied, Fault> {
        let Some(readiness) = job.service::<dyn Readiness>() else {
            return Ok(Readied::Unavailable);
        };
        readiness.ready(self.socket, self.interest).await;

        Ok(Readied::Ready)
    }
}
