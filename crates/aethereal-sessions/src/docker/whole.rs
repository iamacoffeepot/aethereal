//! The requests the Docker client answers whole (ADR-0057): each written on a connection of its own once `Http` has
//! dialed it, its reply read as its typed answer, and the connection hung up.

use super::request::{Body, Call};
use super::{
    Acted, Calling, Create, CreateVolume, Created, Docker, DockerError, Fetches, Inspect, Inspected, Kill, List,
    Listed, Remove, Start, Stop, Unwritable, VolumeCreated, reply,
};
use aethereal_actor::{
    ActorRef, Answer, Answered, Anyone, Bytes, Due, Fault, Handles, Held, Receives, Refused, Reply, Turn,
};
use aethereal_data::Bound;
use aethereal_http::Reply as HttpReply;
use aethereal_http::{Client, Failure, Fetch, Fetched, Header, Outgoing};
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
    VolumeCreated(Held<VolumeCreated>),
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
    type From = Anyone;
    type Returns = Reply<Created>;

    fn handle(&mut self, create: Create, turn: &mut Turn<'_, Self>) -> Result<Reply<Created>, Fault> {
        match create.call() {
            Ok(call) => {
                let (held, later) = turn.hold();
                self.ask(turn, call, Answering::Created(held))?;

                Ok(Reply::Later(later))
            }
            Err(why) => Ok(Reply::Now(Created::Failed(DockerError::Unwritable(why)))),
        }
    }
}

impl Handles<Start> for Docker {
    type From = Anyone;
    type Returns = Reply<Acted>;

    fn handle(&mut self, start: Start, turn: &mut Turn<'_, Self>) -> Result<Reply<Acted>, Fault> {
        self.act(turn, start.call())
    }
}

impl Handles<Stop> for Docker {
    type From = Anyone;
    type Returns = Reply<Acted>;

    fn handle(&mut self, stop: Stop, turn: &mut Turn<'_, Self>) -> Result<Reply<Acted>, Fault> {
        self.act(turn, stop.call())
    }
}

impl Handles<Kill> for Docker {
    type From = Anyone;
    type Returns = Reply<Acted>;

    fn handle(&mut self, kill: Kill, turn: &mut Turn<'_, Self>) -> Result<Reply<Acted>, Fault> {
        self.act(turn, kill.call())
    }
}

impl Handles<Remove> for Docker {
    type From = Anyone;
    type Returns = Reply<Acted>;

    fn handle(&mut self, remove: Remove, turn: &mut Turn<'_, Self>) -> Result<Reply<Acted>, Fault> {
        self.act(turn, remove.call())
    }
}

impl Handles<Inspect> for Docker {
    type From = Anyone;
    type Returns = Reply<Inspected>;

    fn handle(&mut self, inspect: Inspect, turn: &mut Turn<'_, Self>) -> Result<Reply<Inspected>, Fault> {
        match inspect.call() {
            Ok(call) => {
                let (held, later) = turn.hold();
                self.ask(turn, call, Answering::Inspected(held))?;

                Ok(Reply::Later(later))
            }
            Err(why) => Ok(Reply::Now(Inspected::Failed(DockerError::Unwritable(why)))),
        }
    }
}

impl Handles<List> for Docker {
    type From = Anyone;
    type Returns = Reply<Listed>;

    fn handle(&mut self, list: List, turn: &mut Turn<'_, Self>) -> Result<Reply<Listed>, Fault> {
        match list.call() {
            Ok(call) => {
                let (held, later) = turn.hold();
                self.ask(turn, call, Answering::Listed(held))?;

                Ok(Reply::Later(later))
            }
            Err(why) => Ok(Reply::Now(Listed::Failed(DockerError::Unwritable(why)))),
        }
    }
}

impl Handles<CreateVolume> for Docker {
    type From = Anyone;
    type Returns = Reply<VolumeCreated>;

    fn handle(&mut self, create: CreateVolume, turn: &mut Turn<'_, Self>) -> Result<Reply<VolumeCreated>, Fault> {
        match create.call() {
            Ok(call) => {
                let (held, later) = turn.hold();
                self.ask(turn, call, Answering::VolumeCreated(held))?;

                Ok(Reply::Later(later))
            }
            Err(why) => Ok(Reply::Now(VolumeCreated::Failed(DockerError::Unwritable(why)))),
        }
    }
}

impl Receives<Answer<Fetched, Calling>> for Docker {
    /// `Http`'s client answered a request: its asker is answered with what the reply says, and the client hung up.
    fn handle(
        &mut self,
        (answered, Calling { number }): (Answered<Fetched>, Calling),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Whole { answering, step }) = self.wholes.remove(&number) else {
            return Ok(());
        };
        if let Step::Fetching(client) = step {
            turn.to(client).send(&Shut)?;
        }
        match answered {
            Answered::Yes(Fetched::Whole(HttpReply { status, body, .. })) => answering.read(turn, status, &body),
            Answered::Yes(Fetched::Failed(failure)) => answering.fail(turn, DockerError::Broken(failure)),
            Answered::No(why) => answering.fail(turn, DockerError::Unanswered(why)),
        }
    }
}

impl Docker {
    /// Asks for a start, a stop, a kill, or a remove: `call`, or why it could not be written.
    fn act(&mut self, turn: &mut Turn<'_, Self>, call: Result<Call, Unwritable>) -> Result<Reply<Acted>, Fault> {
        match call {
            Ok(call) => {
                let (held, later) = turn.hold();
                self.ask(turn, call, Answering::Acted(held))?;

                Ok(Reply::Later(later))
            }
            Err(why) => Ok(Reply::Now(Acted::Failed(DockerError::Unwritable(why)))),
        }
    }

    /// Asks `Http` to dial the socket for `call`, which `answering` waits on.
    fn ask(&mut self, turn: &mut Turn<'_, Self>, call: Call, answering: Answering) -> Result<(), Fault> {
        let number = self.dial(turn)?;
        self.wholes.insert(number, Whole { answering, step: Step::Dialing(call) });

        Ok(())
    }

    /// Writes the request `whole` waits on to the client `Http` dialed for it, within its asker's deadline, or answers
    /// why `Http` could not dial.
    pub(super) fn fetch(
        &mut self,
        turn: &mut Turn<'_, Self>,
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
        turn.to(client.protocol::<Fetches>()).within(due).ask(&Fetch { request }, &Calling { number })?;
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
            Self::VolumeCreated(held) => held.due(),
        }
    }

    /// Answers with what Docker's reply, of `status` with `body`, says.
    fn read(self, turn: &mut Turn<'_, Docker>, status: u16, body: &Bytes) -> Result<(), Fault> {
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
            Self::VolumeCreated(held) => {
                let created = reply::volume_created(status, turn.read(body));
                turn.answer(held, &created)?;
            }
        }

        Ok(())
    }

    /// Answers that there is no result, as `error` says.
    fn fail(self, turn: &mut Turn<'_, Docker>, error: DockerError) -> Result<(), Fault> {
        match self {
            Self::Created(held) => turn.answer(held, &Created::Failed(error))?,
            Self::Acted(held) => turn.answer(held, &Acted::Failed(error))?,
            Self::Inspected(held) => turn.answer(held, &Inspected::Failed(error))?,
            Self::Listed(held) => turn.answer(held, &Listed::Failed(error))?,
            Self::VolumeCreated(held) => turn.answer(held, &VolumeCreated::Failed(error))?,
        }

        Ok(())
    }
}

/// The request `call` writes, its body put as a blob.
pub(super) fn outgoing(turn: &mut Turn<'_, Docker>, call: Call) -> Result<Outgoing, Refused> {
    let Call { method, path, query, body } = call;
    let (headers, bytes) = match body {
        Body::Empty => (Vec::new(), Vec::new()),
        Body::Json(json) => (vec![Header::new("content-type", "application/json")], json.into_bytes()),
    };
    let body = turn.put(bytes)?;

    Ok(Outgoing { method, path, query, headers, body })
}
