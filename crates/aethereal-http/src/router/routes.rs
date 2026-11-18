//! A stack's routes (ADR-0035): a routes type, the last layer of a stack with routes, names the handlers its router may
//! ask and gives its routes to an empty router, which compiles them to the trie a request walks.

use super::{Captures, Found, Routed, Trie, Unroutable};
use crate::stack::{FoundBy, HandlerList, LastLayer, Picked, Reach, Request, Response, Slot};
use crate::{Header, Method, Status};
use aethereal_actor::{Actor, Beginning, Fault, Lists, Path};
use aethereal_data::{Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use core::marker::PhantomData;

/// The last layer of a stack with routes: the handlers its router may ask, and its routes, each to a handler at the
/// path it makes of its captures.
///
/// Its form, the line beside it as a pure layer writes one, is its router: `type Form = Router<Self>;`.
///
/// A routes type that lists both its handlers:
///
/// ```
/// # use aethereal_actor::{Actor, Beginning, Carries, ChildOf, Ending, Fault, Handles, Top, Turn, path};
/// # use aethereal_data::Plain;
/// # use aethereal_http::stack::{FromStack, LayerForm, Request, Response};
/// # use aethereal_http::{Method, Route, Router, Routes, Status, Unroutable};
/// #
/// # macro_rules! handler {
/// #     ($handler:ident, $name:literal) => {
/// #         #[derive(Plain)]
/// #         #[kind(name = $name)]
/// #         struct $handler;
/// #
/// #         impl ChildOf for $handler {
/// #             type Parent = Top;
/// #         }
/// #
/// #         impl Actor for $handler {
/// #             type Config = Self;
/// #             type Key = ();
/// #             type Handles = (Request<(), ()>,);
/// #             type Depends = ();
/// #             type Spawns = ();
/// #
/// #             fn begin(handler: Self, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 Ok(handler)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #
/// #         impl Carries<Request<(), ()>> for $handler {
/// #             type Answers = (Response,);
/// #         }
/// #
/// #         impl Handles<Request<(), ()>> for $handler {
/// #             type From = FromStack;
/// #
/// #             fn handle(
/// #                 &mut self,
/// #                 _: Request<(), ()>,
/// #                 turn: &mut Turn<'_, Self, Self::From>,
/// #             ) -> Result<(), Fault> {
/// #                 Ok(turn.reply(&Response::empty(Status::Ok))?)
/// #             }
/// #         }
/// #     };
/// # }
/// #
/// # handler!(Mcp, "aether.example.mcp");
/// # handler!(Git, "aether.example.git");
/// #
/// struct ServedRoutes;
///
/// impl LayerForm for ServedRoutes {
///     type Form = Router<Self>;
/// }
///
/// impl Routes for ServedRoutes {
///     type Attached = ();
///     type Handlers = (Route<Mcp, ()>, Route<Git, ()>);
///
///     fn routes(router: Router<Self>) -> Result<Router<Self>, Unroutable> {
///         router
///             .route(Method::Post, "/mcp", |()| path::<Mcp>(()))?
///             .route(Method::Get, "/git", |()| path::<Git>(()))
///     }
/// }
/// ```
///
/// A route to a handler its routes type does not list does not build, as its router holds no place for it:
///
/// ```compile_fail
/// # use aethereal_actor::{Actor, Beginning, Carries, ChildOf, Ending, Fault, Handles, Top, Turn, path};
/// # use aethereal_data::Plain;
/// # use aethereal_http::stack::{FromStack, LayerForm, Request, Response};
/// # use aethereal_http::{Method, Route, Router, Routes, Status, Unroutable};
/// #
/// # macro_rules! handler {
/// #     ($handler:ident, $name:literal) => {
/// #         #[derive(Plain)]
/// #         #[kind(name = $name)]
/// #         struct $handler;
/// #
/// #         impl ChildOf for $handler {
/// #             type Parent = Top;
/// #         }
/// #
/// #         impl Actor for $handler {
/// #             type Config = Self;
/// #             type Key = ();
/// #             type Handles = (Request<(), ()>,);
/// #             type Depends = ();
/// #             type Spawns = ();
/// #
/// #             fn begin(handler: Self, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
/// #                 Ok(handler)
/// #             }
/// #
/// #             fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
/// #                 Ok(())
/// #             }
/// #         }
/// #
/// #         impl Carries<Request<(), ()>> for $handler {
/// #             type Answers = (Response,);
/// #         }
/// #
/// #         impl Handles<Request<(), ()>> for $handler {
/// #             type From = FromStack;
/// #
/// #             fn handle(
/// #                 &mut self,
/// #                 _: Request<(), ()>,
/// #                 turn: &mut Turn<'_, Self, Self::From>,
/// #             ) -> Result<(), Fault> {
/// #                 Ok(turn.reply(&Response::empty(Status::Ok))?)
/// #             }
/// #         }
/// #     };
/// # }
/// #
/// # handler!(Mcp, "aether.example.mcp");
/// # handler!(Git, "aether.example.git");
/// #
/// struct ServedRoutes;
///
/// impl LayerForm for ServedRoutes {
///     type Form = Router<Self>;
/// }
///
/// impl Routes for ServedRoutes {
///     type Attached = ();
///     type Handlers = (Route<Mcp, ()>,);
///
///     fn routes(router: Router<Self>) -> Result<Router<Self>, Unroutable> {
///         router
///             .route(Method::Post, "/mcp", |()| path::<Mcp>(()))?
///             .route(Method::Get, "/git", |()| path::<Git>(()))
///     }
/// }
/// ```
pub trait Routes: Sized + 'static {
    /// What the layers before it attach: `A` of every route's handler.
    type Attached: Plain;
    /// Every handler its router may ask, each a [`Route`] of the handler's type and its routes' captures type, in a
    /// tuple: the stack asks whichever a request reaches.
    type Handlers: HandlerList;

    /// `router`, empty, with this type's routes.
    ///
    /// # Errors
    ///
    /// [`Unroutable`], which fails the stack's `begin`, when a route is refused.
    fn routes(router: Router<Self>) -> Result<Router<Self>, Unroutable>;
}

/// A handler a routes type lists, with the captures type of its routes, and, in its router, the path each of its
/// routes makes of its captures.
pub struct Route<H, C> {
    paths: Vec<fn(C) -> Path<H>>,
}

impl<H, C> Route<H, C> {
    /// The path the route numbered `at` among the handler's makes of its captures: none past its routes.
    pub(crate) fn path_of(&self, at: usize) -> Option<fn(C) -> Path<H>> {
        self.paths.get(at).copied()
    }
}

impl<H, C> Default for Route<H, C> {
    fn default() -> Self {
        Self { paths: Vec::new() }
    }
}

impl<H, C> Clone for Route<H, C> {
    fn clone(&self) -> Self {
        Self { paths: self.paths.clone() }
    }
}

/// A routes type's router: the trie its routes compile to, each route's value which handler it reaches and by which of
/// its routes, and the path each route makes of its captures.
///
/// It is code, not data: the stack holds it as its routes type's form, and it encodes as nothing, so a stack's state
/// decoded compiles it again from its routes.
pub struct Router<R: Routes> {
    trie: Trie<Reach>,
    handlers: R::Handlers,
    of: PhantomData<fn() -> R>,
}

impl<R: Routes> Router<R> {
    /// The router with no route.
    fn empty() -> Self {
        Self { trie: Trie::default(), handlers: R::Handlers::default(), of: PhantomData }
    }

    /// The router of `R`'s routes.
    fn compiled() -> Result<Self, Unroutable> {
        R::routes(Self::empty())
    }

    /// The router with a route for `method` on `pattern` to the handler at the path `to` makes of its captures: a
    /// handler `R` lists among its handlers, which lists `Request<A, C>`, its captures `C` found by the index `I`, so
    /// none is written at the call. `to` is a function, a closure that captures nothing: `|()| path::<Mcp>(())`, or
    /// `|repo: Repo| path::<Git>(repo)` for an instanced handler. A route to a handler `R` does not list does not build.
    ///
    /// # Errors
    ///
    /// [`Unroutable`]: `Malformed` when the pattern is, `Uncaptured` when its `{names}` are not `C`'s, and `Overlaps`
    /// when it matches a path another route for `method` does.
    pub fn route<H, C, I, J>(mut self, method: Method, pattern: &str, to: fn(C) -> Path<H>) -> Result<Self, Unroutable>
    where
        H: Actor,
        C: Captures,
        H::Handles: Lists<Request<R::Attached, C>, I>,
        R::Handlers: Lists<Route<H, C>, J> + Slot<J, Item = Route<H, C>>,
    {
        let handler = <R::Handlers as Slot<J>>::PLACE;
        let routes = self.handlers.slot();
        let reach = Reach { handler, route: routes.paths.len() };
        self.trie = self.trie.route::<C>(method, pattern, reach)?;
        routes.paths.push(to);

        Ok(self)
    }
}

impl<R: Routes> Clone for Router<R> {
    fn clone(&self) -> Self {
        Self { trie: self.trie.clone(), handlers: self.handlers.clone(), of: PhantomData }
    }
}

/// A router is the last layer of a stack with routes: it picks the handler by method and path.
impl<R: Routes> LastLayer<R::Attached> for Router<R> {
    type Reaches = R::Handlers;

    fn picked<'p>(&self, method: Method, path: &'p str) -> Picked<'p> {
        match self.trie.find(method, path) {
            Routed::Found(Found { route, slices }) => Picked::Handler { at: *route, slices },
            Routed::NotFound => Picked::Answer(Response::empty(Status::NotFound)),
            Routed::Unallowed(methods) => Picked::Answer(unallowed(&methods)),
        }
    }

    fn reaches(&self) -> &R::Handlers {
        &self.handlers
    }
}

/// A router is compiled from its routes as the stack begins, whoever the stack is: it finds no actor, as each request
/// finds its handler at the path its captures make.
impl<Me, R: Routes> FoundBy<Me, ()> for Router<R> {
    fn found(_: &Beginning<'_, Me>) -> Result<Self, Fault> {
        Self::compiled().map_err(|unroutable| Fault::new(&unroutable).unwrap_or_else(Fault::from))
    }
}

/// `405`, with `Allow` naming `methods`.
fn unallowed(methods: &[Method]) -> Response {
    let allow = methods.iter().map(|method| method.token()).collect::<Vec<_>>().join(", ");
    let mut response = Response::empty(Status::MethodNotAllowed);
    response.headers.push(Header::new("allow", &allow));

    response
}

/// A router encodes as nothing: it is its routes type's code.
impl<R: Routes> Schema for Router<R> {
    const SHAPE: Shape = <() as Schema>::SHAPE;
}

impl<B: Boundary, R: Routes> Encode<B> for Router<R> {
    fn encode(&self, _: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        Ok(())
    }
}

/// A router decoded is compiled again from its routes, which compiled as the stack began.
impl<B: Boundary, R: Routes> Decode<B> for Router<R> {
    fn decode(_: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Self::compiled().map_err(|_| DecodeError::Refused { name: "Router", rule: "its routes compile" })
    }
}
