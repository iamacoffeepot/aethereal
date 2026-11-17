//! A stack's entry for one connection (ADR-0035): not an actor, but what the stack keeps of it, keyed by its number:
//! what has been read and not parsed, the request out, its pure layers' values, and the deadline it sleeps until.

use super::{Deadline, Owed};
use crate::body::Body;
use crate::stack::{Gathered, Head, StackSettings};
use aethereal_actor::{ActorRef, Answer, Due, Fault, Held, JobId, Number, Receives, Sleep, Slept, Time, Turn};
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
    /// Its pure layers' values, made from the template as it was accepted.
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
    /// A request is out at the handler.
    Out(Out),
    /// A last response is written: the connection closes once it is sent.
    Closing,
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
    /// moved at `moved`.
    Gathering { left: Body, gathered: Vec<u8>, most: u64, held: Held<Gathered>, moved: Time },
    /// It is read, or will not be: its answer is awaited from `since`.
    Read { since: Time },
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

    /// Whether a request is out on it, or its last response is written, so what it reads waits for the response.
    pub const fn is_held(&self) -> bool {
        matches!(self.phase, Phase::Out(_) | Phase::Closing)
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

    /// Enters `phase`, and sleeps until its deadline, in place of the last phase's sleep, which it cancels.
    pub fn enter<Me, S>(
        &mut self,
        phase: Phase,
        number: u64,
        settings: &StackSettings,
        turn: &mut Turn<'_, Me, S>,
    ) -> Result<(), Fault>
    where
        Me: Receives<Answer<Slept, Deadline>>,
    {
        self.phase = phase;

        self.time(number, settings, turn)
    }

    /// Sleeps until the deadline of the phase it is in, in place of the last phase's sleep, which it cancels: a
    /// closing connection sleeps for none. Its number, `number`, names it to the sleep's answer.
    pub fn time<Me, S>(
        &mut self,
        number: u64,
        settings: &StackSettings,
        turn: &mut Turn<'_, Me, S>,
    ) -> Result<(), Fault>
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
    pub fn unsleep<Me, S>(&mut self, turn: &mut Turn<'_, Me, S>) {
        if let Sleeping::On(job) = self.sleeping {
            turn.cancel(job);
        }
        self.sleeping = Sleeping::Not;
    }

    /// When its phase is late under `settings`: a head from its first byte; a body unread from when its request was
    /// asked, and one being gathered once it has not moved; the answer from when the body was read, or the request
    /// asked when it has none; and the next request from the last response.
    fn due(&self, settings: &StackSettings) -> Due {
        let (since, within) = match &self.phase {
            Phase::Idle { since } => (*since, settings.idle_within_millis),
            Phase::Head { since } => (*since, settings.head_within_millis),
            Phase::Out(Out { body: Inflow::Bodiless { since } | Inflow::Read { since }, .. }) => {
                (*since, settings.answer_within_millis)
            }
            Phase::Out(Out { body: Inflow::Unread { since, .. }, .. }) => (*since, settings.stall_within_millis),
            Phase::Out(Out { body: Inflow::Gathering { moved, .. }, .. }) => (*moved, settings.stall_within_millis),
            Phase::Closing => return Due::Never,
        };

        Due::Before(since + Duration::from_millis(u64::from(within)))
    }
}
