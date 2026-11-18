//! An actor layer's answers (ADR-0035): each comes back to the stack with what it parked, and the request goes on from
//! that layer, its head in, its response out, or a part of a body; or, when the layer did not answer, a status takes
//! the response's place.

use super::connection::{Phase, Stage};
use super::{Deadline, Reaches, Serving, Stepping};
use crate::Status;
use crate::stack::actor::{Way, place};
use crate::stack::{
    AsksEach, Entered, Gathered, Gathering, Inward, LayerList, Outward, Parked, ParkedPart, ParkedResponse, PartStep,
    Response, Stack, StackSends,
};
use aethereal_actor::{
    Answer, Answered, Conforms, Fault, Handles, Level, Number, Part, Receives, Slept, Turn, Unanswered,
};
use aethereal_tcp::{Accepting, HungUp, Reading, Unbound};
use core::mem;

impl<S, T> Serving<S, T>
where
    S: Stack
        + Handles<Part<Accepting>>
        + Receives<Answer<Unbound, Number>>
        + Handles<Part<Reading>>
        + Receives<Answer<HungUp, Number>>
        + Receives<Answer<Slept, Deadline>>,
    T: LayerList,
{
    /// An actor layer answered a request's head on the way in, as `answered` says, or did not: the request goes on in
    /// from after that layer, with what it attached; turns back from it, with its answer; or, when it did not answer,
    /// turns back with a status in place of a response, `504` when it was late, `503` when it had ended, and `500`
    /// when its turn failed or it let the ask go. An answer for a request the connection no longer parks there is
    /// passed over.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn entered<X: 'static, Is, Js>(
        &mut self,
        (answered, parked): (Answered<Inward<X>>, Parked),
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        let standing = parked.standing();
        let number = standing.connection;
        let due = self.due(turn);
        let Some(connection) = self.connections.get_mut(&number) else {
            turn.log(Level::Info, "an actor layer answered for a connection that has closed, and was passed over");
            return Ok(());
        };
        if !connection.is_entering(standing) {
            turn.log(Level::Info, "an actor layer answered a request no longer parked there, and was passed over");
            return Ok(());
        }
        let Phase::Entering(entering) = mem::replace(&mut connection.phase, Phase::Closing) else {
            return Ok(());
        };
        let at = place(standing.at);

        let entered = match answered {
            Answered::Yes(Inward::Pass(attached)) => {
                let parked = Parked::new(number, entering.request);
                T::resume(&mut connection.layers, at, attached, &entering.head, turn, parked, due)?
            }
            Answered::Yes(Inward::Answer(response)) => Entered::Answered { at, response },
            Answered::No(why) => Entered::Answered { at, response: Response::empty(unanswered(why, turn)) },
        };
        self.went_in(number, entering, entered, turn)?;

        self.settle(number, turn)
    }

    /// An actor layer answered a response's head on the way out, as `answered` says, or did not: the response goes on
    /// out from before that layer, its head as the layer passed it, or the layer's answer in its place; or, when it
    /// did not answer, a status in its place, as on the way in. An answer for a response the connection no longer
    /// parks there is passed over.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn returned<Is, Js>(
        &mut self,
        (answered, parked): (Answered<Outward>, ParkedResponse),
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        let standing = parked.standing();
        let number = standing.connection;
        let Some(connection) = self.connections.get_mut(&number) else {
            turn.log(Level::Info, "an actor layer answered for a connection that has closed, and was passed over");
            return Ok(());
        };
        if !connection.is_returning(standing, Stage::Head) {
            turn.log(Level::Info, "an actor layer answered a response no longer parked there, and was passed over");
            return Ok(());
        }
        let Phase::Returning(returning) = mem::replace(&mut connection.phase, Phase::Closing) else {
            return Ok(());
        };

        let answered = match answered {
            Answered::Yes(outward) => Ok(outward),
            Answered::No(why) => Err(Response::empty(unanswered(why, turn))),
        };
        self.returned_head(number, returning, answered, turn)?;

        self.settle(number, turn)
    }

    /// An actor layer answered a part of a body, as `answered` says, or did not. A part of the request's body goes on
    /// in through the layers after it, and the body is read on; when the layer did not answer, the handler's gather
    /// ends `Unpassed`, and a status takes the place of the handler's response. A response's body goes on out through
    /// the layers before it; when the layer did not answer, a status takes the response's place, and goes out through
    /// the layers before it. A body the layer refuses is stopped. An answer for a part the connection no longer parks
    /// there is passed over.
    ///
    /// # Errors
    ///
    /// The fault of mail that does not encode, or of a sleep the engine would not run.
    pub fn passed<Is, Js>(
        &mut self,
        (answered, parked): (Answered<PartStep>, ParkedPart),
        turn: &mut Stepping<'_, S, T>,
    ) -> Result<(), Fault>
    where
        S: Conforms<Gathering, Is>,
        StackSends<T>: AsksEach<T::Attached, Reaches<T>, Js>,
    {
        let (standing, way) = parked.standing();
        let number = standing.connection;
        let Some(connection) = self.connections.get_mut(&number) else {
            turn.log(Level::Info, "an actor layer answered for a connection that has closed, and was passed over");
            return Ok(());
        };

        match way {
            Way::In => {
                if !connection.is_passing(standing) {
                    turn.log(Level::Info, "an actor layer answered a part no longer parked there, and was passed over");
                    return Ok(());
                }
                let answered = match answered {
                    Answered::Yes(step) => Ok(step),
                    Answered::No(why) => Err(unanswered(why, turn)),
                };
                self.passed_in(standing, answered, turn)?;
            }
            Way::Out => {
                if !connection.is_returning(standing, Stage::Body) {
                    turn.log(Level::Info, "an actor layer answered a part no longer parked there, and was passed over");
                    return Ok(());
                }
                let Phase::Returning(mut returning) = mem::replace(&mut connection.phase, Phase::Closing) else {
                    return Ok(());
                };
                match answered {
                    Answered::Yes(PartStep::Pass(bytes)) => self.returned_body(number, returning, bytes, turn)?,
                    Answered::Yes(PartStep::Refuse(refused)) => {
                        turn.log(Level::Info, format!("an actor layer refused a response's body: {refused:?}"));
                        self.close(number, &Gathered::HungUp(HungUp::Shut), turn)?;
                    }
                    Answered::No(why) => {
                        // Nothing of the response is written, so the status goes out in its place, its head first.
                        returning.stage = Stage::Head;
                        let response = Response::empty(unanswered(why, turn));
                        self.returned_head(number, returning, Err(response), turn)?;
                    }
                }
            }
        }

        self.settle(number, turn)
    }
}

/// The status a response takes in place of one an actor layer did not answer, as `why` says, logged on `turn`: `504`
/// when its answer was late, `503` when it had ended, and `500` when its turn failed, it let the ask go, or its answer
/// did not read.
fn unanswered<Me, S>(why: Unanswered, turn: &mut Turn<'_, Me, S>) -> Status {
    turn.log(Level::Warn, format!("an actor layer did not answer: {why:?}"));

    match why {
        Unanswered::Expired => Status::GatewayTimeout,
        Unanswered::Ended => Status::ServiceUnavailable,
        Unanswered::Failed | Unanswered::Unreplied | Unanswered::Faulted | Unanswered::Unreadable => {
            Status::InternalServerError
        }
    }
}
