//! A request's body read whole for its handler's gather (ADR-0035), and any body's bytes passed through a stack's
//! layers, either way: a part at a time, then its end, each pure layer handed it in the stack's turn, and each actor
//! layer whose type lists the part's kind asked, no more of the body taken until it answers. How a body that is not
//! read to its end stops being read, gathered or streamed.

use super::connection::{Inflow, Out, Passing, Phase, Returning};
use super::{Carries, Deadline, Owed, Sending, Serving, Stepping};
use crate::body::{Body, Decoded, Progress};
use crate::pooled::put;
use crate::request_head::Refusal;
use crate::response_head;
use crate::stack::actor::{ParkedPart, Standing, Way, held};
use crate::stack::{
    BodyPart, Gather, Gathered, InboundPart, InboundParts, LayerList, PartStep, Passed, Refused, Response, Taker,
};
use crate::{BodyRead, OutboundBody, SourceEnd, Status};
use aethereal_actor::{
    ActorRef, Allows, Answer, Asks, Bytes, Doing, Fault, Guard, Handles, Level, Listed, Number, Part, Pool,
    ProtocolRef, Receives, Slept, StreamKey, ToSender, Turn,
};
use aethereal_tcp::{Accepted, HungUp, Write, Writing};
use core::mem;

/// What a body's bytes came to once every layer passed them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Passage {
    /// They are as they came: no layer wrote anything in their place.
    AsCame,
    /// The layers passed on what they wrote in their place, which is in the buffer at [`PASSED`].
    Rewritten,
}

/// Which of the buffers [`pass`] works in holds, once it returns [`Passage::Rewritten`], what the layers passed on in
/// place of a body's bytes.
pub(super) const PASSED: usize = 0;

/// Passes `bytes`, a part of a body, and then, when `ends`, the body's end, through each layer `order` names, in that
/// order, each handed it by `run`: each layer reads what the one before passed. What a layer writes at the end passes
/// the layers after it as a part, before their end. It works in `buffers`, which the stack takes together from its pool:
/// each layer writes into a spare one, which then changes places with the one it read, so nothing is allocated once
/// they are big enough. When the bytes are rewritten, what the layers passed is left in `buffers[PASSED]`.
///
/// # Errors
///
/// Why a layer stopped the body.
pub(super) fn pass<F>(
    bytes: &[u8],
    ends: bool,
    order: impl Iterator<Item = usize> + Clone,
    mut run: F,
    buffers: &mut [Vec<u8>; 3],
) -> Result<Passage, Refused>
where
    F: FnMut(usize, &mut BodyPart<'_>),
{
    let [passed, carried, spare] = buffers;
    let mut passage = Passage::AsCame;
    for at in order.clone() {
        let current = match passage {
            Passage::AsCame => bytes,
            Passage::Rewritten => passed.as_slice(),
        };
        if current.is_empty() {
            continue;
        }
        if step(current, false, at, &mut run, spare)? == Passage::Rewritten {
            mem::swap(passed, spare);
            passage = Passage::Rewritten;
        }
    }
    if !ends {
        return Ok(passage);
    }

    // What a layer writes at the end is carried through the layers after it, each reading what the one before passed:
    // a layer is handed it only when there is some, as it is handed no empty part.
    carried.clear();
    for at in order {
        if !carried.is_empty() {
            let carried_on = step(carried, false, at, &mut run, spare)?;
            if carried_on == Passage::Rewritten {
                mem::swap(carried, spare);
            }
        }
        if step(&[], true, at, &mut run, spare)? == Passage::Rewritten {
            carried.extend_from_slice(spare);
        }
    }
    if carried.is_empty() {
        return Ok(passage);
    }
    if passage == Passage::AsCame {
        passed.clear();
        passed.extend_from_slice(bytes);
    }
    passed.extend_from_slice(carried);

    Ok(Passage::Rewritten)
}

/// Hands the layer at `at` the part `bytes`, or the end when `end`, through `run`: whether it passed them on as they
/// came, or wrote in their place what `spare` then holds.
fn step<F>(bytes: &[u8], end: bool, at: usize, run: &mut F, spare: &mut Vec<u8>) -> Result<Passage, Refused>
where
    F: FnMut(usize, &mut BodyPart<'_>),
{
    spare.clear();
    let mut part = BodyPart::new(bytes, end, spare);
    run(at, &mut part);

    match part.passed() {
        Passed::AsCame => Ok(Passage::AsCame),
        Passed::Written => Ok(Passage::Rewritten),
        Passed::Refused(refused) => Err(refused),
    }
}

/// A part of a body as it passes the layers: where its bytes are.
pub(super) enum Carried {
    /// Where they came, as the walk is handed them: a gathered body's bytes past where the part starts.
    Came,
    /// As an actor layer passed them, or its source sent them.
    Blob(Bytes),
    /// As they were decoded, or as a pure layer wrote them in their place.
    Written(Vec<u8>),
}

/// Where a part of a body got to on its walk through the layers.
pub(super) enum Walk<P> {
    /// Every layer passed it: its bytes are where what it carries says.
    Through(Carried),
    /// The actor layer at `at`, which takes the body's parts through `parts`, is to be asked it as `bytes`, and the
    /// part parked until it answers.
    Asks { at: usize, parts: ProtocolRef<P>, bytes: Bytes },
    /// A layer stopped the body, as `Refused` says why.
    Refused(Refused),
    /// The engine had no room to put what an actor layer is to be asked.
    Unstored,
}

/// The layers of `values` a walk takes in `order`, each pure layer handed a part by `run`: a request's body in order,
/// or a response's in reverse.
pub(super) struct Layered<'v, V, O> {
    pub values: &'v mut V,
    pub order: O,
    pub run: fn(&mut V, usize, &mut BodyPart<'_>),
}

/// Walks a part of a body, `carried` where its bytes are and `came` the bytes it came as, through `layered`, the
/// body's end after it when `end`: each pure layer handed it in the stack's turn, reading what the one before passed,
/// in buffers the pool lends, and the first actor layer whose type takes the parts `P`, found by a cast, to be asked
/// it, its bytes put. A part of no bytes that is not the end passes an actor layer untouched, as it says nothing the
/// layer could answer.
pub(super) fn walk<T, P, O, Me, S, F>(
    layered: Layered<'_, T::Values, O>,
    came: &[u8],
    mut carried: Carried,
    end: bool,
    (doing, pool): (&mut Doing<'_, Me, S, F>, &mut Pool<'_>),
) -> Walk<P>
where
    T: LayerList,
    P: Listed,
    O: Iterator<Item = usize>,
    F: Guard<Me>,
{
    let Layered { values, order, run } = layered;
    for at in order {
        let reference = match T::taker(values, at) {
            Taker::Pure => {
                let bytes = match &carried {
                    Carried::Came => came,
                    Carried::Blob(blob) => doing.read(blob),
                    Carried::Written(written) => written.as_slice(),
                };
                let mut buffers = pool.buffers::<3>(bytes.len());
                let handed = |at, part: &mut BodyPart<'_>| run(values, at, part);
                match pass(bytes, end, at..=at, handed, &mut buffers) {
                    Ok(Passage::AsCame) => {}
                    // What a layer before wrote goes back to the pool in place of what this one wrote.
                    Ok(Passage::Rewritten) => match &mut carried {
                        Carried::Written(written) => mem::swap(written, &mut buffers[PASSED]),
                        Carried::Came | Carried::Blob(_) => {
                            carried = Carried::Written(mem::take(&mut buffers[PASSED]));
                        }
                    },
                    Err(refused) => return Walk::Refused(refused),
                }
                continue;
            }
            Taker::Actor(reference) => reference,
        };
        let Some(parts) = doing.cast::<P>(reference) else {
            continue;
        };

        let is_empty = match &carried {
            Carried::Came => came.is_empty(),
            Carried::Blob(blob) => doing.read(blob).is_empty(),
            Carried::Written(written) => written.is_empty(),
        };
        if is_empty && !end {
            continue;
        }
        let bytes = match carried {
            Carried::Came => {
                let mut buffer = pool.buffer(came.len());
                buffer.extend_from_slice(came);
                doing.put(buffer)
            }
            Carried::Blob(blob) => Ok(blob),
            Carried::Written(written) => doing.put(written),
        };
        let Ok(bytes) = bytes else {
            return Walk::Unstored;
        };

        return Walk::Asks { at, parts, bytes };
    }

    Walk::Through(carried)
}

/// How a request's body stops being read before its end, which its gather is answered with and its stream ended with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ending {
    /// It runs past what its gather takes, or a layer refused it as too large.
    TooLarge,
    /// Its chunked framing broke, or a layer refused it as malformed.
    Malformed,
    /// It has not moved for `stall_within_millis`, and the connection closes with no answer.
    Stalled,
    /// The connection hung up, as `HungUp` says, or the stack was shut.
    HungUp(HungUp),
    /// An actor layer did not answer a part of it.
    Unpassed,
}

impl Ending {
    /// What a gather of the body is answered.
    const fn gathered(self) -> Gathered {
        match self {
            Self::TooLarge => Gathered::TooLarge,
            Self::Malformed => Gathered::Malformed,
            Self::Stalled => Gathered::Stalled,
            Self::HungUp(how) => Gathered::HungUp(how),
            Self::Unpassed => Gathered::Unpassed,
        }
    }

    /// What a stream of the body is ended with.
    const fn read(self) -> BodyRead {
        match self {
            Self::TooLarge => BodyRead::TooLarge,
            Self::Malformed => BodyRead::Malformed,
            Self::Stalled => BodyRead::Stalled,
            Self::HungUp(how) => BodyRead::HungUp(how),
            Self::Unpassed => BodyRead::Unpassed,
        }
    }

    /// How a layer's refusal, `refused`, stops a body.
    pub(super) const fn refused(refused: Refused) -> Self {
        match refused {
            Refused::TooLarge => Self::TooLarge,
            Refused::Malformed => Self::Malformed,
        }
    }
}

/// What a read of a gathered body came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Read {
    /// More of it is to come.
    More,
    /// It is whole.
    Whole,
    /// It runs past the gather's most bytes, or a layer stopped it, as `Ending` says.
    Stopped(Ending),
    /// Its last part waits on an actor layer, or the connection has gone: nothing more is done now.
    Waits,
    /// The engine had no room to put what an actor layer is asked.
    Unstored,
}

impl Read {
    /// What a read whose last part was its end when `end`, and gathered `past` the most bytes when it is, came to.
    const fn of(end: bool, past: bool) -> Self {
        if past {
            Self::Stopped(Ending::TooLarge)
        } else if end {
            Self::Whole
        } else {
            Self::More
        }
    }
}

/// Where a request's body goes once its layers have passed a part of it.
enum Sink {
    /// Into its gather, the part at `start` of what is gathered.
    Gather { start: usize },
    /// On its reader's stream.
    Stream,
}

/// Tells the client of the connection numbered `number`, `accepted`, which waits to be told before it sends its body,
/// `100 Continue`, from the turn's pool: false, and nothing written, when the engine had no room to put it.
///
/// # Errors
///
/// The fault of a write that does not encode.
pub(super) fn write_continue<Me, Sends, I>(
    accepted: ActorRef<Accepted>,
    number: u64,
    turn: &mut Turn<'_, Me, Sends>,
) -> Result<bool, Fault>
where
    Sends: Allows<Asks<Writing, Write, Sending>, I>,
{
    let Ok(go_on) = put(turn, response_head::CONTINUE) else {
        return Ok(false);
    };
    let writing: ProtocolRef<Writing> = accepted.protocol();
    let sending = Sending { connection: number, carries: Carries::Response { closes: false } };
    turn.to(writing).ask(&Write { parts: vec![go_on] }, &sending)?;

    Ok(true)
}

impl<S, T: LayerList> Serving<S, T>
where
    S: Receives<Answer<Slept, Deadline>> + Handles<Part<OutboundBody>> + Receives<Answer<SourceEnd, Number>>,
{
    /// A handler asks for its request's body whole: the body of the request out on its connection, when nothing reads
    /// it yet, or none, answered `Busy` at once. A body whose length says it runs past `most_bytes` is refused before a
    /// byte is read; one with no length is gathered, and refused once it runs past. A client that waits for
    /// `100 Continue` is told it now.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn gather(&mut self, gather: Gather, turn: &mut Stepping<'_, S, T, ToSender<Gathered>>) -> Result<(), Fault> {
        let request = gather.request();
        let found = self.connections.iter_mut().find(|(_, connection)| connection.is_out(request));
        let Some((&number, connection)) = found else {
            return Ok(turn.reply(&Gathered::Busy)?);
        };
        let Phase::Out(out) = &mut connection.phase else {
            return Ok(turn.reply(&Gathered::Busy)?);
        };
        let left = match out.body {
            Inflow::Unread { left, .. } => left,
            Inflow::Gathering { .. } | Inflow::Streaming { .. } | Inflow::Read { .. } => {
                return Ok(turn.reply(&Gathered::Busy)?);
            }
            Inflow::Bodiless { since } => {
                let Ok(bytes) = turn.put(Vec::new()) else {
                    turn.reply(&Gathered::HungUp(HungUp::Shut))?;
                    return self.unstored(number, turn);
                };
                out.body = Inflow::Read { since };
                return Ok(turn.reply(&Gathered::Whole(bytes))?);
            }
        };
        let most = u64::from(gather.most_bytes);

        let is_too_large = matches!(left, Body::Length { left } if left > most);
        if is_too_large {
            turn.reply(&Gathered::TooLarge)?;
            out.owed.closes = true;
            out.body = Inflow::Read { since: turn.now() };
            return connection.time(number, &self.settings, turn);
        }

        let continues = out.owed.continues;
        let is_unstored = continues && !write_continue(connection.accepted, number, turn)?;
        if is_unstored {
            turn.reply(&Gathered::HungUp(HungUp::Shut))?;
            return self.unstored(number, turn);
        }
        let held = turn.hold()?;
        let room = match left {
            Body::Length { left } => left,
            Body::Chunked { .. } | Body::Close => u64::from(self.settings.chunk_bytes),
        };
        let gathered = Vec::from(turn.buffer(usize::try_from(room).unwrap_or(usize::MAX)));
        let passing = Passing::Through;
        out.body = Inflow::Gathering { left, gathered, most, held, moved: turn.now(), passing };
        connection.time(number, &self.settings, turn)?;

        self.gathered(number, turn)
    }

    /// Reads on the body the connection numbered `number` gathers, from what it has buffered: what is decoded passes
    /// through its layers in order, then, where the body ends, the end, and the gather is answered once. While only
    /// more bytes would take it further, the connection is granted the credit it withheld. While a part waits on an
    /// actor layer, nothing more is read.
    pub(super) fn gathered<Own>(&mut self, number: u64, turn: &mut Stepping<'_, S, T, Own>) -> Result<(), Fault> {
        let most_line = self.settings.most_head_bytes;
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { body: Inflow::Gathering { left, gathered, moved, passing, .. }, .. }) =
            &mut connection.phase
        else {
            return Ok(());
        };
        if let Passing::Parked { .. } = passing {
            return Ok(());
        }

        let start = gathered.len();
        let decoded = match left.decode(&connection.buffer, gathered, most_line) {
            Ok(decoded) => decoded,
            Err(broken) => return self.break_body(number, Refusal::from(broken), turn),
        };
        let Decoded { consumed, progress } = decoded;
        connection.buffer.drain(..consumed);
        if gathered.len() > start {
            *moved = turn.now();
        }
        let read = self.pass_in(number, 0, start, Carried::Came, progress == Progress::Done, turn)?;

        self.settle_gather(number, read, turn)
    }

    /// Passes a part of the body the connection numbered `number` gathers through its layers from the one at `from`,
    /// in order, its bytes where `carried` says and the body's end after them when `end`, its place in the gathered
    /// body `start`: each pure layer's way in reads what the one before passed, and an actor layer whose type lists
    /// [`InboundPart`] is asked, and the part parked until it answers. What the last layer passes is gathered.
    fn pass_in<Own>(
        &mut self,
        number: u64,
        from: usize,
        start: usize,
        carried: Carried,
        end: bool,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<Read, Fault> {
        let due = self.due(turn);
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(Read::Waits);
        };
        let Phase::Out(Out { request, body: Inflow::Gathering { gathered, most, passing, .. }, .. }) =
            &mut connection.phase
        else {
            return Ok(Read::Waits);
        };
        let (doing, pool) = turn.split();
        let layered = Layered { values: &mut connection.layers, order: from..T::LAYERS, run: T::inbound_body };
        let walked: Walk<InboundParts> =
            walk::<T, _, _, _, _, _>(layered, &gathered[start..], carried, end, (doing, pool));

        let carried = match walked {
            Walk::Through(carried) => carried,
            Walk::Asks { at, parts, bytes } => {
                gathered.truncate(start);
                *passing = Passing::Parked { at: held(at), end };
                let standing = Standing { connection: number, request: *request, at: held(at) };
                let asked = InboundPart { connection: number, bytes, end };
                doing.to(parts).within(due).ask(&asked, &ParkedPart::new(standing, Way::In))?;
                connection.time(number, &self.settings, turn)?;
                return Ok(Read::Waits);
            }
            Walk::Refused(refused) => return Ok(Read::Stopped(Ending::refused(refused))),
            Walk::Unstored => return Ok(Read::Unstored),
        };
        match carried {
            Carried::Came => {}
            Carried::Blob(blob) => {
                gathered.truncate(start);
                gathered.extend_from_slice(doing.read(&blob));
            }
            Carried::Written(written) => {
                gathered.truncate(start);
                gathered.extend_from_slice(&written);
            }
        }
        let past = u64::try_from(gathered.len()).unwrap_or(u64::MAX) > *most;

        Ok(Read::of(end, past))
    }

    /// Goes on with the body the connection numbered `number` gathers as `read` says: grants the credit it withheld
    /// while more bytes are all it needs, or answers the gather.
    fn settle_gather<Own>(&mut self, number: u64, read: Read, turn: &mut Stepping<'_, S, T, Own>) -> Result<(), Fault> {
        match read {
            Read::More => self.release(number, turn),
            Read::Whole => self.answer_gather(number, turn),
            Read::Stopped(ending) => self.stop_body(number, ending, turn),
            Read::Waits => Ok(()),
            Read::Unstored => self.unstored(number, turn),
        }
    }

    /// The actor layer a part of the body of the request out on the connection numbered `number` waits on, which
    /// `standing` names, answered as `answered` says: the bytes it passed go on through the layers after it, into the
    /// gather or on the stream, and the body is read on; its refusal stops the body; and when it did not answer, the
    /// body ends `Unpassed`, and the stack answers the request itself with `status`.
    pub(super) fn passed_in<Own>(
        &mut self,
        standing: Standing,
        answered: Result<PartStep, Status>,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        let number = standing.connection;
        let now = turn.now();
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { body, .. }) = &mut connection.phase else {
            return Ok(());
        };
        let (sink, moved, passing) = match body {
            Inflow::Gathering { gathered, moved, passing, .. } => {
                (Sink::Gather { start: gathered.len() }, moved, passing)
            }
            Inflow::Streaming { moved, passing, .. } => (Sink::Stream, moved, passing),
            Inflow::Bodiless { .. } | Inflow::Unread { .. } | Inflow::Read { .. } => return Ok(()),
        };
        let Passing::Parked { end, .. } = *passing else {
            return Ok(());
        };
        *passing = Passing::Through;
        *moved = now;
        connection.time(number, &self.settings, turn)?;

        let bytes = match answered {
            Ok(PartStep::Pass(bytes)) => bytes,
            Ok(PartStep::Refuse(refused)) => return self.stop_body(number, Ending::refused(refused), turn),
            Err(status) => return self.end_body(number, status, Ending::Unpassed, turn),
        };
        let from = usize::try_from(standing.at).unwrap_or(usize::MAX).saturating_add(1);
        let start = match sink {
            Sink::Gather { start } => start,
            Sink::Stream => {
                let reads_on = self.stream_on(number, from, Carried::Blob(bytes), end, turn)?;
                if !reads_on {
                    return Ok(());
                }
                return self.stream(number, turn);
            }
        };
        let read = self.pass_in(number, from, start, Carried::Blob(bytes), end, turn)?;

        match read {
            Read::More => self.gathered(number, turn),
            Read::Whole | Read::Stopped(_) | Read::Waits | Read::Unstored => self.settle_gather(number, read, turn),
        }
    }

    /// Answers the gather of the connection numbered `number` with its body, whole and put, and awaits its handler's
    /// answer.
    fn answer_gather<Own>(&mut self, number: u64, turn: &mut Stepping<'_, S, T, Own>) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(out) = &mut connection.phase else {
            return Ok(());
        };
        let Inflow::Gathering { held, gathered, .. } = mem::replace(&mut out.body, Inflow::Read { since: turn.now() })
        else {
            return Ok(());
        };
        connection.time(number, &self.settings, turn)?;
        let Ok(bytes) = turn.put(gathered) else {
            turn.answer(held, &Gathered::HungUp(HungUp::Shut))?;
            return self.unstored(number, turn);
        };

        Ok(turn.answer(held, &Gathered::Whole(bytes))?)
    }

    /// The body of the request out on the connection numbered `number` stops being read, as `ending` says, which its
    /// gather is answered or its stream ended with: the rest is not read, and the connection closes once its handler's
    /// answer is written.
    pub(super) fn stop_body<Own>(
        &mut self,
        number: u64,
        ending: Ending,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        turn.log(Level::Info, format!("a request's body was not read to its end: {ending:?}"));
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(out) = &mut connection.phase else {
            return Ok(());
        };
        let body = mem::replace(&mut out.body, Inflow::Read { since: turn.now() });
        out.owed.closes = true;
        connection.time(number, &self.settings, turn)?;

        self.end_inflow(body, ending, turn)
    }

    /// The body of the request out on the connection numbered `number` broke its framing, as `refusal` says: the stack
    /// answers the request itself, as [`end_body`](Self::end_body) says.
    pub(super) fn break_body<Own>(
        &mut self,
        number: u64,
        refusal: Refusal,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        turn.log(Level::Info, format!("a request's body was refused: {refusal:?}"));
        let (status, _) = refusal.said();

        self.end_body(number, status, Ending::Malformed, turn)
    }

    /// The body of the request out on the connection numbered `number` will not be read: its gather is answered, or
    /// its stream ended, as `ending` says, and the stack answers the request itself with `status`, through its layers,
    /// and closes once the answer is sent. Its handler's answer, when it comes, is passed over.
    fn end_body<Own>(
        &mut self,
        number: u64,
        status: Status,
        ending: Ending,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { request, head, owed, body }) = mem::replace(&mut connection.phase, Phase::Closing) else {
            return Ok(());
        };
        self.end_inflow(body, ending, turn)?;
        let owed = Owed { closes: true, ..owed };

        self.respond(number, Returning::new(request, head, Response::empty(status), owed, T::LAYERS), turn)
    }

    /// Answers the gather of a body being gathered, or ends the stream of one being streamed, as `ending` says, as the
    /// body stops being read: a body that is not being read has no one to tell.
    pub(super) fn end_inflow<Own>(
        &mut self,
        body: Inflow,
        ending: Ending,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        match body {
            Inflow::Gathering { held, .. } => Ok(turn.answer(held, &ending.gathered())?),
            Inflow::Streaming { reader, .. } => self.finish_reader(reader, ending.read(), turn),
            Inflow::Bodiless { .. } | Inflow::Unread { .. } | Inflow::Read { .. } => Ok(()),
        }
    }

    /// Ends the stream `reader` with `done`, when it is still served.
    pub(super) fn finish_reader<Own>(
        &mut self,
        reader: StreamKey,
        done: BodyRead,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        if let Some(stream) = self.readers.take(reader) {
            stream.finish(turn, &done)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes each byte of a part twice, and `<0>` at the end.
    fn double(part: &mut BodyPart<'_>) {
        if part.is_end() {
            part.out().extend_from_slice(b"<0>");
            return;
        }
        let doubled: Vec<u8> = part.bytes().iter().flat_map(|&byte| [byte, byte]).collect();
        part.out().extend_from_slice(&doubled);
    }

    /// Passes every part and the end as they came.
    fn unchanged(_part: &mut BodyPart<'_>) {}

    /// Writes each part with `!` after it, and `<2>` at the end.
    fn shout(part: &mut BodyPart<'_>) {
        if part.is_end() {
            part.out().extend_from_slice(b"<2>");
            return;
        }
        let shouted = [part.bytes(), b"!"].concat();
        part.out().extend_from_slice(&shouted);
    }

    #[test]
    fn each_layer_reads_what_the_one_before_wrote_and_end_writes_follow() {
        // Catches a layer handed the part as it came, or what a layer before the last that wrote passed, instead of
        // what the one just before it passed, as a mix-up of the buffers that change places would; and what a layer
        // writes at the end not passed through the layers after it, or not following the body's last part.
        let layers: [fn(&mut BodyPart<'_>); 3] = [double, unchanged, shout];
        let mut buffers: [Vec<u8>; 3] = Default::default();

        let passed = pass(b"ab", true, 0..layers.len(), |at, part| layers[at](part), &mut buffers);

        assert_eq!((passed, buffers[PASSED].as_slice()), (Ok(Passage::Rewritten), &b"aabb!<0>!<2>"[..]));
    }
}
