//! An actor layer (ADR-0035): an actor of its own, whose state is shared between connections, which a stack asks and
//! waits for. What it is asked and answers on each way and each half, and what the stack keeps of a request it parks
//! while the layer answers.

use super::{Head, Refused, Response, ResponseHead};
use aethereal_actor::{Bytes, Carries, Protocol};
use aethereal_data::{Bound, Plain};

/// A request's head, asked of an actor layer on the way in, with what the layers before it attached.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.inbound")]
pub struct Inbound<In> {
    pub head: Head,
    pub attached: In,
}

/// What an actor layer answers a request's head with on the way in.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.stack.inward")]
pub enum Inward<Out> {
    /// On, with `Out` attached.
    Pass(Out),
    /// Answered here, and turned back through the layers before this one.
    Answer(Response),
}

/// A response's head, asked of an actor layer on the way out, with the head of the request it answers.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.outbound")]
pub struct Outbound {
    pub head: Head,
    pub response: ResponseHead,
}

/// What an actor layer answers a response's head with on the way out.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.stack.outward")]
pub enum Outward {
    /// On, with this head: the one it was asked, or one it changed.
    Pass(ResponseHead),
    /// In place of the response, which goes on out through the layers before this one.
    Answer(Response),
}

/// A part of a request's body, asked of an actor layer that lists it as it is read: its bytes, and whether the body
/// ends after them. `connection` tells one connection's body from another's.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.stack.inbound_part")]
pub struct InboundPart {
    pub connection: u64,
    pub bytes: Bytes,
    pub end: bool,
}

/// A part of a response's body, asked of an actor layer that lists it before it is written. A whole body is one part,
/// its end.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.stack.outbound_part")]
pub struct OutboundPart {
    pub connection: u64,
    pub bytes: Bytes,
    pub end: bool,
}

/// What an actor layer answers a part of a body with: what goes on in its place.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.http.stack.part_step")]
pub enum PartStep {
    /// These bytes go on: the part as it came, or what the layer writes instead.
    Pass(Bytes),
    /// The body stops, as `Refused` says why.
    Refuse(Refused),
}

/// The parts of a request's body, as an actor layer takes them: what a stack casts the layer's reference to as each
/// part reaches it. A layer whose type does not list [`InboundPart`] is passed each part untouched.
pub struct InboundParts;

impl Protocol for InboundParts {
    type Kinds = (InboundPart,);
}

impl Carries<InboundPart> for InboundParts {
    type Answers = (PartStep,);
}

/// The parts of a response's body, as an actor layer takes them, as [`InboundParts`] are the request's.
pub struct OutboundParts;

impl Protocol for OutboundParts {
    type Kinds = (OutboundPart,);
}

impl Carries<OutboundPart> for OutboundParts {
    type Answers = (PartStep,);
}

/// What a stack keeps of a request it asked an actor layer about on the way in: the connection it is on, its number
/// among the stack's requests, and the layer it stands at. What the layers attached went to the layer, which answers
/// what it attaches in turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.parked")]
pub struct Parked {
    connection: u64,
    request: u64,
    at: u32,
}

/// What a stack keeps of a response it asked an actor layer about on the way out: the connection, the request it
/// answers, and the layer it stands at. The response waits in the connection's entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.parked_response")]
pub struct ParkedResponse {
    connection: u64,
    request: u64,
    at: u32,
}

/// What a stack keeps of a part of a body it asked an actor layer about: the connection, the request whose body it
/// is, which body, and the layer it stands at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.http.stack.parked_part")]
pub struct ParkedPart {
    connection: u64,
    request: u64,
    way: Way,
    at: u32,
}

/// Which of a request's two bodies a part is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Way {
    /// The request's, as it is read.
    In,
    /// The response's, before it is written.
    Out,
}

/// Where a request a stack parked stands: its connection, its number, and the place of the layer it waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Standing {
    pub connection: u64,
    pub request: u64,
    pub at: u32,
}

/// A layer's place, as a context or a stack's state holds it: a list has far fewer layers than `u32` counts.
pub(super) fn held(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

/// A layer's place, as a context or a stack's state held it.
pub(super) fn place(at: u32) -> usize {
    usize::try_from(at).unwrap_or(usize::MAX)
}

impl Parked {
    /// The request numbered `request` on the connection numbered `connection`, at its first layer.
    pub(super) const fn new(connection: u64, request: u64) -> Self {
        Self { connection, request, at: 0 }
    }

    /// The same request, at the layer `at`.
    pub(super) fn at(self, at: usize) -> Self {
        Self { at: held(at), ..self }
    }

    /// Where the request stands.
    pub(super) fn standing(self) -> Standing {
        let Self { connection, request, at } = self;

        Standing { connection, request, at }
    }
}

impl ParkedResponse {
    /// The response to the request numbered `request` on the connection numbered `connection`, at its first layer.
    pub(super) const fn new(connection: u64, request: u64) -> Self {
        Self { connection, request, at: 0 }
    }

    /// The same response, at the layer `at`.
    pub(super) fn at(self, at: usize) -> Self {
        Self { at: held(at), ..self }
    }

    /// Where the response stands.
    pub(super) fn standing(self) -> Standing {
        let Self { connection, request, at } = self;

        Standing { connection, request, at }
    }
}

impl ParkedPart {
    /// The part of the body that goes `way`, of the request `standing` names, parked at its layer.
    pub(super) fn new(standing: Standing, way: Way) -> Self {
        Self { connection: standing.connection, request: standing.request, way, at: standing.at }
    }

    /// Where the part's request stands, and which body the part is of.
    pub(super) fn standing(self) -> (Standing, Way) {
        let Self { connection, request, way, at } = self;

        (Standing { connection, request, at }, way)
    }
}
