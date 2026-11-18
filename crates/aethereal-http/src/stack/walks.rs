//! What a stack holds of each layer before its handler, its form, and how it walks a request through that layer
//! (ADR-0035): a pure layer is called in the stack's turn, and an actor layer asked through its reference, which parks
//! the request until it answers.

use super::actor::{Inbound, Inward, Outbound, Outward, Parked, ParkedResponse};
use super::{BodyPart, Head, Layer, Leaving, Response, Step, Unlayered};
use aethereal_actor::{
    Actor, ActorRef, Actors, Admits, Answer, Beginning, Carries, ChildOf, DependsOn, ErasedActorRef, Fault, Handled,
    Handles, PathTo, Receives, Time, Turn, Within, path,
};
use aethereal_data::{Decode, Encode, Kind, Plain};

/// A layer before the handler, as a stack holds it: its form, a pure layer itself or a reference to an actor layer.
/// What the stack does with it whatever it attaches and whoever the stack is: hands it the parts of a body. A layer's
/// author writes none of this; `aethereal-http` writes it for every pure layer and every actor layer.
pub trait Formed: Encode<Within> + Decode<Within> + Clone {
    /// What it is, as a part of a body reaches it.
    fn taker(&self) -> Taker;

    /// Hands a pure layer a part of the request's body. An actor layer is asked through a cast instead.
    fn inbound_body(&mut self, part: &mut BodyPart<'_>);

    /// Hands a pure layer a part of the response's body. An actor layer is asked through a cast instead.
    fn outbound_body(&mut self, part: &mut BodyPart<'_>);
}

/// A layer or a lone handler as a stack of type `Me` holds it once it begins: a pure layer made with `Default`, and an
/// actor found at its typed path, which builds when its root is the root of a type `Me` depends on, at `I`, its place
/// in `Me`'s depends, which the compiler infers (ADR-0059).
pub trait FoundBy<Me, I>: Sized {
    /// It, as `turn` sees the engine.
    ///
    /// # Errors
    ///
    /// The fault [`Unlayered`], naming an actor's type, when it is not live.
    fn found(turn: &Beginning<'_, Me>) -> Result<Self, Fault>;
}

/// A layer before the handler, as a stack holds it, walked by a request the layers before it attached `In` to: what it
/// passes on attached.
pub trait Walks<In>: Formed {
    /// What it passes on attached.
    type Out: Plain + 'static;
}

/// A layer before the handler, walked by a stack of type `Me` that passes a request through it, the layers before it
/// having attached `In`: a pure layer is called in any stack's turn, and an actor layer is asked, each ask checked at
/// its call, which builds when `Me` handles the answer with its context.
pub trait WalksFor<Me, In>: Walks<In> {
    /// Hands it a request's head on the way in, `attached` what the layers before it attached: a pure layer passes or
    /// answers, and an actor layer is asked, keeping `ask`'s context until it answers.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn inbound(
        &mut self,
        head: &Head,
        attached: In,
        turn: &mut Turn<'_, Me>,
        ask: Ask<Parked>,
    ) -> Result<Walked<Self::Out>, Fault>;

    /// Hands it a response on its way out: a pure layer changes it in place or answers in its place, and an actor
    /// layer is asked, keeping `ask`'s context until it answers.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn outbound(
        &mut self,
        head: &Head,
        leaving: &mut Leaving<'_>,
        turn: &mut Turn<'_, Me>,
        ask: Ask<ParkedResponse>,
    ) -> Result<Returned, Fault>;
}

/// What a layer is, as a part of a body reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taker {
    /// A pure layer, handed the part in the stack's turn.
    Pure,
    /// An actor layer, the actor this refers to: asked the part when its type lists it, and passed it untouched when
    /// not.
    Actor(ErasedActorRef),
}

/// How a stack asks an actor layer: keeping `context` until it answers, by `due`.
#[derive(Debug)]
pub struct Ask<C> {
    context: C,
    due: Time,
}

impl<C> Ask<C> {
    /// The ask that keeps `context`, and is answered by `due`.
    pub(super) const fn new(context: C, due: Time) -> Self {
        Self { context, due }
    }
}

/// What became of a request at one layer on the way in.
#[derive(Debug)]
pub enum Walked<Out> {
    /// It goes on, with `Out` attached.
    Passed(Out),
    /// The layer answered it, and it turns back.
    Answered(Response),
    /// The layer is an actor, asked: the request is parked until it answers.
    Parked,
}

/// What became of a response at one layer on the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Returned {
    /// It goes on, its head as the layer left it.
    Passed,
    /// The layer answered in its place.
    Answered,
    /// The layer is an actor, asked: the response is parked until it answers.
    Parked,
}

/// A pure layer is held as itself.
impl<L> Formed for L
where
    L: Layer + Encode<Within> + Decode<Within>,
{
    fn taker(&self) -> Taker {
        Taker::Pure
    }

    fn inbound_body(&mut self, part: &mut BodyPart<'_>) {
        Layer::inbound_body(self, part);
    }

    fn outbound_body(&mut self, part: &mut BodyPart<'_>) {
        Layer::outbound_body(self, part);
    }
}

/// A pure layer is made with `Default`, whoever the stack is: it reaches no actor.
impl<Me, L> FoundBy<Me, ()> for L
where
    L: Layer + Encode<Within> + Decode<Within>,
{
    fn found(_: &Beginning<'_, Me>) -> Result<Self, Fault> {
        Ok(Self::default())
    }
}

impl<L> Walks<L::In> for L
where
    L: Layer + Encode<Within> + Decode<Within>,
    L::Out: 'static,
{
    type Out = L::Out;
}

/// A pure layer is called in the stack's turn, and asks nothing.
impl<Me, L> WalksFor<Me, L::In> for L
where
    L: Layer + Encode<Within> + Decode<Within>,
    L::Out: 'static,
{
    fn inbound(
        &mut self,
        head: &Head,
        attached: L::In,
        _: &mut Turn<'_, Me>,
        _: Ask<Parked>,
    ) -> Result<Walked<L::Out>, Fault> {
        let walked = match Layer::inbound(self, head, attached) {
            Step::Pass(attached) => Walked::Passed(attached),
            Step::Answer(response) => Walked::Answered(response),
        };

        Ok(walked)
    }

    fn outbound(
        &mut self,
        head: &Head,
        leaving: &mut Leaving<'_>,
        _: &mut Turn<'_, Me>,
        _: Ask<ParkedResponse>,
    ) -> Result<Returned, Fault> {
        Layer::outbound(self, head, leaving);
        let returned = if leaving.answered() {
            Returned::Answered
        } else {
            Returned::Passed
        };

        Ok(returned)
    }
}

/// An actor layer is held as a reference to it, and asked through it with no cast: its type says what it answers each
/// head with, `Carries`.
impl<A> Formed for ActorRef<A>
where
    A: Actor + ChildOf + PathTo<Keys = ()> + Handles<Outbound> + Carries<Outbound, Answers = (Outward,)>,
{
    fn taker(&self) -> Taker {
        Taker::Actor(ErasedActorRef::from(*self))
    }

    fn inbound_body(&mut self, _: &mut BodyPart<'_>) {}

    fn outbound_body(&mut self, _: &mut BodyPart<'_>) {}
}

/// An actor layer or a lone handler, a singleton, is found at its typed path, beneath a root of a type the stack
/// depends on.
impl<Me, A, I> FoundBy<Me, I> for ActorRef<A>
where
    A: Actor + ChildOf + PathTo<Keys = ()>,
    Me: DependsOn<A, I>,
{
    fn found(turn: &Beginning<'_, Me>) -> Result<Self, Fault> {
        turn.found(&path::<A>(())).ok_or_else(unlayered::<A>)
    }
}

/// An actor layer's `In` is the kind of `Inbound` it handles, and its `Out` is what it says it answers that with.
impl<A, In, Out> Walks<In> for ActorRef<A>
where
    A: Actor
        + ChildOf
        + PathTo<Keys = ()>
        + Handles<Inbound<In>>
        + Carries<Inbound<In>, Answers = (Inward<Out>,)>
        + Handles<Outbound>
        + Carries<Outbound, Answers = (Outward,)>,
    Inbound<In>: Kind + Handled + Encode<Actors>,
    Inward<Out>: Kind,
    In: Plain,
    Out: Plain + 'static,
{
    type Out = Out;
}

/// An actor layer is asked about each head, in and out, by a stack that handles each answer with the context it keeps.
impl<Me, A, In, Out> WalksFor<Me, In> for ActorRef<A>
where
    Self: Walks<In, Out = Out>,
    A: Actor
        + Handles<Inbound<In>>
        + Carries<Inbound<In>, Answers = (Inward<Out>,)>
        + Handles<Outbound>
        + Carries<Outbound, Answers = (Outward,)>,
    Inbound<In>: Kind + Handled + Encode<Actors>,
    <A as Handles<Inbound<In>>>::From: Admits<A, Me, ()>,
    <A as Handles<Outbound>>::From: Admits<A, Me, ()>,
    Inward<Out>: Kind + Decode<Actors> + 'static,
    Me: Receives<Answer<Inward<Out>, Parked>> + Receives<Answer<Outward, ParkedResponse>>,
{
    fn inbound(
        &mut self,
        head: &Head,
        attached: In,
        turn: &mut Turn<'_, Me>,
        ask: Ask<Parked>,
    ) -> Result<Walked<Out>, Fault> {
        let asked = Inbound { head: head.clone(), attached };
        turn.to(*self).within(ask.due).ask(&asked, &ask.context)?;

        Ok(Walked::Parked)
    }

    fn outbound(
        &mut self,
        head: &Head,
        leaving: &mut Leaving<'_>,
        turn: &mut Turn<'_, Me>,
        ask: Ask<ParkedResponse>,
    ) -> Result<Returned, Fault> {
        let asked = Outbound { head: head.clone(), response: leaving.head().clone() };
        turn.to(*self).within(ask.due).ask(&asked, &ask.context)?;

        Ok(Returned::Parked)
    }
}

/// The actor layer or handler `A` is not live, as the fault a stack's `begin` fails with.
pub(super) fn unlayered<A: Kind>() -> Fault {
    Fault::new(&Unlayered { layer: A::NAME }).unwrap_or_else(Fault::from)
}
