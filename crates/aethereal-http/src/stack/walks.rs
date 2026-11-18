//! What a stack holds of each layer before its handler, its form, and how it walks a request through that layer
//! (ADR-0035): a pure layer is called in the stack's turn, and an actor layer asked through its reference, which parks
//! the request until it answers.

use super::actor::{Inbound, Inward, Outbound, Outward, Parked, ParkedResponse};
use super::{BodyPart, Gathered, Head, Layer, Leaving, Response, Step, Unlayered};
use aethereal_actor::{
    Actor, ActorRef, Actors, Allows, Asks, Beginning, Carries, ChildOf, ErasedActorRef, Fault, Handled, Handles,
    PathTo, Time, ToSender, Turn, Within, path,
};
use aethereal_data::{Decode, Encode, Kind, Plain};
use core::marker::PhantomData;

/// What a stack's sends list in place of an ask it never makes, of a pure layer or of a place past its last layer: a
/// reply to a gather, which every stack lists already, so the list says nothing it did not.
pub type Unasked = ToSender<Gathered>;

/// A layer before the handler, as a stack holds it: its form, a pure layer itself or a reference to an actor layer.
/// What the stack does with it whatever it attaches: finds it as the stack begins, asks it about a response on the
/// way out, and hands it the parts of a body. A layer's author writes none of this; `aethereal-http` writes it for
/// every pure layer and every actor layer.
pub trait Formed: Encode<Within> + Decode<Within> + Clone {
    /// What the stack's sends list to ask it about a response's head on the way out: [`Unasked`] for a pure layer.
    type Outbound;

    /// The layer, as the stack holds it once it begins: a pure layer made with `Default`, and an actor layer found at
    /// its typed path as `turn` sees the engine.
    ///
    /// # Errors
    ///
    /// The fault [`Unlayered`], naming an actor layer's type, when it is not live.
    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self, Fault>;

    /// What it is, as a part of a body reaches it.
    fn taker(&self) -> Taker;

    /// Hands it a response on its way out: a pure layer changes it in place or answers in its place, and an actor
    /// layer is asked, keeping `ask`'s context until it answers.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn outbound<Me, S, I>(
        &mut self,
        head: &Head,
        leaving: &mut Leaving<'_>,
        turn: &mut Turn<'_, Me, S>,
        ask: Ask<I, ParkedResponse>,
    ) -> Result<Returned, Fault>
    where
        S: Allows<Self::Outbound, I>;

    /// Hands a pure layer a part of the request's body. An actor layer is asked through a cast instead.
    fn inbound_body(&mut self, part: &mut BodyPart<'_>);

    /// Hands a pure layer a part of the response's body. An actor layer is asked through a cast instead.
    fn outbound_body(&mut self, part: &mut BodyPart<'_>);
}

/// A layer before the handler, as a stack holds it, walked by a request the layers before it attached `In` to: what it
/// passes on attached, and how it is asked on the way in.
pub trait Walks<In>: Formed {
    /// What it passes on attached.
    type Out: Plain + 'static;
    /// What the stack's sends list to ask it about a request's head on the way in: [`Unasked`] for a pure layer.
    type Inbound;

    /// Hands it a request's head on the way in, `attached` what the layers before it attached: a pure layer passes or
    /// answers, and an actor layer is asked, keeping `ask`'s context until it answers.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn inbound<Me, S, I>(
        &mut self,
        head: &Head,
        attached: In,
        turn: &mut Turn<'_, Me, S>,
        ask: Ask<I, Parked>,
    ) -> Result<Walked<Self::Out>, Fault>
    where
        S: Allows<Self::Inbound, I>;
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

/// How a stack asks an actor layer: keeping `context` until it answers, by `due`, through the ask its sends list at
/// `I`.
#[derive(Debug)]
pub struct Ask<I, C> {
    context: C,
    due: Time,
    place: PhantomData<fn() -> I>,
}

impl<I, C> Ask<I, C> {
    /// The ask that keeps `context`, and is answered by `due`.
    pub(super) const fn new(context: C, due: Time) -> Self {
        Self { context, due, place: PhantomData }
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

/// A pure layer is held as itself, and called in the stack's turn.
impl<L> Formed for L
where
    L: Layer + Encode<Within> + Decode<Within>,
{
    type Outbound = Unasked;

    fn found<Me, S>(_: &Beginning<'_, Me, S>) -> Result<Self, Fault> {
        Ok(Self::default())
    }

    fn taker(&self) -> Taker {
        Taker::Pure
    }

    fn outbound<Me, S, I>(
        &mut self,
        head: &Head,
        leaving: &mut Leaving<'_>,
        _: &mut Turn<'_, Me, S>,
        _: Ask<I, ParkedResponse>,
    ) -> Result<Returned, Fault>
    where
        S: Allows<Unasked, I>,
    {
        Layer::outbound(self, head, leaving);
        let returned = if leaving.answered() {
            Returned::Answered
        } else {
            Returned::Passed
        };

        Ok(returned)
    }

    fn inbound_body(&mut self, part: &mut BodyPart<'_>) {
        Layer::inbound_body(self, part);
    }

    fn outbound_body(&mut self, part: &mut BodyPart<'_>) {
        Layer::outbound_body(self, part);
    }
}

impl<L> Walks<L::In> for L
where
    L: Layer + Encode<Within> + Decode<Within>,
    L::Out: 'static,
{
    type Out = L::Out;
    type Inbound = Unasked;

    fn inbound<Me, S, I>(
        &mut self,
        head: &Head,
        attached: L::In,
        _: &mut Turn<'_, Me, S>,
        _: Ask<I, Parked>,
    ) -> Result<Walked<L::Out>, Fault>
    where
        S: Allows<Unasked, I>,
    {
        let walked = match Layer::inbound(self, head, attached) {
            Step::Pass(attached) => Walked::Passed(attached),
            Step::Answer(response) => Walked::Answered(response),
        };

        Ok(walked)
    }
}

/// An actor layer is held as a reference to it, found at its typed path, and asked through it with no cast: its type
/// says what it answers each head with, `Carries`.
impl<A> Formed for ActorRef<A>
where
    A: Actor + ChildOf + PathTo<Keys = ()> + Handles<Outbound> + Carries<Outbound, Answers = (Outward,)>,
{
    type Outbound = Asks<Self, Outbound, ParkedResponse>;

    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self, Fault> {
        turn.found(&path::<A>(())).ok_or_else(unlayered::<A>)
    }

    fn taker(&self) -> Taker {
        Taker::Actor(ErasedActorRef::from(*self))
    }

    fn outbound<Me, S, I>(
        &mut self,
        head: &Head,
        leaving: &mut Leaving<'_>,
        turn: &mut Turn<'_, Me, S>,
        ask: Ask<I, ParkedResponse>,
    ) -> Result<Returned, Fault>
    where
        S: Allows<Self::Outbound, I>,
    {
        let asked = Outbound { head: head.clone(), response: leaving.head().clone() };
        turn.to(*self).within(ask.due).ask(&asked, &ask.context)?;

        Ok(Returned::Parked)
    }

    fn inbound_body(&mut self, _: &mut BodyPart<'_>) {}

    fn outbound_body(&mut self, _: &mut BodyPart<'_>) {}
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
    type Inbound = Asks<Self, Inbound<In>, Parked>;

    fn inbound<Me, S, I>(
        &mut self,
        head: &Head,
        attached: In,
        turn: &mut Turn<'_, Me, S>,
        ask: Ask<I, Parked>,
    ) -> Result<Walked<Out>, Fault>
    where
        S: Allows<Self::Inbound, I>,
    {
        let asked = Inbound { head: head.clone(), attached };
        turn.to(*self).within(ask.due).ask(&asked, &ask.context)?;

        Ok(Walked::Parked)
    }
}

/// The actor layer or handler `A` is not live, as the fault a stack's `begin` fails with.
pub(super) fn unlayered<A: Kind>() -> Fault {
    Fault::new(&Unlayered { layer: A::NAME }).unwrap_or_else(Fault::from)
}
