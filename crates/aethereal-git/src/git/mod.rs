//! The git capability's root (ADR-0057): a handler a stack asks each of git's requests of (ADR-0035), which serves each
//! through a run of `git http-backend` (ADR-0040).

mod exchange;
mod repository;

use crate::{Call, GitSettings, programs};
use aethereal_actor::{
    Answered, Beginning, Ended, Enough, Fault, More, Number, Open, Part, Reply, Serving, Slept, Speaks, Streams, Turn,
    actor, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_http::Status;
use aethereal_http::stack::{BodyRead, FromStack, InboundBody, OutboundBody, Request, Response, SendBody, SourceEnd};
use aethereal_process::{Exited, Processes, Started};
use aethereal_substrate::Bootstrap;
use aethereal_tcp::{HungUp, Reading, Wrote};
use aethereal_tools::Caller;
use core::num::NonZeroU32;
use exchange::Exchange;
use repository::Repository;
use std::collections::BTreeMap;

/// The parts of a program's stdout read ahead of what the stack has taken, and of a request's body ahead of what the
/// program's stdin has taken: what the root holds of each exchange at most, either way.
const WINDOW: NonZeroU32 = NonZeroU32::new(4).expect("4 is not zero");

/// What the root keeps for a start it asked of `Processes`: which program it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.git.launch")]
pub enum Launch {
    /// One of the programs that make the repository as the root begins.
    Repository,
    /// The `git http-backend` of the exchange numbered `exchange`.
    Exchange { exchange: u64 },
}

/// What the root keeps for a write to a program's stdin: the exchange whose request's body it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.git.writing")]
pub struct Writing {
    exchange: u64,
}

/// What the root keeps for the sleep that bounds how long a response it answered waits for its stack to open its body:
/// the exchange it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.git.unopened")]
pub struct Unopened {
    exchange: u64,
}

/// Where the repository stands, as the root says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    /// It is being made, or its hook checked: requests are answered `503` until it is served.
    Making,
    /// It is served.
    Ready,
    /// It could not be made, or its hook is not in place: every request is answered `503`.
    Unmade,
}

/// The git capability: serves one bare repository over git's smart HTTP (ADR-0057). It is a handler of a stack
/// (ADR-0035), asked each of git's requests a route passes it, with who sent it attached as a [`Caller`].
///
/// As it begins it makes the repository, `git init --bare`, and checks its hook is in place. Each request it then
/// serves is an exchange of its own, with a run of `git http-backend` of its own, numbered by its count: the request's
/// body is written to the program's stdin as the stack reads it, and the program's stdout, once its CGI head is read,
/// is the body of the response it answers, streamed from here. Exchanges run side by side, `most_running` at most, and
/// git's own locks order two pushes.
#[derive(Bound, Bootstrap)]
#[kind(name = "aether.git")]
pub struct Git {
    settings: GitSettings,
    repository: Repository,
    /// Every exchange whose program has not exited, by its number.
    exchanges: BTreeMap<u64, Exchange>,
    /// The number the next exchange takes: how many requests it has started a program for.
    next: u64,
    /// The programs' stdout, each read with its exchange's number.
    outputs: Streams<Reading, u64>,
    /// The requests' bodies, each read with its exchange's number.
    bodies: Streams<InboundBody, u64>,
    /// The responses' bodies stacks read from here.
    responses: Serving<OutboundBody>,
    /// How each program that exited before its start was answered exited, by the bits of its process's id: mail from
    /// `Processes` and from a process it started has no order between them, and each is claimed as its start is
    /// answered.
    unclaimed: BTreeMap<u64, Exited>,
}

#[actor(depends(Processes))]
impl Git {
    /// Begins making the repository.
    fn begin(settings: GitSettings, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let repository = Repository::make(&settings, turn)?;

        Ok(Self {
            settings,
            repository,
            exchanges: BTreeMap::new(),
            next: 0,
            outputs: Streams::new(WINDOW),
            bodies: Streams::new(WINDOW),
            responses: Serving::new(),
            unclaimed: BTreeMap::new(),
        })
    }

    /// A request that is none of git's is answered at once, as is the owner's, which no route of a session's port
    /// brings, and every request while the repository is not served or `most_running` programs run; any other is
    /// held, and its program started.
    #[request]
    fn request(
        &mut self,
        request: Request<Caller, ()>,
        turn: &mut Turn<'_, Self, FromStack>,
    ) -> Result<Reply<Response>, Fault> {
        let call = match Call::of(request.method, &request.path, &request.query) {
            Ok(call) => call,
            Err(status) => return Ok(Reply::Now(Response::empty(status))),
        };
        let Caller::Session { session } = request.attached else {
            return Ok(Reply::Now(Response::forbidden()));
        };
        let is_ready = self.repository.readiness() == Readiness::Ready;
        let room = usize::try_from(self.settings.most_running).unwrap_or(usize::MAX);
        let is_full = self.exchanges.len() >= room;
        let refuses = !is_ready || is_full;
        if refuses {
            return Ok(Reply::Now(Response::empty(Status::ServiceUnavailable)));
        }

        let exchange = self.next;
        self.next += 1;
        let start = programs::backend(&self.settings, call, session, &request, turn.me().protocol());
        turn.to(path::<Processes>(())).ask(&start, &Launch::Exchange { exchange })?;
        let (held, later) = turn.hold();
        self.exchanges.insert(exchange, Exchange::Starting { held, body: request.body });

        Ok(Reply::Later(later))
    }

    #[answer]
    fn started(
        &mut self,
        (answered, launch): (Answered<Started>, Launch),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        match launch {
            Launch::Repository => self.repository_started(&answered, turn),
            Launch::Exchange { exchange } => self.exchange_started(exchange, &answered, turn),
        }
    }

    /// A program has exited, as its process tells it: one that makes the repository, or an exchange's, or one whose
    /// start is not yet answered, kept until it is.
    #[tell]
    fn exited(&mut self, exited: Exited, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let process = turn.sender().id().bits();
        if self.repository.runs(process) {
            return self.repository_exited(exited, turn);
        }
        let Some(exchange) = self.exchange_of(process) else {
            self.unclaimed.insert(process, exited);
            return Ok(());
        };

        self.exchange_exited(exchange, exited, turn)
    }

    /// What a program wrote to its stdout.
    #[tell]
    fn output(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.take_output(part, turn)
    }

    /// A program's stdout has ended.
    #[answer]
    fn output_ended(
        &mut self,
        (answered, number): (Answered<HungUp>, Number),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(exchange) = self.outputs.answered(number) else {
            return Ok(());
        };
        let how = match answered {
            Answered::Yes(how) => how,
            Answered::No(_) => HungUp::Reset,
        };

        self.end_output(exchange, how, turn)
    }

    /// Bytes of a request's body.
    #[tell]
    fn body(&mut self, part: Part<InboundBody>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.take_body(part, turn)
    }

    /// A request's body has been read, or will not be.
    #[answer]
    fn body_read(
        &mut self,
        (answered, number): (Answered<BodyRead>, Number),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(exchange) = self.bodies.answered(number) else {
            return Ok(());
        };

        self.end_body(exchange, &answered, turn)
    }

    /// A write to a program's stdin.
    #[answer]
    fn wrote(
        &mut self,
        (answered, Writing { exchange }): (Answered<Wrote>, Writing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.written(exchange, &answered, turn)
    }

    /// A stack opens the body of a response it was answered: the exchange it names streams it, or, when it is over or
    /// gave up waiting, it is ended at once.
    #[request]
    fn open(
        &mut self,
        open: Open<OutboundBody>,
        turn: &mut Turn<'_, Self, Speaks<(Part<OutboundBody>,)>>,
    ) -> Result<aethereal_actor::Later<SourceEnd>, Fault> {
        let (stream, SendBody { body }, later) = self.responses.open(turn, open);
        turn.unguarded(|turn| self.opened(body, stream, turn))?;

        Ok(later)
    }

    /// A stack lets a response's body send more.
    #[tell]
    fn more(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(stream) = self.responses.more(turn, &more) else {
            return Ok(());
        };
        let Some(exchange) = self.exchange_streaming(stream) else {
            return Ok(());
        };

        self.flush(exchange, turn)
    }

    /// A stack takes no more of a response's body: its client hung up or stopped reading. The body is ended, and its
    /// program stopped.
    #[tell]
    fn enough(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(stopped) = self.responses.stop(turn, &enough) else {
            return Ok(());
        };
        let stream = stopped.key();
        stopped.finish(turn, &SourceEnd::Stopped)?;
        let Some(exchange) = self.exchange_streaming(stream) else {
            return Ok(());
        };

        self.stop(exchange, Status::BadGateway, turn)
    }

    /// A stack that read responses' bodies from here has ended: their programs are stopped.
    #[ended]
    fn ended(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let streams: Vec<_> = self.responses.ended(&ended).iter().map(aethereal_actor::Streaming::key).collect();
        for stream in streams {
            if let Some(exchange) = self.exchange_streaming(stream) {
                self.stop(exchange, Status::BadGateway, turn)?;
            }
        }

        Ok(())
    }

    /// A response answered has waited its time for its stack to open its body.
    #[answer]
    fn slept(
        &mut self,
        (_, Unopened { exchange }): (Answered<Slept>, Unopened),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        self.lapse(exchange, turn)
    }
}

impl Git {
    /// Where the repository stands.
    #[must_use]
    pub const fn readiness(&self) -> Readiness {
        self.repository.readiness()
    }

    /// How many requests it has started a program for since it began.
    #[must_use]
    pub const fn runs(&self) -> u64 {
        self.next
    }

    /// The exchange whose program `process`, the bits of its process's id, is: none when no exchange's start was
    /// answered with it.
    fn exchange_of(&self, process: u64) -> Option<u64> {
        self.exchanges.iter().find(|(_, exchange)| exchange.runs(process)).map(|(&number, _)| number)
    }
}
