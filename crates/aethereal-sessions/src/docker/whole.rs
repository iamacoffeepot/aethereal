//! The requests the Docker client answers whole (ADR-0057): each written on a connection of its own once `Http` has
//! dialed it, its reply read as its typed answer, and the connection hung up.

use super::request::{Body, Call};
use super::{
    Acted, Calling, Connecting, Create, Created, Dial, Docker, DockerError, Fetches, HangUp, Inspect, Inspected, Kill,
    List, Listed, Remove, Start, Stop, Unwritable, reply,
};
use aethereal_actor::{ActorRef, Allows, Answer, Answered, Bytes, Due, Fault, Handles, Held, Refused, ToSender, Turn};
use aethereal_data::Bound;
use aethereal_http::{Client, Failure, Fetch, Fetched, Header, Outgoing, Reply};
use aethereal_tcp::Shut;

/// A request answered whole: who waits on it, and where it is.
#[derive(Bound)]
pub(super) struct Whole {
    answering: Answering,
    step: Step,
}

/// Who waits on a request answered whole, and the kind of its answer.
#[derive(Bound)]
enum Answering {
    Created(Held<Created>),
    Acted(Held<Acted>),
    Inspected(Held<Inspected>),
    Listed(Held<Listed>),
}

/// Where a request answered whole is.
#[derive(Bound)]
enum Step {
    /// `Http` dials the socket, and the request waits to be written.
    Dialing(Call),
    /// It is asked of `Http`'s client, which is hung up once it answers.
    Fetching(ActorRef<Client>),
}

impl Handles<Create> for Docker {
    type Sends = (Dial, ToSender<Created>);

    fn handle(&mut self, create: Create, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match create.call() {
            Ok(call) => {
                let answering = Answering::Created(turn.hold()?);
                self.ask(turn, call, answering)?;
            }
            Err(why) => turn.reply(&Created::Failed(DockerError::Unwritable(why)))?,
        }

        Ok(())
    }
}

impl Handles<Start> for Docker {
    type Sends = (Dial, ToSender<Acted>);

    fn handle(&mut self, start: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.act(turn, start.call())
    }
}

impl Handles<Stop> for Docker {
    type Sends = (Dial, ToSender<Acted>);

    fn handle(&mut self, stop: Stop, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.act(turn, stop.call())
    }
}

impl Handles<Kill> for Docker {
    type Sends = (Dial, ToSender<Acted>);

    fn handle(&mut self, kill: Kill, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.act(turn, kill.call())
    }
}

impl Handles<Remove> for Docker {
    type Sends = (Dial, ToSender<Acted>);

    fn handle(&mut self, remove: Remove, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.act(turn, remove.call())
    }
}

impl Handles<Inspect> for Docker {
    type Sends = (Dial, ToSender<Inspected>);

    fn handle(&mut self, inspect: Inspect, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match inspect.call() {
            Ok(call) => {
                let answering = Answering::Inspected(turn.hold()?);
                self.ask(turn, call, answering)?;
            }
            Err(why) => turn.reply(&Inspected::Failed(DockerError::Unwritable(why)))?,
        }

        Ok(())
    }
}

impl Handles<List> for Docker {
    type Sends = (Dial, ToSender<Listed>);

    fn handle(&mut self, list: List, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match list.call() {
            Ok(call) => {
                let answering = Answering::Listed(turn.hold()?);
                self.ask(turn, call, answering)?;
            }
            Err(why) => turn.reply(&Listed::Failed(DockerError::Unwritable(why)))?,
        }

        Ok(())
    }
}

impl Handles<Answer<Fetched, Calling>> for Docker {
    type Sends = (HangUp,);

    /// `Http`'s client answered a request: its asker is answered with what the reply says, and the client hung up.
    fn handle(
        &mut self,
        (answered, Calling { number }): (Answered<Fetched>, Calling),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Some(Whole { answering, step }) = self.wholes.remove(&number) else {
            return Ok(());
        };
        if let Step::Fetching(client) = step {
            turn.to(client).send(&Shut)?;
        }
        match answered {
            Answered::Yes(Fetched::Whole(Reply { status, body, .. })) => answering.read(turn, status, &body),
            Answered::Yes(Fetched::Failed(failure)) => answering.fail(turn, DockerError::Broken(failure)),
            Answered::No(why) => answering.fail(turn, DockerError::Unanswered(why)),
        }
    }
}

impl Docker {
    /// Asks for a start, a stop, a kill, or a remove: `call`, or why it could not be written.
    fn act<S, ID, IA>(&mut self, turn: &mut Turn<'_, Self, S>, call: Result<Call, Unwritable>) -> Result<(), Fault>
    where
        S: Allows<Dial, ID> + Allows<ToSender<Acted>, IA>,
    {
        match call {
            Ok(call) => {
                let answering = Answering::Acted(turn.hold::<_, IA>()?);
                self.ask::<_, ID>(turn, call, answering)?;
            }
            Err(why) => turn.reply::<_, IA>(&Acted::Failed(DockerError::Unwritable(why)))?,
        }

        Ok(())
    }

    /// Asks `Http` to dial the socket for `call`, which `answering` waits on.
    fn ask<S, I>(&mut self, turn: &mut Turn<'_, Self, S>, call: Call, answering: Answering) -> Result<(), Fault>
    where
        S: Allows<Dial, I>,
    {
        let number = self.dial::<_, I>(turn)?;
        self.wholes.insert(number, Whole { answering, step: Step::Dialing(call) });

        Ok(())
    }

    /// Writes the request `whole` waits on to the client `Http` dialed for it, within its asker's deadline, or answers
    /// why `Http` could not dial.
    pub(super) fn fetch(
        &mut self,
        turn: &mut Turn<'_, Self, Connecting>,
        number: u64,
        whole: Whole,
        reached: Result<ActorRef<Client>, DockerError>,
    ) -> Result<(), Fault> {
        let Whole { answering, step } = whole;
        let client = match reached {
            Ok(client) => client,
            Err(error) => return answering.fail(turn, error),
        };
        let Step::Dialing(call) = step else {
            turn.to(client).send(&Shut)?;
            return Ok(());
        };
        let Ok(request) = outgoing(turn, call) else {
            turn.to(client).send(&Shut)?;
            return answering.fail(turn, DockerError::Broken(Failure::NoRoom));
        };
        let due = answering.due();
        turn.to(client.protocol::<Fetches, _>()).within(due).ask(&Fetch { request }, &Calling { number })?;
        self.wholes.insert(number, Whole { answering, step: Step::Fetching(client) });

        Ok(())
    }
}

impl Answering {
    /// When its asker stops waiting (ADR-0041).
    const fn due(&self) -> Due {
        match self {
            Self::Created(held) => held.due(),
            Self::Acted(held) => held.due(),
            Self::Inspected(held) => held.due(),
            Self::Listed(held) => held.due(),
        }
    }

    /// Answers with what Docker's reply, of `status` with `body`, says.
    fn read<S>(self, turn: &mut Turn<'_, Docker, S>, status: u16, body: &Bytes) -> Result<(), Fault> {
        match self {
            Self::Created(held) => {
                let created = reply::created(status, turn.read(body));
                turn.answer(held, &created)?;
            }
            Self::Acted(held) => {
                let acted = reply::acted(status, turn.read(body));
                turn.answer(held, &acted)?;
            }
            Self::Inspected(held) => {
                let inspected = reply::inspected(status, turn.read(body));
                turn.answer(held, &inspected)?;
            }
            Self::Listed(held) => {
                let listed = reply::listed(status, turn.read(body));
                turn.answer(held, &listed)?;
            }
        }

        Ok(())
    }

    /// Answers that there is no result, as `error` says.
    fn fail<S>(self, turn: &mut Turn<'_, Docker, S>, error: DockerError) -> Result<(), Fault> {
        match self {
            Self::Created(held) => turn.answer(held, &Created::Failed(error))?,
            Self::Acted(held) => turn.answer(held, &Acted::Failed(error))?,
            Self::Inspected(held) => turn.answer(held, &Inspected::Failed(error))?,
            Self::Listed(held) => turn.answer(held, &Listed::Failed(error))?,
        }

        Ok(())
    }
}

/// The request `call` writes, its body put as a blob.
pub(super) fn outgoing<S>(turn: &mut Turn<'_, Docker, S>, call: Call) -> Result<Outgoing, Refused> {
    let Call { method, path, query, body } = call;
    let (headers, bytes) = match body {
        Body::Empty => (Vec::new(), Vec::new()),
        Body::Json(json) => (vec![Header::new("content-type", "application/json")], json.into_bytes()),
    };
    let body = turn.put(bytes)?;

    Ok(Outgoing { method, path, query, headers, body })
}
