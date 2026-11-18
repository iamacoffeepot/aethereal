//! A stack's last layer (ADR-0035): the layer that answers, a handler asked every request its layers pass, or a routes
//! type, whose router picks the handler by method and path. What the stack holds of it, how it picks the handler a
//! request reaches, and how the stack asks that handler, each ask checked at its call.

use super::{Answering, BodyHandle, Request, Response};
use crate::Method;
use crate::router::{Captures, Route};
use aethereal_actor::{
    Actor, ActorRef, Actors, Admits, Answer, AnswersByType, ChildOf, DependsOn, Due, Fault, Handled, Handles, PathTo,
    Receives, Turn, Within,
};
use aethereal_data::{Decode, Encode, Kind, Plain};
use aethereal_tcp::SocketAddress;

/// The last layer of a stack, as the stack holds it: the handler, `ActorRef<H>`, asked every request its layers pass,
/// whatever its method and path; or a routes type's [`Router`](crate::Router), which picks the handler. `A` is what the
/// layers before it attach. The stack holds it once it begins as [`FoundBy`](super::FoundBy) says.
pub trait LastLayer<A>: Encode<Within> + Decode<Within> + Clone {
    /// The handlers it reaches, as it holds them: the handler's reference, or the routes' handlers and the paths their
    /// routes make of their captures. How a stack asks them is [`AskedBy`].
    type Reaches;

    /// What `method` and `path` reach: the handler and the slices of the path its route captured, or the router's
    /// answer, `404` or `405`.
    fn picked<'p>(&self, method: Method, path: &'p str) -> Picked<'p>;

    /// The handlers it reaches.
    fn reaches(&self) -> &Self::Reaches;
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

/// The handlers a last layer reaches, `Self`, asked the requests its layers attach `A` to by a stack of type `Me`:
/// each ask is checked at its call, and builds when `Me` handles the [`Response`] with [`Answering`], and, for a
/// routed handler found at the path its captures make, when its root is the root of a type `Me` depends on, at `Is`,
/// the places in `Me`'s depends the compiler infers (ADR-0059).
pub trait AskedBy<Me, A, Is>: Sized {
    /// Asks the handler `at` names the request `unrouted`, its captures read from `slices`, within no deadline, keeping
    /// `context`: or none, when its captures do not read, or nothing live stands where they lead.
    ///
    /// # Errors
    ///
    /// The fault of an ask that does not encode.
    fn ask_reached(
        &self,
        turn: &mut Turn<'_, Me>,
        at: Reach,
        slices: &[&str],
        unrouted: Unrouted<A>,
        context: &Answering,
    ) -> Result<Handed, Fault>;
}

/// Asks `handler` the request `unrouted`, its captures `captures`, within no deadline: a body may take as long as it
/// keeps moving, which the stack times itself, and once it is read the stack times the answer (ADR-0041).
fn ask_handler<Me, H, A, C>(
    turn: &mut Turn<'_, Me>,
    handler: ActorRef<H>,
    unrouted: Unrouted<A>,
    captures: C,
    context: &Answering,
) -> Result<Handed, Fault>
where
    H: AnswersByType<Request<A, C>, Answer = Response>,
    H::From: Admits<H, Me, ()>,
    Request<A, C>: Kind + Encode<Actors> + Handled,
    Me: Receives<Answer<Response, Answering>>,
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

    fn picked<'p>(&self, _: Method, _: &'p str) -> Picked<'p> {
        Picked::Handler { at: Reach { handler: 0, route: 0 }, slices: Vec::new() }
    }

    fn reaches(&self) -> &Self {
        self
    }
}

/// A lone handler is asked through its reference, found as the stack began.
impl<Me, A, H> AskedBy<Me, A, ()> for ActorRef<H>
where
    H: AnswersByType<Request<A, ()>, Answer = Response>,
    H::From: Admits<H, Me, ()>,
    Request<A, ()>: Kind + Encode<Actors> + Handled,
    Me: Receives<Answer<Response, Answering>>,
{
    fn ask_reached(
        &self,
        turn: &mut Turn<'_, Me>,
        _: Reach,
        _: &[&str],
        unrouted: Unrouted<A>,
        context: &Answering,
    ) -> Result<Handed, Fault> {
        ask_handler(turn, *self, unrouted, (), context)
    }
}

/// Asks the handler of `route` the request `unrouted`, by the route numbered `at` among its routes: its captures read
/// from `slices`, and the handler found live at the path they make, beneath a root of a type `Me` depends on, at `I`.
fn ask_routed<Me, H, A, C, I>(
    turn: &mut Turn<'_, Me>,
    route: &Route<H, C>,
    at: usize,
    slices: &[&str],
    unrouted: Unrouted<A>,
    context: &Answering,
) -> Result<Handed, Fault>
where
    H: ChildOf + PathTo + AnswersByType<Request<A, C>, Answer = Response>,
    H::From: Admits<H, Me, ()>,
    C: Captures,
    Request<A, C>: Kind + Encode<Actors> + Handled,
    Me: Receives<Answer<Response, Answering>> + DependsOn<H, I>,
{
    let Some(to) = route.path_of(at) else {
        return Ok(Handed::Unreached);
    };
    let Some(captures) = C::read(slices) else {
        return Ok(Handed::Unreached);
    };
    let Some(handler) = turn.found(&to(captures.clone())) else {
        return Ok(Handed::Unreached);
    };

    ask_handler(turn, handler, unrouted, captures, context)
}

/// A routes type's handlers: a tuple of up to twelve [`Route`]s, each a handler and its routes' captures type, which
/// its router holds.
pub trait HandlerList: Default + Clone {}

/// The routes to up to twelve handlers, each with its place in a tuple of them, its type parameters, and the place in
/// the asking stack's depends of its handler's root.
macro_rules! routed {
    ($( [$($handler:ident $captures:ident $index:ident $place:tt)+]; )*) => {$(
        impl<$($handler, $captures),+> HandlerList for ($(Route<$handler, $captures>,)+) {}

        impl<Me, A, $($handler, $captures, $index),+> AskedBy<Me, A, ($($index,)+)> for ($(Route<$handler, $captures>,)+)
        where
            Me: Receives<Answer<Response, Answering>>,
            $(
                $handler: ChildOf + PathTo + AnswersByType<Request<A, $captures>, Answer = Response>,
                <$handler as Handles<Request<A, $captures>>>::From: Admits<$handler, Me, ()>,
                $captures: Captures,
                Request<A, $captures>: Kind + Encode<Actors> + Handled,
                Me: DependsOn<$handler, $index>,
            )+
        {
            fn ask_reached(
                &self,
                turn: &mut Turn<'_, Me>,
                at: Reach,
                slices: &[&str],
                unrouted: Unrouted<A>,
                context: &Answering,
            ) -> Result<Handed, Fault> {
                match at.handler {
                    $($place => ask_routed(turn, &self.$place, at.route, slices, unrouted, context),)+
                    _ => Ok(Handed::Unreached),
                }
            }
        }
    )*};
}

routed! {
    [H0 C0 I0 0];
    [H0 C0 I0 0 H1 C1 I1 1];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7 H8 C8 I8 8];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7 H8 C8 I8 8 H9 C9 I9 9];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7 H8 C8 I8 8 H9 C9 I9 9 H10 C10 I10 10];
    [H0 C0 I0 0 H1 C1 I1 1 H2 C2 I2 2 H3 C3 I3 3 H4 C4 I4 4 H5 C5 I5 5 H6 C6 I6 6 H7 C7 I7 7 H8 C8 I8 8 H9 C9 I9 9 H10 C10 I10 10 H11 C11 I11 11];
}
