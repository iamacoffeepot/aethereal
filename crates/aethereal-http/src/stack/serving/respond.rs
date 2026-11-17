//! A response on its way out of a stack (ADR-0035): back through the pure layers before where it turned, its head and
//! then its body, and written to its connection, its head by the stack, its body after it as it is.

use super::body::{Passage, pass};
use super::connection::Phase;
use super::{Deadline, Owed, Sending, Serving, Stepping};
use crate::response_head::{self, Frame};
use crate::stack::{BodyPart, Gathered, Head, LayerList, Leaving, Response, ResponseBody, ResponseHead};
use aethereal_actor::{Answer, Fault, Level, ProtocolRef, Receives, Slept};
use aethereal_tcp::{HungUp, Write, Writing};

impl<S, T: LayerList> Serving<S, T>
where
    S: Receives<Answer<Slept, Deadline>>,
{
    /// Passes `response` to the request `head` on the connection numbered `number` back out through each pure layer
    /// before `before`, in reverse: its head through every one, a layer that answers in its place starting its own
    /// response from there; then its body through the layers its head passed. It is written as `owed` says.
    pub(super) fn respond(
        &mut self,
        number: u64,
        before: usize,
        head: &Head,
        response: Response,
        owed: Owed,
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault> {
        let Some(connection) = self.connections.get_mut(&number) else {
            return Ok(());
        };
        let (mut leaving, mut body) = response.into_parts();
        let mut body_before = before;
        for at in (0..before).rev() {
            let mut layer = Leaving::new(&mut leaving, &mut body);
            T::outbound(&mut connection.layers, at, head, &mut layer);
            if layer.answered() {
                body_before = at;
            }
        }

        let (doing, pool) = turn.split();
        let mut spare = pool.buffer(0);
        let bytes = match &body {
            ResponseBody::Empty => &[][..],
            ResponseBody::Whole(bytes) => doing.read(bytes),
        };
        let run = |at, part: &mut BodyPart<'_>| T::outbound_body(&mut connection.layers, at, part);
        let passed = pass(bytes, true, (0..body_before).rev(), run, &mut spare);
        drop(spare);
        let body = match passed {
            Ok(Passage::AsCame) => body,
            Ok(Passage::Rewritten(rewritten)) => match doing.put(rewritten) {
                Ok(bytes) => ResponseBody::Whole(bytes),
                Err(_) => return self.unstored(number, turn),
            },
            Err(refused) => {
                // Its head goes out with its length, which the refused body no longer has: it is cut short, as none.
                turn.log(Level::Info, format!("a layer refused a response's body: {refused:?}"));
                return self.close(number, &Gathered::HungUp(HungUp::Shut), turn);
            }
        };

        self.write(number, (leaving, body), owed, turn)
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
