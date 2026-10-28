//! The job that dials out: made by the TCP capability's root alone (ADR-0033).

use crate::{Dialer, Socket, SocketAddress, Undialed};
use aethereal_actor::{Fault, Given, Job, Runner, Running};
use aethereal_data::{Bound, Plain};

/// Connects to `port` on `host` through the host's dialer, and answers with the socket it made (ADR-0018:
/// `Running::hand`). It is private to this crate, so no actor but the root dials, and the root's limit on connections
/// holds.
#[derive(Plain)]
#[kind(name = "aether.tcp.dial")]
pub struct Dial {
    host: String,
    port: u16,
}

impl Dial {
    pub const fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }
}

/// What a dial came to.
#[derive(Bound)]
#[kind(name = "aether.tcp.reached")]
pub enum Reached {
    /// Connected: the socket, on its way to the root, and the address it reached.
    Open { socket: Given<Socket>, peer: SocketAddress },
    /// The name found no address.
    Unresolved,
    /// Every address refused, or none answered in time.
    Refused,
    /// The host serves no dialer.
    Unavailable,
}

/// It waits on the host's dialer, which dials on threads of its own and wakes it.
impl Job for Dial {
    type Done = Reached;
    const RUNNER: Runner = Runner::Io;

    async fn run(self, job: &mut Running) -> Result<Reached, Fault> {
        let Some(dialer) = job.service::<dyn Dialer>() else {
            return Ok(Reached::Unavailable);
        };
        let connected = dialer.dial(&self.host, self.port).await;
        let reached = match connected {
            Ok(stream) => {
                let Ok(peer) = stream.peer_addr() else {
                    return Ok(Reached::Refused);
                };
                Reached::Open { socket: job.hand(Socket(stream)), peer: SocketAddress::from(peer) }
            }
            Err(Undialed::Unresolved) => Reached::Unresolved,
            Err(Undialed::Refused) => Reached::Refused,
        };

        Ok(reached)
    }
}
