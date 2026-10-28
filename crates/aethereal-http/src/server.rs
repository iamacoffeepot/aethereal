//! A server: the listener `Tcp` bound for it, an exchange for each connection it accepts, and the stream of parsed
//! requests its handler takes, paced by the handler's credit (ADR-0035).

use crate::{Exchange, ExchangeConfig, Http, Limits, Request};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, ChildOf, Ended, Ending, Enough, Fault, Handles, More,
    Number, On, Open, Part, Serving, Stream, StreamKey, Streamed, Streams, Tick, ToAny, ToSender, ToType, Turn, Unsent,
};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{Accept, Accepted, Accepting, Listener, Shut, Unbound};
use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroU32;

/// A server's stream of the requests its exchanges parsed: opened with [`Take`], its parts each an [`Incoming`],
/// ended with why it stopped (ADR-0017). Its handler's credit is how many requests it takes at once; until it has
/// more, the server keeps the rest in the order they were parsed.
pub struct Requests;

impl Stream for Requests {
    type Open = Take;
    type Part = Incoming;
    type Done = Stopped;
}

/// What a handler opens a server's stream with.
#[derive(Debug, Plain)]
#[kind(name = "aether.http.take")]
pub struct Take;

/// A request, and the exchange it came on, to which the handler sends its [`Respond`](crate::Respond).
#[derive(Debug, Bound)]
#[kind(name = "aether.http.incoming")]
pub struct Incoming {
    pub request: Request,
    pub exchange: ActorRef<Exchange>,
}

/// Why a server's stream ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stopped")]
pub enum Stopped {
    /// The server was sent `Shut`, or its handler stopped taking.
    Shut,
    /// `Tcp`'s stream of the connections it accepts ended.
    Unbound,
    /// The server already has a handler.
    Busy,
}

/// What an exchange sends its server: the request it parsed, to send on to the handler.
#[derive(Debug, Plain)]
#[kind(name = "aether.http.parsed")]
pub struct Parsed {
    pub(crate) request: Request,
}

/// What a server is made from: the listener `Tcp` bound for it, and its limits.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.server_config")]
pub struct ServerConfig {
    pub listener: ActorRef<Listener>,
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

/// Who takes a server's requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Handler {
    /// No one yet: requests wait.
    Waiting,
    /// The stream `key` names.
    Taking(StreamKey),
}

/// A server on one port, beneath the root, keyed by its port (ADR-0035): an [`Exchange`] beneath it for each
/// connection its listener accepts, the requests they parse sent on to its handler through a [`Requests`] stream. It
/// stops when sent [`Shut`], when its handler ends, or when its listener stops, shutting its listener and every
/// exchange.
#[derive(Bound)]
#[kind(name = "aether.http.server")]
pub struct Server {
    listener: ActorRef<Listener>,
    most_exchanges: u32,
    limits: Limits,
    start: Start,
    accepting: Streams<Accepting, ()>,
    requests: Serving<Requests>,
    handler: Handler,
    /// Each exchange standing, by the bits of its id.
    exchanges: BTreeMap<u64, ActorRef<Exchange>>,
    /// The requests parsed and not yet sent to the handler, in the order they were parsed.
    queued: VecDeque<Incoming>,
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
    type Handles =
        (Open<Requests>, More, Enough, Ended, Shut, Part<Accepting>, Answer<Unbound, Number>, Parsed, On<Tick>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: ServerConfig, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();

        Ok(Self {
            listener: config.listener,
            most_exchanges: config.most_exchanges,
            limits: config.limits,
            start: Start::Starting,
            accepting: Streams::new(ACCEPTING_WINDOW),
            requests: Serving::new(),
            handler: Handler::Waiting,
            exchanges: BTreeMap::new(),
            queued: VecDeque::new(),
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

impl Handles<Open<Requests>> for Server {
    type Sends = (ToSender<Stopped>, ToAny<(Part<Requests>,), Part<Requests>>);

    fn handle(&mut self, open: Open<Requests>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let (key, Take) = self.requests.open(turn, open)?;
        if let Handler::Taking(_) = self.handler {
            if let Some(second) = self.requests.take(key) {
                second.finish(turn, &Stopped::Busy)?;
            }
            return Ok(());
        }
        self.handler = Handler::Taking(key);

        self.send_queued(turn)
    }
}

impl Handles<More> for Server {
    type Sends = (ToAny<(Part<Requests>,), Part<Requests>>,);

    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.requests.more(turn, &more);

        self.send_queued(turn)
    }
}

impl Handles<Enough> for Server {
    type Sends = (ToType<Listener, Shut>, ToType<Exchange, Shut>);

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(stopped) = self.requests.stop(turn, &enough) else {
            return Ok(());
        };
        self.handler = Handler::Waiting;
        stopped.finish(turn, &Stopped::Shut)?;

        self.shut(turn, Stopped::Shut)
    }
}

impl Handles<Ended> for Server {
    type Sends = (ToType<Listener, Shut>, ToType<Exchange, Shut>);

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let handler_left = !self.requests.ended(&ended).is_empty();
        if handler_left {
            // No one is left to answer what the server takes.
            self.handler = Handler::Waiting;
            return self.shut(turn, Stopped::Shut);
        }
        let gone = ended.actor().id();
        self.exchanges.remove(&gone.bits());
        self.queued.retain(|incoming| incoming.exchange.id() != gone);

        Ok(())
    }
}

impl Handles<Shut> for Server {
    type Sends = (ToType<Listener, Shut>, ToType<Exchange, Shut>);

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.shut(turn, Stopped::Shut)
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
        let config = ExchangeConfig { connection: incoming.connection, peer: incoming.peer, limits: self.limits };
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

        self.shut(turn, Stopped::Unbound)
    }
}

impl Handles<Parsed> for Server {
    type Sends = (ToAny<(Part<Requests>,), Part<Requests>>,);

    fn handle(&mut self, parsed: Parsed, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(exchange) = self.exchanges.get(&turn.sender().id().bits()) else {
            return Ok(());
        };
        self.queued.push_back(Incoming { request: parsed.request, exchange: *exchange });

        self.send_queued(turn)
    }
}

impl Server {
    /// Sends the handler as many queued requests as its credit allows, in the order they were parsed.
    fn send_queued<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToAny<(Part<Requests>,), Part<Requests>>, I>,
    {
        let Handler::Taking(key) = self.handler else {
            return Ok(());
        };
        while let Some(incoming) = self.queued.pop_front() {
            let Some(stream) = self.requests.get_mut(key) else {
                self.queued.push_front(incoming);
                return Ok(());
            };
            match stream.send::<_, _, I>(turn, incoming) {
                Ok(()) => {}
                Err(Unsent::NoCredit(incoming)) => {
                    self.queued.push_front(incoming);
                    return Ok(());
                }
                Err(Unsent::Unwritable(error)) => return Err(error.into()),
            }
        }

        Ok(())
    }

    /// Shuts the listener and every exchange, ends the handler's stream with `why`, and closes the server.
    fn shut<S, IL, IE>(&mut self, turn: &mut Turn<'_, Self, S>, why: Stopped) -> Result<(), Fault>
    where
        S: Allows<ToType<Listener, Shut>, IL> + Allows<ToType<Exchange, Shut>, IE>,
    {
        turn.to(self.listener).send::<_, IL>(&Shut)?;
        for exchange in self.exchanges.values() {
            turn.to(*exchange).send::<_, IE>(&Shut)?;
        }
        if let Handler::Taking(key) = self.handler
            && let Some(stream) = self.requests.take(key)
        {
            stream.finish(turn, &why)?;
        }
        self.handler = Handler::Waiting;
        turn.close();

        Ok(())
    }
}
