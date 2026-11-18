//! A request's body streamed to its reader (ADR-0035, ADR-0017): an [`InboundBody`] stream its handler opens on the
//! stack through the body's handle, served as the body arrives, a part at a time through the layers, and paced by the
//! reader's credit: the connection is decoded only while the reader may be sent a part, and granted what it withheld
//! only once more bytes are all the body needs.

use super::body::{Carried, Ending, Layered, Walk, walk, write_continue};
use super::connection::{Inflow, Out, Passing, Phase};
use super::{Deadline, Sending, Serving};
use crate::http1::body::{Decoded, Progress};
use crate::stack::actor::{ParkedPart, Standing, Way, held};
use crate::stack::request_head::Refusal;
use crate::stack::{BodyBytes, BodyRead, InboundBody, OutboundBody, SourceEnd};
use crate::stack::{InboundPart, InboundParts, PartStep, WalkedBy};
use aethereal_actor::{
    Answer, Bytes, Ended, Enough, Fault, Handles, Level, More, Number, Open, Part, Receives, Slept, StreamKey, Turn,
    Unsent,
};
use aethereal_tcp::HungUp;
use aethereal_tcp::Wrote;

impl<S, T> Serving<S, T>
where
    S: Receives<Answer<Slept, Deadline>>
        + Handles<Part<OutboundBody>>
        + Receives<Answer<SourceEnd, Number>>
        + Receives<Answer<Wrote, Sending>>
        + Receives<Answer<PartStep, ParkedPart>>,
    T: WalkedBy<S>,
{
    /// A handler opens the stream that reads its request's body as it arrives: the body of the request out on its
    /// connection, when nothing reads it yet; a body of no bytes ends `Whole` at once; and one another stream or
    /// gather reads, one that has been read, or one of a request that is not its connection's request out, `Busy`. A
    /// client that waits for `100 Continue` is told it now.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn open_body(&mut self, open: Open<InboundBody>, turn: &mut Turn<'_, S>) -> Result<(), Fault>
    where
        S: Receives<Ended>,
    {
        let (reader, asked) = self.readers.open(turn, open)?;
        let request = asked.request();
        let found = self.connections.iter().find(|(_, connection)| connection.is_out(request));
        let Some(number) = found.map(|(&number, _)| number) else {
            return self.finish_reader(reader, BodyRead::Busy, turn);
        };
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(out) = &mut connection.phase else {
            return self.finish_reader(reader, BodyRead::Busy, turn);
        };
        let left = match out.body {
            Inflow::Unread { left, .. } => left,
            Inflow::Bodiless { since } => {
                out.body = Inflow::Read { since };
                return self.finish_reader(reader, BodyRead::Whole, turn);
            }
            Inflow::Gathering { .. } | Inflow::Streaming { .. } | Inflow::Read { .. } => {
                return self.finish_reader(reader, BodyRead::Busy, turn);
            }
        };

        let continues = out.owed.continues;
        let is_unstored = continues && !write_continue(connection.accepted, number, turn)?;
        if is_unstored {
            self.finish_reader(reader, BodyRead::HungUp(HungUp::Shut), turn)?;
            return self.unstored(number, turn);
        }
        out.body = Inflow::Streaming { left, reader, moved: turn.now(), passing: Passing::Through };
        connection.time(number, &self.settings, turn)?;

        self.stream(number, turn)
    }

    /// A body's reader grants its stream more credit: what the connection has buffered of the body is read on.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn grant_body(&mut self, more: &More, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let Some(reader) = self.readers.more(turn, more) else {
            return Ok(());
        };
        let Some(number) = self.read_by(reader) else {
            return Ok(());
        };

        self.stream(number, turn)
    }

    /// A body's reader stops its stream before the body ends: the stream ends `Cut`, the rest of the body is not read,
    /// and the stack waits for the handler's answer, then closes the connection.
    ///
    /// # Errors
    ///
    /// The fault of an answer that does not encode, or of a sleep the engine would not run.
    pub fn stop_reading(&mut self, enough: &Enough, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let Some(stopped) = self.readers.stop(turn, enough) else {
            return Ok(());
        };
        let reader = stopped.key();
        stopped.finish(turn, &BodyRead::Cut)?;
        let Some(number) = self.read_by(reader) else {
            return Ok(());
        };

        self.leave_unread(number, turn)
    }

    /// An actor that reads bodies has ended: the streams it read are let go of with it, the rest of each body is not
    /// read, and the stack waits for each handler's answer, then closes its connection.
    ///
    /// # Errors
    ///
    /// The fault of a sleep the engine would not run.
    pub fn lose_reader(&mut self, ended: &Ended, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        for lost in self.readers.ended(ended) {
            let reader = lost.key();
            drop(lost);
            if let Some(number) = self.read_by(reader) {
                self.leave_unread(number, turn)?;
            }
        }

        Ok(())
    }

    /// Reads on the body the connection numbered `number` streams to its reader, from what it has buffered, while the
    /// reader may be sent a part: what is decoded passes through the layers in order and goes on the stream as one
    /// part, and the stream ends `Whole` where the body ends. While only more bytes would take it further, the
    /// connection is granted the credit it withheld. While a part waits on an actor layer, nothing more is read.
    pub(super) fn stream(&mut self, number: u64, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let most_line = self.settings.most_head_bytes;
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(Out { body: Inflow::Streaming { left, reader, moved, passing }, .. }) = &mut connection.phase
        else {
            return Ok(());
        };
        if let Passing::Parked { .. } = passing {
            return Ok(());
        }
        // Bytes the reader cannot yet be sent wait in the buffer; with none, more are read, up to the read window.
        let has_bytes = !connection.buffer.is_empty();
        let is_held = has_bytes && !self.readers.has_credit(*reader);
        if is_held {
            return Ok(());
        }

        let (doing, pool) = turn.split();
        let mut part = pool.buffer(connection.buffer.len());
        let Decoded { consumed, progress } = match left.decode(&connection.buffer, &mut part, most_line) {
            Ok(decoded) => decoded,
            Err(broken) => {
                drop(part);
                return self.break_body(number, Refusal::from(broken), turn);
            }
        };
        connection.buffer.drain(..consumed);
        let end = progress == Progress::Done;
        if !part.is_empty() {
            *moved = doing.now();
        }
        let is_idle = part.is_empty() && !end;
        if is_idle {
            drop(part);
            return self.release(number, turn);
        }
        let part = Vec::from(part);

        let reads_on = self.stream_on(number, 0, Carried::Written(part), end, turn)?;
        if !reads_on {
            return Ok(());
        }

        self.release(number, turn)
    }

    /// Passes a part of the body the connection numbered `number` streams, `carried` where its bytes are and the body's
    /// end after them when `end`, through its layers from the one at `from`, in order: each pure layer's way in reads
    /// what the one before passed, and an actor layer whose type lists [`InboundPart`] is asked, and the part parked
    /// until it answers. What the last layer passes goes on the stream as one part, and the stream ends `Whole` after
    /// the end. Whether more of the body is to be read: not once it ends, while a part is parked, or once it stops.
    pub(super) fn stream_on(
        &mut self,
        number: u64,
        from: usize,
        carried: Carried,
        end: bool,
        turn: &mut Turn<'_, S>,
    ) -> Result<bool, Fault> {
        let due = self.due(turn);
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(false);
        };
        let Phase::Out(Out { request, body: Inflow::Streaming { reader, passing, .. }, .. }) = &mut connection.phase
        else {
            return Ok(false);
        };
        let reader = *reader;
        let (doing, pool) = turn.split();
        let layered = Layered { values: &mut connection.layers, order: from..T::LAYERS, run: T::inbound_body };
        let walked: Walk<InboundParts> = walk::<T, _, _, _, _>(layered, &[], carried, end, (doing, pool));

        let carried = match walked {
            Walk::Through(carried) => carried,
            Walk::Asks { at, parts, bytes } => {
                *passing = Passing::Parked { at: held(at), end };
                let standing = Standing { connection: number, request: *request, at: held(at) };
                let asked = InboundPart { connection: number, bytes, end };
                doing.to(parts).within(due).ask(&asked, &ParkedPart::new(standing, Way::In))?;
                connection.time(number, &self.settings, turn)?;
                return Ok(false);
            }
            Walk::Refused(refused) => {
                self.stop_body(number, Ending::refused(refused), turn)?;
                return Ok(false);
            }
            Walk::Unstored => {
                self.unstored(number, turn)?;
                return Ok(false);
            }
        };
        let put = match carried {
            Carried::Blob(bytes) => Ok(bytes),
            Carried::Written(written) if !written.is_empty() => turn.put(written),
            Carried::Came | Carried::Written(_) => return self.read_through(number, reader, end, turn),
        };
        let Ok(bytes) = put else {
            self.unstored(number, turn)?;
            return Ok(false);
        };
        self.send_part(reader, bytes, turn)?;

        self.read_through(number, reader, end, turn)
    }

    /// The part last read of the body the connection numbered `number` streams to `reader` is on the stream: when it was
    /// the body's end, the stream ends `Whole`, and the connection awaits its handler's answer. Whether more of the
    /// body is to be read.
    fn read_through(
        &mut self,
        number: u64,
        reader: StreamKey,
        end: bool,
        turn: &mut Turn<'_, S>,
    ) -> Result<bool, Fault> {
        if !end {
            return Ok(true);
        }
        self.finish_reader(reader, BodyRead::Whole, turn)?;
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(false);
        };
        let Phase::Out(out) = &mut connection.phase else {
            return Ok(false);
        };
        out.body = Inflow::Read { since: turn.now() };
        connection.time(number, &self.settings, turn)?;

        Ok(false)
    }

    /// Sends the stream `reader` a part of the body it reads, `bytes`, which its credit allows: none is decoded
    /// without it.
    fn send_part(&mut self, reader: StreamKey, bytes: Bytes, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let Some(stream) = self.readers.get_mut(reader) else {
            return Ok(());
        };
        match stream.send(turn, BodyBytes { bytes }) {
            Ok(()) => Ok(()),
            Err(Unsent::Unwritable(error)) => Err(error.into()),
            Err(Unsent::NoCredit(_)) => {
                turn.log(Level::Error, "a part of a body was sent past its stream's credit, and was dropped");
                Ok(())
            }
        }
    }

    /// The connection whose body the stream `reader` reads: none when no connection's does.
    fn read_by(&self, reader: StreamKey) -> Option<u64> {
        let found = self.connections.iter().find(|(_, connection)| connection.is_read_by(reader));

        found.map(|(&number, _)| number)
    }

    /// The body the connection numbered `number` streams is left unread, as its reader stopped or ended: the stack
    /// awaits the handler's answer, and closes the connection once it is written, since where the next request starts
    /// is not known.
    fn leave_unread(&mut self, number: u64, turn: &mut Turn<'_, S>) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let Phase::Out(out) = &mut connection.phase else {
            return Ok(());
        };
        out.body = Inflow::Read { since: turn.now() };
        out.owed.closes = true;

        connection.time(number, &self.settings, turn)
    }
}
