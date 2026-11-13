//! The TCP capability's root (ADR-0033).

use crate::dial::{Dial, DialUnix, Reached, ReachedUnix};
use crate::{
    BoundSocket, ConnectionConfig, Dialed, Framing, Listener, ListenerConfig, Shut, Socket, SocketAddress, SocketPath,
    Unaddressable,
};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Beginning, Carries, ChildOf, Ended, Ending, Fault, Given, Handles, Held,
    ToSender, ToType, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::net::{IpAddr, TcpListener};

/// What an actor asks the root to listen on: a port, 0 for any the host picks, and how the connections it accepts
/// frame what they read.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.listen")]
pub struct Listen {
    pub port: u16,
    pub framing: Framing,
}

/// What a listen came to.
#[derive(Debug, Bound)]
#[kind(name = "aether.tcp.listening")]
pub enum Listening {
    /// Bound: the listener beneath the root, and the port bound, which is its key.
    Bound { listener: ActorRef<Listener>, port: u16 },
    /// A listener of this engine holds the port, or another process does.
    Taken,
    /// The root holds its most listeners.
    Full,
    /// The host refused the bind.
    Refused,
}

/// What an actor asks the root to dial: a host, a name or an address written out, a port, and how the connection
/// frames what it reads.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.connect")]
pub struct Connect {
    pub host: String,
    pub port: u16,
    pub framing: Framing,
}

/// What a connect came to.
#[derive(Debug, Bound)]
#[kind(name = "aether.tcp.connected")]
pub enum Connected {
    /// Connected: the connection beneath the root, and the address it reached.
    Open { connection: ActorRef<Dialed>, peer: SocketAddress },
    /// The name found no address.
    Unresolved,
    /// Every address refused, or none answered in time.
    Refused,
    /// The root holds its most connections.
    Full,
    /// The host serves no dialer.
    Unavailable,
}

/// What an actor asks the root to dial on this machine: the Unix socket at `path`, and how the connection frames what
/// it reads. The connection is a [`Dialed`], read and written as one dialed over TCP is.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.connect_unix")]
pub struct ConnectUnix {
    pub path: SocketPath,
    pub framing: Framing,
}

/// What a connect to a Unix socket came to.
#[derive(Debug, Bound)]
#[kind(name = "aether.tcp.connected_unix")]
pub enum ConnectedUnix {
    /// Connected: the connection beneath the root.
    Open { connection: ActorRef<Dialed> },
    /// The path can name no socket, so nothing was dialed.
    Unaddressable(Unaddressable),
    /// No socket is at the path.
    Missing,
    /// Nothing listens on the socket, or the host refused the connect.
    Refused,
    /// The root holds its most connections.
    Full,
    /// The host serves no dialer, or has no Unix sockets.
    Unavailable,
}

/// What the root is made from: where listeners bind, and its limits (ADR-0033).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.config")]
pub struct TcpConfig {
    pub interface: String,
    pub most_listeners: u32,
    pub most_connections: u32,
    pub most_unsent_bytes: u32,
}

/// What the root keeps for a dial it runs: which connect it was run for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.dialing")]
pub struct Dialing {
    number: u64,
}

/// What the root keeps for a dial to a Unix socket it runs: which connect it was run for.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.dialing_unix")]
pub struct DialingUnix {
    number: u64,
}

/// A connect the root holds until its dial is answered, and answers with an `A`.
#[derive(Bound)]
struct Waiting<A> {
    held: Held<A>,
    framing: Framing,
    asker: u64,
}

/// What became of a socket a dial made, as the root stands a connection up on it.
enum Opening {
    /// The connection stands beneath the root.
    Open(ActorRef<Dialed>),
    /// Its asker has ended: the socket is not taken, and the engine drops it.
    Unasked,
    /// The connection could not be spawned.
    Refused,
}

/// A listener or a connection the root opened, and the actor that asked for it.
#[derive(Bound)]
struct Opened<C> {
    child: ActorRef<C>,
    asker: u64,
}

/// The TCP capability: listens and dials for every other actor (ADR-0033). An actor asks it a [`Listen`], a
/// [`Connect`], or a [`ConnectUnix`] at its typed path, `path::<Tcp>(())`.
///
/// It watches each actor that asks, and when one ends, shuts every listener
/// and connection it opened for it.
#[derive(Bound)]
#[kind(name = "aether.tcp")]
pub struct Tcp {
    config: TcpConfig,
    listeners: BTreeMap<u16, Opened<Listener>>,
    dialed: BTreeMap<u64, Opened<Dialed>>,
    waiting: BTreeMap<u64, Waiting<Connected>>,
    waiting_unix: BTreeMap<u64, Waiting<ConnectedUnix>>,
    /// The actors watched for having asked.
    askers: Vec<u64>,
    next: u64,
}

impl ChildOf for Tcp {
    type Parent = Top;
}

/// Asked at its typed path, it answers a listen with what became of it (ADR-0022).
impl Carries<Listen> for Tcp {
    type Answers = (Listening,);
}

/// Asked at its typed path, it answers a connect once its dial is answered.
impl Carries<Connect> for Tcp {
    type Answers = (Connected,);
}

/// Asked at its typed path, it answers a connect to a Unix socket once its dial is answered, or at once when the path
/// can name no socket.
impl Carries<ConnectUnix> for Tcp {
    type Answers = (ConnectedUnix,);
}

impl Actor for Tcp {
    type Config = TcpConfig;
    type Key = ();
    type Handles = (Listen, Connect, ConnectUnix, Answer<Reached, Dialing>, Answer<ReachedUnix, DialingUnix>, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: TcpConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self {
            config,
            listeners: BTreeMap::new(),
            dialed: BTreeMap::new(),
            waiting: BTreeMap::new(),
            waiting_unix: BTreeMap::new(),
            askers: Vec::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Listen> for Tcp {
    type Sends = (ToSender<Listening>,);

    fn handle(&mut self, listen: Listen, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let listening = self.listen(turn, &listen)?;
        turn.reply(&listening)?;

        Ok(())
    }
}

impl Handles<Connect> for Tcp {
    type Sends = (ToSender<Connected>,);

    fn handle(&mut self, connect: Connect, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if self.is_full() {
            turn.reply(&Connected::Full)?;
            return Ok(());
        }
        let held = turn.hold()?;
        let asker = self.watch_asker(turn);
        let number = self.number();
        turn.run(&Dial::new(connect.host, connect.port), &Dialing { number })?;
        self.waiting.insert(number, Waiting { held, framing: connect.framing, asker });

        Ok(())
    }
}

impl Handles<ConnectUnix> for Tcp {
    type Sends = (ToSender<ConnectedUnix>,);

    fn handle(&mut self, connect: ConnectUnix, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let path = match connect.path.checked() {
            Ok(path) => String::from(path),
            Err(why) => {
                turn.reply(&ConnectedUnix::Unaddressable(why))?;
                return Ok(());
            }
        };
        if self.is_full() {
            turn.reply(&ConnectedUnix::Full)?;
            return Ok(());
        }
        let held = turn.hold()?;
        let asker = self.watch_asker(turn);
        let number = self.number();
        turn.run(&DialUnix::new(path), &DialingUnix { number })?;
        self.waiting_unix.insert(number, Waiting { held, framing: connect.framing, asker });

        Ok(())
    }
}

impl Handles<Answer<Reached, Dialing>> for Tcp {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, dialing): (Answered<Reached>, Dialing),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(Waiting { held, framing, asker }) = self.waiting.remove(&dialing.number) else {
            return Ok(());
        };
        let connected = match answered {
            Answered::Yes(Reached::Open { socket, peer }) => {
                match self.open(turn, dialing.number, socket, framing, asker)? {
                    Opening::Open(connection) => Connected::Open { connection, peer },
                    Opening::Unasked => return Ok(()),
                    Opening::Refused => Connected::Refused,
                }
            }
            Answered::Yes(Reached::Unresolved) => Connected::Unresolved,
            Answered::Yes(Reached::Refused) | Answered::No(_) => Connected::Refused,
            Answered::Yes(Reached::Unavailable) => Connected::Unavailable,
        };
        turn.answer(held, &connected)?;

        Ok(())
    }
}

impl Handles<Answer<ReachedUnix, DialingUnix>> for Tcp {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, dialing): (Answered<ReachedUnix>, DialingUnix),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(Waiting { held, framing, asker }) = self.waiting_unix.remove(&dialing.number) else {
            return Ok(());
        };
        let connected = match answered {
            Answered::Yes(ReachedUnix::Open { socket }) => {
                match self.open(turn, dialing.number, socket, framing, asker)? {
                    Opening::Open(connection) => ConnectedUnix::Open { connection },
                    Opening::Unasked => return Ok(()),
                    Opening::Refused => ConnectedUnix::Refused,
                }
            }
            Answered::Yes(ReachedUnix::Missing) => ConnectedUnix::Missing,
            Answered::Yes(ReachedUnix::Refused) | Answered::No(_) => ConnectedUnix::Refused,
            Answered::Yes(ReachedUnix::Unavailable) => ConnectedUnix::Unavailable,
        };
        turn.answer(held, &connected)?;

        Ok(())
    }
}

impl Handles<Ended> for Tcp {
    type Sends = (ToType<Listener, Shut>, ToType<Dialed, Shut>);

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let gone = ended.actor().id().bits();
        self.listeners.retain(|_, opened| opened.child.id().bits() != gone);
        self.dialed.retain(|_, opened| opened.child.id().bits() != gone);
        let was_asker = self.askers.contains(&gone);
        if !was_asker {
            return Ok(());
        }
        self.askers.retain(|&asker| asker != gone);
        for opened in self.listeners.values().filter(|opened| opened.asker == gone) {
            turn.to(opened.child).send(&Shut)?;
        }
        for opened in self.dialed.values().filter(|opened| opened.asker == gone) {
            turn.to(opened.child).send(&Shut)?;
        }

        Ok(())
    }
}

impl Tcp {
    /// Binds `listen`'s port on the interface the root's settings name and stands a listener up on it, or says why
    /// not.
    fn listen<S>(&mut self, turn: &mut Turn<'_, Self, S>, listen: &Listen) -> Result<Listening, Fault> {
        let is_full = self.listeners.len() >= usize::try_from(self.config.most_listeners).unwrap_or(usize::MAX);
        if is_full {
            return Ok(Listening::Full);
        }
        let is_held = self.listeners.contains_key(&listen.port);
        if is_held {
            return Ok(Listening::Taken);
        }
        let Ok(interface) = self.config.interface.parse::<IpAddr>() else {
            return Ok(Listening::Refused);
        };
        let bound = match TcpListener::bind((interface, listen.port)) {
            Ok(bound) => bound,
            Err(error) if error.kind() == ErrorKind::AddrInUse => return Ok(Listening::Taken),
            Err(_) => return Ok(Listening::Refused),
        };
        let port = bound.local_addr().map(|address| address.port());
        let nonblocking = bound.set_nonblocking(true);
        let (Ok(port), Ok(())) = (port, nonblocking) else {
            return Ok(Listening::Refused);
        };
        let bound = turn.keep(BoundSocket(bound))?;
        let config = ListenerConfig {
            bound: turn.give(bound)?,
            framing: listen.framing,
            most_connections: self.config.most_connections,
            most_unsent_bytes: self.config.most_unsent_bytes,
        };
        let Ok(listener) = turn.spawn::<Listener>(&u64::from(port), config) else {
            return Ok(Listening::Refused);
        };
        turn.watch(listener);
        let asker = self.watch_asker(turn);
        self.listeners.insert(port, Opened { child: listener, asker });

        Ok(Listening::Bound { listener, port })
    }

    /// Whether the root holds its most connections: those dialed, and those it is dialing.
    fn is_full(&self) -> bool {
        let open = self.dialed.len() + self.waiting.len() + self.waiting_unix.len();

        open >= usize::try_from(self.config.most_connections).unwrap_or(usize::MAX)
    }

    /// The number of the next dial, which keys the connection it makes.
    fn number(&mut self) -> u64 {
        let number = self.next;
        self.next += 1;

        number
    }

    /// Stands a connection up beneath the root on the socket a dial made, keyed `number`, for `asker`: unless `asker`
    /// has ended in the meantime, when the socket is not taken.
    fn open<S>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        number: u64,
        socket: Given<Socket>,
        framing: Framing,
        asker: u64,
    ) -> Result<Opening, Fault> {
        let is_asked = self.askers.contains(&asker);
        if !is_asked {
            return Ok(Opening::Unasked);
        }
        let socket = turn.take(socket)?;
        let config =
            ConnectionConfig { socket: turn.give(socket)?, framing, most_unsent_bytes: self.config.most_unsent_bytes };
        let Ok(connection) = turn.spawn::<Dialed>(&number, config) else {
            return Ok(Opening::Refused);
        };
        turn.watch(connection);
        self.dialed.insert(number, Opened { child: connection, asker });

        Ok(Opening::Open(connection))
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
