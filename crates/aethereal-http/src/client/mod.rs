//! A client: one connection `Tcp` dialed for it, over TCP or to a Unix socket, on which it writes one request at a
//! time and reads each response as it arrives, checking every limit before a response goes further (ADR-0035). A
//! response is answered whole, or streamed to its asker as it arrives, paced by the asker's credit; and a connection
//! that switches protocols is handed to its asker as raw bytes. The root, [`Http`], dials the connection for whoever
//! asks, and stands the client up beneath it.

mod message;
mod parse;
mod root;
mod settings;
mod write;

pub use message::{
    Failure, Fetch, Fetched, Finished, Follow, Limit, Outgoing, Received, Receiving, Reply, ReplyHead, Switch,
    Unwritable,
};
pub use root::{Connect, Connected, Http, Peer, Reaching, ReachingUnix, Unreached};
pub use settings::{ClientLimits, HttpSettings};

use crate::Method;
use crate::http1::body::{Body, Decoded, Progress};
use aethereal_actor::{
    ActorRef, Answered, Beginning, Ended, Enough, Fault, Held, Later, Level, More, Number, Open, Part, Serving, Speaks,
    StreamKey, Streams, Taken, Turn, Unsent, actor,
};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{Dialed, HungUp, Read, Reading, Shut, Write, Writing, Wrote};
use core::mem;
use parse::{Framing, Head, Parsed};
use std::num::NonZeroU32;
use write::Lines;

/// What a client is made from: the connection `Tcp` dialed for it, what it writes as each request's `Host`, and its
/// limits.
#[derive(Debug, Bound)]
#[kind(name = "aether.http.client_config")]
pub struct ClientConfig {
    pub connection: ActorRef<Dialed>,
    pub host: String,
    pub limits: ClientLimits,
}

/// What a client keeps for a request it writes to its connection: nothing, since it has one out at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.requesting")]
pub struct Requesting;

/// Who waits on the response to the request out.
#[derive(Bound)]
enum Asker {
    /// A fetch, answered once the response's body is whole.
    Fetching(Held<Fetched>),
    /// The stream `key`, sent the response as it comes, which asked to switch protocols when it `switches`.
    Following { key: StreamKey, switches: bool },
}

/// Who takes a response's body.
#[derive(Bound)]
enum Taker {
    /// A fetch: the response's head, and its body as far as it has come, answered once it is whole.
    Gathering { held: Held<Fetched>, head: ReplyHead, body: Vec<u8> },
    /// The stream `key`, sent each part of the body as it comes.
    Following(StreamKey),
}

/// Where a client is in its connection's requests.
#[derive(Bound)]
enum Phase {
    /// No request is out: the connection waits for the next.
    Idle,
    /// A request is written, and its response's head is read: the response answers a `HEAD` when `head_only`.
    Head { asker: Asker, head_only: bool },
    /// A response's head is read, and its body is, as `body` says: the connection closes once it ends when it
    /// `closes`.
    Body { taker: Taker, body: Body, closes: bool },
    /// The connection switched protocols: its bytes go to the stream `key` as they come, and HTTP is no longer read.
    Switched { key: StreamKey },
}

/// Whether the connection's read stream still runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Line {
    /// It does.
    Open,
    /// It has ended, the connection hung up as `HungUp` says: what the buffer holds is all that will come.
    Ended(HungUp),
}

/// What a client's reading waits on, once it has read all it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stalled {
    /// More bytes from the connection.
    Bytes,
    /// More credit from the stream it sends the response to.
    Credit,
    /// Nothing: the client has closed.
    Closed,
}

/// What a client's reading does once it has read one thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// It reads on: the phase it went to may take more of the buffer.
    Again,
    /// It stops, waiting on what `Stalled` says.
    Stop(Stalled),
}

/// What became of a request the client was asked to write.
enum Written {
    /// It is asked of the connection.
    Asked,
    /// It was not sent, for this reason.
    Unsent(Failure),
}

/// A client on one connection `Tcp` dialed for it, beneath the root, keyed by the root's count (ADR-0035): asked a
/// [`Fetch`], it writes the request and answers with the response whole; opened a [`Receiving`] stream, it writes the
/// request and streams the response as it arrives, or hands over the connection once it switches protocols. It takes
/// one request at a time, keeps its connection between them, and closes when the connection does, when a response
/// breaks HTTP's rules or a limit, when the connection closes after a response, or when sent [`Shut`].
#[derive(Bound)]
#[kind(name = "aether.http.client")]
pub struct Client {
    connection: ActorRef<Dialed>,
    host: String,
    limits: ClientLimits,
    reading: Streams<Reading, ()>,
    /// The stream it reads its connection through, opened as it begins.
    reader: Number,
    /// What has been read and not yet parsed.
    buffer: Vec<u8>,
    /// The chunks taken whose credit is not yet granted back: granted once only more bytes would take the client
    /// further, so a stream whose asker has no credit holds the connection back.
    withheld: u32,
    line: Line,
    serving: Serving<Receiving>,
    phase: Phase,
}

#[actor(key = u64, parent = Http)]
impl Client {
    /// Opens its connection's stream: it reads from the first, so a connection that hangs up while idle is noticed.
    fn begin(config: ClientConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let window = NonZeroU32::new(config.limits.window).unwrap_or(NonZeroU32::MIN);
        let mut reading = Streams::new(window);
        let reader = reading.open(turn, config.connection.protocol(), Read, ())?;

        Ok(Self {
            connection: config.connection,
            host: config.host,
            limits: config.limits,
            reading,
            reader,
            buffer: Vec::new(),
            withheld: 0,
            line: Line::Open,
            serving: Serving::new(),
            phase: Phase::Idle,
        })
    }

    #[request]
    fn fetch(&mut self, fetch: Fetch, turn: &mut Turn<'_, Self>) -> Result<aethereal_actor::Reply<Fetched>, Fault> {
        let head_only = fetch.request.method == Method::Head;
        let written = self.write(turn, fetch.request, &Switch::Stay)?;
        let reply = match written {
            Written::Asked => {
                let (held, later) = turn.hold();
                self.phase = Phase::Head { asker: Asker::Fetching(held), head_only };
                aethereal_actor::Reply::Later(later)
            }
            Written::Unsent(failure) => aethereal_actor::Reply::Now(Fetched::Failed(failure)),
        };

        Ok(reply)
    }

    #[request]
    fn open(
        &mut self,
        open: Open<Receiving>,
        turn: &mut Turn<'_, Self, Speaks<(Part<Receiving>,)>>,
    ) -> Result<Later<Finished>, Fault> {
        let (key, Follow { request, switch }, later) = self.serving.open(turn, open);
        let head_only = request.method == Method::Head;
        let switches = matches!(switch, Switch::To { .. });
        let written = turn.unguarded(|turn| self.write(turn, request, &switch))?;
        match written {
            Written::Asked => self.phase = Phase::Head { asker: Asker::Following { key, switches }, head_only },
            Written::Unsent(failure) => turn.unguarded(|turn| self.finish(turn, key, &Finished::Failed(failure)))?,
        }

        Ok(later)
    }

    #[tell]
    fn more(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(_) = self.serving.more(turn, &more) else {
            return Ok(());
        };

        self.advance(turn)
    }

    /// Its asker stops the stream: the connection is left mid-response, or switched, so it closes.
    #[tell]
    fn enough(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(stopped) = self.serving.stop(turn, &enough) else {
            return Ok(());
        };
        stopped.finish(turn, &Finished::Closed(HungUp::Shut))?;
        turn.close();

        Ok(())
    }

    /// The asker of a stream it serves has ended: when the stream was still open, the connection is left
    /// mid-response, or switched, so it closes.
    #[ended]
    fn ended(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let left = self.serving.ended(&ended);
        if left.is_empty() {
            return Ok(());
        }
        turn.close();

        Ok(())
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.fail(turn, Failure::HungUp(HungUp::Shut))
    }

    #[tell]
    fn part(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(Taken { value, .. }) = self.reading.part_held(turn, part) else {
            return Ok(());
        };
        self.withheld += 1;
        self.buffer.extend_from_slice(turn.read(&value.bytes));

        self.advance(turn)
    }

    /// The connection's stream has ended: what the buffer holds is read on as far as it goes, and then the client
    /// closes.
    #[answer]
    fn hung_up(
        &mut self,
        (answered, number): (Answered<HungUp>, Number),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.reading.answered(number);
        let how = match answered {
            Answered::Yes(how) => how,
            Answered::No(_) => HungUp::Reset,
        };
        self.line = Line::Ended(how);

        self.advance(turn)
    }

    /// A request the connection would not take is answered so, and the client takes the next: nothing of it was
    /// written. A request sent waits on its response, and one the connection hung up on on the read stream's end.
    #[answer]
    fn wrote(
        &mut self,
        (answered, Requesting): (Answered<Wrote>, Requesting),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let is_full = answered == Answered::Yes(Wrote::Full);
        if !is_full {
            return Ok(());
        }
        match mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Head { asker: Asker::Fetching(held), .. } => turn.answer(held, &Fetched::Failed(Failure::Full))?,
            Phase::Head { asker: Asker::Following { key, .. }, .. } => {
                self.finish(turn, key, &Finished::Failed(Failure::Full))?;
            }
            other => self.phase = other,
        }

        Ok(())
    }
}

impl Client {
    /// Writes `request` to the connection, switching as `switch` says, when no other request is out: its head built
    /// in a buffer the turn hands out and put, and its body after it as a part of its own (ADR-0042).
    fn write(&self, turn: &mut Turn<'_, Self>, request: Outgoing, switch: &Switch) -> Result<Written, Fault> {
        let is_idle = matches!(self.phase, Phase::Idle);
        if !is_idle {
            return Ok(Written::Unsent(Failure::Busy));
        }
        let lines = Lines::from(&request);
        let body_length = turn.read(&request.body).len();
        let (doing, pool) = turn.split();
        let mut head = pool.buffer(write::room(lines, &self.host, switch));
        if let Err(why) = write::head(lines, body_length, &self.host, switch, &mut head) {
            return Ok(Written::Unsent(Failure::Unwritable(why)));
        }
        let Ok(head) = doing.put(head) else {
            return Ok(Written::Unsent(Failure::NoRoom));
        };
        let parts = if body_length == 0 {
            vec![head]
        } else {
            vec![head, request.body]
        };
        turn.to(self.connection.protocol::<Writing>()).ask(&Write { parts }, &Requesting)?;

        Ok(Written::Asked)
    }

    /// Reads on as far as the buffer goes, then grants the connection the credit withheld when only more of its bytes
    /// would take the client further, or, when the connection has ended and nothing more will come, ends what is
    /// out.
    fn advance(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let stalled = self.read(turn)?;
        match (stalled, self.line) {
            (Stalled::Closed | Stalled::Credit, _) => Ok(()),
            (Stalled::Bytes, Line::Ended(how)) => self.conclude(turn, how),
            (Stalled::Bytes, Line::Open) => {
                if let Some(withheld) = NonZeroU32::new(self.withheld) {
                    self.reading.grant(turn, self.reader, withheld)?;
                }
                self.withheld = 0;
                Ok(())
            }
        }
    }

    /// Reads what the buffer holds as far as it goes: a response's head, then its body, or after a switch, the
    /// connection's bytes. It says what it then waits on.
    fn read(&mut self, turn: &mut Turn<'_, Self>) -> Result<Stalled, Fault> {
        loop {
            let step = match self.phase {
                Phase::Idle => self.unasked(turn),
                Phase::Head { .. } => self.read_head(turn)?,
                Phase::Body { taker: Taker::Gathering { .. }, .. } => self.gather(turn)?,
                Phase::Body { taker: Taker::Following(key), .. } => self.follow(turn, key)?,
                Phase::Switched { key } => self.relay(turn, key)?,
            };
            if let Step::Stop(stalled) = step {
                return Ok(stalled);
            }
        }
    }

    /// Between requests, bytes no request asked for: the server is not speaking HTTP to this client, which closes.
    fn unasked(&self, turn: &mut Turn<'_, Self>) -> Step {
        if self.buffer.is_empty() {
            return Step::Stop(Stalled::Bytes);
        }
        turn.log(Level::Warn, "the server sent bytes no request asked for, and the client closes");
        turn.close();

        Step::Stop(Stalled::Closed)
    }

    /// Parses the response's head at the front of the buffer, and goes on to its body or the switch it makes. An
    /// informational head is passed over, as the final one follows it.
    fn read_head(&mut self, turn: &mut Turn<'_, Self>) -> Result<Step, Fault> {
        let Phase::Head { ref asker, head_only } = self.phase else {
            return Ok(Step::Again);
        };
        let (head, consumed) = match parse::head(&self.buffer, self.limits.most_head_bytes, head_only) {
            Parsed::More => return Ok(Step::Stop(Stalled::Bytes)),
            Parsed::Failed(failure) => {
                self.fail(turn, failure)?;
                return Ok(Step::Stop(Stalled::Closed));
            }
            Parsed::Head { head, consumed } => (head, consumed),
        };
        let is_informational = (100..200).contains(&head.status) && head.status != 101;
        if is_informational {
            self.buffer.drain(..consumed);
            return Ok(Step::Again);
        }
        let waits = match *asker {
            Asker::Following { key, .. } => !self.serving.has_credit(key),
            Asker::Fetching(_) => false,
        };
        if waits {
            // The head is a part, and waits for credit, unparsed: it parses the same once credit comes.
            return Ok(Step::Stop(Stalled::Credit));
        }
        self.buffer.drain(..consumed);
        let Phase::Head { asker, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            return Ok(Step::Again);
        };

        self.headed(turn, asker, head)
    }

    /// Goes on from the response's `head`, which `asker` waits on: to its body, or to the switch it makes. A switch a
    /// request did not ask for, and a body past the client's limit by its length, end the client.
    fn headed(&mut self, turn: &mut Turn<'_, Self>, asker: Asker, parsed: Head) -> Result<Step, Fault> {
        let Head { status, headers, framing, closes } = parsed;
        let switched = status == 101;
        let is_success = (200..300).contains(&status);
        let takes_over = switched || (is_success && framing == Framing::Close);
        let reply = ReplyHead { status, headers };
        let body = framing.body();
        match asker {
            Asker::Following { key, switches: true } if takes_over => {
                self.phase = Phase::Switched { key };
                let writing = self.connection.protocol::<Writing>();
                self.send(turn, key, Received::Switched { head: reply, writing })?;
                Ok(Step::Again)
            }
            asker if switched => {
                // A server switches only when asked (RFC 9110 §15.2.2).
                self.phase = Phase::Head { asker, head_only: false };
                self.fail(turn, Failure::Malformed)?;
                Ok(Step::Stop(Stalled::Closed))
            }
            Asker::Fetching(held) => {
                let most = u64::from(self.limits.most_body_bytes);
                let is_past = matches!(framing, Framing::Length(length) if length > most);
                let gathered = match framing {
                    Framing::Length(length) if !is_past => Vec::from(turn.buffer(usize::try_from(length).unwrap_or(0))),
                    _ => Vec::new(),
                };
                let taker = Taker::Gathering { held, head: reply, body: gathered };
                self.phase = Phase::Body { taker, body, closes };
                if is_past {
                    self.fail(turn, Failure::TooLarge(Limit::Body))?;
                    return Ok(Step::Stop(Stalled::Closed));
                }
                Ok(Step::Again)
            }
            Asker::Following { key, .. } => {
                self.phase = Phase::Body { taker: Taker::Following(key), body, closes };
                self.send(turn, key, Received::Head(reply))?;
                Ok(Step::Again)
            }
        }
    }

    /// Reads on the body of a response a fetch waits on, and answers the fetch once it is whole: a body past the
    /// client's limit ends it.
    fn gather(&mut self, turn: &mut Turn<'_, Self>) -> Result<Step, Fault> {
        let most_line = self.limits.most_head_bytes;
        let most_body = usize::try_from(self.limits.most_body_bytes).unwrap_or(usize::MAX);
        let Phase::Body { taker: Taker::Gathering { body: ref mut gathered, .. }, ref mut body, .. } = self.phase
        else {
            return Ok(Step::Again);
        };
        let decoded = body.decode(&self.buffer, gathered, most_line);
        let is_past = gathered.len() > most_body;
        let progress = match decoded {
            Ok(Decoded { consumed, progress }) => {
                self.buffer.drain(..consumed);
                progress
            }
            Err(broken) => {
                self.fail(turn, Failure::from(broken))?;
                return Ok(Step::Stop(Stalled::Closed));
            }
        };
        if is_past {
            self.fail(turn, Failure::TooLarge(Limit::Body))?;
            return Ok(Step::Stop(Stalled::Closed));
        }
        match progress {
            Progress::More => Ok(Step::Stop(Stalled::Bytes)),
            Progress::Done => self.done(turn),
        }
    }

    /// Reads on the body of a response the stream `key` waits on, and sends what came of it as one part, while the
    /// stream has credit: the body is never held whole.
    fn follow(&mut self, turn: &mut Turn<'_, Self>, key: StreamKey) -> Result<Step, Fault> {
        let has_bytes = !self.buffer.is_empty();
        let is_held = has_bytes && !self.serving.has_credit(key);
        if is_held {
            return Ok(Step::Stop(Stalled::Credit));
        }
        let most_line = self.limits.most_head_bytes;
        // The body is decoded into a buffer the pool hands out, which goes back to it, emptied, as it is dropped
        // whenever it is not put: before the turn is used whole again.
        let (doing, pool) = turn.split();
        let mut out = pool.buffer(self.buffer.len());
        let Phase::Body { ref mut body, .. } = self.phase else {
            return Ok(Step::Again);
        };
        let progress = match body.decode(&self.buffer, &mut out, most_line) {
            Ok(Decoded { consumed, progress }) => {
                self.buffer.drain(..consumed);
                progress
            }
            Err(broken) => {
                drop(out);
                self.fail(turn, Failure::from(broken))?;
                return Ok(Step::Stop(Stalled::Closed));
            }
        };
        if out.is_empty() {
            drop(out);
        } else {
            let Ok(bytes) = doing.put(out) else {
                self.fail(turn, Failure::NoRoom)?;
                return Ok(Step::Stop(Stalled::Closed));
            };
            self.send(turn, key, Received::Bytes(bytes))?;
        }
        match progress {
            Progress::More => Ok(Step::Stop(Stalled::Bytes)),
            Progress::Done => self.done(turn),
        }
    }

    /// Sends the stream `key` what the buffer holds of a connection that switched, as one part, while the stream has
    /// credit.
    fn relay(&mut self, turn: &mut Turn<'_, Self>, key: StreamKey) -> Result<Step, Fault> {
        if self.buffer.is_empty() {
            return Ok(Step::Stop(Stalled::Bytes));
        }
        if !self.serving.has_credit(key) {
            return Ok(Step::Stop(Stalled::Credit));
        }
        let (doing, pool) = turn.split();
        let mut raw = pool.buffer(self.buffer.len());
        raw.extend_from_slice(&self.buffer);
        self.buffer.clear();
        let Ok(bytes) = doing.put(raw) else {
            self.fail(turn, Failure::NoRoom)?;
            return Ok(Step::Stop(Stalled::Closed));
        };
        self.send(turn, key, Received::Bytes(bytes))?;

        Ok(Step::Again)
    }

    /// The response's body has ended: its asker is answered, and the client takes the next request, or closes when
    /// the connection closes once the response is read.
    fn done(&mut self, turn: &mut Turn<'_, Self>) -> Result<Step, Fault> {
        let Phase::Body { taker, closes, .. } = mem::replace(&mut self.phase, Phase::Idle) else {
            return Ok(Step::Again);
        };
        self.answer_whole(turn, taker, &Finished::Whole)?;
        if closes {
            turn.close();
            return Ok(Step::Stop(Stalled::Closed));
        }

        Ok(Step::Again)
    }

    /// The connection has ended, and the buffer gives nothing more: a body that runs until the connection closes is
    /// whole, and anything else out has failed. Either way the client closes.
    fn conclude(&mut self, turn: &mut Turn<'_, Self>, how: HungUp) -> Result<(), Fault> {
        let runs_to_close = matches!(self.phase, Phase::Body { body: Body::Close, .. });
        let ends_body = runs_to_close && how == HungUp::Peer;
        if !ends_body {
            return self.fail(turn, Failure::HungUp(how));
        }
        if let Phase::Body { taker, .. } = mem::replace(&mut self.phase, Phase::Idle) {
            self.answer_whole(turn, taker, &Finished::Closed(how))?;
        }
        turn.close();

        Ok(())
    }

    /// Answers `taker` with the body it took, which has ended: a fetch with the response whole, and a stream with
    /// `finished`.
    fn answer_whole(&mut self, turn: &mut Turn<'_, Self>, taker: Taker, finished: &Finished) -> Result<(), Fault> {
        match taker {
            Taker::Gathering { held, head, body } => {
                let ReplyHead { status, headers } = head;
                let fetched = turn
                    .put(body)
                    .map_or(Fetched::Failed(Failure::NoRoom), |body| Fetched::Whole(Reply { status, headers, body }));
                turn.answer(held, &fetched)?;
            }
            Taker::Following(key) => self.finish(turn, key, finished)?,
        }

        Ok(())
    }

    /// Answers whoever waits on the response with `failure`, and closes: the connection is left mid-response, so no
    /// later request could be read on it. A stream of a switched connection that hung up has not failed: it is
    /// closed.
    fn fail(&mut self, turn: &mut Turn<'_, Self>, failure: Failure) -> Result<(), Fault> {
        match mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Idle => {}
            Phase::Head { asker: Asker::Fetching(held), .. }
            | Phase::Body { taker: Taker::Gathering { held, .. }, .. } => {
                turn.answer(held, &Fetched::Failed(failure))?;
            }
            Phase::Head { asker: Asker::Following { key, .. }, .. }
            | Phase::Body { taker: Taker::Following(key), .. } => {
                self.finish(turn, key, &Finished::Failed(failure))?;
            }
            Phase::Switched { key } => {
                let finished = match failure {
                    Failure::HungUp(how) => Finished::Closed(how),
                    other => Finished::Failed(other),
                };
                self.finish(turn, key, &finished)?;
            }
        }
        turn.close();

        Ok(())
    }

    /// Sends the stream `key` `part`, which its credit allows.
    fn send(&mut self, turn: &mut Turn<'_, Self>, key: StreamKey, part: Received) -> Result<(), Fault> {
        let Some(stream) = self.serving.get_mut(key) else {
            return Ok(());
        };
        match stream.send(turn, part) {
            Ok(()) => Ok(()),
            Err(Unsent::Unwritable(error)) => Err(error.into()),
            Err(Unsent::NoCredit(_)) => {
                turn.log(Level::Error, "a part was sent past its stream's credit, and was dropped");
                Ok(())
            }
        }
    }

    /// Ends the stream `key` with `finished`, when it is still served.
    fn finish(&mut self, turn: &mut Turn<'_, Self>, key: StreamKey, finished: &Finished) -> Result<(), Fault> {
        if let Some(stream) = self.serving.take(key) {
            stream.finish(turn, finished)?;
        }

        Ok(())
    }
}
