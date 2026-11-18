//! What a stack holds and does (ADR-0035): its listener, an entry for each open connection, its pure layers' template,
//! and the body streams it serves and opens. Each of the stack's handlers of HTTP's and TCP's kinds, of its actor
//! layers' answers, and of its body streams' kinds, is one call into it.

mod body;
mod connection;
mod inflow;
mod outflow;
mod parked;
mod respond;

use super::actor::{Parked, ParkedPart, held};
use super::{
    AskedBy, BodyHandle, Credentials, Entered, Gathered, Gathering, Handed, Head, LastLayer, Layer, LayerList, Layers,
    PartStep, Picked, Response, ResponseBody, Slot, Stack, StackSettings, Unrouted, WalkedBy,
};
use crate::http1::head::Malformed;
use crate::stack::request_head::{self, Framing, Parsed, Refusal, RequestHead};
use crate::stack::{BodyRead, InboundBody, OutboundBody, SourceEnd};
use crate::{Method, Status};
use aethereal_actor::{
    ActorRef, Answer, Answered, Beginning, Conforms, DependsOn, Fault, Handles, Level, Lists, Number, Part, Receives,
    Slept, Streamed, Streams, Taken, Time, Turn, Unanswered, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{
    Accept, Accepting, Finish, Framing as Chunking, HungUp, Incoming, Listen, Listener, Listening, Read, Reading, Shut,
    Tcp, Unbound, Wrote,
};
use body::Ending;
use connection::{Connection, Entering, Inflow, Out, Passing, Phase, Returning};
use core::mem;
use core::time::Duration;
use outflow::Outflow;
use std::collections::BTreeMap;
use std::num::NonZeroU32;

/// The handlers a stack's last layer reaches, for its layers `T`.
type Reaches<T> = <<T as LayerList>::Last as LastLayer<<T as LayerList>::Attached>>::Reaches;

/// What a stack keeps for its listen: nothing, as it asks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.binding")]
pub struct Binding;

/// Why a stack failed: its listener stopped without being asked to, so its port is closed and it serves nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.unlistened")]
pub struct Unlistened;

/// What a stack keeps for a write: the connection it is on, and what it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.sending")]
pub struct Sending {
    connection: u64,
    carries: Carries,
}

/// What a stack's write carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Carries {
    /// A response, or the end of a streamed one, or `100 Continue`: the connection closes once it is sent when it
    /// `closes`.
    Response { closes: bool },
    /// A streamed response's head, when `parts` is none, or the first `parts` of the parts of its body queued.
    Body { parts: u32 },
}

/// What a stack keeps for a request it asked its handler: the connection it is on, and its number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.answering")]
pub struct Answering {
    connection: u64,
    request: u64,
}

/// What a stack keeps for the sleep that times a connection's phase: the connection, and the number of the phase it
/// was run for, so a sleep for a phase the connection has left is passed over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.deadline")]
pub struct Deadline {
    connection: u64,
    phase: u64,
}

/// What a stack's listen came to, handed to the stack to decide on.
#[derive(Debug)]
pub enum Listened {
    /// Bound on `port`: the stack accepts its connections.
    Bound { port: u16 },
    /// `Tcp` answered otherwise, and the stack serves nothing.
    Refused(Listening),
    /// `Tcp` did not answer, as the engine says why, and the stack serves nothing.
    Unanswered(Unanswered),
}

/// What a request is owed once its answer comes: whether the connection then closes, whether the response answers a
/// `HEAD` and is written with no body, whether the client waits for `100 Continue` before it sends the body, and how
/// it reads a body of no known length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
struct Owed {
    closes: bool,
    head_only: bool,
    continues: bool,
    reads: Reads,
}

impl Owed {
    /// What is owed a request the stack refuses before its layers, and then closes: its response has no body.
    const CLOSING: Self = Self { closes: true, head_only: false, continues: false, reads: Reads::Close };
}

/// How a client reads a response's body of no known length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Reads {
    /// In chunks, as an HTTP/1.1 client does.
    Chunks,
    /// Until the connection closes, as an HTTP/1.0 client does, which reads no chunks.
    Close,
}

/// Where a stack's listen stands.
#[derive(Debug, Bound)]
enum Bind {
    /// Asked of `Tcp`, not yet answered.
    Asked,
    /// Bound on `port`, its connections accepted from `listener`.
    Bound { listener: ActorRef<Listener>, port: u16 },
    /// Refused, or unanswered: the stack serves nothing.
    Unbound,
}

/// What a stack holds: its listener, an entry for each open connection, its pure layers' template, and the body streams
/// it serves and opens, for the stack `S`, whose layers are `T`.
#[derive(Bound)]
pub struct Serving<S, T: LayerList> {
    /// The stack itself, which a request's body handle names as its reader and its gatherer.
    me: ActorRef<S>,
    settings: StackSettings,
    /// What each connection's pure layers are made from.
    template: Layers<T>,
    bind: Bind,
    accepting: Streams<Accepting, ()>,
    /// The stream of each open connection, each keeping the connection's number.
    reading: Streams<Reading, u64>,
    /// Each open connection, by its number.
    connections: BTreeMap<u64, Connection<T::Values>>,
    /// How many connections it has accepted: the number of the next.
    accepted: u64,
    /// How many requests it has taken in through its layers: the number of the next.
    requests: u64,
    /// The streams its handlers read their requests' bodies through.
    readers: aethereal_actor::Serving<InboundBody>,
    /// The streams its responses' sources send their bodies through, each keeping the number of the connection it is
    /// written to.
    sources: Streams<OutboundBody, u64>,
}

/// The connections a stack's listener may accept ahead of the stack taking them: four.
const ACCEPTING_WINDOW: NonZeroU32 = NonZeroU32::MIN.saturating_add(3);

impl<S, T> Serving<S, T>
where
    S: Stack
        + Handles<Part<Accepting>>
        + Receives<Answer<Unbound, Number>>
        + Handles<Part<Reading>>
        + Receives<Answer<HungUp, Number>>
        + Receives<Answer<Slept, Deadline>>
        + Handles<Part<OutboundBody>>
        + Receives<Answer<SourceEnd, Number>>
        + Receives<Answer<Wrote, Sending>>
        + Receives<Answer<PartStep, ParkedPart>>,
    T: WalkedBy<S>,
{
    /// The stack's serving, its layers `layers`, as it begins: asks `Tcp` to listen as `settings` say, reading each
    /// connection in chunks of `chunk_bytes`. The stack is handed what `Tcp` answers, as [`Listened`]. It asks `Tcp` at
    /// its typed path, so it builds for a stack that depends on `Tcp`, at `I`.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    pub fn listen<I>(turn: &mut Beginning<'_, S>, settings: &StackSettings, layers: Layers<T>) -> Result<Self, Fault>
    where
        S: DependsOn<Tcp, I> + Receives<Answer<Listening, Binding>>,
    {
        let framing = Chunking::Chunks { most_bytes: settings.chunk_bytes };
        let listen = Listen { port: settings.port, interface: settings.interface.clone(), framing };
        turn.to(path::<Tcp>(())).ask(&listen, &Binding)?;
        let window = NonZeroU32::new(settings.window).unwrap_or(NonZeroU32::MIN);

        Ok(Self {
            me: turn.me(),
            settings: settings.clone(),
            template: layers,
            bind: Bind::Asked,
            accepting: Streams::new(ACCEPTING_WINDOW),
            reading: Streams::new(window),
            connections: BTreeMap::new(),
            accepted: 0,
            requests: 0,
            readers: aethereal_actor::Serving::new(),
            sources: Streams::new(window),
        })
    }

    /// The pure layer of type `L` in the template a new connection's layers are made from, found at its place, `I`,
    /// which the compiler infers. A change applies to the connections made after it.
    pub fn layer_mut<L, I>(&mut self) -> &mut L
    where
        L: Layer,
        T: Lists<L, I>,
        T::Forms: Slot<I, Item = L>,
    {
        self.template.layer_mut()
    }

    /// The port it listens on: none until `Tcp` has bound it, or when `Tcp` refused.
    #[must_use]
    pub const fn port(&self) -> Option<u16> {
        match self.bind {
            Bind::Bound { port, .. } => Some(port),
            Bind::Asked | Bind::Unbound => None,
        }
    }

    /// The bytes it holds of what its connections have read and it has not yet parsed or decoded: what it has read
    /// ahead of its requests and their bodies' readers.
    #[must_use]
    pub fn held_bytes(&self) -> usize {
        self.connections.values().map(|connection| connection.buffer.len()).sum()
    }

    /// How many connections it holds open, those draining after their last response among them.
    #[must_use]
    pub fn connections(&self) -> usize {
        self.connections.len()
    }

    /// `Tcp` answered the listen: once bound, the stack opens its listener's stream of what it accepts. What it answered
    /// otherwise is handed back, for the stack to decide on.
    ///
    /// # Errors
    ///
    /// The fault of an open that does not encode.
    pub fn listened(
        &mut self,
        (answered, Binding): (Answered<Listening>, Binding),
        turn: &mut Turn<'_, S>,
    ) -> Result<Listened, Fault> {
        let listened = match answered {
            Answered::Yes(Listening::Bound { listener, port }) => {
                self.accepting.open(turn, listener.protocol(), Accept, ())?;
                self.bind = Bind::Bound { listener, port };
                return Ok(Listened::Bound { port });
            }
            Answered::Yes(refused) => Listened::Refused(refused),
            Answered::No(why) => Listened::Unanswered(why),
        };
        self.bind = Bind::Unbound;

        Ok(listened)
    }

    /// A connection was accepted: it is given an entry, its pure layers made from the template, and its stream is
    /// opened. Past `most_connections`, it is shut at once.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn accept(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let Some(Taken { value: Incoming { connection: accepted, peer }, .. }) = self.accepting.part(turn, part)?
        else {
            return Ok(());
        };
        let is_full = self.connections.len() >= usize::try_from(self.settings.most_connections).unwrap_or(usize::MAX);
        if is_full {
            turn.to(accepted).send(&Shut)?;
            return Ok(());
        }

        let number = self.accepted;
        self.accepted += 1;
        let reader = self.reading.open(turn, accepted.protocol(), Read, number)?;
        let mut connection = Connection::new(accepted, peer, reader, self.template.made(), turn.now());
        connection.time(number, &self.settings, turn)?;
        self.connections.insert(number, connection);

        Ok(())
    }

    /// The listener's stream has ended. Asked to stop, it shuts every connection, and closes. Otherwise its socket
    /// failed, or it stopped unanswered, and the stack's port is closed: the stack fails, so a stack that is a root
    /// stops its program as failed (ADR-0020).
    ///
    /// # Errors
    ///
    /// [`Unlistened`] when the listener stopped without being asked to, and the fault of mail that does not encode.
    pub fn unbound(
        &mut self,
        (answered, number): (Answered<Unbound>, Number),
        turn: &mut Turn<'_, S>,
    ) -> Result<(), Fault> {
        self.accepting.answered(number);
        self.bind = Bind::Unbound;
        if answered != Answered::Yes(Unbound::Shut) {
            turn.log(Level::Error, format_args!("the stack's listener stopped: {answered:?}"));
            return Err(Fault::new(&Unlistened)?);
        }

        self.shut(turn)
    }

    /// A connection read a chunk: it is buffered, and what it completes is parsed and passed on. While a request is out
    /// on the connection, or its response is being written, the chunk's credit is withheld, so a client that sends
    /// ahead waits in the kernel. A connection draining after its last response drops the chunk, and grants its credit
    /// back.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn read<Js>(&mut self, part: Part<Reading>, turn: &mut Turn<'_, S>) -> Result<(), Fault>
    where
        S: Conforms<Gathering> + Conforms<Streamed<InboundBody>>,
        Reaches<T>: AskedBy<S, T::Attached, Js>,
    {
        let Some(Taken { number: stream, value, kept }) = self.reading.part_held(turn, part) else {
            return Ok(());
        };
        let number = *kept;
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        if let Phase::Draining(draining) = &mut connection.phase {
            draining.read(turn.now());
            self.reading.grant(turn, stream, NonZeroU32::MIN)?;
            return Ok(());
        }
        connection.buffer.extend_from_slice(turn.read(&value.bytes));
        if connection.is_held() {
            connection.withheld += 1;
        } else {
            self.reading.grant(turn, stream, NonZeroU32::MIN)?;
        }

        self.advance(number, turn)
    }

    /// A connection's stream has ended, as `Tcp` says how: a body being read from it ends `HungUp`, a response's source
    /// is stopped, and its entry is dropped.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode.
    pub fn hung_up(
        &mut self,
        (answered, number): (Answered<HungUp>, Number),
        turn: &mut Turn<'_, S>,
    ) -> Result<(), Fault> {
        let Some(connection) = self.reading.answered(number) else {
            return Ok(());
        };
        let how = match answered {
            Answered::Yes(how) => how,
            Answered::No(_) => HungUp::Reset,
        };

        self.close(connection, Ending::HungUp(how), turn)
    }

    /// A write was answered: a streamed response's head or parts go on as [`flowed`](Self::flowed) says; otherwise the
    /// connection drains when it was its last, sent whole, and closes when the write was not sent whole.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn wrote<Js>(
        &mut self,
        (answered, Sending { connection, carries }): (Answered<Wrote>, Sending),
        turn: &mut Turn<'_, S>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering> + Conforms<Streamed<InboundBody>>,
        Reaches<T>: AskedBy<S, T::Attached, Js>,
    {
        let closes = match carries {
            Carries::Response { closes } => closes,
            Carries::Body { parts } => return self.flowed(connection, &answered, parts, turn),
        };
        let is_sent = answered == Answered::Yes(Wrote::Sent);
        if !is_sent {
            return self.close(connection, Ending::HungUp(HungUp::Shut), turn);
        }
        if !closes {
            return Ok(());
        }

        self.drain(connection, turn)
    }

    /// The handler answered a request: its response goes out through every layer, in reverse, and is written. A
    /// handler that has ended is `503`, and one that failed, let the request go, or did not answer, `500`. A body it
    /// answered before the end of is left unread: its gather is answered `Busy`, and its stream ends `Cut`. An answer
    /// to a request the connection no longer waits on is passed over.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn answered<Js>(
        &mut self,
        (answered, Answering { connection: number, request }): (Answered<Response>, Answering),
        turn: &mut Turn<'_, S>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering> + Conforms<Streamed<InboundBody>>,
        Reaches<T>: AskedBy<S, T::Attached, Js>,
    {
        let Some(connection) = self.connections.get_mut(&number) else {
            turn.log(Level::Info, "an answer came for a connection that has closed, and was passed over");
            return Ok(());
        };
        if !connection.is_out(request) {
            turn.log(Level::Info, "an answer came to a request no longer waited on, and was passed over");
            return Ok(());
        }
        let Phase::Out(Out { head, owed, body, .. }) = mem::replace(&mut connection.phase, Phase::Closing) else {
            return Ok(());
        };

        // A body its handler did not read whole, or whose part an actor layer holds, is left unread, so where the next
        // request starts is not known.
        let cut = match body {
            Inflow::Bodiless { .. } | Inflow::Read { .. } => false,
            Inflow::Unread { .. } => true,
            Inflow::Gathering { held, .. } => {
                turn.answer(held, &Gathered::Busy)?;
                true
            }
            Inflow::Streaming { reader, .. } => {
                self.finish_reader(reader, BodyRead::Cut, turn)?;
                true
            }
        };
        let owed = Owed { closes: owed.closes || cut, ..owed };
        let response = match answered {
            Answered::Yes(response) => response,
            Answered::No(Unanswered::Ended) => Response::empty(Status::ServiceUnavailable),
            Answered::No(why) => {
                turn.log(Level::Warn, format!("the handler did not answer a request: {why:?}"));
                Response::empty(Status::InternalServerError)
            }
        };
        self.respond(number, Returning::new(request, head, response, owed, T::LAYERS), turn)?;

        self.settle(number, turn)
    }

    /// The sleep that times a connection's phase has come, when it still stands: a request that did not come, or came
    /// too slowly, a body that stalled, either way, close the connection with no answer, its streams ended or stopped;
    /// a request whose answer is late is answered `504`, and the connection closes; and a drain past its bound closes
    /// the connection, its client still not closed.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn slept(
        &mut self,
        (slept, deadline): (Answered<Slept>, Deadline),
        turn: &mut Turn<'_, S>,
    ) -> Result<(), Fault> {
        let number = deadline.connection;
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        if !connection.is_due(deadline) {
            return Ok(());
        }
        connection.woke();
        if let Answered::No(why) = slept {
            turn.log(Level::Warn, format!("a deadline's sleep was not answered: {why:?}"));
            return Ok(());
        }

        // A body is late once it has not moved for `stall_within_millis`, which a sleep run before it last moved has
        // not yet come to: it sleeps again until then.
        let stall = Duration::from_millis(u64::from(self.settings.stall_within_millis));
        let now = turn.now();
        let late = match &connection.phase {
            // A request, or a part of a body, parked at an actor layer is timed by its ask, and a connection closing
            // by its last write.
            Phase::Entering(_)
            | Phase::Returning(_)
            | Phase::Closing
            | Phase::Out(Out {
                body:
                    Inflow::Gathering { passing: Passing::Parked { .. }, .. }
                    | Inflow::Streaming { passing: Passing::Parked { .. }, .. },
                ..
            })
            | Phase::Outflow(Outflow { passing: Passing::Parked { .. }, .. }) => return Ok(()),
            Phase::Out(Out { body: Inflow::Gathering { moved, .. } | Inflow::Streaming { moved, .. }, .. })
                if now < *moved + stall =>
            {
                Late::Not
            }
            Phase::Outflow(outflow) if now < outflow.moved + stall => Late::Not,
            Phase::Draining(draining) if !draining.is_over(now, &self.settings) => Late::Not,
            Phase::Draining(_) => Late::Drained,
            Phase::Idle { .. }
            | Phase::Head { .. }
            | Phase::Out(Out {
                body: Inflow::Unread { .. } | Inflow::Gathering { .. } | Inflow::Streaming { .. },
                ..
            })
            | Phase::Outflow(_) => Late::Close,
            Phase::Out(Out { body: Inflow::Bodiless { .. } | Inflow::Read { .. }, .. }) => Late::Unanswered,
        };

        match late {
            Late::Not => connection.time(number, &self.settings, turn),
            Late::Close => {
                turn.log(Level::Info, "a connection's request or a body was late, and it closes");
                self.close(number, Ending::Stalled, turn)
            }
            Late::Unanswered => self.time_out(number, turn),
            Late::Drained => {
                turn.log(Level::Info, "a connection's client did not close within its drain, and it closes");
                self.close(number, Ending::HungUp(HungUp::Shut), turn)
            }
        }
    }

    /// The stack was sent `Shut`, or its listener stopped: it shuts its listener and every connection, ends each body
    /// being read `HungUp`, stops each source it has open, and closes.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode.
    pub fn shut(&mut self, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        if let Bind::Bound { listener, .. } = self.bind {
            turn.to(listener).send(&Shut)?;
        }
        let numbers: Vec<u64> = self.connections.keys().copied().collect();
        for number in numbers {
            self.close(number, Ending::HungUp(HungUp::Shut), turn)?;
        }
        turn.close();

        Ok(())
    }

    /// Parses what the connection numbered `number` has buffered, as far as it goes: each whole head is passed in
    /// through the layers, and on to the handler or answered by a layer; a body being gathered or streamed is read on.
    /// A request refused is answered, and the connection closes once the answer is sent.
    fn advance<Js>(&mut self, number: u64, turn: &mut Turn<'_, S>) -> Result<(), Fault>
    where
        S: Conforms<Gathering> + Conforms<Streamed<InboundBody>>,
        Reaches<T>: AskedBy<S, T::Attached, Js>,
    {
        loop {
            let Some(connection) = self.connections.get_mut(&number) else {
                return Ok(());
            };
            match connection.phase {
                Phase::Idle { .. } | Phase::Head { .. } => {}
                Phase::Out(Out { body: Inflow::Gathering { .. }, .. }) => return self.gathered(number, turn),
                Phase::Out(Out { body: Inflow::Streaming { .. }, .. }) => return self.stream(number, turn),
                Phase::Entering(_)
                | Phase::Out(_)
                | Phase::Returning(_)
                | Phase::Outflow(_)
                | Phase::Closing
                | Phase::Draining(_) => {
                    return Ok(());
                }
            }

            // A stack takes no limit on a body: its reader's gather names one.
            match request_head::parse(&connection.buffer, self.settings.most_head_bytes) {
                Parsed::More => {
                    let started = matches!(connection.phase, Phase::Idle { .. }) && !connection.buffer.is_empty();
                    if started {
                        connection.enter(Phase::Head { since: turn.now() }, number, &self.settings, turn)?;
                    }
                    return Ok(());
                }
                Parsed::Refused(refusal) => return self.refuse(number, refusal, turn),
                Parsed::Head { head, consumed } => {
                    connection.buffer.drain(..consumed);
                    self.enter(number, head, turn)?;
                }
            }
        }
    }

    /// Passes the request `parsed` in through the layers of the connection numbered `number`: on to the handler when
    /// every layer passes it, back out from the layer that answers it, or parked at the actor layer asked about it.
    /// Credentials sent twice, or that do not decode, are refused.
    fn enter<Js>(&mut self, number: u64, parsed: RequestHead, turn: &mut Turn<'_, S>) -> Result<(), Fault>
    where
        S: Conforms<Gathering> + Conforms<Streamed<InboundBody>>,
        Reaches<T>: AskedBy<S, T::Attached, Js>,
    {
        let RequestHead { method, path, query, mut headers, framing, closes, expects_continue, chunks, .. } = parsed;
        let Ok(credentials) = Credentials::taken(&mut headers) else {
            return self.refuse(number, Refusal::from(Malformed), turn);
        };
        let due = self.due(turn);
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let request = self.requests;
        self.requests += 1;
        let head = Head { method, path, query, headers, credentials, peer: connection.peer };
        let has_body = framing != Framing::Empty;
        let reads = if chunks {
            Reads::Chunks
        } else {
            Reads::Close
        };
        let owed = Owed { closes, head_only: method == Method::Head, continues: expects_continue && has_body, reads };
        let entered = T::inbound(&mut connection.layers, &head, turn, Parked::new(number, request), due)?;

        self.went_in(number, Entering { request, head, framing, owed, at: 0 }, entered, turn)
    }

    /// The request `entering` has walked in as far as `entered` says: on to the last layer, back out from the layer that
    /// answered it, or parked at the actor layer asked about it.
    fn went_in<Js>(
        &mut self,
        number: u64,
        entering: Entering,
        entered: Entered<T::Attached>,
        turn: &mut Turn<'_, S>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering> + Conforms<Streamed<InboundBody>>,
        Reaches<T>: AskedBy<S, T::Attached, Js>,
    {
        match entered {
            Entered::Passed(attached) => self.ask(number, entering, attached, turn),
            Entered::Answered { at, response } => self.turn_back(number, entering, at, response, turn),
            Entered::Parked { at } => {
                let Some(connection) = self.connections.get_mut(&number) else {
                    return Ok(());
                };
                let parked = Entering { at: held(at), ..entering };
                connection.enter(Phase::Entering(parked), number, &self.settings, turn)
            }
        }
    }

    /// Settles the connection numbered `number` once a step of its request is taken: when its response is written and
    /// it is kept, it is granted the credit withheld while the request was out, and what it buffered is parsed.
    fn settle<Js>(&mut self, number: u64, turn: &mut Turn<'_, S>) -> Result<(), Fault>
    where
        S: Conforms<Gathering> + Conforms<Streamed<InboundBody>>,
        Reaches<T>: AskedBy<S, T::Attached, Js>,
    {
        let is_idle = self.connections.get(&number).is_some_and(Connection::is_idle);
        if !is_idle {
            return Ok(());
        }
        self.release(number, turn)?;

        self.advance(number, turn)
    }

    /// Asks the handler the request `entering` names reaches, with what the layers `attached`: the lone handler, or the
    /// one its route reaches, found where the route's captures lead. A path no route matches, a path routes match for
    /// other methods, a capture that does not read as its type, and one that leads to nothing live are the router's
    /// to answer, `404` or `405`, which goes back out through every layer. The handler is asked within no deadline: a
    /// body may take as long as it keeps moving, which the stack times itself, and once it is read the stack times the
    /// answer (ADR-0041).
    fn ask<Js>(
        &mut self,
        number: u64,
        entering: Entering,
        attached: T::Attached,
        turn: &mut Turn<'_, S>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering> + Conforms<Streamed<InboundBody>>,
        Reaches<T>: AskedBy<S, T::Attached, Js>,
    {
        if !self.connections.contains_key(&number) {
            return Ok(());
        }
        let last = self.template.last();
        let (at, slices) = match last.picked(entering.head.method, &entering.head.path) {
            Picked::Handler { at, slices } => (at, slices),
            Picked::Answer(response) => return self.turn_back(number, entering, T::LAYERS, response, turn),
        };
        let Entering { request, head, framing, .. } = &entering;
        let unrouted = Unrouted {
            method: head.method,
            path: head.path.clone(),
            query: head.query.clone(),
            headers: head.headers.clone(),
            attached,
            body: BodyHandle::new(framing.length(), self.me.protocol(), self.me.protocol(), *request),
            peer: head.peer,
        };
        let context = Answering { connection: number, request: *request };
        let handed = last.reaches().ask_reached(turn, at, &slices, unrouted, &context)?;

        if handed == Handed::Unreached {
            let response = Response::empty(Status::NotFound);
            return self.turn_back(number, entering, T::LAYERS, response, turn);
        }
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Entering { request, head, framing, owed, .. } = entering;
        let since = turn.now();
        let inflow = match framing {
            Framing::Empty => Inflow::Bodiless { since },
            Framing::Length(_) | Framing::Chunked => Inflow::Unread { left: framing.body(), since },
        };

        connection.enter(Phase::Out(Out { request, head, owed, body: inflow }), number, &self.settings, turn)
    }

    /// Turns the request `entering` names back with `response`, from the layer `at`, which answered it, or from the
    /// last layer, its router's answer: the response goes out through the layers before it.
    fn turn_back(
        &mut self,
        number: u64,
        entering: Entering,
        at: usize,
        response: Response,
        turn: &mut Turn<'_, S>,
    ) -> Result<(), Fault> {
        let Entering { request, head, framing, owed, .. } = entering;
        // The body of a request answered before its handler is not read, so where the next request starts is not
        // known.
        let owed = Owed { closes: owed.closes || framing != Framing::Empty, ..owed };

        self.respond(number, Returning::new(request, head, response, owed, at), turn)
    }
}

/// Whether a connection's phase is late, as the sleep that timed it comes.
enum Late {
    /// Not yet: a body moved since the sleep was run.
    Not,
    /// It closes with no answer: a request that did not come, or came too slowly, or a body that stalled.
    Close,
    /// Its answer is late, and it is answered `504`.
    Unanswered,
    /// It drained past its bound, its client still not closed, and it closes.
    Drained,
}

impl<S, T> Serving<S, T>
where
    S: Receives<Answer<Slept, Deadline>>
        + Handles<Part<OutboundBody>>
        + Receives<Answer<SourceEnd, Number>>
        + Receives<Answer<Wrote, Sending>>
        + Receives<Answer<PartStep, ParkedPart>>,
    T: WalkedBy<S>,
{
    /// Answers a request refused before its layers, as `refusal` says, and closes once the answer is sent.
    fn refuse(&mut self, number: u64, refusal: Refusal, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        turn.log(Level::Info, format!("a request was refused: {refusal:?}"));
        let (status, _) = refusal.said();
        let (head, _) = Response::empty(status).into_parts();

        self.write(number, &head, respond::Unstreamed::Empty, Owed::CLOSING, turn)
    }

    /// Answers `504` to the request out on the connection numbered `number`, whose handler did not answer in time, and
    /// closes once it is sent: the answer, when it comes, is passed over.
    fn time_out(&mut self, number: u64, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { request, head, owed, .. }) = mem::replace(&mut connection.phase, Phase::Closing) else {
            return Ok(());
        };
        let response = Response::empty(Status::GatewayTimeout);

        let owed = Owed { closes: true, ..owed };

        self.respond(number, Returning::new(request, head, response, owed, T::LAYERS), turn)
    }

    /// When an ask of an actor layer made now is due: `answer_within_millis` from now.
    pub(super) fn due(&self, turn: &Turn<'_, S>) -> Time {
        turn.now() + Duration::from_millis(u64::from(self.settings.answer_within_millis))
    }

    /// Grants the connection numbered `number` the credit withheld while a request was out.
    fn release(&mut self, number: u64, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        if let Some(withheld) = NonZeroU32::new(connection.withheld) {
            self.reading.grant(turn, connection.reader, withheld)?;
        }
        connection.withheld = 0;

        Ok(())
    }

    /// Closes the connection numbered `number`: a body being read from it is ended as `ending` says, a response's
    /// source is stopped, and one not yet opened opened and stopped, its sleep is cancelled, its `Accepted` is sent
    /// `Shut`, and its entry, with its pure layers' values, is dropped.
    fn close(&mut self, number: u64, ending: Ending, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let Some(mut connection) = self.connections.remove(&number) else {
            return Ok(());
        };
        connection.unsleep(turn);
        match connection.phase {
            Phase::Out(Out { body, .. }) => self.end_inflow(body, ending, turn)?,
            Phase::Outflow(Outflow { source, .. }) => {
                self.sources.stop(turn, source)?;
            }
            Phase::Returning(Returning { body: ResponseBody::Streamed(streamed), .. }) => {
                outflow::abandon(&mut self.sources, number, streamed, turn)?;
            }
            Phase::Returning(_)
            | Phase::Idle { .. }
            | Phase::Head { .. }
            | Phase::Entering(_)
            | Phase::Closing
            | Phase::Draining(_) => {}
        }
        turn.to(connection.accepted).send(&Shut)?;

        Ok(())
    }

    /// Closes the connection numbered `number` in stages once its last response is sent, as RFC 9112 §9.6 does: its
    /// `Accepted` is sent `Finish`, which shuts its write half, so the client reads the response and then its end; and
    /// what the client still sends is read and dropped, the credit withheld for it granted, until the client closes or
    /// a bound of the drain passes (`drain_stall_within_millis`, `drain_within_millis`), when it is closed. A socket
    /// closed with bytes it has not read resets the connection, and the reset can destroy the response before the
    /// client reads it.
    fn drain(&mut self, number: u64, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        turn.to(connection.accepted).send(&Finish)?;
        connection.drain(number, &self.settings, turn)?;

        self.release(number, turn)
    }

    /// Closes the connection numbered `number`, which the engine had no room to write to (ADR-0014): the client is told
    /// nothing more.
    fn unstored(&mut self, number: u64, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        turn.log(Level::Warn, "the stack had no room to hold what it would write, and closes the connection");

        self.close(number, Ending::HungUp(HungUp::Shut), turn)
    }
}
