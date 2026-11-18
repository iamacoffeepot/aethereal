//! A response's body streamed from its source (ADR-0035, ADR-0017): once its head is written, an [`OutboundBody`]
//! stream the stack opens on the source its handler named, each part taken without granting for it, passed back out
//! through the layers, queued, and written as the connection takes it, one write out at a time; the source is granted
//! a part back once the write that carried it is sent.

use super::body::{Carried, Ending, Layered, Walk, walk};
use super::connection::{Passing, Phase};
use super::respond::put_head;
use super::{Carries, Deadline, Owed, Reaches, Reads, Sending, Serving, StackSends, Stepping};
use crate::stack::actor::{ParkedPart, Standing, Way, held, place};
use crate::stack::chunk;
use crate::stack::pooled::put;
use crate::stack::response_head::Frame;
use crate::stack::{AsksEach, Gathering, LayerList, OutboundPart, OutboundParts, PartStep, ResponseHead, Stack};
use crate::stack::{InboundBody, OutboundBody, ResponseLength, SendBody, SourceEnd, StreamedBody};
use aethereal_actor::{
    Allows, Answer, Answered, Asks, Bytes, Conforms, Doing, Enough, Fault, Guard, Handles, Level, More, Number, Open,
    Part, Pool, ProtocolRef, Receives, Refused, Slept, Streamed, Streams, Taken, Time, ToAny, Turn,
};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{Accepting, HungUp, Reading, Unbound, Write, Writing, Wrote};
use core::mem;
use std::num::NonZeroU32;

/// A streamed response being written to its connection, its head written: what its source has sent, what its layers
/// have passed, and what the connection has sent.
#[derive(Bound)]
pub(super) struct Outflow {
    /// The request it answers, by which a part parked at an actor layer is found again.
    pub request: u64,
    /// The stream its source sends it through.
    pub source: Number,
    /// How it is delimited on the connection: by the length its head carried once every layer had passed it, in
    /// chunks, or by the close.
    pub frame: Frame,
    /// The layers its parts pass back out through, in reverse: those before this place.
    pub before: u32,
    /// The parts taken from the source and not yet passed through the layers, in order, none of them empty: no more
    /// than the source's window, as each is taken without granting for it.
    pub arrived: Vec<Bytes>,
    /// Where the part being passed stands among the layers.
    pub passing: Passing,
    /// What the layers passed, not yet sent, in order, none of it empty: a write out carries the first of it. Each is
    /// pinned until its write is sent.
    pub queued: Vec<Queued>,
    pub wire: Wire,
    pub making: Making,
    /// The bytes the layers have passed, which a sized body may not run past.
    pub framed: u64,
    /// The bytes the connection has sent.
    pub written: u64,
    /// When it last moved: its head written, a part taken, or a write sent.
    pub moved: Time,
    /// Whether the connection closes once it is written.
    pub closes: bool,
}

/// Bytes of a streamed response's body every layer passed, waiting to be sent, and the parts of its source's they are
/// granted back for once they are: one for a part the source sent, none for what the layers wrote at the body's end.
#[derive(Bound)]
pub(super) struct Queued {
    bytes: Bytes,
    credit: u32,
}

/// Whether a write of a streamed response is out: one at a time, so a write the connection is too full to take is
/// written again before any after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Wire {
    /// None: what is queued is written at once.
    Idle,
    /// One, carrying the first `parts` of what is queued, or the response's head when it carries none.
    Out { parts: u32 },
}

/// How far a streamed response's source has made its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Making {
    /// It still sends it.
    Sending,
    /// It has sent every byte, and the body's end has yet to pass the layers.
    Ended,
    /// The body's end has passed every layer: once what is queued is sent, the body is ended as its frame says.
    Passed,
}

impl Outflow {
    /// Whether a part of it is parked at the actor layer of the request `standing` names.
    pub(super) fn stands(&self, standing: Standing) -> bool {
        let at_layer = matches!(self.passing, Passing::Parked { at, .. } if at == standing.at);

        self.request == standing.request && at_layer
    }

    /// Whether a part of it is parked at an actor layer, which times it by its ask.
    pub(super) const fn is_parked(&self) -> bool {
        matches!(self.passing, Passing::Parked { .. })
    }
}

/// Opens and stops at once the stream of `streamed`, a response's body the connection numbered `number` will not
/// write: a source that waits for its open, or holds a child process for it, is told at once it will not be read.
///
/// # Errors
///
/// The fault of an open or a stop that does not encode.
pub(super) fn abandon<Me, Sends, IA, IM, IS>(
    sources: &mut Streams<OutboundBody, u64>,
    number: u64,
    streamed: StreamedBody,
    turn: &mut Turn<'_, Me, Sends>,
) -> Result<(), Fault>
where
    Me: Handles<Part<OutboundBody>> + Receives<Answer<SourceEnd, Number>>,
    Sends: Allows<Asks<Streamed<OutboundBody>, Open<OutboundBody>, Number>, IA>
        + Allows<ToAny<Streamed<OutboundBody>, More>, IM>
        + Allows<ToAny<Streamed<OutboundBody>, Enough>, IS>,
{
    let StreamedBody { source, body, .. } = streamed;
    let opened = sources.open(turn, source, SendBody { body }, number)?;
    sources.stop(turn, opened)?;

    Ok(())
}

/// The first `parts` of what `outflow` has queued, framed as its body is: each after its chunk's size line, a small
/// blob of its own from the pool, when it is chunked, and itself, not copied.
///
/// # Errors
///
/// The engine had no room to put a size line.
fn framed<Me, S, F: Guard<Me>>(
    outflow: &Outflow,
    parts: usize,
    (doing, pool): (&mut Doing<'_, Me, S, F>, &mut Pool<'_>),
) -> Result<Vec<Bytes>, Refused> {
    let chunked = outflow.frame == Frame::Chunked;
    let mut written = Vec::with_capacity(parts * 2);
    for (place, queued) in outflow.queued.iter().take(parts).enumerate() {
        if chunked {
            let after_written = outflow.written > 0;
            let after_queued = place > 0;
            let mut line = pool.buffer(chunk::LINE_BYTES);
            chunk::line(doing.read(&queued.bytes).len(), after_written || after_queued, &mut line);
            written.push(doing.put(line)?);
        }
        written.push(queued.bytes.clone());
    }

    Ok(written)
}

impl<S, T: LayerList> Serving<S, T>
where
    S: Receives<Answer<Slept, Deadline>> + Handles<Part<OutboundBody>> + Receives<Answer<SourceEnd, Number>>,
{
    /// Writes the head `head` of the streamed response to the request numbered `request` on the connection numbered
    /// `number`, as `owed` says, its body `streamed`, which passes back out through the layers before `before`: framed
    /// by the length its head carries once every layer has passed it, `Content-Length` when known, chunks to an
    /// HTTP/1.1 client when not, and the close to an HTTP/1.0 one. Then it opens the source's stream, and the
    /// connection enters `Outflow`. A response with no body to send, to a `HEAD` or of a status that has none, is
    /// written as its head alone, and its source's stream is opened and stopped at once.
    #[allow(clippy::too_many_arguments, reason = "a streamed response is handed on with what it is owed")]
    pub(super) fn send_streamed<Own>(
        &mut self,
        number: u64,
        request: u64,
        before: u32,
        mut head: ResponseHead,
        streamed: StreamedBody,
        owed: Owed,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        let frame = match head.take_length() {
            ResponseLength::Known(length) => Frame::Length(length),
            ResponseLength::Unknown => match owed.reads {
                Reads::Chunks => Frame::Chunked,
                Reads::Close => Frame::Close,
            },
        };
        let sends_body = head.status.has_body() && !owed.head_only;
        // A body that ends only with the close closes the connection; a head written alone ends where it does.
        let delimits = sends_body && frame == Frame::Close;
        let closes = owed.closes || delimits;
        let Ok(written) = put_head(turn, &head, frame, closes) else {
            abandon(&mut self.sources, number, streamed, turn)?;
            return self.unstored(number, turn);
        };
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let writing: ProtocolRef<Writing> = connection.accepted.protocol();

        if !sends_body {
            let sending = Sending { connection: number, carries: Carries::Response { closes } };
            turn.to(writing).ask(&Write { parts: vec![written] }, &sending)?;
            abandon(&mut self.sources, number, streamed, turn)?;
            let next = if closes {
                Phase::Closing
            } else {
                Phase::Idle { since: turn.now() }
            };
            return connection.enter(next, number, &self.settings, turn);
        }

        let sending = Sending { connection: number, carries: Carries::Body { parts: 0 } };
        turn.to(writing).ask(&Write { parts: vec![written] }, &sending)?;
        let StreamedBody { source, body, .. } = streamed;
        let opened = self.sources.open(turn, source, SendBody { body }, number)?;
        let outflow = Outflow {
            request,
            source: opened,
            frame,
            before,
            arrived: Vec::new(),
            passing: Passing::Through,
            queued: Vec::new(),
            wire: Wire::Out { parts: 0 },
            making: Making::Sending,
            framed: 0,
            written: 0,
            moved: turn.now(),
            closes,
        };

        connection.enter(Phase::Outflow(outflow), number, &self.settings, turn)
    }

    /// Passes the part `carried` of the streamed response on the connection numbered `number`, the body's end after
    /// it when `end`, back out through each layer before the one at `below`, in reverse: each pure layer's way out
    /// reads what the one after it passed, and an actor layer whose type lists [`OutboundPart`] is asked, and the part
    /// parked until it answers. What every layer passed is queued. Whether the next part may follow it: not while one
    /// is parked, nor once the connection has closed, as it does when a layer refuses the part or it runs past the
    /// body's length.
    fn flow_out<Own>(
        &mut self,
        number: u64,
        below: usize,
        carried: Carried,
        end: bool,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<bool, Fault> {
        let due = self.due(turn);
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(false);
        };
        let Phase::Outflow(outflow) = &mut connection.phase else {
            return Ok(false);
        };
        let (doing, pool) = turn.split();
        let layered = Layered { values: &mut connection.layers, order: (0..below).rev(), run: T::outbound_body };
        let walked: Walk<OutboundParts> = walk::<T, _, _, _, _, _>(layered, &[], carried, end, (doing, pool));

        let carried = match walked {
            Walk::Through(carried) => carried,
            Walk::Asks { at, parts, bytes } => {
                outflow.passing = Passing::Parked { at: held(at), end };
                let standing = Standing { connection: number, request: outflow.request, at: held(at) };
                let asked = OutboundPart { connection: number, bytes, end };
                doing.to(parts).within(due).ask(&asked, &ParkedPart::new(standing, Way::Out))?;
                connection.time(number, &self.settings, turn)?;
                return Ok(false);
            }
            Walk::Refused(refused) => {
                turn.log(Level::Info, format!("a layer refused a streamed response's body: {refused:?}"));
                self.close(number, Ending::HungUp(HungUp::Shut), turn)?;
                return Ok(false);
            }
            Walk::Unstored => {
                self.unstored(number, turn)?;
                return Ok(false);
            }
        };

        self.queue(number, carried, end, turn)
    }

    /// Queues what the layers passed of a part of the streamed response on the connection numbered `number`, or of its
    /// end when `end`, to be written: a part they passed as nothing is granted back to the source at once. A sized body
    /// that runs past its length closes the connection, cut short. Whether the next part may follow it.
    fn queue<Own>(
        &mut self,
        number: u64,
        carried: Carried,
        end: bool,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<bool, Fault> {
        let length = match &carried {
            Carried::Came => 0,
            Carried::Blob(bytes) => turn.read(bytes).len(),
            Carried::Written(written) => written.len(),
        };
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(false);
        };
        let Phase::Outflow(outflow) = &mut connection.phase else {
            return Ok(false);
        };
        if end {
            outflow.making = Making::Passed;
        }
        let credit = u32::from(!end);
        if length == 0 {
            if let Some(credit) = NonZeroU32::new(credit) {
                self.sources.grant(turn, outflow.source, credit)?;
            }
            return Ok(true);
        }

        outflow.framed = outflow.framed.saturating_add(u64::try_from(length).unwrap_or(u64::MAX));
        let is_long = matches!(outflow.frame, Frame::Length(length) if outflow.framed > length);
        if is_long {
            turn.log(Level::Info, "a streamed response's body ran past its length, and the connection closes");
            self.close(number, Ending::HungUp(HungUp::Shut), turn)?;
            return Ok(false);
        }
        let put = match carried {
            Carried::Blob(bytes) => Ok(bytes),
            Carried::Written(written) => turn.put(written),
            Carried::Came => return Ok(true),
        };
        let Ok(bytes) = put else {
            self.unstored(number, turn)?;
            return Ok(false);
        };
        outflow.queued.push(Queued { bytes, credit });

        Ok(true)
    }
}

impl<S, T> Serving<S, T>
where
    S: Stack
        + Handles<Part<Accepting>>
        + Receives<Answer<Unbound, Number>>
        + Handles<Part<Reading>>
        + Receives<Answer<HungUp, Number>>
        + Receives<Answer<Slept, Deadline>>
        + Handles<Part<OutboundBody>>
        + Receives<Answer<SourceEnd, Number>>,
    T: LayerList,
{
    /// A source sent a part of a streamed response's body: it is taken without granting for it, passed back out
    /// through the layers once the parts before it have, and written once the write before it is sent. A part of no
    /// bytes writes nothing, and would end a chunked body, so it is granted back at once.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn take_part<Is, Ks, Js>(
        &mut self,
        part: Part<OutboundBody>,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is> + Conforms<Streamed<InboundBody>, Ks>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        let Some(Taken { number: stream, value, kept }) = self.sources.part_held(turn, part) else {
            return Ok(());
        };
        let number = *kept;
        let is_empty = turn.read(&value.bytes).is_empty();
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Outflow(outflow) = &mut connection.phase else {
            return Ok(());
        };
        outflow.moved = turn.now();
        if is_empty {
            self.sources.grant(turn, stream, NonZeroU32::MIN)?;
            return Ok(());
        }
        outflow.arrived.push(value.bytes);

        self.pass_out(number, turn)
    }

    /// A source ended its stream: a body sent whole has its end passed back out through the layers, and is ended as
    /// its frame says once every byte is written. A source that failed, was stopped, or ended without answering
    /// closes the connection, since the head is already written and its status cannot change. A stream stopped as it
    /// opened, for a response with no body, ends here too, and is passed over.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn end_source<Is, Ks, Js>(
        &mut self,
        (answered, stream): (Answered<SourceEnd>, Number),
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is> + Conforms<Streamed<InboundBody>, Ks>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        let Some(number) = self.sources.answered(stream) else {
            return Ok(());
        };
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Outflow(outflow) = &mut connection.phase else {
            return Ok(());
        };
        if outflow.source != stream {
            return Ok(());
        }
        if answered == Answered::Yes(SourceEnd::Whole) {
            outflow.making = Making::Ended;
            outflow.moved = turn.now();
            return self.pass_out(number, turn);
        }

        turn.log(Level::Info, format!("a streamed response's source ended before its body did: {answered:?}"));
        self.close(number, Ending::HungUp(HungUp::Shut), turn)
    }

    /// An actor layer answered a part of the streamed response on the connection numbered `number`, which `standing`
    /// names, as `answered` says: what it passed goes on out through the layers before it, and the parts after it
    /// follow. One that refused it, or did not answer, closes the connection, the body cut short, and its source is
    /// stopped.
    pub(super) fn passed_out<Is, Ks, Js>(
        &mut self,
        standing: Standing,
        answered: Answered<PartStep>,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is> + Conforms<Streamed<InboundBody>, Ks>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        let number = standing.connection;
        let now = turn.now();
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Outflow(outflow) = &mut connection.phase else {
            return Ok(());
        };
        let Passing::Parked { at, end } = outflow.passing else {
            return Ok(());
        };
        outflow.passing = Passing::Through;
        outflow.moved = now;
        connection.time(number, &self.settings, turn)?;
        let Answered::Yes(PartStep::Pass(bytes)) = answered else {
            turn.log(Level::Info, format!("an actor layer did not pass a streamed response's part: {answered:?}"));
            return self.close(number, Ending::HungUp(HungUp::Shut), turn);
        };

        let flows = self.flow_out(number, place(at), Carried::Blob(bytes), end, turn)?;
        if !flows {
            return Ok(());
        }

        self.pass_out(number, turn)
    }

    /// A write of the streamed response on the connection numbered `number` is answered, carrying `parts` of what was
    /// queued, or its head when none: once sent, what it carried is granted back to the source and what is queued is
    /// written. Nothing of a write the connection is too full to take was taken, so it is written again, its first
    /// part alone; the connection holds nothing else of the response unsent, so a part that alone does not fit never
    /// will, and the connection closes. A write the connection hung up on closes it, and its source is stopped.
    pub(super) fn flowed<Is, Ks, Js>(
        &mut self,
        number: u64,
        answered: &Answered<Wrote>,
        parts: u32,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is> + Conforms<Streamed<InboundBody>, Ks>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Outflow(outflow) = &mut connection.phase else {
            return Ok(());
        };
        outflow.wire = Wire::Idle;
        match answered {
            Answered::Yes(Wrote::Sent) => {
                let sent = usize::try_from(parts).unwrap_or(usize::MAX).min(outflow.queued.len());
                let (bytes, credit) = outflow.queued.drain(..sent).fold((0, 0), |(bytes, credit), queued| {
                    (bytes + turn.read(&queued.bytes).len(), credit + queued.credit)
                });
                outflow.written = outflow.written.saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
                outflow.moved = turn.now();
                if let Some(credit) = NonZeroU32::new(credit) {
                    self.sources.grant(turn, outflow.source, credit)?;
                }
                self.pump(number, turn)
            }
            Answered::Yes(Wrote::Full) if parts > 1 => self.write_out(number, 1, turn),
            Answered::Yes(Wrote::Full | Wrote::HungUp | Wrote::Finished) | Answered::No(_) => {
                turn.log(Level::Info, format!("a streamed response's write was not sent: {answered:?}"));
                self.close(number, Ending::HungUp(HungUp::Shut), turn)
            }
        }
    }

    /// Passes the parts the source of the streamed response on the connection numbered `number` has sent back out
    /// through its layers, one at a time and in order, then its end once the source has ended it, and writes what
    /// they passed.
    fn pass_out<Is, Ks, Js>(&mut self, number: u64, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is> + Conforms<Streamed<InboundBody>, Ks>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        loop {
            let Some(connection) = self.connections.get_mut(&number) else {
                return Ok(());
            };
            let Phase::Outflow(outflow) = &mut connection.phase else {
                return Ok(());
            };
            if outflow.is_parked() {
                return Ok(());
            }
            let below = place(outflow.before);
            let (carried, end) = if !outflow.arrived.is_empty() {
                (Carried::Blob(outflow.arrived.remove(0)), false)
            } else if outflow.making == Making::Ended {
                (Carried::Written(Vec::new()), true)
            } else {
                break;
            };

            let flows = self.flow_out(number, below, carried, end, turn)?;
            if !flows {
                return Ok(());
            }
        }

        self.pump(number, turn)
    }

    /// Writes what is queued of the streamed response on the connection numbered `number` when no write is out; or,
    /// once the body's end has passed every layer and every byte is sent, ends the body as its frame says.
    fn pump<Is, Ks, Js>(&mut self, number: u64, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is> + Conforms<Streamed<InboundBody>, Ks>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        let Some(connection) = self.connections.get(&number) else {
            return Ok(());
        };
        let Phase::Outflow(outflow) = &connection.phase else {
            return Ok(());
        };
        if outflow.wire != Wire::Idle {
            return Ok(());
        }
        if !outflow.queued.is_empty() {
            return self.write_out(number, outflow.queued.len(), turn);
        }
        if outflow.making != Making::Passed {
            return Ok(());
        }

        self.finish_outflow(number, turn)
    }

    /// Writes the first `parts` of what is queued of the streamed response on the connection numbered `number` as one
    /// write, each framed as the body is.
    fn write_out<Own>(&mut self, number: u64, parts: usize, turn: &mut Stepping<'_, S, T, Own>) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Outflow(outflow) = &mut connection.phase else {
            return Ok(());
        };
        let Ok(written) = framed(outflow, parts, turn.split()) else {
            return self.unstored(number, turn);
        };
        let parts = u32::try_from(parts).unwrap_or(u32::MAX);
        outflow.wire = Wire::Out { parts };
        let writing: ProtocolRef<Writing> = connection.accepted.protocol();
        let sending = Sending { connection: number, carries: Carries::Body { parts } };
        turn.to(writing).ask(&Write { parts: written }, &sending)?;

        Ok(())
    }

    /// Ends the streamed response on the connection numbered `number`, every byte of it sent, as its frame says: a
    /// sized body that came short of its length closes the connection, cut short; a chunked body is ended with its last
    /// chunk; and a body delimited by the close is ended by it. The connection then takes its next request, or closes
    /// as its request said.
    fn finish_outflow<Is, Ks, Js>(&mut self, number: u64, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is> + Conforms<Streamed<InboundBody>, Ks>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Outflow(outflow) = mem::replace(&mut connection.phase, Phase::Closing) else {
            return Ok(());
        };
        let Outflow { frame, framed, written, closes, .. } = outflow;
        let ends = match frame {
            Frame::Length(length) if framed != length => {
                turn.log(Level::Info, "a streamed response's body came short of its length, and the connection closes");
                Ends::Close
            }
            Frame::Length(_) if closes => Ends::Close,
            Frame::Length(_) => Ends::Next,
            Frame::Chunked => Ends::LastChunk,
            Frame::Close => Ends::Close,
        };

        match ends {
            Ends::Close => self.close(number, Ending::HungUp(HungUp::Shut), turn),
            Ends::Next => {
                connection.enter(Phase::Idle { since: turn.now() }, number, &self.settings, turn)?;
                self.settle(number, turn)
            }
            Ends::LastChunk => {
                let Ok(last) = put(turn, chunk::last(written > 0)) else {
                    return self.unstored(number, turn);
                };
                let writing: ProtocolRef<Writing> = connection.accepted.protocol();
                let sending = Sending { connection: number, carries: Carries::Response { closes } };
                turn.to(writing).ask(&Write { parts: vec![last] }, &sending)?;
                let next = if closes {
                    Phase::Closing
                } else {
                    Phase::Idle { since: turn.now() }
                };
                connection.enter(next, number, &self.settings, turn)?;

                self.settle(number, turn)
            }
        }
    }
}

/// How a streamed response's body ends once every byte of it is sent.
enum Ends {
    /// With the close: delimited by it, or cut short.
    Close,
    /// Where its length says, and the connection takes its next request.
    Next,
    /// With its last chunk.
    LastChunk,
}
