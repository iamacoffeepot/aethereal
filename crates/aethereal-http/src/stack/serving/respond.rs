//! A response on its way out of a stack (ADR-0035): back through the layers before where it turned, its head and then
//! a whole body, each actor layer asked and the response parked until it answers, and written to its connection, its
//! head by the stack, a whole body after it as it is. A streamed body passes the layers a part at a time once its head
//! is written ([`outflow`](super::outflow)).

use super::body::{Ending, PASSED, Passage, pass};
use super::connection::{Phase, Returning, Stage};
use super::outflow::abandon;
use super::{Carries, Deadline, Owed, Sending, Serving, Stepping};
use crate::stack::actor::{ParkedPart, ParkedResponse, Standing, Way, place};
use crate::stack::response_head::{self, Frame};
use crate::stack::{
    BodyPart, LayerList, Leaving, OutboundPart, OutboundParts, Outward, Response, ResponseBody, ResponseHead, Returned,
    Taker,
};
use crate::stack::{OutboundBody, SourceEnd};
use aethereal_actor::{
    Answer, Bytes, Fault, Guard, Handles, Level, Number, Part, ProtocolRef, Receives, Refused, Slept, Turn,
};
use aethereal_tcp::{HungUp, Write, Writing};
use core::mem;

/// A response's body as it is written with its head when no source streams it.
pub(super) enum Unstreamed {
    /// None: written with `Content-Length: 0`.
    Empty,
    /// Put whole, written after the head as it is.
    Whole(Bytes),
}

/// The head of a response, `head`, its body delimited as `frame` says, written into a buffer of `turn`'s pool and put,
/// with `Connection: close` when the connection `closes`: each header of `head`'s that the stack writes itself, or that
/// would not write, dropped and logged.
///
/// # Errors
///
/// The engine had no room to put it.
pub(super) fn put_head<Me, S, F: Guard<Me>>(
    turn: &mut Turn<'_, Me, S, F>,
    head: &ResponseHead,
    frame: Frame,
    closes: bool,
) -> Result<Bytes, Refused> {
    let ResponseHead { status, headers } = head;
    let (doing, pool) = turn.split();
    let mut written = pool.buffer(response_head::room(headers));
    let wrote = response_head::head(*status, headers, frame, closes, &mut written);
    let put = doing.put(written);
    for name in &wrote.dropped {
        turn.log(Level::Warn, format!("the response's header {name} was not written"));
    }

    put
}

impl<S, T: LayerList> Serving<S, T>
where
    S: Receives<Answer<Slept, Deadline>> + Handles<Part<OutboundBody>> + Receives<Answer<SourceEnd, Number>>,
{
    /// Passes `returning`, its head standing at a layer, back out through each layer before it, in reverse: its head
    /// through every one, a layer that answers in its place starting its own response from there, and a streamed body
    /// it answered in place of told at once it will not be read; then its body through the layers its head passed. An
    /// actor layer is asked, and the response parked until it answers. It is written as its `owed` says.
    pub(super) fn respond<Own>(
        &mut self,
        number: u64,
        mut returning: Returning,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        let due = self.due(turn);
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        while returning.at > 0 {
            returning.at -= 1;
            let at = place(returning.at);
            let parked = ParkedResponse::new(number, returning.request);
            let streamed = returning.body.streamed();
            let mut leaving = Leaving::new(&mut returning.response, &mut returning.body);
            let returned = T::outbound(&mut connection.layers, at, &returning.head, &mut leaving, turn, parked, due)?;
            match returned {
                Returned::Passed => {}
                Returned::Answered => {
                    returning.body_before = returning.at;
                    let answered_over = returning.body.streamed() != streamed;
                    match streamed {
                        Some(streamed) if answered_over => abandon(&mut self.sources, number, streamed, turn)?,
                        Some(_) | None => {}
                    }
                }
                Returned::Parked => return connection.enter(Phase::Returning(returning), number, &self.settings, turn),
            }
        }
        returning.stage = Stage::Body;
        returning.at = returning.body_before;

        self.respond_body(number, returning, turn)
    }

    /// Passes the whole body of `returning` back out through each layer before the one it stands at, in reverse, as
    /// one part and its end: a pure layer's way out reads it and writes what goes on in its place, and an actor layer
    /// whose type lists [`OutboundPart`] is asked, and the response parked until it answers; one that does not passes
    /// it untouched. A layer that refuses it closes the connection, the response unwritten. Once every layer has
    /// passed it, the response is written. A streamed body is written as its head, and passes the layers a part at a
    /// time once its source sends them.
    fn respond_body<Own>(
        &mut self,
        number: u64,
        mut returning: Returning,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        if let ResponseBody::Streamed(streamed) = returning.body {
            let Returning { request, response, owed, body_before, .. } = returning;
            return self.send_streamed(number, request, body_before, response, streamed, owed, turn);
        }
        let due = self.due(turn);
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let (doing, pool) = turn.split();
        while returning.at > 0 {
            returning.at -= 1;
            let at = place(returning.at);
            let reference = match T::taker(&connection.layers, at) {
                Taker::Pure => {
                    // A streamed body never stands here: it is written as its head before any of it passes.
                    let bytes = match &returning.body {
                        ResponseBody::Empty | ResponseBody::Streamed(_) => &[][..],
                        ResponseBody::Whole(bytes) => doing.read(bytes),
                    };
                    let mut buffers = pool.buffers::<3>(bytes.len());
                    let run = |at, part: &mut BodyPart<'_>| T::outbound_body(&mut connection.layers, at, part);
                    let passed = pass(bytes, true, at..=at, run, &mut buffers);
                    // The buffers go back to the pool before the turn goes on: what the layer passed is put first.
                    match passed {
                        Ok(Passage::AsCame) => {}
                        Ok(Passage::Rewritten) => {
                            let Ok(bytes) = doing.put(mem::take(&mut buffers[PASSED])) else {
                                drop(buffers);
                                return self.unstored(number, turn);
                            };
                            returning.body = ResponseBody::Whole(bytes);
                        }
                        Err(refused) => {
                            drop(buffers);
                            // Its head goes out with its length, which the refused body no longer has: it is cut
                            // short, as none.
                            turn.log(Level::Info, format!("a layer refused a response's body: {refused:?}"));
                            return self.close(number, Ending::HungUp(HungUp::Shut), turn);
                        }
                    }
                    continue;
                }
                Taker::Actor(reference) => reference,
            };
            let Some(parts) = doing.cast::<OutboundParts>(reference) else {
                continue;
            };

            // The layer's answer takes the body's place, so the response holds none while it waits.
            let bytes = match mem::replace(&mut returning.body, ResponseBody::Empty) {
                ResponseBody::Empty | ResponseBody::Streamed(_) => doing.put(Vec::new()),
                ResponseBody::Whole(bytes) => Ok(bytes),
            };
            let Ok(bytes) = bytes else {
                return self.unstored(number, turn);
            };
            let standing = Standing { connection: number, request: returning.request, at: returning.at };
            let asked = OutboundPart { connection: number, bytes, end: true };
            doing.to(parts).within(due).ask(&asked, &ParkedPart::new(standing, Way::Out))?;
            return connection.enter(Phase::Returning(returning), number, &self.settings, turn);
        }
        let Returning { response, body, owed, .. } = returning;
        let unstreamed = match body {
            ResponseBody::Whole(bytes) => Unstreamed::Whole(bytes),
            ResponseBody::Empty | ResponseBody::Streamed(_) => Unstreamed::Empty,
        };

        self.write(number, &response, unstreamed, owed, turn)
    }

    /// An actor layer answered the response `returning` stands at it with, or did not, as `answered` says: on the way
    /// out it goes on from before that layer, the head as the layer passed it, or the layer's answer, or a status in
    /// its place when it did not answer, which goes out as the layer's answer would; a streamed body answered in place
    /// of is told at once it will not be read.
    pub(super) fn returned_head<Own>(
        &mut self,
        number: u64,
        mut returning: Returning,
        answered: Result<Outward, Response>,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        let response = match answered {
            Ok(Outward::Pass(head)) => {
                returning.response = head;
                return self.respond(number, returning, turn);
            }
            Ok(Outward::Answer(response)) | Err(response) => response,
        };
        let (head, body) = response.into_parts();
        let streamed = returning.body.streamed();
        let answered_over = body.streamed() != streamed;
        returning.response = head;
        returning.body = body;
        match streamed {
            Some(streamed) if answered_over => abandon(&mut self.sources, number, streamed, turn)?,
            Some(_) | None => {}
        }
        returning.body_before = returning.at;

        self.respond(number, returning, turn)
    }

    /// An actor layer passed the body of `returning` as `bytes`: it goes on out through the layers before it.
    pub(super) fn returned_body<Own>(
        &mut self,
        number: u64,
        mut returning: Returning,
        bytes: Bytes,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        returning.body = ResponseBody::Whole(bytes);

        self.respond_body(number, returning, turn)
    }

    /// Writes the response `head` and its body `body` to the connection numbered `number`, as `owed` says: its head,
    /// with its `Content-Length` and, when the connection closes once it is sent, `Connection: close`, and a whole body
    /// after it as a part of its own, passed on without being read (ADR-0042). The connection then waits for its next
    /// request, or for the write to be sent, to close.
    pub(super) fn write<Own>(
        &mut self,
        number: u64,
        head: &ResponseHead,
        body: Unstreamed,
        owed: Owed,
        turn: &mut Stepping<'_, S, T, Own>,
    ) -> Result<(), Fault> {
        let length = match &body {
            Unstreamed::Empty => 0,
            Unstreamed::Whole(bytes) => u64::try_from(turn.read(bytes).len()).unwrap_or(u64::MAX),
        };
        let Ok(written) = put_head(turn, head, Frame::Length(length), owed.closes) else {
            return self.unstored(number, turn);
        };
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let sends_body = head.status.has_body() && !owed.head_only;
        let parts = match body {
            Unstreamed::Whole(bytes) if sends_body => vec![written, bytes],
            Unstreamed::Empty | Unstreamed::Whole(_) => vec![written],
        };
        let writing: ProtocolRef<Writing> = connection.accepted.protocol();
        let sending = Sending { connection: number, carries: Carries::Response { closes: owed.closes } };
        turn.to(writing).ask(&Write { parts }, &sending)?;

        let next = if owed.closes {
            Phase::Closing
        } else {
            Phase::Idle { since: turn.now() }
        };

        connection.enter(next, number, &self.settings, turn)
    }
}
