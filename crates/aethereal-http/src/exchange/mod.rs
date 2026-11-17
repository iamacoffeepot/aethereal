//! An exchange: the server's side of one accepted connection, which parses each request as its chunks arrive, asks
//! it of the server's handler, or of its mount's handler with the body streamed, and writes the response the handler
//! answers with (ADR-0035).

mod inflow;
mod parse;
mod write;

use crate::body::{Body, Decoded, Progress};
use crate::mount::mounted;
use crate::{
    Handling, Limits, Method, Mount, Request, RequestBody, Response, Server, Status, StreamHandling, StreamedRequest,
};
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Asks, Beginning, Bytes, ChildOf, Due, Ended, Ending, Enough, Fault,
    Handles, JobId, Level, More, Number, Open, Part, ProtocolRef, Refused, Serving, Sleep, Slept, Streamed, Streams,
    Taken, Time, ToAny, Turn, Unanswered,
};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{Accepted, HungUp, Read, Reading, Shut, SocketAddress, Write, Writing, Wrote};
use core::mem;
use core::time::Duration;
use inflow::Inflow;
use parse::{Framing, Head, Parsed as Front, Refusal};
use std::num::NonZeroU32;

/// What an exchange is made from: the connection it reads, where it came from, the port it came in on, the handler it
/// asks each request of, the mounts it streams requests to instead, and the limits on each request.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.exchange_config")]
pub struct ExchangeConfig {
    pub connection: ActorRef<Accepted>,
    pub peer: SocketAddress,
    pub port: u16,
    pub handler: ProtocolRef<Handling>,
    pub mounts: Vec<Mount>,
    pub limits: Limits,
}

/// What an exchange keeps for a request it asked of the handler, or of a mount's: nothing, since it has one out at a
/// time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.asking")]
pub struct Asking;

/// What an exchange keeps for a write: whether the connection closes once it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.sending")]
pub struct Sending {
    closes: bool,
}

/// What an exchange keeps for the sleep that times a phase: the number of the phase it was run for, so a sleep for a
/// phase the exchange has left is passed over. A request waiting on the handler is timed by its ask's deadline, not a
/// sleep (ADR-0041).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.deadline")]
pub struct Deadline {
    phase: u64,
}

/// Whether an exchange's phase has a deadline it sleeps until.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
enum Sleeping {
    /// It has none: a request waits on the handler, asked within its deadline, or the exchange is closing.
    Not,
    /// The [`Sleep`] it ran until the phase's deadline, not yet answered.
    On(JobId),
}

/// Where an exchange is in its connection's requests.
#[derive(Debug, Plain)]
enum Phase {
    /// Between requests: nothing of the next has come since `since`.
    Idle { since: Time },
    /// Part of a head has come, its first byte at `since`.
    Head { since: Time },
    /// A head is parsed, and its body is coming, decoded as `left` says is left of it into a buffer the turn handed
    /// out, with room for all of it when its length is known; the request's first byte came at `since`.
    Body { head: Head, left: Body, body: Vec<u8>, since: Time },
    /// A head under a mount is asked of the mount's handler, with no deadline, and its body is read to the handler as
    /// `inflow` says. The connection then closes when it `closes`, and a response to a `HEAD` is written with no body.
    Inflow { inflow: Inflow, closes: bool, head_only: bool },
    /// The request is asked of the handler, within its deadline, and its answer is waited for. The connection then
    /// closes when it `closes`, and a response to a `HEAD` is written with no body.
    Answering { closes: bool, head_only: bool },
    /// A streamed request's body has been read, or will not be, and its answer is waited for from `since`: its ask
    /// carried no deadline, so the exchange times it itself, `answer_within_millis` after `since`.
    Awaiting { closes: bool, head_only: bool, since: Time },
    /// A last response is written: the exchange closes once it is sent.
    Closing,
}

/// The server's side of one accepted connection, beneath the server, keyed by its count (ADR-0035): reads it through
/// a [`Reading`] stream, parses each request, asks it of the handler, and writes the [`Response`] answered. A request
/// under a [`Mount`] is asked of the mount's handler as a [`StreamedRequest`], and its body served to the handler as a
/// [`RequestBody`] stream, paced by its credit. It takes one request at a time: while one is out it grants its
/// connection no credit but what a streamed body needs, so requests sent ahead wait in the kernel.
#[derive(Bound)]
#[kind(name = "aether.http.exchange")]
pub struct Exchange {
    connection: ActorRef<Accepted>,
    peer: SocketAddress,
    port: u16,
    handler: ProtocolRef<Handling>,
    mounts: Vec<Mount>,
    limits: Limits,
    reading: Streams<Reading, ()>,
    /// The stream it reads its connection through, opened as it begins.
    reader: Number,
    /// What has been read and not yet parsed.
    buffer: Vec<u8>,
    /// The chunks taken while a request was out, whose credit is granted once it is answered, or once a streamed body
    /// needs more bytes.
    withheld: u32,
    /// The stream a mount's handler reads a body through.
    serving: Serving<RequestBody>,
    /// How many requests it has asked of a mount's handler: the number of the last.
    streamed: u64,
    phase: Phase,
    /// How many phases with a deadline it has entered: the number of the one it is in.
    phases: u64,
    sleeping: Sleeping,
}

impl ChildOf for Exchange {
    type Parent = Server;
}

impl Actor for Exchange {
    type Config = ExchangeConfig;
    type Key = u64;
    type Handles = (
        Part<Reading>,
        Answer<HungUp, Number>,
        Answer<Wrote, Sending>,
        Answer<Response, Asking>,
        Answer<Slept, Deadline>,
        Shut,
        Open<RequestBody>,
        More,
        Enough,
        Ended,
    );
    type BeginSends = (Asks<Streamed<Reading>, Open<Reading>, Number>, ToAny<Streamed<Reading>, More>);
    type EndSends = ();

    /// Opens its connection's stream, and sleeps until the first request is late.
    fn begin(config: ExchangeConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let window = NonZeroU32::new(config.limits.window).unwrap_or(NonZeroU32::MIN);
        let mut reading = Streams::new(window);
        let reader = reading.open(turn, config.connection.protocol(), Read, ())?;
        let phase = Phase::Idle { since: turn.now() };
        let sleeping = match due(&phase, &config.limits) {
            Due::Before(until) => Sleeping::On(turn.run(&Sleep { until }, &Deadline { phase: 0 })?),
            Due::Never => Sleeping::Not,
        };

        Ok(Self {
            connection: config.connection,
            peer: config.peer,
            port: config.port,
            handler: config.handler,
            mounts: config.mounts,
            limits: config.limits,
            reading,
            reader,
            buffer: Vec::new(),
            withheld: 0,
            serving: Serving::new(),
            streamed: 0,
            phase,
            phases: 0,
            sleeping,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What an exchange sends as it reads its connection: credit, writes, the requests it asks, and the parts of a body it
/// streams.
type Advancing =
    (Credit, Writes, Asks<Handling, Request, Asking>, Asks<StreamHandling, StreamedRequest, Asking>, Parts);

/// What an exchange sends its connection as it reads: credit.
type Credit = ToAny<Streamed<Reading>, More>;

/// What an exchange asks its connection: a write.
type Writes = Asks<Writing, Write, Sending>;

/// What an exchange sends as it streams a body: the parts of the stream a mount's handler reads it through.
type Parts = ToAny<(Part<RequestBody>,), Part<RequestBody>>;

impl Handles<Part<Reading>> for Exchange {
    type Sends = Advancing;

    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let is_out = !matches!(self.phase, Phase::Idle { .. } | Phase::Head { .. } | Phase::Body { .. });
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
        self.buffer.extend_from_slice(turn.read(&value.bytes));

        self.advance(turn)
    }
}

impl Handles<Answer<HungUp, Number>> for Exchange {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, number): (Answered<HungUp>, Number),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.reading.answered(number);
        let how = match answered {
            Answered::Yes(how) => how,
            Answered::No(_) => HungUp::Reset,
        };

        self.hang_up(turn, how)
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
    type Sends = Advancing;

    fn handle(
        &mut self,
        (answered, Asking): (Answered<Response>, Asking),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let (closes, head_only) = match self.phase {
            Phase::Answering { closes, head_only } | Phase::Awaiting { closes, head_only, .. } => (closes, head_only),
            Phase::Inflow { inflow, closes, head_only } => (self.cut(turn, inflow)? || closes, head_only),
            Phase::Idle { .. } | Phase::Head { .. } | Phase::Body { .. } | Phase::Closing => {
                // No request waits on the handler: an answer after its request's deadline is dropped by the engine,
                // so only one to a request the exchange no longer waits on comes here.
                turn.log(Level::Info, "an answer came to no request waiting, and was passed over");
                return Ok(());
            }
        };
        let response = match answered {
            Answered::Yes(response) => Ok(response),
            Answered::No(Unanswered::Ended) => said(turn, Status::ServiceUnavailable, "the handler has ended"),
            // The request's deadline came first: the engine stopped waiting for the handler, and the connection closes
            // once the client is told (ADR-0041).
            Answered::No(Unanswered::Expired) => return self.time_out(turn, head_only),
            Answered::No(why) => {
                turn.log(Level::Warn, format!("the handler did not answer a request: {why:?}"));
                said(turn, Status::InternalServerError, "the handler did not answer")
            }
        };
        let Ok(response) = response else {
            unstored(turn);
            return Ok(());
        };
        self.respond(turn, response, closes, head_only)?;
        if closes {
            return Ok(());
        }
        self.phase = Phase::Idle { since: turn.now() };
        self.time(turn)?;
        self.release(turn)?;

        self.advance(turn)
    }
}

impl Handles<Shut> for Exchange {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.hang_up(turn, HungUp::Shut)
    }
}

impl Handles<Answer<Slept, Deadline>> for Exchange {
    type Sends = (Asks<Writing, Write, Sending>,);

    /// The phase it slept for is late, when it still stands: a request that did not come, or came too slowly, is owed
    /// no response. A request waiting on the handler is timed by its ask's deadline, which runs no sleep (ADR-0041).
    fn handle(
        &mut self,
        (slept, deadline): (Answered<Slept>, Deadline),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let is_current = deadline.phase == self.phases;
        if !is_current {
            return Ok(());
        }
        self.sleeping = Sleeping::Not;
        if let Answered::No(why) = slept {
            turn.log(Level::Warn, format!("a deadline's sleep was not answered: {why:?}"));
            return Ok(());
        }
        match self.phase {
            Phase::Idle { .. } | Phase::Head { .. } | Phase::Body { .. } => {
                turn.close();
                Ok(())
            }
            Phase::Inflow { inflow, .. } => self.stall(turn, inflow),
            Phase::Awaiting { head_only, .. } => self.time_out(turn, head_only),
            Phase::Answering { .. } | Phase::Closing => Ok(()),
        }
    }
}

impl Exchange {
    /// Sleeps until the deadline of the phase it has entered, in place of the last phase's sleep, which it cancels: a
    /// request waiting on the handler, timed by its ask's deadline, and a closing exchange sleep for none.
    fn time<S>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        if let Sleeping::On(job) = self.sleeping {
            turn.cancel(job);
        }
        self.phases += 1;
        self.sleeping = match due(&self.phase, &self.limits) {
            Due::Before(until) => Sleeping::On(turn.run(&Sleep { until }, &Deadline { phase: self.phases })?),
            Due::Never => Sleeping::Not,
        };

        Ok(())
    }

    /// Grants the connection the credit withheld while a request was out.
    fn release<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Credit, I>,
    {
        if let Some(withheld) = NonZeroU32::new(self.withheld) {
            self.reading.grant::<_, _, I>(turn, self.reader, withheld)?;
        }
        self.withheld = 0;

        Ok(())
    }

    /// Parses what the buffer holds, as far as it goes: a head, then its body, then the request asked of the handler;
    /// or a head under a mount, asked of the mount's handler, and its body streamed to it. A request refused is
    /// answered, and the exchange closes once the answer is sent.
    fn advance<S, IC, IW, IP, IS, IB>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Credit, IC>
            + Allows<Writes, IW>
            + Allows<Asks<Handling, Request, Asking>, IP>
            + Allows<Asks<StreamHandling, StreamedRequest, Asking>, IS>
            + Allows<Parts, IB>,
    {
        loop {
            match self.phase {
                Phase::Idle { .. } | Phase::Head { .. } => {
                    let front = parse::head(&self.buffer, &self.limits, &self.mounts);
                    match front {
                        Front::More => {
                            let started = matches!(self.phase, Phase::Idle { .. }) && !self.buffer.is_empty();
                            if started {
                                self.phase = Phase::Head { since: turn.now() };
                                self.time(turn)?;
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
                            let continues = head.expects_continue && head.framing != Framing::Empty;
                            if continues {
                                let Ok(go_on) = put(turn, write::CONTINUE) else {
                                    unstored(turn);
                                    return Ok(());
                                };
                                self.write::<_, IW>(turn, vec![go_on], false)?;
                            }
                            let mount =
                                mounted(&self.mounts, &head.path).map(|mount| (mount.handler, mount.most_body_bytes));
                            if let Some((handler, most)) = mount {
                                self.stream::<_, IS>(turn, head, handler, most)?;
                                continue;
                            }
                            let room = match head.framing {
                                Framing::Length(length) => usize::try_from(length).unwrap_or(usize::MAX),
                                Framing::Empty | Framing::Chunked => 0,
                            };
                            let body = turn.buffer(room);
                            let left = head.framing.body();
                            self.phase = Phase::Body { head, left, body, since };
                        }
                    }
                }
                Phase::Body { ref mut left, ref mut body, .. } => {
                    let most_body = usize::try_from(self.limits.most_body_bytes).unwrap_or(usize::MAX);
                    let Decoded { consumed, progress } =
                        match left.decode(&self.buffer, body, self.limits.most_head_bytes) {
                            Ok(decoded) => decoded,
                            Err(broken) => return self.refuse(turn, Refusal::from(broken)),
                        };
                    self.buffer.drain(..consumed);

                    // A chunked body has no length to check before it is read, so it is checked as it is decoded.
                    if body.len() > most_body {
                        return self.refuse(turn, Refusal::BodyTooLarge);
                    }
                    if progress == Progress::More {
                        return Ok(());
                    }
                    return self.ask::<_, IW, IP>(turn);
                }
                Phase::Inflow { .. } => return self.flow::<_, IC, IW, IB>(turn),
                Phase::Answering { .. } | Phase::Awaiting { .. } | Phase::Closing => return Ok(()),
            }
        }
    }

    /// Asks the handler the request whose body is whole, put as a blob, within `answer_within_millis` of now, and waits
    /// for its answer: the engine answers it `Expired` if that time comes first, as it answers every request made for
    /// it (ADR-0041). A body the engine has no room to hold is answered `503`, and the exchange closes.
    fn ask<S, IW, IP>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, IW> + Allows<Asks<Handling, Request, Asking>, IP>,
    {
        let answering = Phase::Answering { closes: true, head_only: false };
        let Phase::Body { head, body, .. } = mem::replace(&mut self.phase, answering) else {
            return Ok(());
        };
        let Head { method, path, query, headers, closes, .. } = head;
        let head_only = method == Method::Head;
        let Ok(body) = turn.put(body) else {
            let unheld = said(turn, Status::ServiceUnavailable, "the server has no room for the request's body");
            let Ok(unheld) = unheld else {
                unstored(turn);
                return Ok(());
            };
            return self.respond::<_, IW>(turn, unheld, true, head_only);
        };
        self.phase = Phase::Answering { closes, head_only };
        self.time(turn)?;
        let request = Request { method, path, query, headers, body, peer: self.peer, port: self.port };
        let due = turn.now() + Duration::from_millis(u64::from(self.limits.answer_within_millis));
        turn.to(self.handler).within(due).ask::<_, _, IP>(&request, &Asking)?;

        Ok(())
    }

    /// Asks `head` of the mount's `handler`, its body to be streamed to it up to `most` bytes, with no deadline: the
    /// body may take as long as it keeps moving, and once it is read the answer is timed by the exchange (ADR-0041). A
    /// stream the handler opens in the turn it is asked inherits the same, so a long body is not cut off.
    fn stream<S, I>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        head: Head,
        handler: ProtocolRef<StreamHandling>,
        most: u64,
    ) -> Result<(), Fault>
    where
        S: Allows<Asks<StreamHandling, StreamedRequest, Asking>, I>,
    {
        let Head { method, path, query, headers, framing, closes, .. } = head;
        self.streamed += 1;
        let request = StreamedRequest {
            method,
            path,
            query,
            headers,
            length: framing.length(),
            body: turn.me().protocol(),
            request: self.streamed,
            peer: self.peer,
            port: self.port,
        };
        let inflow = Inflow::new(self.streamed, framing.body(), most, turn.now());
        self.phase = Phase::Inflow { inflow, closes, head_only: method == Method::Head };
        self.time(turn)?;
        turn.to(handler).within(Due::Never).ask::<_, _, I>(&request, &Asking)?;

        Ok(())
    }

    /// Answers `504` to a request its handler did not answer in time, and closes once it is sent.
    fn time_out<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, head_only: bool) -> Result<(), Fault>
    where
        S: Allows<Writes, I>,
    {
        let Ok(timed_out) = said(turn, Status::GatewayTimeout, "the handler did not respond in time") else {
            unstored(turn);
            return Ok(());
        };

        self.respond::<_, I>(turn, timed_out, true, head_only)
    }

    /// Answers a request refused before it reached the handler, and closes once the answer is sent.
    fn refuse<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, refusal: Refusal) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        turn.log(Level::Info, format!("a request was refused: {refusal:?}"));
        let (status, text) = refusal.said();
        let Ok(response) = said(turn, status, text) else {
            unstored(turn);
            return Ok(());
        };

        self.respond::<_, I>(turn, response, true, false)
    }

    /// Writes `response`, and closes once it is sent when the connection `closes`: its head, built in a buffer the turn
    /// hands out and put, and its body after it as a part of its own, passed on without being read (ADR-0042).
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
        let Response { status, headers, body } = response;
        let body_length = turn.read(&body).len();
        let mut head = turn.buffer(write::room(&headers));
        let written = write::head(status, &headers, body_length, closes, head_only, &mut head);
        for name in &written.dropped {
            turn.log(Level::Warn, format!("the response's header {name} was not written"));
        }
        if closes {
            self.phase = Phase::Closing;
            self.time(turn)?;
        }
        let Ok(head) = turn.put(head) else {
            unstored(turn);
            return Ok(());
        };
        let parts = if written.sends_body {
            vec![head, body]
        } else {
            vec![head]
        };

        self.write::<_, I>(turn, parts, closes)
    }

    /// Asks the connection to write `parts`, in order, noting whether it closes once they are sent.
    fn write<S, I>(&self, turn: &mut Turn<'_, Self, S>, parts: Vec<Bytes>, closes: bool) -> Result<(), Fault>
    where
        S: Allows<Asks<Writing, Write, Sending>, I>,
    {
        turn.to(self.connection.protocol::<Writing, _>()).ask::<_, _, I>(&Write { parts }, &Sending { closes })?;

        Ok(())
    }
}

/// When `phase` is late under `limits`, as the exchange sleeps until it: a request's head and body share the deadline
/// of its first byte, a streamed body is late once it has not moved for `stall_within_millis`, and a streamed request
/// whose body is done is answered within `answer_within_millis`. A request waiting on the handler is timed by its ask's
/// deadline instead, and a closing exchange by none.
fn due(phase: &Phase, limits: &Limits) -> Due {
    let (since, within) = match phase {
        Phase::Idle { since } => (*since, limits.idle_within_millis),
        Phase::Head { since } | Phase::Body { since, .. } => (*since, limits.request_within_millis),
        Phase::Inflow { inflow, .. } => (inflow.moved(), limits.stall_within_millis),
        Phase::Awaiting { since, .. } => (*since, limits.answer_within_millis),
        Phase::Answering { .. } | Phase::Closing => return Due::Never,
    };

    Due::Before(since + Duration::from_millis(u64::from(within)))
}

/// The response an exchange writes of its own: `status`, saying `why`, its body put from a buffer the turn hands out.
fn said<S>(turn: &mut Turn<'_, Exchange, S>, status: Status, why: &str) -> Result<Response, Refused> {
    Ok(Response::new(status, "text/plain; charset=utf-8", put(turn, why.as_bytes())?))
}

/// `bytes`, copied into a buffer the turn hands out, and put.
fn put<S>(turn: &mut Turn<'_, Exchange, S>, bytes: &[u8]) -> Result<Bytes, Refused> {
    let mut buffer = turn.buffer(bytes.len());
    buffer.extend_from_slice(bytes);

    turn.put(buffer)
}

/// Closes an exchange that had no room to hold what it would write (ADR-0014): the client is told nothing more.
fn unstored<S>(turn: &mut Turn<'_, Exchange, S>) {
    turn.log(Level::Warn, "the server had no room to hold a response, and closes the connection");
    turn.close();
}
