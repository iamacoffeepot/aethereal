//! A streamed response's body, read from its source through a [`ResponseSource`] stream and written to the connection
//! as it comes, paced by what the connection has sent (ADR-0035, ADR-0017).

use super::write::{self, Frame};
use super::{Carries, Exchange, Headed, Owed, Phase, Responding, Sending, SourceStop, put, unstored};
use crate::{Header, ResponseLength, ResponseSource, SendBody, SourceEnd, Status, StreamedBody};
use aethereal_actor::{Allows, Answer, Answered, Bytes, Fault, Handles, Level, Number, Part, Taken, Time, Turn};
use aethereal_data::{Bound, Plain};
use aethereal_tcp::{Write, Writing, Wrote};
use core::time::Duration;
use std::num::NonZeroU32;

/// How far a streamed response's body has been written.
#[derive(Debug, Bound)]
pub(super) struct Outflow {
    /// The stream its source sends it through.
    source: Number,
    frame: Frame,
    /// The parts taken from the source and not yet sent, in order, none of them empty: a write out carries the first
    /// of them. Each is pinned until it is sent, and the source is granted it back then.
    queued: Vec<Bytes>,
    wire: Wire,
    from: Source,
    /// The bytes of it the source has sent.
    taken: u64,
    /// The bytes of it the connection has sent.
    written: u64,
    /// When it last moved: when its head was written, or it last took a part or had one sent.
    moved: Time,
}

/// Whether a write of a streamed response is out: one at a time, so a write the connection is too full to take is
/// written again before any after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Wire {
    /// None: what is queued is written at once.
    Idle,
    /// One, carrying the first `parts` of the parts queued, or the response's head when it carries none.
    Out { parts: u32 },
}

/// Whether a source still sends a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Source {
    /// It does.
    Sending,
    /// It has sent every byte.
    Whole,
}

impl Outflow {
    /// When it last moved.
    pub(super) const fn moved(&self) -> Time {
        self.moved
    }
}

impl Handles<Part<ResponseSource>> for Exchange {
    type Sends = Responding;

    /// A part of a streamed response's body is taken without granting for it, and written once the write before it is
    /// sent. A source that sends past its length closes the connection.
    fn handle(&mut self, part: Part<ResponseSource>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(Taken { number, value, .. }) = self.sourcing.part_held(turn, part) else {
            return Ok(());
        };
        let Phase::Outflow { ref mut outflow, .. } = self.phase else {
            return Ok(());
        };
        outflow.moved = turn.now();
        let length = u64::try_from(turn.read(&value.bytes).len()).unwrap_or(u64::MAX);
        if length == 0 {
            // A part of no bytes writes nothing, and would end a chunked body: it is granted back at once.
            self.sourcing.grant(turn, number, NonZeroU32::MIN)?;
            return Ok(());
        }
        outflow.taken = outflow.taken.saturating_add(length);
        let is_long = matches!(outflow.frame, Frame::Length(length) if outflow.taken > length);
        if is_long {
            return self.abort(turn, "a streamed response's source sent past its length");
        }
        outflow.queued.push(value.bytes);

        self.pump(turn)
    }
}

impl Handles<Answer<SourceEnd, Number>> for Exchange {
    type Sends = Responding;

    /// A source ends its stream: once every byte it sent is written, a whole body is ended as its framing says. A
    /// source that failed, or ended before it said it would, closes the connection, since the head is already written
    /// and its status cannot change. A stream stopped as it opened, for a response with no body, ends here too.
    fn handle(
        &mut self,
        (answered, number): (Answered<SourceEnd>, Number),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.sourcing.answered(number);
        let Phase::Outflow { ref mut outflow, .. } = self.phase else {
            return Ok(());
        };
        if outflow.source != number {
            return Ok(());
        }
        match answered {
            Answered::Yes(SourceEnd::Whole) => {
                outflow.from = Source::Whole;
                self.pump(turn)
            }
            Answered::Yes(SourceEnd::Failed | SourceEnd::Stopped) | Answered::No(_) => {
                self.abort(turn, "a streamed response's source ended before its body did")
            }
        }
    }
}

impl Exchange {
    /// A write of a streamed response, carrying `parts` of its source's parts, is answered: once sent, its parts are
    /// granted back to the source and what is queued is written. Nothing of a write the connection is too full to take
    /// was taken, so it is written again, its first part alone; the connection holds nothing else of the response
    /// unsent, so a part that alone does not fit never will, and the connection closes.
    pub(super) fn flowed(
        &mut self,
        turn: &mut Turn<'_, Self, Responding>,
        answered: &Answered<Wrote>,
        parts: u32,
    ) -> Result<(), Fault> {
        let Phase::Outflow { ref mut outflow, .. } = self.phase else {
            return Ok(());
        };
        outflow.wire = Wire::Idle;
        match answered {
            Answered::Yes(Wrote::Sent) => {
                let sent = usize::try_from(parts).unwrap_or(usize::MAX).min(outflow.queued.len());
                let bytes: usize = outflow.queued.drain(..sent).map(|part| turn.read(&part).len()).sum();
                outflow.written = outflow.written.saturating_add(u64::try_from(bytes).unwrap_or(u64::MAX));
                outflow.moved = turn.now();
                if let Some(parts) = NonZeroU32::new(parts) {
                    self.sourcing.grant(turn, outflow.source, parts)?;
                }
                self.pump(turn)
            }
            Answered::Yes(Wrote::Full) if parts > 1 => self.write_out(turn, 1),
            Answered::Yes(Wrote::Full) => {
                self.abort(turn, "a streamed response's write is more than its connection holds")
            }
            Answered::Yes(Wrote::HungUp | Wrote::Finished) | Answered::No(_) => {
                self.abort(turn, "a streamed response's connection hung up")
            }
        }
    }

    /// Writes the head of a response of `status` with `headers` whose body is `streamed`, as `owed` says, and opens the
    /// source's stream: its body is written in chunks to a client that reads them when its length is not known, and to
    /// one that does not until the connection closes. A response with no body to send, as one to `HEAD`, stops the
    /// stream as it opens, so its source is not left waiting to be read, and the next request is taken.
    pub(super) fn send_out(
        &mut self,
        turn: &mut Turn<'_, Self, Responding>,
        status: Status,
        headers: &[Header],
        streamed: StreamedBody,
        owed: Owed,
    ) -> Result<(), Fault> {
        let StreamedBody { source, body, length } = streamed;
        let frame = match length {
            ResponseLength::Known(length) => Frame::Length(length),
            ResponseLength::Unknown if owed.chunks => Frame::Chunked,
            ResponseLength::Unknown => Frame::Close,
        };
        let owed = Owed { closes: owed.closes || frame == Frame::Close, ..owed };
        let number = self.sourcing.open(turn, source, SendBody { body }, ())?;
        let Headed::Put { head, sends_body } = self.head(turn, status, headers, frame, owed)? else {
            return Ok(());
        };
        if !sends_body {
            self.sourcing.stop(turn, number)?;
            self.write(turn, vec![head], owed.closes)?;
            return self.answered(turn);
        }

        let outflow = Outflow {
            source: number,
            frame,
            queued: Vec::new(),
            wire: Wire::Out { parts: 0 },
            from: Source::Sending,
            taken: 0,
            written: 0,
            moved: turn.now(),
        };
        self.phase = Phase::Outflow { outflow, closes: owed.closes };
        self.time(turn)?;
        let flowing = Sending { carries: Carries::Flow { parts: 0 } };
        turn.to(self.connection.protocol::<Writing, _>()).ask(&Write { parts: vec![head] }, &flowing)?;

        Ok(())
    }

    /// Writes what is queued when no write is out, or, once the source has sent every byte and all of it is written,
    /// ends the body as its framing says and takes the next request.
    fn pump(&mut self, turn: &mut Turn<'_, Self, Responding>) -> Result<(), Fault> {
        let Phase::Outflow { ref outflow, closes } = self.phase else {
            return Ok(());
        };
        if outflow.wire != Wire::Idle {
            return Ok(());
        }
        if !outflow.queued.is_empty() {
            return self.write_out(turn, outflow.queued.len());
        }
        if outflow.from == Source::Sending {
            return Ok(());
        }

        let is_short = matches!(outflow.frame, Frame::Length(length) if outflow.taken != length);
        if is_short {
            return self.abort(turn, "a streamed response's source sent less than its length");
        }
        let follows = outflow.written > 0;
        if outflow.frame == Frame::Chunked {
            let Ok(last) = put(turn, &write::last_chunk(follows)) else {
                self.phase = Phase::Closing;
                unstored(turn);
                return Ok(());
            };
            self.write(turn, vec![last], closes)?;
        } else if closes {
            // Every byte is sent, and nothing more ends the body than the close.
            self.phase = Phase::Closing;
            turn.close();
            return Ok(());
        }
        if closes {
            self.phase = Phase::Closing;
            return self.time(turn);
        }

        self.answered(turn)
    }

    /// Writes the first `parts` of the parts queued as one write, each framed as the body is.
    fn write_out(&mut self, turn: &mut Turn<'_, Self, Responding>, parts: usize) -> Result<(), Fault> {
        let Phase::Outflow { ref mut outflow, .. } = self.phase else {
            return Ok(());
        };
        let mut written = Vec::with_capacity(parts * 2);
        for (place, part) in outflow.queued.iter().take(parts).enumerate() {
            if outflow.frame == Frame::Chunked {
                let follows = outflow.written > 0 || place > 0;
                let Ok(line) = put(turn, &write::chunk_line(turn.read(part).len(), follows)) else {
                    unstored(turn);
                    return Ok(());
                };
                written.push(line);
            }
            written.push(part.clone());
        }
        let parts = u32::try_from(parts).unwrap_or(u32::MAX);
        outflow.wire = Wire::Out { parts };
        let flowing = Sending { carries: Carries::Flow { parts } };
        turn.to(self.connection.protocol::<Writing, _>()).ask(&Write { parts: written }, &flowing)?;

        Ok(())
    }

    /// The sleep for a streamed response's stall has come: it is stalled when it has not moved since
    /// `stall_within_millis` before now, and the connection closes; otherwise it sleeps again until it would be.
    pub(super) fn stall_out<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<SourceStop, I>,
    {
        let Phase::Outflow { ref outflow, .. } = self.phase else {
            return Ok(());
        };
        let late = outflow.moved + Duration::from_millis(u64::from(self.limits.stall_within_millis));
        if turn.now() < late {
            return self.time(turn);
        }

        self.abort::<_, I>(turn, "a streamed response stalled")
    }

    /// Ends a streamed response before its body ends, as `why` says: its source is stopped, and the connection closes,
    /// since the head is already written and only the close can tell the client the body broke.
    fn abort<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, why: &str) -> Result<(), Fault>
    where
        S: Allows<SourceStop, I>,
    {
        turn.log(Level::Info, format!("{why}, and the connection closes"));
        self.stop_source::<_, I>(turn)?;
        self.phase = Phase::Closing;
        turn.close();

        Ok(())
    }

    /// Stops the source of the streamed response being written, when one is.
    pub(super) fn stop_source<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<SourceStop, I>,
    {
        if let Phase::Outflow { ref outflow, .. } = self.phase {
            self.sourcing.stop::<_, _, I>(turn, outflow.source)?;
        }

        Ok(())
    }
}
