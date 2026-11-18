//! A listener: its bound socket, owned by an actor that accepts on it as a stream paced by its acceptor's credit, in
//! the handlers that make accepting possible, waiting for the socket through the host's poller when it would wait,
//! and backing off while the host is out of descriptors or memory (ADR-0033).

mod failure;

use crate::ready::Ready;
use crate::{Accepted, BoundSocket, ConnectionConfig, Framing, Interest, Readied, Shut, Socket, SocketAddress, Tcp};
use aethereal_actor::{
    ActorRef, Answered, Beginning, Ended, Enough, Fault, Given, Handle, JobId, Later, More, Open, Serving, Sleep,
    Slept, Spawn, Stream, StreamKey, Turn, Unsent, actor,
};
use aethereal_data::{Bound, Plain};
use core::time::Duration;
use failure::Failure;
use std::collections::BTreeMap;

/// How long a listener stops accepting once an accept finds the host out of descriptors or memory, in milliseconds:
/// long enough that it does not spin on accepts that fail, short enough that a connection freed is soon taken up.
const BACK_OFF_MILLIS: u64 = 100;

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
    /// The socket failed: an accept failed with an error that says the listening socket itself is broken, or the host
    /// has no poller to wait for it with.
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

/// What a listener waits for before it accepts again, besides its acceptor's credit and room for a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
enum Waiting {
    /// Nothing: an accept it makes now does not wait.
    Not,
    /// Its socket to have a connection to accept: the [`Ready`] it ran, not yet answered.
    Readable(JobId),
    /// The end of a back-off, begun when an accept found the host out of descriptors or memory: the [`Sleep`] it ran,
    /// not yet answered.
    BackingOff(JobId),
}

/// What a listener keeps for the [`Sleep`] of its back-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.backing_off")]
pub struct BackingOff;

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
    waiting: Waiting,
}

#[actor(key = u64, parent = Tcp, spawns(Accepted))]
impl Listener {
    fn begin(config: ListenerConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let bound = turn.take(config.bound)?;

        Ok(Self {
            bound,
            framing: config.framing,
            most_connections: config.most_connections,
            most_unsent_bytes: config.most_unsent_bytes,
            accepting: Serving::new(),
            acceptor: Acceptor::Waiting,
            accepted: BTreeMap::new(),
            next: 0,
            waiting: Waiting::Not,
        })
    }

    #[request]
    fn open(&mut self, open: Open<Accepting>, turn: &mut Turn<'_, Self>) -> Result<Later<Unbound>, Fault> {
        let (key, Accept, later) = self.accepting.open(turn, open)?;
        if let Acceptor::Accepting(_) = self.acceptor {
            if let Some(second) = self.accepting.take(key) {
                second.finish(turn, &Unbound::Busy)?;
            }
            return Ok(later);
        }
        self.acceptor = Acceptor::Accepting(key);

        Ok(later)
    }

    #[tell]
    fn more(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.accepting.more(turn, &more);

        self.accept(turn)
    }

    #[tell]
    fn enough(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if let Some(stopped) = self.accepting.stop(turn, &enough) {
            self.acceptor = Acceptor::Waiting;
            stopped.finish(turn, &Unbound::Shut)?;
        }

        Ok(())
    }

    /// An acceptor that ended accepts no more; a connection that ended leaves room for another, which it accepts.
    #[ended]
    fn ended(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let acceptor_left = !self.accepting.ended(&ended).is_empty();
        if acceptor_left {
            self.acceptor = Acceptor::Waiting;
        }
        let gone = ended.actor().id();
        self.accepted.retain(|_, connection| connection.id() != gone);

        self.accept(turn)
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        for connection in self.accepted.values() {
            turn.to(*connection).send(&Shut)?;
        }

        self.unbind(turn, Unbound::Shut)
    }

    /// Its socket has a connection to accept, or may have; or the host has no poller, and it can never wait for one,
    /// so it stops listening.
    #[answer]
    fn readied(&mut self, (readied, _): (Answered<Readied>, Interest), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.waiting = Waiting::Not;
        let is_ready = readied == Answered::Yes(Readied::Ready);
        if !is_ready {
            return self.unbind(turn, Unbound::Failed);
        }

        self.accept(turn)
    }

    /// Its back-off has ended: it accepts again.
    #[answer]
    fn slept(&mut self, _: (Answered<Slept>, BackingOff), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.waiting = Waiting::Not;

        self.accept(turn)
    }
}

impl Listener {
    /// Accepts while its acceptor has credit and it has room for another connection, each one spawned beneath it and
    /// sent to its acceptor. An accept that would wait waits for the socket to be readable; one stopped for want of
    /// credit or room waits for the acceptor's `More`, or for a connection to end. An accept that fails is met as its
    /// [`Failure`] says: the listener accepts the next connection, backs off, or unbinds.
    fn accept(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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
            let accepted = turn.open(&mut self.bound)?.0.accept().map_err(|error| Failure::of(&error));
            let (stream, peer) = match accepted {
                Ok(accepted) => accepted,
                Err(Failure::Drained) => return self.wait(turn),
                Err(Failure::Next) => continue,
                Err(Failure::BackOff) => return self.back_off(turn),
                Err(Failure::Broken) => return self.unbind(turn, Unbound::Failed),
            };
            // A stream that cannot be set not to block would stall the turn that reads it, so it is dropped.
            if stream.set_nonblocking(true).is_err() {
                continue;
            }
            let socket = turn.keep(Socket::Tcp(stream))?;
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

    /// Runs a [`Ready`] for its socket to have a connection to accept, unless one is in flight already. A back-off it
    /// was in ends: the accept that brought it here no longer failed for want of descriptors or memory.
    fn wait(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if let Waiting::Readable(_) = self.waiting {
            return Ok(());
        }
        self.stop_waiting(turn);
        let descriptor = turn.open(&mut self.bound)?.descriptor();
        let job = turn.run(&Ready::new(descriptor, Interest::Readable), &Interest::Readable)?;
        self.waiting = Waiting::Readable(job);

        Ok(())
    }

    /// Stops accepting until a [`Sleep`] of [`BACK_OFF_MILLIS`] ends, as the host is out of descriptors or memory and
    /// an accept made sooner fails the same way. A wait for the socket in flight is cancelled, as a readable socket
    /// would only bring that accept sooner; a `More`, or a connection of its own that ends, still tries one.
    fn back_off(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.stop_waiting(turn);
        let until = turn.now() + Duration::from_millis(BACK_OFF_MILLIS);
        let job = turn.run(&Sleep { until }, &BackingOff)?;
        self.waiting = Waiting::BackingOff(job);

        Ok(())
    }

    /// Cancels the job it waits on, whose answer then never comes (ADR-0018).
    fn stop_waiting(&mut self, turn: &mut Turn<'_, Self>) {
        if let Waiting::Readable(job) | Waiting::BackingOff(job) = self.waiting {
            turn.cancel(job);
        }
        self.waiting = Waiting::Not;
    }

    /// Ends the acceptor's stream with `why`, stops its wait for the socket or its back-off, and closes the listener,
    /// whose socket the engine drops after its closing turn (ADR-0010).
    fn unbind(&mut self, turn: &mut Turn<'_, Self>, why: Unbound) -> Result<(), Fault> {
        if let Acceptor::Accepting(key) = self.acceptor
            && let Some(stream) = self.accepting.take(key)
        {
            stream.finish(turn, &why)?;
        }
        self.acceptor = Acceptor::Waiting;
        self.stop_waiting(turn);
        turn.close();

        Ok(())
    }
}
