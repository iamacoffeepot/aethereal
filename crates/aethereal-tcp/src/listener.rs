//! A listener: its bound socket, owned by an actor that accepts on it as a stream paced by its acceptor's credit,
//! polling on every `Tick` (ADR-0033).

use crate::{Accepted, BoundSocket, ConnectionConfig, Framing, Shut, Socket, SocketAddress, Tcp};
use aethereal_actor::{
    Actor, ActorRef, Beginning, ChildOf, Ended, Ending, Enough, Fault, Given, Handle, Handles, More, On, Open, Part,
    Serving, Stream, StreamKey, Tick, ToAny, ToSender, ToType, Turn, Unsent,
};
use aethereal_data::{Bound, Plain};
use std::collections::BTreeMap;
use std::io::ErrorKind;

/// A listener's stream of the connections it accepts: opened with [`Accept`], its parts each an [`Incoming`], ended
/// with why it stopped (ADR-0017). Its acceptor's credit is how many connections it accepts; until it has more, the
/// rest wait in the kernel's backlog.
pub struct Accepting;

impl Stream for Accepting {
    type Open = Accept;
    type Part = Incoming;
    type Done = Unbound;
}

/// What an acceptor opens a listener's stream with.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.accept")]
pub struct Accept;

/// A connection a listener accepted, which stands beneath it, and the address it came from.
#[derive(Debug, Bound)]
#[kind(name = "aether.tcp.incoming")]
pub struct Incoming {
    pub connection: ActorRef<Accepted>,
    pub peer: SocketAddress,
}

/// Why a listener's stream ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.unbound")]
pub enum Unbound {
    /// The listener was asked to stop, or its acceptor stopped accepting.
    Shut,
    /// The socket failed.
    Failed,
    /// The listener already has an acceptor.
    Busy,
}

/// What a listener is made from: its bound socket, how its connections frame what they read, and their limits.
#[derive(Bound)]
#[kind(name = "aether.tcp.listener_config")]
pub struct ListenerConfig {
    pub bound: Given<BoundSocket>,
    pub framing: Framing,
    pub most_connections: u32,
    pub most_unsent_bytes: u32,
}

/// Who accepts on a listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Acceptor {
    /// No one yet: the listener accepts nothing.
    Waiting,
    /// The stream `key` names.
    Accepting(StreamKey),
}

/// A listener on one port, beneath the root, keyed by its port (ADR-0033): accepts through an [`Accepting`] stream,
/// each connection an [`Accepted`] beneath it, and stops when sent [`Shut`], hanging up every connection it accepted.
#[derive(Bound)]
#[kind(name = "aether.tcp.listener")]
pub struct Listener {
    bound: Handle<BoundSocket>,
    framing: Framing,
    most_connections: u32,
    most_unsent_bytes: u32,
    accepting: Serving<Accepting>,
    acceptor: Acceptor,
    accepted: BTreeMap<u64, ActorRef<Accepted>>,
    next: u64,
}

impl ChildOf for Listener {
    type Parent = Tcp;
}

impl Actor for Listener {
    type Config = ListenerConfig;
    type Key = u64;
    type Handles = (Open<Accepting>, More, Enough, Ended, Shut, On<Tick>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: ListenerConfig, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let bound = turn.take(config.bound)?;
        turn.subscribe::<Tick>();

        Ok(Self {
            bound,
            framing: config.framing,
            most_connections: config.most_connections,
            most_unsent_bytes: config.most_unsent_bytes,
            accepting: Serving::new(),
            acceptor: Acceptor::Waiting,
            accepted: BTreeMap::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Open<Accepting>> for Listener {
    type Sends = (ToSender<Unbound>,);

    fn handle(&mut self, open: Open<Accepting>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let (key, Accept) = self.accepting.open(turn, open)?;
        if let Acceptor::Accepting(_) = self.acceptor {
            if let Some(second) = self.accepting.take(key) {
                second.finish(turn, &Unbound::Busy)?;
            }
            return Ok(());
        }
        self.acceptor = Acceptor::Accepting(key);

        Ok(())
    }
}

impl Handles<More> for Listener {
    type Sends = ();

    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.accepting.more(turn, &more);

        Ok(())
    }
}

impl Handles<Enough> for Listener {
    type Sends = ();

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        if let Some(stopped) = self.accepting.stop(turn, &enough) {
            self.acceptor = Acceptor::Waiting;
            stopped.finish(turn, &Unbound::Shut)?;
        }

        Ok(())
    }
}

impl Handles<Ended> for Listener {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let acceptor_left = !self.accepting.ended(&ended).is_empty();
        if acceptor_left {
            self.acceptor = Acceptor::Waiting;
        }
        let gone = ended.actor().id();
        self.accepted.retain(|_, connection| connection.id() != gone);

        Ok(())
    }
}

impl Handles<Shut> for Listener {
    type Sends = (ToType<Accepted, Shut>,);

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        for connection in self.accepted.values() {
            turn.to(*connection).send(&Shut)?;
        }

        self.unbind(turn, Unbound::Shut)
    }
}

impl Handles<On<Tick>> for Listener {
    type Sends = (ToAny<(Part<Accepting>,), Part<Accepting>>,);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Acceptor::Accepting(key) = self.acceptor else {
            return Ok(());
        };
        loop {
            let credit = self.accepting.get_mut(key).map_or(0, |stream| stream.credit());
            let has_room = self.accepted.len() < usize::try_from(self.most_connections).unwrap_or(usize::MAX);
            let takes = credit > 0 && has_room;
            if !takes {
                return Ok(());
            }
            let accepted = turn.open(&mut self.bound)?.0.accept();
            let (stream, peer) = match accepted {
                Ok(accepted) => accepted,
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return self.unbind(turn, Unbound::Failed),
            };
            // A stream that cannot be set not to block would stall the turn that reads it, so it is dropped.
            if stream.set_nonblocking(true).is_err() {
                continue;
            }
            let socket = turn.keep(Socket(stream))?;
            let config = ConnectionConfig {
                socket: turn.give(socket)?,
                framing: self.framing,
                most_unsent_bytes: self.most_unsent_bytes,
            };
            let number = self.next;
            self.next += 1;
            let Ok(connection) = turn.spawn::<Accepted>(&number, config) else {
                continue;
            };
            turn.watch(connection);
            self.accepted.insert(number, connection);
            let incoming = Incoming { connection, peer: SocketAddress::from(peer) };
            if let Some(stream) = self.accepting.get_mut(key) {
                // Credit was counted above, so a part is refused only when it cannot be written.
                if let Err(Unsent::Unwritable(error)) = stream.send(turn, incoming) {
                    return Err(error.into());
                }
            }
        }
    }
}

impl Listener {
    /// Ends the acceptor's stream with `why`, and closes the listener, whose socket the engine drops after its
    /// closing turn (ADR-0010).
    fn unbind<S>(&mut self, turn: &mut Turn<'_, Self, S>, why: Unbound) -> Result<(), Fault> {
        if let Acceptor::Accepting(key) = self.acceptor
            && let Some(stream) = self.accepting.take(key)
        {
            stream.finish(turn, &why)?;
        }
        self.acceptor = Acceptor::Waiting;
        turn.close();

        Ok(())
    }
}
