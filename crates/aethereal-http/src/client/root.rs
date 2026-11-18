//! The HTTP capability's root (ADR-0035): the client side. It dials a server for each actor that asks, and stands a
//! [`Client`] up beneath it on the connection.

use crate::http1::fit::{BlobBytesTooLow, ReadLimitTooLow};
use crate::{Client, ClientConfig, HttpSettings};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anyone, Beginning, ChildOf, Ended, Ending, Fault, Handles, Held, Receives,
    Spawn, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tcp::{self as tcp, ConnectUnix, ConnectedUnix, Dialed, Framing, Shut, SocketPath, Tcp, Unaddressable};
use std::collections::BTreeMap;

/// What an actor asks the root to connect to: a server over TCP or on a Unix socket, which a [`Client`] beneath the
/// root then writes requests to.
#[derive(Debug, Plain)]
#[kind(name = "aether.http.connect")]
pub struct Connect {
    pub peer: Peer,
}

/// Where a server is.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.peer")]
pub enum Peer {
    /// On `host`, a name or an address written out, at `port`.
    Tcp { host: String, port: u16 },
    /// On the Unix socket at `path`, on this machine.
    Unix { path: SocketPath },
}

/// What a connect came to.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.connected")]
pub enum Connected {
    /// Connected: the client beneath the root, which writes each request it is asked on the one connection.
    Open { client: ActorRef<Client> },
    /// The connection failed, for the reason `Tcp` gave.
    Unreached(Unreached),
    /// The root holds its most clients, or the engine would hold no more actors.
    Full,
    /// The program has no `Tcp`.
    Unavailable,
}

/// Why a client's connection failed: the reason `Tcp` gave.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.unreached")]
pub enum Unreached {
    /// The name found no address, or is not one a request's `Host` can write.
    Unresolved,
    /// The socket's path can name no socket.
    Unaddressable(Unaddressable),
    /// No socket is at the path.
    Missing,
    /// Every address refused, none answered in time, or nothing listens on the socket.
    Refused,
    /// `Tcp` holds its most connections.
    Full,
    /// The host serves no dialer, or has no Unix sockets.
    Unavailable,
}

/// What the root keeps for a connect over TCP it asked of `Tcp`: which connect it was asked for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.reaching")]
pub struct Reaching {
    number: u64,
}

/// What the root keeps for a connect to a Unix socket it asked of `Tcp`: which connect it was asked for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.reaching_unix")]
pub struct ReachingUnix {
    number: u64,
}

/// A client the root stood up, and the actor that asked for it.
#[derive(Bound)]
struct Opened {
    client: ActorRef<Client>,
    asker: u64,
}

/// A connect the root holds until `Tcp` answers it, the `Host` its client writes, and the actor that asked it.
#[derive(Bound)]
struct Connecting {
    held: Held<Connected>,
    host: String,
    asker: u64,
}

/// The HTTP capability: asks HTTP/1.1 of other servers, over `Tcp`, for every other actor (ADR-0035). An actor asks it
/// to [`Connect`] to a server at its typed path, `path::<Http>(())`, and is answered with the client that asks it. A
/// program serves a port with a [stack](crate::stack) of its own, which needs no root.
///
/// It watches each actor that asks, and when one ends, shuts every client it stood up for it.
#[derive(Bound)]
#[kind(name = "aether.http")]
pub struct Http {
    settings: HttpSettings,
    /// Each client standing, by its key.
    clients: BTreeMap<u64, Opened>,
    connecting: BTreeMap<u64, Connecting>,
    /// The actors watched for having asked.
    askers: Vec<u64>,
    next: u64,
}

/// It stands up on every host, and refuses to where a response's head it would read could not be: the asker it was sent
/// to would never be handed it, and the engine would answer it `Unreadable` (ADR-0017), so a response within the
/// client's limits would never reach its asker; and where the engine's blobs could not hold a response's body read
/// whole, which is a blob, beside a chunk its connection reads (ADR-0042).
impl<H: Host> Bootstrap<H> for Http {
    const SECTION: &'static str = "http";
    type Settings = HttpSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(settings: HttpSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let read_limit = cx.read_limit();
        let needed = settings.most_read_bytes();
        if read_limit < needed {
            return Err(Unbuilt::Failed(Box::new(ReadLimitTooLow { read_limit, needed })));
        }
        let blob_bytes = cx.blob_bytes();
        let needed = settings.most_blob_bytes();
        if blob_bytes < needed {
            return Err(Unbuilt::Failed(Box::new(BlobBytesTooLow { blob_bytes, needed })));
        }

        cx.begin(&(), settings)?;

        Ok(())
    }
}

impl ChildOf for Http {
    type Parent = Top;
}

impl Actor for Http {
    type Config = HttpSettings;
    type Key = ();
    type Handles = (Connect, Answer<tcp::Connected, Reaching>, Answer<ConnectedUnix, ReachingUnix>, Ended);
    type Depends = (Tcp,);
    type Spawns = (Client,);

    fn begin(settings: HttpSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { settings, clients: BTreeMap::new(), connecting: BTreeMap::new(), askers: Vec::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Connect> for Http {
    type From = Anyone;
    type Returns = aethereal_actor::Reply<Connected>;

    fn handle(
        &mut self,
        connect: Connect,
        turn: &mut Turn<'_, Self>,
    ) -> Result<aethereal_actor::Reply<Connected>, Fault> {
        let open = self.clients.len() + self.connecting.len();
        let is_full = open >= usize::try_from(self.settings.most_clients).unwrap_or(usize::MAX);
        if is_full {
            return Ok(aethereal_actor::Reply::Now(Connected::Full));
        }
        let framing = Framing::Chunks { most_bytes: self.settings.chunk_bytes };
        let number = self.next;
        let host = match &connect.peer {
            Peer::Tcp { host, port } => host_of(host, *port),
            // A Unix socket names no host, and a server on one reads none: this machine's name stands for it.
            Peer::Unix { .. } => Ok(String::from("localhost")),
        };
        let Ok(host) = host else {
            return Ok(aethereal_actor::Reply::Now(Connected::Unreached(Unreached::Unresolved)));
        };
        let (held, later) = turn.hold();
        let asker = self.watch_asker(turn);
        self.next += 1;
        match connect.peer {
            Peer::Tcp { host, port } => {
                turn.to(path::<Tcp>(())).ask(&tcp::Connect { host, port, framing }, &Reaching { number })?;
            }
            Peer::Unix { path: socket } => {
                turn.to(path::<Tcp>(())).ask(&ConnectUnix { path: socket, framing }, &ReachingUnix { number })?;
            }
        }
        self.connecting.insert(number, Connecting { held, host, asker });

        Ok(aethereal_actor::Reply::Later(later))
    }
}

impl Receives<Answer<tcp::Connected, Reaching>> for Http {
    fn handle(
        &mut self,
        (answered, reaching): (Answered<tcp::Connected>, Reaching),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(connecting) = self.connecting.remove(&reaching.number) else {
            return Ok(());
        };
        let connected = match answered {
            Answered::Yes(tcp::Connected::Open { connection, .. }) => {
                self.stand_client(turn, reaching.number, connection, connecting.host, connecting.asker)?
            }
            Answered::Yes(tcp::Connected::Unresolved) => Connected::Unreached(Unreached::Unresolved),
            Answered::Yes(tcp::Connected::Refused) => Connected::Unreached(Unreached::Refused),
            Answered::Yes(tcp::Connected::Full) => Connected::Unreached(Unreached::Full),
            Answered::Yes(tcp::Connected::Unavailable) => Connected::Unreached(Unreached::Unavailable),
            Answered::No(_) => Connected::Unavailable,
        };
        turn.answer(connecting.held, &connected)?;

        Ok(())
    }
}

impl Receives<Answer<ConnectedUnix, ReachingUnix>> for Http {
    fn handle(
        &mut self,
        (answered, reaching): (Answered<ConnectedUnix>, ReachingUnix),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(connecting) = self.connecting.remove(&reaching.number) else {
            return Ok(());
        };
        let connected = match answered {
            Answered::Yes(ConnectedUnix::Open { connection }) => {
                self.stand_client(turn, reaching.number, connection, connecting.host, connecting.asker)?
            }
            Answered::Yes(ConnectedUnix::Unaddressable(why)) => Connected::Unreached(Unreached::Unaddressable(why)),
            Answered::Yes(ConnectedUnix::Missing) => Connected::Unreached(Unreached::Missing),
            Answered::Yes(ConnectedUnix::Refused) => Connected::Unreached(Unreached::Refused),
            Answered::Yes(ConnectedUnix::Full) => Connected::Unreached(Unreached::Full),
            Answered::Yes(ConnectedUnix::Unavailable) => Connected::Unreached(Unreached::Unavailable),
            Answered::No(_) => Connected::Unavailable,
        };
        turn.answer(connecting.held, &connected)?;

        Ok(())
    }
}

impl Receives<Ended> for Http {
    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let gone = ended.actor().id().bits();
        self.clients.retain(|_, opened| opened.client.id().bits() != gone);
        let was_asker = self.askers.contains(&gone);
        if !was_asker {
            return Ok(());
        }
        self.askers.retain(|&asker| asker != gone);
        for opened in self.clients.values().filter(|opened| opened.asker == gone) {
            turn.to(opened.client).send(&Shut)?;
        }

        Ok(())
    }
}

impl Http {
    /// Stands a client up, keyed `number`, on the connection `Tcp` dialed for `asker`, writing `host` as each request's
    /// `Host`; or shuts the connection and says why not.
    fn stand_client(
        &mut self,
        turn: &mut Turn<'_, Self>,
        number: u64,
        connection: ActorRef<Dialed>,
        host: String,
        asker: u64,
    ) -> Result<Connected, Fault> {
        let is_asked = self.askers.contains(&asker);
        if !is_asked {
            // Its asker has ended: no one is left to ask the client anything.
            turn.to(connection).send(&Shut)?;
            return Ok(Connected::Full);
        }
        let config = ClientConfig { connection, host, limits: self.settings.client_limits() };
        let Ok(client) = turn.spawn::<Client>(&number, config) else {
            turn.to(connection).send(&Shut)?;
            return Ok(Connected::Full);
        };
        turn.watch(client);
        self.clients.insert(number, Opened { client, asker });

        Ok(Connected::Open { client })
    }

    /// Watches the sender of the mail handled, once, and answers who it is.
    fn watch_asker(&mut self, turn: &mut Turn<'_, Self>) -> u64 {
        let asker = turn.sender();
        let id = asker.id().bits();
        let is_watched = self.askers.contains(&id);
        if !is_watched {
            turn.watch(asker);
            self.askers.push(id);
        }

        id
    }
}

/// What a client writes as `Host` for a server on `host` at `port` (RFC 9110 §7.2): an IPv6 address in brackets. A
/// host that is not all visible bytes would write a header of its own, and is refused.
fn host_of(host: &str, port: u16) -> Result<String, Unreached> {
    let is_visible = !host.is_empty() && host.bytes().all(|byte| byte.is_ascii_graphic());
    if !is_visible {
        return Err(Unreached::Unresolved);
    }
    let is_bare_v6 = host.contains(':') && !host.starts_with('[');
    let written = if is_bare_v6 {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };

    Ok(written)
}
