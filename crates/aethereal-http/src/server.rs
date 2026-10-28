//! A server: the listener `Tcp` bound for it, and an exchange for each connection it accepts, each asking the
//! server's handler its requests (ADR-0035).

use crate::{Exchange, ExchangeConfig, Handling, Http, Limits};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, ChildOf, Ended, Ending, Fault, Handles, More, Number,
    On, Open, Part, ProtocolRef, Streamed, Streams, Tick, ToAny, ToType, Turn,
};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{Accept, Accepted, Accepting, Listener, Shut, Unbound};
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// What a server is made from: the listener `Tcp` bound for it, the handler its exchanges ask, and its limits.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.server_config")]
pub struct ServerConfig {
    pub listener: ActorRef<Listener>,
    pub handler: ProtocolRef<Handling>,
    pub most_exchanges: u32,
    pub limits: Limits,
}

/// Whether a server has opened its listener's stream: it does on its first `Tick`, since a stream is opened in a
/// turn and not in a beginning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Start {
    Starting,
    Accepting,
}

/// A server on one port, beneath the root, keyed by its port (ADR-0035): an [`Exchange`] beneath it for each
/// connection its listener accepts, each asking the handler its requests. It stops when sent [`Shut`], when its
/// handler ends, or when its listener stops, shutting its listener and every exchange.
#[derive(Bound)]
#[kind(name = "aether.http.server")]
pub struct Server {
    listener: ActorRef<Listener>,
    handler: ProtocolRef<Handling>,
    most_exchanges: u32,
    limits: Limits,
    start: Start,
    accepting: Streams<Accepting, ()>,
    /// Each exchange standing, by the bits of its id.
    exchanges: BTreeMap<u64, ActorRef<Exchange>>,
    next: u64,
}

impl ChildOf for Server {
    type Parent = Http;
}

/// The connections a server's listener may accept ahead of the server spawning their exchanges: four.
const ACCEPTING_WINDOW: NonZeroU32 = NonZeroU32::MIN.saturating_add(3);

impl Actor for Server {
    type Config = ServerConfig;
    type Key = u64;
    type Handles = (Ended, Shut, Part<Accepting>, Answer<Unbound, Number>, On<Tick>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: ServerConfig, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();
        turn.watch(config.handler);

        Ok(Self {
            listener: config.listener,
            handler: config.handler,
            most_exchanges: config.most_exchanges,
            limits: config.limits,
            start: Start::Starting,
            accepting: Streams::new(ACCEPTING_WINDOW),
            exchanges: BTreeMap::new(),
            next: 0,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Tick>> for Server {
    type Sends = (Asks<Streamed<Accepting>, Open<Accepting>, Number>, ToAny<Streamed<Accepting>, More>);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if self.start == Start::Starting {
            self.accepting.open(turn, self.listener.protocol(), Accept, ())?;
            self.start = Start::Accepting;
        }
        turn.unsubscribe::<Tick>();

        Ok(())
    }
}

impl Handles<Ended> for Server {
    type Sends = (ToType<Listener, Shut>, ToType<Exchange, Shut>);

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let gone = ended.actor().id();
        let handler_left = gone == self.handler.id();
        if handler_left {
            // No one is left to answer what the exchanges ask.
            return self.shut(turn);
        }
        self.exchanges.remove(&gone.bits());

        Ok(())
    }
}

impl Handles<Shut> for Server {
    type Sends = (ToType<Listener, Shut>, ToType<Exchange, Shut>);

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.shut(turn)
    }
}

impl Handles<Part<Accepting>> for Server {
    type Sends = (ToAny<Streamed<Accepting>, More>, ToType<Accepted, Shut>);

    fn handle(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(taken) = self.accepting.part(turn, part)? else {
            return Ok(());
        };
        let incoming = taken.value;
        let is_full = self.exchanges.len() >= usize::try_from(self.most_exchanges).unwrap_or(usize::MAX);
        if is_full {
            turn.to(incoming.connection).send(&Shut)?;
            return Ok(());
        }
        let config = ExchangeConfig {
            connection: incoming.connection,
            peer: incoming.peer,
            handler: self.handler,
            limits: self.limits,
        };
        let number = self.next;
        self.next += 1;
        let Ok(exchange) = turn.spawn::<Exchange>(&number, config) else {
            turn.to(incoming.connection).send(&Shut)?;
            return Ok(());
        };
        turn.watch(exchange);
        self.exchanges.insert(exchange.id().bits(), exchange);

        Ok(())
    }
}

impl Handles<Answer<Unbound, Number>> for Server {
    type Sends = (ToType<Listener, Shut>, ToType<Exchange, Shut>);

    fn handle(
        &mut self,
        (_, number): (Answered<Unbound>, Number),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.accepting.answered(number);

        self.shut(turn)
    }
}

impl Server {
    /// Shuts the listener and every exchange, and closes the server.
    fn shut<S, IL, IE>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToType<Listener, Shut>, IL> + Allows<ToType<Exchange, Shut>, IE>,
    {
        turn.to(self.listener).send::<_, IL>(&Shut)?;
        for exchange in self.exchanges.values() {
            turn.to(*exchange).send::<_, IE>(&Shut)?;
        }
        turn.close();

        Ok(())
    }
}
