//! A stack's last layer (ADR-0035): the layer that answers, a handler asked every request its layers pass, or a routes
//! type, whose router picks the handler by method and path. What the stack holds of it, how it picks the handler a
//! request reaches, and how the stack asks that handler, through the one ask its sends list for it.

use super::walks::{Unasked, unlayered};
use super::{Answering, BodyHandle, Request, Response};
use crate::Method;
use crate::router::{Captures, Route};
use aethereal_actor::{
    Actor, ActorRef, Actors, Allows, AnswersByType, Asks, At, Beginning, ChildOf, Due, Fault, Handled, PathTo, Turn,
    Within, path,
};
use aethereal_data::{Decode, Encode, Kind, Plain};
use aethereal_tcp::SocketAddress;

/// The last layer of a stack, as the stack holds it: the handler, `ActorRef<H>`, asked every request its layers pass,
/// whatever its method and path; or a routes type's [`Router`](crate::Router), which picks the handler. `A` is what the
/// layers before it attach.
pub trait LastLayer<A>: Encode<Within> + Decode<Within> + Clone {
    /// The handlers it reaches, as it holds them: the handler's reference, or the routes' handlers and the paths their
    /// routes make of their captures.
    type Reaches;
    /// What the stack's sends list to ask its handlers, twelve places: one ask for each handler, in order, and then
    /// [`Unasked`]. The stack lists as many of them as the places its layers leave.
    type Asked: Slot12;

    /// It, as the stack begins: the handler found at its typed path as `turn` sees the engine, or the routes compiled.
    ///
    /// # Errors
    ///
    /// The fault [`Unlayered`](super::Unlayered), naming a handler that is not live; or
    /// [`Unroutable`](crate::Unroutable), when the routes are refused.
    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self, Fault>;

    /// What `method` and `path` reach: the handler and the slices of the path its route captured, or the router's
    /// answer, `404` or `405`.
    fn picked<'p>(&self, method: Method, path: &'p str) -> Picked<'p>;

    /// The handlers it reaches.
    fn reaches(&self) -> &Self::Reaches;
}

/// A tuple of twelve places, each reached by [`Slot`](super::Slot): what a last layer's asks of its handlers are.
pub trait Slot12:
    super::Slot<At<0>>
    + super::Slot<At<1>>
    + super::Slot<At<2>>
    + super::Slot<At<3>>
    + super::Slot<At<4>>
    + super::Slot<At<5>>
    + super::Slot<At<6>>
    + super::Slot<At<7>>
    + super::Slot<At<8>>
    + super::Slot<At<9>>
    + super::Slot<At<10>>
    + super::Slot<At<11>>
{
}

impl<T> Slot12 for T where
    T: super::Slot<At<0>>
        + super::Slot<At<1>>
        + super::Slot<At<2>>
        + super::Slot<At<3>>
        + super::Slot<At<4>>
        + super::Slot<At<5>>
        + super::Slot<At<6>>
        + super::Slot<At<7>>
        + super::Slot<At<8>>
        + super::Slot<At<9>>
        + super::Slot<At<10>>
        + super::Slot<At<11>>
{
}

/// What a request's method and path reach at a stack's last layer.
#[derive(Debug)]
pub enum Picked<'p> {
    /// The handler `at` names, its route's captures the `slices` of the path they matched, as sent.
    Handler { at: Reach, slices: Vec<&'p str> },
    /// The router's answer: no route matches, `404`; or routes match the path but not its method, `405`.
    Answer(Response),
}

/// Which handler of a last layer a request reaches, and by which of its routes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    /// The handler's place among the last layer's handlers.
    pub handler: usize,
    /// The route's place among that handler's routes.
    pub route: usize,
}

/// A request a stack asks its handler, but its captures, which the handler's route reads: every layer has passed it.
#[derive(Debug)]
pub struct Unrouted<A> {
    pub method: Method,
    pub path: String,
    pub query: String,
    pub headers: Vec<crate::Header>,
    pub attached: A,
    pub body: BodyHandle,
    pub peer: SocketAddress,
}

impl<A> Unrouted<A> {
    /// The request, its captures `captures`.
    fn captured<C>(self, captures: C) -> Request<A, C> {
        let Self { method, path, query, headers, attached, body, peer } = self;

        Request { method, path, query, headers, attached, captures, body, peer }
    }
}

/// What became of a request its last layer reached a handler for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handed {
    /// The handler was asked.
    Asked,
    /// Its captures did not read as their type, or nothing live stands at the path they make: `404`.
    Unreached,
}

/// A stack's sends, `Self`, list the ask of each handler `L` reaches, at `Is`, the places the compiler infers: what lets
/// the stack ask whichever handler a request reaches through the ask its sends list for it, each checked as its own.
pub trait AsksEach<A, L, Is>: Sized {
    /// Asks the handler `at` names of `reaches` the request `unrouted`, its captures read from `slices`, within no
    /// deadline, keeping `context`: or none, when its captures do not read, or nothing live stands where they lead.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn ask_reached<Me>(
        turn: &mut Turn<'_, Me, Self>,
        reaches: &L,
        at: Reach,
        slices: &[&str],
        unrouted: Unrouted<A>,
        context: &Answering,
    ) -> Result<Handed, Fault>;
}

/// A handler a stack's sends ask `Request<A, C>` of, keeping [`Answering`].
type AskOf<H, A, C> = Asks<ActorRef<H>, Request<A, C>, Answering>;

/// Asks `handler` the request `unrouted`, its captures `captures`, within no deadline: a body may take as long as it
/// keeps moving, which the stack times itself, and once it is read the stack times the answer (ADR-0041).
fn ask_handler<Me, S, H, A, C, I>(
    turn: &mut Turn<'_, Me, S>,
    handler: ActorRef<H>,
    unrouted: Unrouted<A>,
    captures: C,
    context: &Answering,
) -> Result<Handed, Fault>
where
    H: AnswersByType<Request<A, C>, Answer = Response>,
    Request<A, C>: Kind + Encode<Actors> + Handled,
    S: Allows<AskOf<H, A, C>, I>,
{
    turn.to(handler).within(Due::Never).ask(&unrouted.captured(captures), context)?;

    Ok(Handed::Asked)
}

/// A handler alone is the last layer: a singleton root, found at its typed path as the stack begins, and asked every
/// request its layers pass, whatever its method and path, with no captures.
impl<A, H> LastLayer<A> for ActorRef<H>
where
    A: Plain,
    H: Actor + ChildOf + PathTo<Keys = ()> + AnswersByType<Request<A, ()>, Answer = Response>,
    Request<A, ()>: Kind + Encode<Actors> + Handled,
{
    type Reaches = Self;
    type Asked = (
        AskOf<H, A, ()>,
        Unasked,
        Unasked,
        Unasked,
        Unasked,
        Unasked,
        Unasked,
        Unasked,
        Unasked,
        Unasked,
        Unasked,
        Unasked,
    );

    fn found<Me, S>(turn: &Beginning<'_, Me, S>) -> Result<Self, Fault> {
        turn.found(&path::<H>(())).ok_or_else(unlayered::<H>)
    }

    fn picked<'p>(&self, _: Method, _: &'p str) -> Picked<'p> {
        Picked::Handler { at: Reach { handler: 0, route: 0 }, slices: Vec::new() }
    }

    fn reaches(&self) -> &Self {
        self
    }
}

impl<S, A, H, I> AsksEach<A, ActorRef<H>, (I,)> for S
where
    H: AnswersByType<Request<A, ()>, Answer = Response>,
    Request<A, ()>: Kind + Encode<Actors> + Handled,
    S: Allows<AskOf<H, A, ()>, I>,
{
    fn ask_reached<Me>(
        turn: &mut Turn<'_, Me, Self>,
        handler: &ActorRef<H>,
        _: Reach,
        _: &[&str],
        unrouted: Unrouted<A>,
        context: &Answering,
    ) -> Result<Handed, Fault> {
        ask_handler(turn, *handler, unrouted, (), context)
    }
}

/// Asks the handler of `route` the request `unrouted`, by the route numbered `at` among its routes: its captures read
/// from `slices`, once for the path they make and once for the request, and the handler found live at that path.
fn ask_routed<Me, S, H, A, C, I>(
    turn: &mut Turn<'_, Me, S>,
    route: &Route<H, C>,
    at: usize,
    slices: &[&str],
    unrouted: Unrouted<A>,
    context: &Answering,
) -> Result<Handed, Fault>
where
    H: ChildOf + PathTo + AnswersByType<Request<A, C>, Answer = Response>,
    C: Captures,
    Request<A, C>: Kind + Encode<Actors> + Handled,
    S: Allows<AskOf<H, A, C>, I>,
{
    let Some(to) = route.path_of(at) else {
        return Ok(Handed::Unreached);
    };
    let Some(keys) = C::read(slices) else {
        return Ok(Handed::Unreached);
    };
    let Some(handler) = turn.found(&to(keys)) else {
        return Ok(Handed::Unreached);
    };
    let Some(captures) = C::read(slices) else {
        return Ok(Handed::Unreached);
    };

    ask_handler(turn, handler, unrouted, captures, context)
}

/// A routes type's handlers, a tuple of up to twelve [`Route`]s, each a handler and its routes' captures type, which
/// its router holds: what the stack's sends list to ask them, each `Request<A, C>` of its captures.
pub trait HandlerList<A>: Default + Clone {
    /// An ask for each handler, in order, and [`Unasked`] past the last of twelve places.
    type Asked: Slot12;
}

/// The asks of routes to up to twelve handlers, each with its place in a tuple of them and its type parameters, and
/// an [`Unasked`] for each place past the last.
macro_rules! routed {
    ($( [$($handler:ident $captures:ident $index:ident $place:tt)+] [$($unasked:ident)*]; )*) => {$(
        impl<A, $($handler, $captures),+> HandlerList<A> for ($(Route<$handler, $captures>,)+) {
            type Asked = ($(AskOf<$handler, A, $captures>,)+ $($unasked,)*);
        }

        impl<S, A, $($handler, $captures, $index),+> AsksEach<A, ($(Route<$handler, $captures>,)+), ($($index,)+)> for S
        where
            $(
                $handler: ChildOf + PathTo + AnswersByType<Request<A, $captures>, Answer = Response>,
                $captures: Captures,
                Request<A, $captures>: Kind + Encode<Actors> + Handled,
                S: Allows<AskOf<$handler, A, $captures>, $index>,
            )+
        {
            fn ask_reached<Me>(
                turn: &mut Turn<'_, Me, Self>,
                routes: &($(Route<$handler, $captures>,)+),
                at: Reach,
                slices: &[&str],
                unrouted: Unrouted<A>,
                context: &Answering,
            ) -> Result<Handed, Fault> {
                match at.handler {
                    $($place => ask_routed(turn, &routes.$place, at.route, slices, unrouted, context),)+
                    _ => Ok(Handed::Unreached),
                }
            }
        }
    )*};
}

routed! {
    [H0 C0 I0 0] [Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1] [Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2] [Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3] [Unasked Unasked Unasked Unasked Unasked Unasked Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4] [Unasked Unasked Unasked Unasked Unasked Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5] [Unasked Unasked Unasked Unasked Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6] [Unasked Unasked Unasked Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7] [Unasked Unasked Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7 H8 C8 I8 8] [Unasked Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7 H8 C8 I8 8 H9 C9 I9 9] [Unasked Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7 H8 C8 I8 8 H9 C9 I9 9 H10 C10 I10 10] [Unasked];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7 H8 C8 I8 8 H9 C9 I9 9 H10 C10 I10 10 H11 C11 I11 11] [];
}
