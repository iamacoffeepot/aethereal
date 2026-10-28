//! An exchange: the server's side of one accepted connection, which parses each request as its chunks arrive, asks
//! it of the server's handler, and writes the response the handler answers with (ADR-0035).

mod parse;
mod write;

use crate::{Handling, Limits, Method, Request, Response, Server, Status};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, More, Number,
    On, Open, Part, ProtocolRef, Streamed, Streams, Taken, Tick, Time, ToAny, Turn, Unanswered,
};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{Accepted, HungUp, Read, Reading, Shut, SocketAddress, Write, Writing, Wrote};
use core::mem;
use core::time::Duration;
use parse::{Head, Parsed as Front, Refusal};
use std::num::NonZeroU32;

/// What an exchange is made from: the connection it reads, where it came from, the handler it asks each request
/// of, and the limits on each request.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.exchange_config")]
pub struct ExchangeConfig {
    pub connection: ActorRef<Accepted>,
    pub peer: SocketAddress,
    pub handler: ProtocolRef<Handling>,
    pub limits: Limits,
}

/// What an exchange keeps for a request it asked of the handler: nothing, since it has one out at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.asking")]
pub struct Asking;

/// What an exchange keeps for a write: whether the connection closes once it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.sending")]
pub struct Sending {
    closes: bool,
}

/// Whether an exchange reads its connection: it opens the stream on its first `Tick`, since a stream is opened in a
/// turn and not in a beginning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Reader {
    Unopened,
    Reading(Number),
}

/// Where an exchange is in its connection's requests.
#[derive(Debug, Plain)]
enum Phase {
    /// Between requests: nothing of the next has come since `since`.
    Idle { since: Time },
    /// Part of a head has come, its first byte at `since`.
    Head { since: Time },
    /// A head is parsed, and its body is coming; the request's first byte came at `since`.
    Body { head: Head, body: Vec<u8>, since: Time },
    /// The request is asked of the handler, and its answer is waited for since `since`. The connection then closes when it
    /// `closes`, and a response to a `HEAD` is written with no body.
    Answering { since: Time, closes: bool, head_only: bool },
    /// A last response is written: the exchange closes once it is sent.
    Closing,
}

/// The server's side of one accepted connection, beneath the server, keyed by its count (ADR-0035): reads it through
/// a [`Reading`] stream, parses each request, asks it of the handler, and writes the [`Response`] answered. It
/// takes one request at a time: while one is out it grants its connection no credit, so requests sent ahead wait in
/// the kernel.
#[derive(Bound)]
#[kind(name = "aether.http.exchange")]
pub struct Exchange {
    connection: ActorRef<Accepted>,
    peer: SocketAddress,
    handler: ProtocolRef<Handling>,
    limits: Limits,
    reading: Streams<Reading, ()>,
    reader: Reader,
    /// What has been read and not yet parsed.
    buffer: Vec<u8>,
    /// The chunks taken while a request was out, whose credit is granted once it is answered.
    withheld: u32,
    phase: Phase,
}

impl ChildOf for Exchange {
    type Parent = Server;
}

impl Actor for Exchange {
    type Config = ExchangeConfig;
    type Key = u64;
    type Handles =
        (Part<Reading>, Answer<HungUp, Number>, Answer<Wrote, Sending>, Answer<Response, Asking>, Shut, On<Tick>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: ExchangeConfig, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();
        let window = NonZeroU32::new(config.limits.window).unwrap_or(NonZeroU32::MIN);

        Ok(Self {
            connection: config.connection,
            peer: config.peer,
            handler: config.handler,
            limits: config.limits,
            reading: Streams::new(window),
            reader: Reader::Unopened,
            buffer: Vec::new(),
            withheld: 0,
            phase: Phase::Idle { since: turn.now() },
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Part<Reading>> for Exchange {
    type Sends = (ToAny<Streamed<Reading>, More>, Asks<Writing, Write, Sending>, Asks<Handling, Request, Asking>);

    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let is_out = matches!(self.phase, Phase::Answering { .. } | Phase::Closing);
        let taken = if is_out {
            self.reading.part_held(turn, part)
        } else {
            self.reading.part(turn, part)?
        };
        let Some(Taken { value, .. }) = taken else {
            return Ok(());
        };
        if is_out {
            self.withheld += 1;
        }
        self.buffer.extend_from_slice(&value.bytes);

        self.advance(turn)
    }
}

impl Handles<Answer<HungUp, Number>> for Exchange {
    type Sends = ();

    fn handle(&mut self, (_, number): (Answered<HungUp>, Number), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.reading.answered(number);
        turn.close();

        Ok(())
    }
}

impl Handles<Answer<Wrote, Sending>> for Exchange {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, sending): (Answered<Wrote>, Sending),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let is_sent = answered == Answered::Yes(Wrote::Sent);
        let stays = is_sent && !sending.closes;
        if !stays {
            turn.close();
        }

        Ok(())
    }
}

impl Handles<Answer<Response, Asking>> for Exchange {
    type Sends = (ToAny<Streamed<Reading>, More>, Asks<Writing, Write, Sending>, Asks<Handling, Request, Asking>);

    fn handle(
        &mut self,
        (answered, Asking): (Answered<Response>, Asking),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Phase::Answering { closes, head_only, .. } = self.phase else {
            // Answered after its deadline, which the exchange already answered `504`.
            turn.log(Level::Info, "an answer came after its request's deadline, and was passed over");
            return Ok(());
        };
        let response = match answered {
            Answered::Yes(response) => response,
            Answered::No(Unanswered::Ended) => unanswered(Status::ServiceUnavailable, "the handler has ended"),
            Answered::No(why) => {
                turn.log(Level::Warn, format!("the handler did not answer a request: {why:?}"));
                unanswered(Status::InternalServerError, "the handler did not answer")
            }
        };
        self.respond(turn, response, closes, head_only)?;
        if closes {
            return Ok(());
        }
        self.phase = Phase::Idle { since: turn.now() };
        if let (Reader::Reading(number), Some(withheld)) = (self.reader, NonZeroU32::new(self.withheld)) {
            self.reading.grant(turn, number, withheld)?;
        }
        self.withheld = 0;

        self.advance(turn)
    }
}

impl Handles<Shut> for Exchange {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

impl Handles<On<Tick>> for Exchange {
    type Sends =
        (Asks<Streamed<Reading>, Open<Reading>, Number>, ToAny<Streamed<Reading>, More>, Asks<Writing, Write, Sending>);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if self.reader == Reader::Unopened {
            let number = self.reading.open(turn, self.connection.protocol(), Read, ())?;
            self.reader = Reader::Reading(number);
        }
        let now = turn.now();
        let (since, within) = match self.phase {
            Phase::Idle { since } => (since, self.limits.idle_within_millis),
            Phase::Head { since } | Phase::Body { since, .. } => (since, self.limits.request_within_millis),
            Phase::Answering { since, .. } => (since, self.limits.answer_within_millis),
            Phase::Closing => return Ok(()),
        };
        let is_late = since + Duration::from_millis(u64::from(within)) <= now;
        if !is_late {
            return Ok(());
        }
        if let Phase::Answering { head_only, .. } = self.phase {
            let timed_out = Response::new(
                Status::GatewayTimeout,
                "text/plain; charset=utf-8",
                b"the handler did not respond in time".to_vec(),
            );
            return self.respond(turn, timed_out, true, head_only);
        }
        // A request that did not come, or came too slowly, is owed no response.
        turn.close();

        Ok(())
    }
}

impl Exchange {
    /// Parses what the buffer holds, as far as it goes: a head, then its body, then the request asked of the handler.
    /// A request refused is answered, and the exchange closes once the answer is sent.
    fn advance<S, IW, IP>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, IW> + Allows<Asks<Handling, Request, Asking>, IP>,
    {
        loop {
            match self.phase {
                Phase::Idle { .. } | Phase::Head { .. } => {
                    let front = parse::head(&self.buffer, &self.limits);
                    match front {
                        Front::More => {
                            let started = matches!(self.phase, Phase::Idle { .. }) && !self.buffer.is_empty();
                            if started {
                                self.phase = Phase::Head { since: turn.now() };
                            }
                            return Ok(());
                        }
                        Front::Refused(refusal) => return self.refuse(turn, refusal),
                        Front::Head { head, consumed } => {
                            self.buffer.drain(..consumed);
                            let since = match self.phase {
                                Phase::Head { since } => since,
                                _ => turn.now(),
                            };
                            let continues = head.expects_continue && head.length > 0;
                            if continues {
                                self.write::<_, IW>(turn, write::CONTINUE.to_vec(), false)?;
                            }
                            self.phase = Phase::Body { head, body: Vec::new(), since };
                        }
                    }
                }
                Phase::Body { ref head, ref mut body, .. } => {
                    let length = usize::try_from(head.length).unwrap_or(usize::MAX);
                    let taking = (length - body.len()).min(self.buffer.len());
                    body.extend(self.buffer.drain(..taking));
                    let is_whole = body.len() == length;
                    if !is_whole {
                        return Ok(());
                    }
                    return self.ask::<_, IP>(turn);
                }
                Phase::Answering { .. } | Phase::Closing => return Ok(()),
            }
        }
    }

    /// Asks the handler the request whose body is whole, and waits for its answer.
    fn ask<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Asks<Handling, Request, Asking>, I>,
    {
        let answering = Phase::Answering { since: turn.now(), closes: true, head_only: false };
        let Phase::Body { head, body, .. } = mem::replace(&mut self.phase, answering) else {
            return Ok(());
        };
        let Head { method, path, query, headers, closes, .. } = head;
        self.phase = Phase::Answering { since: turn.now(), closes, head_only: method == Method::Head };
        let request = Request { method, path, query, headers, body, peer: self.peer };
        turn.to(self.handler).ask::<_, _, I>(&request, &Asking)?;

        Ok(())
    }

    /// Answers a request refused before it reached the handler, and closes once the answer is sent.
    fn refuse<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, refusal: Refusal) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        turn.log(Level::Info, format!("a request was refused: {refusal:?}"));

        self.respond::<_, I>(turn, refusal.response(), true, false)
    }

    /// Writes `response`, and closes once it is sent when the connection `closes`.
    fn respond<S, I>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        response: Response,
        closes: bool,
        head_only: bool,
    ) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        let written = write::written(response, closes, head_only);
        for name in &written.dropped {
            turn.log(Level::Warn, format!("the response's header {name} was not written"));
        }
        if closes {
            self.phase = Phase::Closing;
        }

        self.write::<_, I>(turn, written.bytes, closes)
    }

    /// Asks the connection to write `bytes`, noting whether it closes once they are sent.
    fn write<S, I>(&self, turn: &mut Turn<'_, Self, S>, bytes: Vec<u8>, closes: bool) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        turn.to(self.connection.protocol::<Writing, _>()).ask::<_, _, I>(&Write { bytes }, &Sending { closes })?;

        Ok(())
    }
}

/// The response written for a request its handler did not answer: `status`, saying `why`.
fn unanswered(status: Status, why: &str) -> Response {
    Response::new(status, "text/plain; charset=utf-8", why.as_bytes().to_vec())
}
