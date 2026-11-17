//! A streamed request's body, served to its mount's handler as a [`RequestBody`] stream as it arrives, paced by the
//! handler's credit, as a client streams a response to its asker (ADR-0035, ADR-0017).

use super::{Credit, Exchange, Owed, Parts, Phase, Refusal, SourceStop, Writes, unstored};
use crate::body::{Body, Decoded, Progress};
use crate::{BodyBytes, BodyRead, RequestBody};
use aethereal_actor::{
    Allows, Anyone, Ended, Enough, Fault, Handles, Level, More, Open, Receives, StreamKey, Time, ToSender, Turn, Unsent,
};
use aethereal_data::Plain;
use aethereal_tcp::HungUp;
use core::time::Duration;

/// How far a streamed request's body has been read to its handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) struct Inflow {
    /// The number of the request among the exchange's streamed ones, which the stream that reads it names.
    request: u64,
    /// What is left of it.
    left: Body,
    /// The bytes of it decoded and sent.
    read: u64,
    /// The most bytes of it its mount takes.
    most: u64,
    reader: Reader,
    /// When it last moved: when it was asked, or last sent its handler a part.
    moved: Time,
}

/// Who reads a streamed request's body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Reader {
    /// No one yet: what comes of it waits in the exchange's buffer, and the connection is granted no credit.
    Unopened,
    /// The stream `key`, sent each part of it as it is decoded, while the stream has credit.
    Open(StreamKey),
}

impl Inflow {
    /// The body of the streamed request numbered `request`, of which `left` is to come, up to `most` bytes, asked at
    /// `now`.
    pub(super) const fn new(request: u64, left: Body, most: u64, now: Time) -> Self {
        Self { request, left, read: 0, most, reader: Reader::Unopened, moved: now }
    }

    /// When it last moved.
    pub(super) const fn moved(&self) -> Time {
        self.moved
    }

    /// Whether nothing is left of it: a body of no bytes, which no stream has read.
    fn is_empty(&self) -> bool {
        self.left == (Body::Length { left: 0 })
    }
}

impl Handles<Open<RequestBody>> for Exchange {
    type Sends = (ToSender<BodyRead>, Credit, Writes, Parts);
    type From = Anyone;

    /// A handler opens the stream that reads a body: the body of the streamed request out, when no stream reads it
    /// yet, or none, answered `Busy` at once.
    fn handle(&mut self, open: Open<RequestBody>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let (key, asked) = self.serving.open(turn, open)?;
        let Phase::Inflow { ref mut inflow, .. } = self.phase else {
            return self.end_body(turn, key, BodyRead::Busy);
        };

        let is_unread = inflow.reader == Reader::Unopened;
        let takes = is_unread && asked.reads(inflow.request);
        if !takes {
            return self.end_body(turn, key, BodyRead::Busy);
        }
        inflow.reader = Reader::Open(key);

        self.flow(turn)
    }
}

impl Handles<More> for Exchange {
    type Sends = (Credit, Writes, Parts);
    type From = Anyone;

    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(_) = self.serving.more(turn, &more) else {
            return Ok(());
        };

        self.flow(turn)
    }
}

impl Handles<Enough> for Exchange {
    type Sends = ();
    type From = Anyone;

    /// The handler stops the stream before the body ends: the rest of it is not read, and the exchange waits for the
    /// handler's answer, then closes.
    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Some(stopped) = self.serving.stop(turn, &enough) else {
            return Ok(());
        };
        stopped.finish(turn, &BodyRead::Cut)?;

        self.abandon(turn)
    }
}

impl Receives<Ended> for Exchange {
    type Sends = ();

    /// The actor reading a body has ended: nothing more of it is read, and the exchange waits for the handler's answer,
    /// which the engine gives for a handler that ended, then closes.
    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let left = self.serving.ended(&ended);
        if left.is_empty() {
            return Ok(());
        }

        self.abandon(turn)
    }
}

impl Exchange {
    /// Reads on the body of the streamed request out, and sends what came of it as one part, while its stream has
    /// credit: the body is never held whole. Once only more bytes would take it further, the connection is granted
    /// the credit withheld. A body past its mount's limit is answered `413`, and one that does not parse `400`.
    pub(super) fn flow<S, IC, IW, IB>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Credit, IC> + Allows<Writes, IW> + Allows<Parts, IB>,
    {
        let most_line = self.limits.most_head_bytes;
        let Phase::Inflow { ref mut inflow, owed } = self.phase else {
            return Ok(());
        };
        let Reader::Open(key) = inflow.reader else {
            return Ok(());
        };

        let has_bytes = !self.buffer.is_empty();
        let is_held = has_bytes && !self.serving.has_credit(key);
        if is_held {
            return Ok(());
        }
        let mut out = turn.buffer(self.buffer.len());
        let Decoded { consumed, progress } = match inflow.left.decode(&self.buffer, &mut out, most_line) {
            Ok(decoded) => decoded,
            Err(broken) => return self.refuse_body::<_, IW>(turn, key, Refusal::from(broken)),
        };
        self.buffer.drain(..consumed);
        inflow.read = inflow.read.saturating_add(u64::try_from(out.len()).unwrap_or(u64::MAX));
        if inflow.read > inflow.most {
            return self.refuse_body::<_, IW>(turn, key, Refusal::BodyTooLarge);
        }
        if !out.is_empty() {
            inflow.moved = turn.now();
            let Ok(bytes) = turn.put(out) else {
                // The stream is let go of with the exchange, and the engine answers it for the exchange.
                unstored(turn);
                return Ok(());
            };
            self.send_body::<_, IB>(turn, key, BodyBytes { bytes })?;
        }

        match progress {
            Progress::More => self.release::<_, IC>(turn),
            Progress::Done => {
                self.end_body(turn, key, BodyRead::Whole)?;
                self.phase = Phase::Awaiting { owed, since: turn.now() };
                self.time(turn)
            }
        }
    }

    /// Ends the stream reading `inflow`'s body, which the handler answered before it ended: `Cut`, and the connection
    /// closes, since the rest of the body is not read. Says whether it closes: a body of no bytes has nothing left.
    pub(super) fn cut<S>(&mut self, turn: &mut Turn<'_, Self, S>, inflow: Inflow) -> Result<bool, Fault> {
        if inflow.is_empty() {
            return Ok(false);
        }
        if let Reader::Open(key) = inflow.reader {
            self.end_body(turn, key, BodyRead::Cut)?;
        }

        Ok(true)
    }

    /// The sleep for `inflow`'s stall has come: the body is stalled when it has not moved since `stall_within_millis`
    /// before now, and its stream ends `Stalled` as the exchange closes; otherwise it sleeps again until it would be.
    pub(super) fn stall<S>(&mut self, turn: &mut Turn<'_, Self, S>, inflow: Inflow) -> Result<(), Fault> {
        let late = inflow.moved + Duration::from_millis(u64::from(self.limits.stall_within_millis));
        if turn.now() < late {
            return self.time(turn);
        }
        turn.log(Level::Info, "a streamed request's body stalled, and the connection closes");
        if let Reader::Open(key) = inflow.reader {
            self.end_body(turn, key, BodyRead::Stalled)?;
        }
        turn.close();

        Ok(())
    }

    /// The connection has hung up, as `how` says, or the exchange is shut: a body being read ends `HungUp`, a response's
    /// source is stopped, and the exchange closes.
    pub(super) fn hang_up(&mut self, turn: &mut Turn<'_, Self, (SourceStop,)>, how: HungUp) -> Result<(), Fault> {
        if let Phase::Inflow { inflow: Inflow { reader: Reader::Open(key), .. }, .. } = self.phase {
            self.end_body(turn, key, BodyRead::HungUp(how))?;
        }
        if let Some(source) = self.source() {
            self.sourcing.stop(turn, source)?;
        }
        turn.close();

        Ok(())
    }

    /// Leaves the body of the streamed request out unread: the exchange waits for the handler's answer, and closes
    /// once it is sent.
    fn abandon<S>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        let Phase::Inflow { owed, .. } = self.phase else {
            return Ok(());
        };
        self.phase = Phase::Awaiting { owed: Owed { closes: true, ..owed }, since: turn.now() };

        self.time(turn)
    }

    /// Ends the stream `key` reading a body that broke its framing or its limit, as `refusal` says, and answers the
    /// refusal.
    fn refuse_body<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, key: StreamKey, refusal: Refusal) -> Result<(), Fault>
    where
        S: Allows<Writes, I>,
    {
        let read = match refusal {
            Refusal::BodyTooLarge => BodyRead::TooLarge,
            Refusal::Malformed | Refusal::HeadTooLarge | Refusal::Unimplemented | Refusal::Version => {
                BodyRead::Malformed
            }
        };
        self.end_body(turn, key, read)?;

        self.refuse::<_, I>(turn, refusal)
    }

    /// Ends the stream `key` with `read`, when it is still served.
    fn end_body<S>(&mut self, turn: &mut Turn<'_, Self, S>, key: StreamKey, read: BodyRead) -> Result<(), Fault> {
        if let Some(stream) = self.serving.take(key) {
            stream.finish(turn, &read)?;
        }

        Ok(())
    }

    /// Sends the stream `key` `part`, which its credit allows.
    fn send_body<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, key: StreamKey, part: BodyBytes) -> Result<(), Fault>
    where
        S: Allows<Parts, I>,
    {
        let Some(stream) = self.serving.get_mut(key) else {
            return Ok(());
        };
        match stream.send::<_, _, I>(turn, part) {
            Ok(()) => Ok(()),
            Err(Unsent::Unwritable(error)) => Err(error.into()),
            Err(Unsent::NoCredit(_)) => {
                turn.log(Level::Error, "a part of a body was sent past its stream's credit, and was dropped");
                Ok(())
            }
        }
    }
}
