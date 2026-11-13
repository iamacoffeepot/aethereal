//! The HTTP capability's root (ADR-0035).

use crate::{BlobBytesTooLow, Client, ClientConfig, Handling, HttpSettings, ReadLimitTooLow, Server, ServerConfig};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, Held,
    Path, ProtocolRef, ToSender, ToType, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_tcp::{
    self as tcp, ConnectUnix, ConnectedUnix, Dialed, Framing, Listen, Listener, Listening, Shut, SocketPath, Tcp,
    Unaddressable,
};
use std::collections::BTreeMap;

/// What an actor asks the root to serve on: a port, 0 for any the host picks, and the handler each request is asked
/// of, usually the asker itself (`turn.me().protocol()`).
#[derive(Debug, Bound)]
#[kind(name = "aether.http.serve")]
pub struct Serve {
    pub port: u16,
    pub handler: ProtocolRef<Handling>,
}

/// What a serve came to.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.served")]
pub enum Served {
    /// Bound: the server beneath the root, and the port bound, which is its key.
    Bound { server: ActorRef<Server>, port: u16 },
    /// Something holds the port.
    Taken,
    /// The root holds its most servers.
    Full,
    /// The host refused the bind.
    Refused,
    /// The program has no `Tcp`.
    Unavailable,
}

/// What the root keeps for a listen it asked of `Tcp`: which serve it was asked for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.binding")]
pub struct Binding {
    number: u64,
}

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

/// A serve the root holds until `Tcp` answers its listen, the handler it names, and the actor that asked it.
#[derive(Bound)]
struct Waiting {
    held: Held<Served>,
    handler: ProtocolRef<Handling>,
    asker: u64,
}

/// A listener `Tcp` bound, its port, and the serve's handler and asker: what a server is stood up from.
#[derive(Clone, Copy)]
struct Opening {
    listener: ActorRef<Listener>,
    port: u16,
    handler: ProtocolRef<Handling>,
    asker: u64,
}

/// A server or a client the root stood up, and the actor that asked for it.
#[derive(Bound)]
struct Opened<C> {
    child: ActorRef<C>,
    asker: u64,
}

/// A connect the root holds until `Tcp` answers it, the `Host` its client writes, and the actor that asked it.
#[derive(Bound)]
struct Connecting {
    held: Held<Connected>,
    host: String,
    asker: u64,
}

/// The HTTP capability: serves HTTP/1.1 on `Tcp` for every other actor, and asks it of other servers (ADR-0035). An
/// actor asks it to [`Serve`] at its typed path, `path::<Http>(())`, naming the handler each request is asked of, or
/// to [`Connect`] to a server, and is answered with the client that asks it.
///
/// It watches each actor that asks, and when one ends, shuts every server and client it stood up for it.
#[derive(Bound)]
#[kind(name = "aether.http")]
pub struct Http {
    settings: HttpSettings,
    servers: BTreeMap<u16, Opened<Server>>,
    waiting: BTreeMap<u64, Waiting>,
    /// Each client standing, by its key.
    clients: BTreeMap<u64, Opened<Client>>,
    connecting: BTreeMap<u64, Connecting>,
    /// The actors watched for having asked.
    askers: Vec<u64>,
    next: u64,
}

/// It stands up on every host, and refuses to where a request it would take could not be read: the handler it was sent
/// to would never be handed it, and the engine would answer it `Unreadable` (ADR-0017), so a request within the
/// server's limits would go unserved; and where the engine's blobs could not hold a request's body, which is a blob,
/// so that request would be answered `503` (ADR-0042).
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

        cx.begin(&(), settings)
    }
}

impl ChildOf for Http {
    type Parent = Top;
}

/// Asked at its typed path, it answers a serve once `Tcp` has answered its listen.
impl Carries<Serve> for Http {
    type Answers = (Served,);
}

/// Asked at its typed path, it answers a connect once `Tcp` has answered its own.
impl Carries<Connect> for Http {
    type Answers = (Connected,);
}

impl Actor for Http {
    type Config = HttpSettings;
    type Key = ();
    type Handles = (
        Serve,
        Answer<Listening, Binding>,
        Connect,
        Answer<tcp::Connected, Reaching>,
        Answer<ConnectedUnix, ReachingUnix>,
        Ended,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(settings: HttpSettings, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self {
            settings,
            servers: BTreeMap::new(),
            waiting: BTreeMap::new(),
            clients: BTreeMap::new(),
            connecting: BTreeMap::new(),
            askers: Vec::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Serve> for Http {
    type Sends = (Asks<Path<Tcp>, Listen, Binding>, ToSender<Served>);

    fn handle(&mut self, serve: Serve, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let open = self.servers.len() + self.waiting.len();
        let is_full = open >= usize::try_from(self.settings.most_servers).unwrap_or(usize::MAX);
        if is_full {
            turn.reply(&Served::Full)?;
            return Ok(());
        }
        let held = turn.hold()?;
        let asker = self.watch_asker(turn);
        let number = self.next;
        self.next += 1;
        let listen = Listen { port: serve.port, framing: Framing::Chunks { most_bytes: self.settings.chunk_bytes } };
        turn.to(path::<Tcp>(())).ask(&listen, &Binding { number })?;
        self.waiting.insert(number, Waiting { held, handler: serve.handler, asker });

        Ok(())
    }
}

impl Handles<Answer<Listening, Binding>> for Http {
    type Sends = (ToType<Listener, Shut>,);

    fn handle(
        &mut self,
        (answered, binding): (Answered<Listening>, Binding),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Some(Waiting { held, handler, asker }) = self.waiting.remove(&binding.number) else {
            return Ok(());
        };
        let served = match answered {
            Answered::Yes(Listening::Bound { listener, port }) => {
                self.stand_up(turn, Opening { listener, port, handler, asker })?
            }
            Answered::Yes(Listening::Taken) => Served::Taken,
            Answered::Yes(Listening::Full | Listening::Refused) => Served::Refused,
            Answered::No(_) => Served::Unavailable,
        };
        turn.answer(held, &served)?;

        Ok(())
    }
}

impl Handles<Connect> for Http {
    type Sends =
        (Asks<Path<Tcp>, tcp::Connect, Reaching>, Asks<Path<Tcp>, ConnectUnix, ReachingUnix>, ToSender<Connected>);

    fn handle(&mut self, connect: Connect, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let open = self.clients.len() + self.connecting.len();
        let is_full = open >= usize::try_from(self.settings.most_clients).unwrap_or(usize::MAX);
        if is_full {
            turn.reply(&Connected::Full)?;
            return Ok(());
        }
        let framing = Framing::Chunks { most_bytes: self.settings.chunk_bytes };
        let number = self.next;
        let host = match &connect.peer {
            Peer::Tcp { host, port } => host_of(host, *port),
            // A Unix socket names no host, and a server on one reads none: this machine's name stands for it.
            Peer::Unix { .. } => Ok(String::from("localhost")),
        };
        let Ok(host) = host else {
            turn.reply(&Connected::Unreached(Unreached::Unresolved))?;
            return Ok(());
        };
        let held = turn.hold()?;
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

        Ok(())
    }
}

impl Handles<Answer<tcp::Connected, Reaching>> for Http {
    type Sends = (ToType<Dialed, Shut>,);

    fn handle(
        &mut self,
        (answered, reaching): (Answered<tcp::Connected>, Reaching),
        turn: &mut Turn<'_, Self, Self::Sends>,
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

impl Handles<Answer<ConnectedUnix, ReachingUnix>> for Http {
    type Sends = (ToType<Dialed, Shut>,);

    fn handle(
        &mut self,
        (answered, reaching): (Answered<ConnectedUnix>, ReachingUnix),
        turn: &mut Turn<'_, Self, Self::Sends>,
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

impl Handles<Ended> for Http {
    type Sends = (ToType<Server, Shut>, ToType<Client, Shut>);

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let gone = ended.actor().id().bits();
        self.servers.retain(|_, opened| opened.child.id().bits() != gone);
        self.clients.retain(|_, opened| opened.child.id().bits() != gone);
        let was_asker = self.askers.contains(&gone);
        if !was_asker {
            return Ok(());
        }
        self.askers.retain(|&asker| asker != gone);
        for opened in self.servers.values().filter(|opened| opened.asker == gone) {
            turn.to(opened.child).send(&Shut)?;
        }
        for opened in self.clients.values().filter(|opened| opened.asker == gone) {
            turn.to(opened.child).send(&Shut)?;
        }

        Ok(())
    }
}

impl Http {
    /// Stands a server up on the listener `Tcp` bound, or shuts the listener and says why not.
    fn stand_up<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, opening: Opening) -> Result<Served, Fault>
    where
        S: Allows<ToType<Listener, Shut>, I>,
    {
        let Opening { listener, port, handler, asker } = opening;
        let is_asked = self.askers.contains(&asker);
        if !is_asked {
            // Its asker has ended: no one is left to handle what the server would take.
            turn.to(listener).send::<_, I>(&Shut)?;
            return Ok(Served::Refused);
        }
        let config = ServerConfig {
            listener,
            handler,
            most_exchanges: self.settings.most_exchanges,
            limits: self.settings.limits(),
        };
        let Ok(server) = turn.spawn::<Server>(&u64::from(port), config) else {
            turn.to(listener).send::<_, I>(&Shut)?;
            return Ok(Served::Refused);
        };
        turn.watch(server);
        self.servers.insert(port, Opened { child: server, asker });

        Ok(Served::Bound { server, port })
    }

    /// Stands a client up, keyed `number`, on the connection `Tcp` dialed for `asker`, writing `host` as each request's
    /// `Host`; or shuts the connection and says why not.
    fn stand_client<S, I>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        number: u64,
        connection: ActorRef<Dialed>,
        host: String,
        asker: u64,
    ) -> Result<Connected, Fault>
    where
        S: Allows<ToType<Dialed, Shut>, I>,
    {
        let is_asked = self.askers.contains(&asker);
        if !is_asked {
            // Its asker has ended: no one is left to ask the client anything.
            turn.to(connection).send::<_, I>(&Shut)?;
            return Ok(Connected::Full);
        }
        let config = ClientConfig { connection, host, limits: self.settings.client_limits() };
        let Ok(client) = turn.spawn::<Client>(&number, config) else {
            turn.to(connection).send::<_, I>(&Shut)?;
            return Ok(Connected::Full);
        };
        turn.watch(client);
        self.clients.insert(number, Opened { child: client, asker });

        Ok(Connected::Open { client })
    }

    /// Watches the sender of the mail handled, once, and answers who it is.
    fn watch_asker<S>(&mut self, turn: &mut Turn<'_, Self, S>) -> u64 {
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
