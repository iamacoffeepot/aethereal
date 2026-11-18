//! A pure layer (ADR-0035): a value a stack keeps for each open connection, and calls as a function in its own turn on
//! the way in, on the way out, and with each part of the two bodies; and the form a stack holds each layer in.

use super::{Head, Response, ResponseBody, ResponseHead};
use aethereal_actor::{Actor, ActorRef, Within};
use aethereal_data::{Decode, Encode, Plain};

/// A layer whose state is one connection's: a value the stack makes for each connection and calls in its own turn.
///
/// It is plain data, as an actor's state encodes and a stack holds its pure layers' values (ADR-0004). Its type writes
/// one line beside this impl, `impl LayerForm for MarkOwner { type Form = Self; }`, which says the stack holds it as
/// itself.
pub trait Layer: Plain + Default + Clone + 'static {
    /// What the layers before it attached.
    type In: Plain;
    /// What it passes on attached.
    type Out: Plain;

    /// The way in: passes `Out` on, or answers. Required, as it is what turns `In` into `Out`.
    fn inbound(&mut self, head: &Head, attached: Self::In) -> Step<Self::Out>;

    /// The way out: changes the response's head in place, or answers in its place. By default it passes.
    fn outbound(&mut self, _: &Head, _: &mut Leaving<'_>) {}

    /// The request's body, a part at a time and then its end. By default a part passes as it came.
    fn inbound_body(&mut self, _: &mut BodyPart<'_>) {}

    /// The response's body, the same way out.
    fn outbound_body(&mut self, _: &mut BodyPart<'_>) {}
}

/// What a layer does with a request on the way in.
#[derive(Debug)]
pub enum Step<Out> {
    /// On, with `Out` attached.
    Pass(Out),
    /// Answered here, and turned back through the layers before this one.
    Answer(Response),
}

/// What a stack holds to use a layer: a pure layer itself, or a reference to an actor layer or a handler.
///
/// A pure layer states its form, `type Form = Self`, and an actor does not: one impl here covers every actor, and a
/// second blanket impl for every [`Layer`] would overlap it (E0119). A type that is both a `Layer` and an `Actor` is
/// refused by the compiler.
pub trait LayerForm {
    /// What the stack holds: a value, or a reference, which encodes in its state.
    type Form: Encode<Within> + Decode<Within> + Clone;
}

/// An actor is held by a reference to it, found at its typed path as the stack begins.
impl<A: Actor> LayerForm for A {
    type Form = ActorRef<A>;
}

/// A response on its way out, as one layer is handed it: its head, which the layer may change, or the whole response,
/// which it may answer in place of.
#[derive(Debug)]
pub struct Leaving<'r> {
    head: &'r mut ResponseHead,
    body: &'r mut ResponseBody,
    /// Whether the layer answered in place of the response.
    answered: bool,
}

impl<'r> Leaving<'r> {
    /// The response whose head is `head` and body `body`, which no layer has answered in place of yet.
    pub(super) const fn new(head: &'r mut ResponseHead, body: &'r mut ResponseBody) -> Self {
        Self { head, body, answered: false }
    }

    /// The response's head, changed in place.
    pub const fn head(&mut self) -> &mut ResponseHead {
        self.head
    }

    /// Answers in place of the response: `response` goes on out through the layers before this one.
    pub fn answer(&mut self, response: Response) {
        let Response { status, headers, body } = response;
        *self.head = ResponseHead { status, headers };
        *self.body = body;
        self.answered = true;
    }

    /// Whether the layer answered in place of the response.
    pub(super) const fn answered(&self) -> bool {
        self.answered
    }
}

/// A part of a body as one layer is handed it: the bytes as they came, read in place, and a buffer for what the layer
/// passes on instead; or the body's end, which has no bytes and may be followed by what the layer still has to pass.
#[derive(Debug)]
pub struct BodyPart<'p> {
    bytes: &'p [u8],
    end: bool,
    out: &'p mut Vec<u8>,
    passed: Passed,
}

/// What a layer did with a part of a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Passed {
    /// It passed the part as it came.
    AsCame,
    /// It wrote what it passes on instead into the part's out buffer.
    Written,
    /// It stopped the body.
    Refused(Refused),
}

impl<'p> BodyPart<'p> {
    /// The part `bytes`, or the body's end when `end`, its layer to write what it passes instead into `out`, which is
    /// empty.
    pub(super) const fn new(bytes: &'p [u8], end: bool, out: &'p mut Vec<u8>) -> Self {
        Self { bytes, end, out, passed: Passed::AsCame }
    }

    /// The part as it came, read in place: empty at the end.
    #[must_use]
    pub const fn bytes(&self) -> &[u8] {
        self.bytes
    }

    /// Whether this is the body's end, after its last part.
    #[must_use]
    pub const fn is_end(&self) -> bool {
        self.end
    }

    /// What the layer passes on instead of the part, empty until it writes: a buffer of the stack's pool. Taken at the
    /// end, what it holds passes on after the body's last part.
    pub fn out(&mut self) -> &mut Vec<u8> {
        if self.passed == Passed::AsCame {
            self.passed = Passed::Written;
        }

        self.out
    }

    /// Stops the body, as `refused` says why.
    pub const fn refuse(&mut self, refused: Refused) {
        self.passed = Passed::Refused(refused);
    }

    /// What the layer did with the part.
    pub(super) const fn passed(&self) -> Passed {
        self.passed
    }
}

/// Why a layer stops a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Refused {
    /// It is larger than the layer takes.
    TooLarge,
    /// It does not read as the layer reads it.
    Malformed,
}
