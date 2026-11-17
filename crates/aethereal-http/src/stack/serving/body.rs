//! A request's body read whole for its handler's gather (ADR-0035), and any body's bytes passed through a stack's pure
//! layers: a part at a time, then its end. A part of a request's body that reaches an actor layer whose type lists
//! [`InboundPart`] is asked of it, and no more of the body is read until it answers.

use super::connection::{Inflow, Out, Passing, Phase, Returning};
use super::{Deadline, Owed, Sending, Serving, Stepping};
use crate::Status;
use crate::body::{Body, Decoded, Progress};
use crate::pooled::put;
use crate::request_head::Refusal;
use crate::response_head;
use crate::stack::actor::{ParkedPart, Standing, Way, held};
use crate::stack::{
    BodyPart, Gather, Gathered, InboundPart, InboundParts, LayerList, PartStep, Passed, Refused, Response, Taker,
};
use aethereal_actor::{Answer, Bytes, Fault, Level, ProtocolRef, Receives, Slept};
use aethereal_tcp::{HungUp, Write, Writing};
use core::mem;

/// What a body's bytes came to once every layer passed them.
pub(super) enum Passage {
    /// They are as they came: no layer wrote anything in their place.
    AsCame,
    /// What the layers passed on in their place.
    Rewritten(Vec<u8>),
}

/// Passes `bytes`, a part of a body, and then, when `ends`, the body's end, through each layer `order` names, in that
/// order, each handed it by `run`: each layer reads what the one before passed. What a layer writes at the end passes
/// the layers after it as a part, before their end. Each layer writes into `spare`; what one writes is taken out of it,
/// so the next writes into a buffer of its own.
///
/// # Errors
///
/// Why a layer stopped the body.
pub(super) fn pass<F>(
    bytes: &[u8],
    ends: bool,
    order: impl Iterator<Item = usize> + Clone,
    mut run: F,
    spare: &mut Vec<u8>,
) -> Result<Passage, Refused>
where
    F: FnMut(usize, &mut BodyPart<'_>),
{
    let mut passage = Passage::AsCame;
    for at in order.clone() {
        let current = match &passage {
            Passage::AsCame => bytes,
            Passage::Rewritten(rewritten) => rewritten.as_slice(),
        };
        if current.is_empty() {
            continue;
        }
        if let Passage::Rewritten(written) = step(current, false, at, &mut run, spare)? {
            passage = Passage::Rewritten(written);
        }
    }
    if !ends {
        return Ok(passage);
    }

    let mut carried = Vec::new();
    for at in order {
        let carried_on = if carried.is_empty() {
            Passage::AsCame
        } else {
            step(&carried, false, at, &mut run, spare)?
        };
        if let Passage::Rewritten(written) = carried_on {
            carried = written;
        }
        if let Passage::Rewritten(written) = step(&[], true, at, &mut run, spare)? {
            carried.extend_from_slice(&written);
        }
    }
    if carried.is_empty() {
        return Ok(passage);
    }
    let mut whole = match passage {
        Passage::AsCame => bytes.to_vec(),
        Passage::Rewritten(rewritten) => rewritten,
    };
    whole.extend_from_slice(&carried);

    Ok(Passage::Rewritten(whole))
}

/// Hands the layer at `at` the part `bytes`, or the end when `end`, through `run`: what it passed on, as they came, or
/// what it wrote in their place, taken out of `spare`.
fn step<F>(bytes: &[u8], end: bool, at: usize, run: &mut F, spare: &mut Vec<u8>) -> Result<Passage, Refused>
where
    F: FnMut(usize, &mut BodyPart<'_>),
{
    spare.clear();
    let mut part = BodyPart::new(bytes, end, spare);
    run(at, &mut part);

    match part.passed() {
        Passed::AsCame => Ok(Passage::AsCame),
        Passed::Written => Ok(Passage::Rewritten(mem::take(spare))),
        Passed::Refused(refused) => Err(refused),
    }
}

/// What a read of a gathered body came to.
enum Read {
    /// More of it is to come.
    More,
    /// It is whole.
    Whole,
    /// It runs past the gather's most bytes, or a layer stopped it: what the gather is answered.
    Stopped(Gathered),
    /// Its last part waits on an actor layer, or the connection has gone: nothing more is done now.
    Waits,
    /// The engine had no room to put what an actor layer is asked.
    Unstored,
}

impl Read {
    /// What a read whose last part was its end when `end`, and gathered `past` the most bytes when it is, came to.
    const fn of(end: bool, past: bool) -> Self {
        if past {
            Self::Stopped(Gathered::TooLarge)
        } else if end {
            Self::Whole
        } else {
            Self::More
        }
    }
}

/// A part of a request's body, as it passes the layers: where its bytes are.
enum Carried {
    /// As they were decoded: the gathered body past where the part starts.
    Came,
    /// As an actor layer passed them.
    Blob(Bytes),
    /// As a pure layer wrote them in their place.
    Written(Vec<u8>),
}

impl<S, T: LayerList> Serving<S, T>
where
    S: Receives<Answer<Slept, Deadline>>,
{
    /// A handler asks for its request's body whole: the body of the request out on its connection, when no gather
    /// reads it yet, or none, answered `Busy` at once. A body whose length says it runs past `most_bytes` is refused
    /// before a byte is read; one with no length is gathered, and refused once it runs past. A client that waits for
    /// `100 Continue` is told it now.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn gather(&mut self, gather: Gather, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault> {
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
            Inflow::Gathering { .. } | Inflow::Read { .. } => return Ok(turn.reply(&Gathered::Busy)?),
            Inflow::Bodiless { .. } => {
                let Ok(bytes) = turn.put(Vec::new()) else {
                    turn.reply(&Gathered::HungUp(HungUp::Shut))?;
                    return self.unstored(number, turn);
                };
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

        if out.owed.continues {
            let Ok(go_on) = put(turn, response_head::CONTINUE) else {
                turn.reply(&Gathered::HungUp(HungUp::Shut))?;
                return self.unstored(number, turn);
            };
            let writing: ProtocolRef<Writing> = connection.accepted.protocol();
            turn.to(writing).ask(&Write { parts: vec![go_on] }, &Sending { connection: number, closes: false })?;
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
    pub(super) fn gathered(&mut self, number: u64, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault> {
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
    fn pass_in(
        &mut self,
        number: u64,
        from: usize,
        start: usize,
        mut carried: Carried,
        end: bool,
        turn: &mut Stepping<'_, S, T>,
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
        for at in from..T::LAYERS {
            let reference = match T::taker(&connection.layers, at) {
                Taker::Pure => {
                    let mut spare = pool.buffer(0);
                    let bytes = match &carried {
                        Carried::Came => &gathered[start..],
                        Carried::Blob(blob) => doing.read(blob),
                        Carried::Written(written) => written.as_slice(),
                    };
                    let run = |at, part: &mut BodyPart<'_>| T::inbound_body(&mut connection.layers, at, part);
                    let passed = pass(bytes, end, at..=at, run, &mut spare);
                    drop(spare);
                    match passed {
                        Ok(Passage::AsCame) => {}
                        Ok(Passage::Rewritten(rewritten)) => carried = Carried::Written(rewritten),
                        Err(Refused::TooLarge) => return Ok(Read::Stopped(Gathered::TooLarge)),
                        Err(Refused::Malformed) => return Ok(Read::Stopped(Gathered::Malformed)),
                    }
                    continue;
                }
                Taker::Actor(reference) => reference,
            };
            let Some(parts) = doing.cast::<InboundParts>(reference) else {
                continue;
            };

            // A part of no bytes that is not the end says nothing a layer could answer.
            let is_empty = match &carried {
                Carried::Came => gathered.len() == start,
                Carried::Blob(blob) => doing.read(blob).is_empty(),
                Carried::Written(written) => written.is_empty(),
            };
            if is_empty && !end {
                continue;
            }

            let bytes = match carried {
                Carried::Came => {
                    let mut buffer = pool.buffer(gathered.len() - start);
                    buffer.extend_from_slice(&gathered[start..]);
                    doing.put(buffer)
                }
                Carried::Blob(blob) => Ok(blob),
                Carried::Written(written) => doing.put(written),
            };
            let Ok(bytes) = bytes else {
                return Ok(Read::Unstored);
            };
            gathered.truncate(start);
            *passing = Passing::Parked { at: held(at), end };
            let standing = Standing { connection: number, request: *request, at: held(at) };
            let asked = InboundPart { connection: number, bytes, end };
            doing.to(parts).within(due).ask(&asked, &ParkedPart::new(standing, Way::In))?;
            connection.time(number, &self.settings, turn)?;
            return Ok(Read::Waits);
        }

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
    fn settle_gather(&mut self, number: u64, read: Read, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault> {
        match read {
            Read::More => self.release(number, turn),
            Read::Whole => self.answer_gather(number, turn),
            Read::Stopped(why) => self.stop_gather(number, &why, turn),
            Read::Waits => Ok(()),
            Read::Unstored => self.unstored(number, turn),
        }
    }

    /// The actor layer a part of the body the connection numbered `number` gathers waits on, which `standing` names,
    /// answered as `answered` says: the bytes it passed go on through the layers after it, and the body is read on;
    /// its refusal stops the gather; and when it did not answer, the gather is answered `Unpassed`, and the stack
    /// answers the request itself with `status`.
    pub(super) fn passed_in(
        &mut self,
        standing: Standing,
        answered: Result<PartStep, Status>,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault> {
        let number = standing.connection;
        let now = turn.now();
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { body: Inflow::Gathering { gathered, moved, passing, .. }, .. }) = &mut connection.phase
        else {
            return Ok(());
        };
        let Passing::Parked { end, .. } = *passing else {
            return Ok(());
        };
        *passing = Passing::Through;
        *moved = now;
        let start = gathered.len();
        connection.time(number, &self.settings, turn)?;

        let bytes = match answered {
            Ok(PartStep::Pass(bytes)) => bytes,
            Ok(PartStep::Refuse(Refused::TooLarge)) => {
                return self.stop_gather(number, &Gathered::TooLarge, turn);
            }
            Ok(PartStep::Refuse(Refused::Malformed)) => {
                return self.stop_gather(number, &Gathered::Malformed, turn);
            }
            Err(status) => return self.end_body(number, status, &Gathered::Unpassed, turn),
        };
        let from = usize::try_from(standing.at).unwrap_or(usize::MAX).saturating_add(1);
        let read = self.pass_in(number, from, start, Carried::Blob(bytes), end, turn)?;

        match read {
            Read::More => self.gathered(number, turn),
            Read::Whole | Read::Stopped(_) | Read::Waits | Read::Unstored => self.settle_gather(number, read, turn),
        }
    }

    /// Answers the gather of the connection numbered `number` with its body, whole and put, and awaits its handler's
    /// answer.
    fn answer_gather(&mut self, number: u64, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { body: Inflow::Gathering { gathered, .. }, .. }) = &mut connection.phase else {
            return Ok(());
        };
        let Ok(bytes) = turn.put(mem::take(gathered)) else {
            return self.unstored(number, turn);
        };

        self.end_gather(number, &Gathered::Whole(bytes), false, turn)
    }

    /// Answers the gather of the connection numbered `number` with `why` it stopped: the rest of the body is not read,
    /// and the connection closes once its handler's answer is written.
    fn stop_gather(&mut self, number: u64, why: &Gathered, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault> {
        turn.log(Level::Info, format!("a request's body was not gathered: {why:?}"));

        self.end_gather(number, why, true, turn)
    }

    /// Answers the gather of the connection numbered `number` with `gathered`, the body now read or never to be, and
    /// awaits its handler's answer, after which the connection closes when it `closes`.
    fn end_gather(
        &mut self,
        number: u64,
        gathered: &Gathered,
        closes: bool,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(out) = &mut connection.phase else {
            return Ok(());
        };
        let Inflow::Gathering { held, .. } = mem::replace(&mut out.body, Inflow::Read { since: turn.now() }) else {
            return Ok(());
        };
        out.owed.closes |= closes;
        turn.answer(held, gathered)?;

        connection.time(number, &self.settings, turn)
    }

    /// The body of the request out on the connection numbered `number` broke its framing, as `refusal` says: the stack
    /// answers the request itself, as [`end_body`](Self::end_body) says.
    fn break_body(&mut self, number: u64, refusal: Refusal, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault> {
        turn.log(Level::Info, format!("a request's body was refused: {refusal:?}"));
        let (status, _) = refusal.said();

        self.end_body(number, status, &Gathered::Malformed, turn)
    }

    /// The body of the request out on the connection numbered `number` will not be read: its gather is answered
    /// `ending`, and the stack answers the request itself with `status`, through its layers, and closes once the
    /// answer is sent. Its handler's answer, when it comes, is passed over.
    fn end_body(
        &mut self,
        number: u64,
        status: Status,
        ending: &Gathered,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { request, head, owed, body }) = mem::replace(&mut connection.phase, Phase::Closing) else {
            return Ok(());
        };
        if let Inflow::Gathering { held, .. } = body {
            turn.answer(held, ending)?;
        }
        let owed = Owed { closes: true, ..owed };

        self.respond(number, Returning::new(request, head, Response::empty(status), owed, T::LAYERS), turn)
    }
}
