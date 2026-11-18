//! A stack's entry for one connection (ADR-0035): not an actor, but what the stack keeps of it, keyed by its number:
//! what has been read and not parsed, the request out or parked at an actor layer, the streamed response being
//! written, its layers' forms, and the deadline it sleeps until.

use super::outflow::Outflow;
use super::{Deadline, Owed};
use crate::http1::body::Body;
use crate::stack::actor::{Standing, held};
use crate::stack::request_head::Framing;
use crate::stack::{Gathered, Head, Response, ResponseBody, ResponseHead, StackSettings};
use aethereal_actor::{
    ActorRef, Answer, Due, Fault, Held, JobId, Number, Receives, Sleep, Slept, StreamKey, Time, Turn,
};
use aethereal_data::Bound;
use aethereal_tcp::{Accepted, SocketAddress};
use core::time::Duration;

/// One open connection of a stack, its pure layers' values `F`.
#[derive(Bound)]
pub(super) struct Connection<F> {
    /// What `Tcp` accepted.
    pub accepted: ActorRef<Accepted>,
    /// Where it came from.
    pub peer: SocketAddress,
    /// The stream the stack reads it through.
    pub reader: Number,
    /// Its layers' forms, made from the template as it was accepted: its pure layers' values, and a reference to each
    /// actor layer.
    pub layers: F,
    /// What has been read and not yet parsed.
    pub buffer: Vec<u8>,
    /// The chunks taken while a request was out, whose credit is granted once it is answered, or once its body needs
    /// more bytes.
    pub withheld: u32,
    pub phase: Phase,
    /// How many phases with a deadline it has entered: the number of the one it is in.
    phases: u64,
    sleeping: Sleeping,
}

/// Where a connection is in its requests.
#[derive(Bound)]
#[allow(
    clippy::large_enum_variant,
    reason = "one phase is kept for each connection, in place: a box would allocate for each request"
)]
pub(super) enum Phase {
    /// Between requests: nothing of the next has come since `since`.
    Idle { since: Time },
    /// Part of a head has come, its first byte at `since`.
    Head { since: Time },
    /// A request is parked at an actor layer on the way in, whose ask has a deadline of its own.
    Entering(Entering),
    /// A request is out at the handler.
    Out(Out),
    /// A response is parked at an actor layer on the way out, whose ask has a deadline of its own.
    Returning(Returning),
    /// A streamed response's head is written, and its body is written as its source sends it.
    Outflow(Outflow),
    /// A last response is written: the connection drains once it is sent, and closes when the write is not sent whole.
    Closing,
    /// A last response is sent and the connection's write half shut: what the client still sends is read and dropped
    /// until it closes, or its drain's bounds pass.
    Draining(Draining),
}

/// A connection drained after its last response, as RFC 9112 §9.6 closes one in stages: its write half was shut at
/// `began`, so the client reads the response and then its end, and what the client still sends is read and dropped, the
/// last of it at `moved`. A socket closed with bytes it has not read resets the connection, and the reset can destroy
/// the response before the client reads it; so the connection closes once the client closes, or once a bound passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
pub(super) struct Draining {
    began: Time,
    moved: Time,
}

/// A request parked on its way in, numbered `request` among the stack's, its head `head`, its body framed as `framing`
/// and unread, owed a response as `owed` says: at the actor layer `at`, which was asked about it.
#[derive(Bound)]
pub(super) struct Entering {
    pub request: u64,
    pub head: Head,
    pub framing: Framing,
    pub owed: Owed,
    pub at: u32,
}

/// A response on its way out to the request numbered `request`, whose head is `head`, written as `owed` says: its
/// head `response` and its body `body`, which pass back through the layers before `body_before`. It stands at the
/// layer `at`, and has passed every layer after it; `stage` says which half of it is there.
#[derive(Bound)]
pub(super) struct Returning {
    pub request: u64,
    pub head: Head,
    pub response: ResponseHead,
    pub body: ResponseBody,
    pub owed: Owed,
    pub body_before: u32,
    pub at: u32,
    pub stage: Stage,
}

/// Which half of a response stands at a layer on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
pub(super) enum Stage {
    /// Its head.
    Head,
    /// Its body, as one part and its end.
    Body,
}

/// A request asked of the handler, numbered `request` among the stack's, its head kept for the layers on the way out,
/// owed a response as `owed` says, and its body as `body` says.
#[derive(Bound)]
pub(super) struct Out {
    pub request: u64,
    pub head: Head,
    pub owed: Owed,
    pub body: Inflow,
}

/// How far a request out has had its body read.
#[derive(Bound)]
pub(super) enum Inflow {
    /// It has none: its answer is awaited from `since`, when it was asked.
    Bodiless { since: Time },
    /// Its handler has not asked for it: it waits in the connection, framed as `left` says, since `since`, when the
    /// request was asked.
    Unread { left: Body, since: Time },
    /// It is gathered into `gathered`, as `left` says is left of it, for the gather `held`, up to `most` bytes: it last
    /// moved at `moved`, and its last part read is `passing` the layers.
    Gathering { left: Body, gathered: Vec<u8>, most: u64, held: Held<Gathered>, moved: Time, passing: Passing },
    /// It is streamed to the stream `reader`, as `left` says is left of it, a part at a time as the reader's credit
    /// allows: it last moved at `moved`, and its last part read is `passing` the layers.
    Streaming { left: Body, reader: StreamKey, moved: Time, passing: Passing },
    /// It is read, or will not be: its answer is awaited from `since`.
    Read { since: Time },
}

/// Where the last part of a body, either way, stands among the layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
pub(super) enum Passing {
    /// It has passed them all, so the next is taken.
    Through,
    /// It is parked at the actor layer `at`, which was asked about it, and the body ends after it when `end`: no more
    /// of the body is passed until the layer answers.
    Parked { at: u32, end: bool },
}

/// Whether a connection's phase has a deadline it sleeps until.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Bound)]
enum Sleeping {
    /// It has none: it is closing.
    Not,
    /// The [`Sleep`] it ran until the phase's deadline, not yet answered.
    On(JobId),
}

impl<F> Connection<F> {
    /// The connection `accepted`, from `peer`, read through `reader`, its pure layers' values `layers`, idle from `now`
    /// and sleeping on no deadline yet.
    pub const fn new(accepted: ActorRef<Accepted>, peer: SocketAddress, reader: Number, layers: F, now: Time) -> Self {
        Self {
            accepted,
            peer,
            reader,
            layers,
            buffer: Vec::new(),
            withheld: 0,
            phase: Phase::Idle { since: now },
            phases: 0,
            sleeping: Sleeping::Not,
        }
    }

    /// Whether a request is out on it, or parked at a layer, or its response is being written, or its last is
    /// written, so what it reads waits for the response.
    pub const fn is_held(&self) -> bool {
        matches!(
            self.phase,
            Phase::Entering(_) | Phase::Out(_) | Phase::Returning(_) | Phase::Outflow(_) | Phase::Closing
        )
    }

    /// Whether it is between requests, so the credit it withheld is granted and what it buffered is parsed.
    pub const fn is_idle(&self) -> bool {
        matches!(self.phase, Phase::Idle { .. })
    }

    /// Whether the request `standing` names is parked at its actor layer on the way in.
    pub const fn is_entering(&self, standing: Standing) -> bool {
        matches!(&self.phase, Phase::Entering(entering) if entering.stands(standing))
    }

    /// Whether the response to the request `standing` names is parked at its actor layer on the way out, its `stage`.
    pub fn is_returning(&self, standing: Standing, stage: Stage) -> bool {
        matches!(&self.phase, Phase::Returning(returning) if returning.stands(standing, stage))
    }

    /// Whether a part of the body of the request `standing` names is parked at its actor layer on the way in.
    pub const fn is_passing(&self, standing: Standing) -> bool {
        matches!(
            self.phase,
            Phase::Out(Out {
                request,
                body: Inflow::Gathering { passing: Passing::Parked { at, .. }, .. }
                    | Inflow::Streaming { passing: Passing::Parked { at, .. }, .. },
                ..
            }) if request == standing.request && at == standing.at
        )
    }

    /// Whether a part of the streamed body of the response to the request `standing` names is parked at its actor
    /// layer on the way out.
    pub fn is_flowing(&self, standing: Standing) -> bool {
        matches!(&self.phase, Phase::Outflow(outflow) if outflow.stands(standing))
    }

    /// Whether the body of the request out on it is streamed to the stream `reader`.
    pub fn is_read_by(&self, reader: StreamKey) -> bool {
        matches!(&self.phase, Phase::Out(Out { body: Inflow::Streaming { reader: streamed, .. }, .. }) if *streamed == reader)
    }

    /// Whether the request numbered `request` is the one out on it.
    pub const fn is_out(&self, request: u64) -> bool {
        matches!(self.phase, Phase::Out(Out { request: out, .. }) if out == request)
    }

    /// Whether a sleep run for `deadline` times the phase it is in.
    pub const fn is_due(&self, deadline: Deadline) -> bool {
        deadline.phase == self.phases
    }

    /// The sleep that timed its phase has been answered: it sleeps on none.
    pub const fn woke(&mut self) {
        self.sleeping = Sleeping::Not;
    }

    /// Its last response is sent and its write half shut, at `now`: it drains, keeping nothing of what it read and did
    /// not parse, and sleeps until its drain's bound.
    pub fn drain<Me>(&mut self, number: u64, settings: &StackSettings, turn: &mut Turn<'_, Me>) -> Result<(), Fault>
    where
        Me: Receives<Answer<Slept, Deadline>>,
    {
        self.buffer = Vec::new();
        let draining = Draining::began(turn.now());

        self.enter(Phase::Draining(draining), number, settings, turn)
    }

    /// Enters `phase`, and sleeps until its deadline, in place of the last phase's sleep, which it cancels.
    pub fn enter<Me>(
        &mut self,
        phase: Phase,
        number: u64,
        settings: &StackSettings,
        turn: &mut Turn<'_, Me>,
    ) -> Result<(), Fault>
    where
        Me: Receives<Answer<Slept, Deadline>>,
    {
        self.phase = phase;

        self.time(number, settings, turn)
    }

    /// Sleeps until the deadline of the phase it is in, in place of the last phase's sleep, which it cancels: a
    /// closing connection sleeps for none. Its number, `number`, names it to the sleep's answer.
    pub fn time<Me>(&mut self, number: u64, settings: &StackSettings, turn: &mut Turn<'_, Me>) -> Result<(), Fault>
    where
        Me: Receives<Answer<Slept, Deadline>>,
    {
        self.unsleep(turn);
        self.phases += 1;
        self.sleeping = match self.due(settings) {
            Due::Before(until) => {
                let deadline = Deadline { connection: number, phase: self.phases };
                Sleeping::On(turn.run(&Sleep { until }, &deadline)?)
            }
            Due::Never => Sleeping::Not,
        };

        Ok(())
    }

    /// Cancels the sleep it runs, when it runs one.
    pub fn unsleep<Me>(&mut self, turn: &mut Turn<'_, Me>) {
        if let Sleeping::On(job) = self.sleeping {
            turn.cancel(job);
        }
        self.sleeping = Sleeping::Not;
    }

    /// When its phase is late under `settings`: a head from its first byte; a body unread from when its request was
    /// asked, and one being gathered or streamed, either way, once it has not moved; the answer from when the body was
    /// read, or the request asked when it has none; the next request from the last response; and a drain at its bound.
    fn due(&self, settings: &StackSettings) -> Due {
        let (since, within) = match &self.phase {
            Phase::Idle { since } => (*since, settings.idle_within_millis),
            Phase::Head { since } => (*since, settings.head_within_millis),
            // Each ask of an actor layer is answered by its own deadline.
            Phase::Entering(_)
            | Phase::Returning(_)
            | Phase::Out(Out {
                body:
                    Inflow::Gathering { passing: Passing::Parked { .. }, .. }
                    | Inflow::Streaming { passing: Passing::Parked { .. }, .. },
                ..
            })
            | Phase::Outflow(Outflow { passing: Passing::Parked { .. }, .. }) => {
                return Due::Never;
            }
            Phase::Out(Out { body: Inflow::Bodiless { since } | Inflow::Read { since }, .. }) => {
                (*since, settings.answer_within_millis)
            }
            Phase::Out(Out { body: Inflow::Unread { since, .. }, .. }) => (*since, settings.stall_within_millis),
            Phase::Out(Out { body: Inflow::Gathering { moved, .. } | Inflow::Streaming { moved, .. }, .. }) => {
                (*moved, settings.stall_within_millis)
            }
            Phase::Outflow(outflow) => (outflow.moved, settings.stall_within_millis),
            Phase::Closing => return Due::Never,
            Phase::Draining(draining) => return Due::Before(draining.until(settings)),
        };

        Due::Before(since + Duration::from_millis(u64::from(within)))
    }
}

impl Draining {
    /// A drain begun at `now`.
    const fn began(now: Time) -> Self {
        Self { began: now, moved: now }
    }

    /// Its client sent more at `now`, which is dropped.
    pub const fn read(&mut self, now: Time) {
        self.moved = now;
    }

    /// Whether it has passed its bound by `now`, under `settings`.
    pub fn is_over(&self, now: Time, settings: &StackSettings) -> bool {
        now >= self.until(settings)
    }

    /// When it ends under `settings`: `drain_stall_within_millis` after its client last sent, or `drain_within_millis`
    /// after it began, whichever comes first.
    fn until(&self, settings: &StackSettings) -> Time {
        let stalled = self.moved + Duration::from_millis(u64::from(settings.drain_stall_within_millis));
        let spent = self.began + Duration::from_millis(u64::from(settings.drain_within_millis));

        stalled.min(spent)
    }
}

impl Entering {
    /// Whether it is the request `standing` names, at its layer.
    const fn stands(&self, standing: Standing) -> bool {
        self.request == standing.request && self.at == standing.at
    }
}

impl Returning {
    /// The response `response` to the request numbered `request`, whose head is `head`, written as `owed` says: it
    /// passes back through each layer before `before`, its head first.
    pub fn new(request: u64, head: Head, response: Response, owed: Owed, before: usize) -> Self {
        let (response, body) = response.into_parts();
        let before = held(before);

        Self { request, head, response, body, owed, body_before: before, at: before, stage: Stage::Head }
    }

    /// Whether it is the response to the request `standing` names, its `stage` at its layer.
    fn stands(&self, standing: Standing, stage: Stage) -> bool {
        self.request == standing.request && self.at == standing.at && self.stage == stage
    }
}
