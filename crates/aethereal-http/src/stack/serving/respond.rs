//! A response on its way out of a stack (ADR-0035): back through the layers before where it turned, its head and then
//! its body, each actor layer asked and the response parked until it answers, and written to its connection, its head
//! by the stack, its body after it as it is.

use super::body::{Passage, pass};
use super::connection::{Phase, Returning, Stage};
use super::{Deadline, Owed, Sending, Serving, Stepping};
use crate::response_head::{self, Frame};
use crate::stack::actor::{ParkedPart, ParkedResponse, Standing, Way, place};
use crate::stack::{
    BodyPart, Gathered, LayerList, Leaving, OutboundPart, OutboundParts, Outward, Response, ResponseBody, ResponseHead,
    Returned, Taker,
};
use aethereal_actor::{Answer, Bytes, Fault, Level, ProtocolRef, Receives, Slept};
use aethereal_tcp::{HungUp, Write, Writing};
use core::mem;

impl<S, T: LayerList> Serving<S, T>
where
    S: Receives<Answer<Slept, Deadline>>,
{
    /// Passes `returning`, its head standing at a layer, back out through each layer before it, in reverse: its head
    /// through every one, a layer that answers in its place starting its own response from there; then its body
    /// through the layers its head passed. An actor layer is asked, and the response parked until it answers. It is
    /// written as its `owed` says.
    pub(super) fn respond(
        &mut self,
        number: u64,
        mut returning: Returning,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault> {
        let due = self.due(turn);
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        while returning.at > 0 {
            returning.at -= 1;
            let at = place(returning.at);
            let parked = ParkedResponse::new(number, returning.request);
            let mut leaving = Leaving::new(&mut returning.response, &mut returning.body);
            let returned = T::outbound(&mut connection.layers, at, &returning.head, &mut leaving, turn, parked, due)?;
            match returned {
                Returned::Passed => {}
                Returned::Answered => returning.body_before = returning.at,
                Returned::Parked => return connection.enter(Phase::Returning(returning), number, &self.settings, turn),
            }
        }
        returning.stage = Stage::Body;
        returning.at = returning.body_before;

        self.respond_body(number, returning, turn)
    }

    /// Passes the body of `returning` back out through each layer before the one it stands at, in reverse, as one part
    /// and its end: a pure layer's way out reads it and writes what goes on in its place, and an actor layer whose
    /// type lists [`OutboundPart`] is asked, and the response parked until it answers; one that does not passes it
    /// untouched. A layer that refuses it closes the connection, the response unwritten. Once every layer has passed
    /// it, the response is written.
    fn respond_body(
        &mut self,
        number: u64,
        mut returning: Returning,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault> {
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
                    let mut spare = pool.buffer(0);
                    let bytes = match &returning.body {
                        ResponseBody::Empty => &[][..],
                        ResponseBody::Whole(bytes) => doing.read(bytes),
                    };
                    let run = |at, part: &mut BodyPart<'_>| T::outbound_body(&mut connection.layers, at, part);
                    let passed = pass(bytes, true, at..=at, run, &mut spare);
                    drop(spare);
                    match passed {
                        Ok(Passage::AsCame) => {}
                        Ok(Passage::Rewritten(rewritten)) => match doing.put(rewritten) {
                            Ok(bytes) => returning.body = ResponseBody::Whole(bytes),
                            Err(_) => return self.unstored(number, turn),
                        },
                        Err(refused) => {
                            // Its head goes out with its length, which the refused body no longer has: it is cut
                            // short, as none.
                            turn.log(Level::Info, format!("a layer refused a response's body: {refused:?}"));
                            return self.close(number, &Gathered::HungUp(HungUp::Shut), turn);
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
                ResponseBody::Empty => doing.put(Vec::new()),
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

        self.write(number, (response, body), owed, turn)
    }

    /// An actor layer answered the response `returning` stands at it with, or did not, as `answered` says: on the way
    /// out it goes on from before that layer, the head as the layer passed it, or the layer's answer, or a status in
    /// its place when it did not answer, which goes out as the layer's answer would.
    pub(super) fn returned_head(
        &mut self,
        number: u64,
        mut returning: Returning,
        answered: Result<Outward, Response>,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault> {
        let response = match answered {
            Ok(Outward::Pass(head)) => {
                returning.response = head;
                return self.respond(number, returning, turn);
            }
            Ok(Outward::Answer(response)) | Err(response) => response,
        };
        let (head, body) = response.into_parts();
        returning.response = head;
        returning.body = body;
        returning.body_before = returning.at;

        self.respond(number, returning, turn)
    }

    /// An actor layer passed the body of `returning` as `bytes`: it goes on out through the layers before it.
    pub(super) fn returned_body(
        &mut self,
        number: u64,
        mut returning: Returning,
        bytes: Bytes,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault> {
        returning.body = ResponseBody::Whole(bytes);

        self.respond_body(number, returning, turn)
    }

    /// Writes the response `(head, body)` to the connection numbered `number`, as `owed` says: its head, with its
    /// `Content-Length` and, when the connection closes once it is sent, `Connection: close`, and its body after it as
    /// a part of its own, passed on without being read (ADR-0042). The connection then waits for its next request, or
    /// for the write to be sent, to close.
    pub(super) fn write(
        &mut self,
        number: u64,
        (head, body): (ResponseHead, ResponseBody),
        owed: Owed,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let (doing, pool) = turn.split();
        let length = match &body {
            ResponseBody::Empty => 0,
            ResponseBody::Whole(bytes) => u64::try_from(doing.read(bytes).len()).unwrap_or(u64::MAX),
        };
        let ResponseHead { status, headers } = head;
        let mut written = pool.buffer(response_head::room(&headers));
        let wrote =
            response_head::head(status, &headers, Frame::Length(length), owed.closes, owed.head_only, &mut written);
        let put = doing.put(written);

        for name in &wrote.dropped {
            turn.log(Level::Warn, format!("the response's header {name} was not written"));
        }
        let Ok(head) = put else {
            return self.unstored(number, turn);
        };
        let parts = match body {
            ResponseBody::Whole(bytes) if wrote.sends_body => vec![head, bytes],
            ResponseBody::Empty | ResponseBody::Whole(_) => vec![head],
        };
        let writing: ProtocolRef<Writing> = connection.accepted.protocol();
        turn.to(writing).ask(&Write { parts }, &Sending { connection: number, closes: owed.closes })?;

        let next = if owed.closes {
            Phase::Closing
        } else {
            Phase::Idle { since: turn.now() }
        };

        connection.enter(next, number, &self.settings, turn)
    }
}
