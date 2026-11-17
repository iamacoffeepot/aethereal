//! A request's body read whole for its handler's gather (ADR-0035), and any body's bytes passed through a stack's pure
//! layers: a part at a time, then its end.

use super::connection::{Inflow, Out, Phase};
use super::{Deadline, Owed, Sending, Serving, Stepping};
use crate::body::{Body, Decoded, Progress};
use crate::pooled::put;
use crate::request_head::Refusal;
use crate::response_head;
use crate::stack::{BodyPart, Gather, Gathered, LayerList, Passed, Refused, Response};
use aethereal_actor::{Answer, Fault, Level, ProtocolRef, Receives, Slept};
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
    /// Its framing broke, which the stack answers itself.
    Broken(Refusal),
}

impl Read {
    /// What a read that came to `progress`, and gathered `past` the most bytes when it is, came to.
    fn of(progress: Progress, past: bool) -> Self {
        match progress {
            _ if past => Self::Stopped(Gathered::TooLarge),
            Progress::More => Self::More,
            Progress::Done => Self::Whole,
        }
    }
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
        out.body = Inflow::Gathering { left, gathered, most, held, moved: turn.now() };
        connection.time(number, &self.settings, turn)?;

        self.gathered(number, turn)
    }

    /// Reads on the body the connection numbered `number` gathers, from what it has buffered: what is decoded passes
    /// through its pure layers in order, then, where the body ends, the end, and the gather is answered once. While
    /// only more bytes would take it further, the connection is granted the credit it withheld.
    pub(super) fn gathered(&mut self, number: u64, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault> {
        let most_line = self.settings.most_head_bytes;
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { body: Inflow::Gathering { left, gathered, most, moved, .. }, .. }) = &mut connection.phase
        else {
            return Ok(());
        };

        let start = gathered.len();
        let read = match left.decode(&connection.buffer, gathered, most_line) {
            Err(broken) => Read::Broken(Refusal::from(broken)),
            Ok(Decoded { consumed, progress }) => {
                connection.buffer.drain(..consumed);
                if gathered.len() > start {
                    *moved = turn.now();
                }
                let (_, pool) = turn.split();
                let mut spare = pool.buffer(0);
                let run = |at, part: &mut BodyPart<'_>| T::inbound_body(&mut connection.layers, at, part);
                let passed = pass(&gathered[start..], progress == Progress::Done, 0..T::PURE, run, &mut spare);
                drop(spare);
                match passed {
                    Err(Refused::TooLarge) => Read::Stopped(Gathered::TooLarge),
                    Err(Refused::Malformed) => Read::Stopped(Gathered::Malformed),
                    Ok(passage) => {
                        if let Passage::Rewritten(rewritten) = passage {
                            gathered.truncate(start);
                            gathered.extend_from_slice(&rewritten);
                        }
                        let past = u64::try_from(gathered.len()).unwrap_or(u64::MAX) > *most;
                        Read::of(progress, past)
                    }
                }
            }
        };

        match read {
            Read::More => self.release(number, turn),
            Read::Whole => self.answer_gather(number, turn),
            Read::Stopped(why) => self.stop_gather(number, &why, turn),
            Read::Broken(refusal) => self.break_body(number, refusal, turn),
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

    /// The body of the request out on the connection numbered `number` broke its framing, as `refusal` says: its
    /// gather is answered `Malformed`, and the stack answers the request itself, through its pure layers, and closes
    /// once the answer is sent. Its handler's answer, when it comes, is passed over.
    fn break_body(&mut self, number: u64, refusal: Refusal, turn: &mut Stepping<'_, S, T>) -> Result<(), Fault> {
        turn.log(Level::Info, format!("a request's body was refused: {refusal:?}"));
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { head, owed, body, .. }) = mem::replace(&mut connection.phase, Phase::Closing) else {
            return Ok(());
        };
        if let Inflow::Gathering { held, .. } = body {
            turn.answer(held, &Gathered::Malformed)?;
        }
        let (status, _) = refusal.said();

        self.respond(number, T::PURE, &head, Response::empty(status), Owed { closes: true, ..owed }, turn)
    }
}
