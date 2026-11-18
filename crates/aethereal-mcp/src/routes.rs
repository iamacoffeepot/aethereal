//! The last layer of a stack that serves MCP (ADR-0035): its one route.

use crate::Mcp;
use aethereal_actor::path;
use aethereal_http::stack::LayerForm;
use aethereal_http::{Method, Route, Router, Routes, Unroutable};
use aethereal_tools::Caller;

/// The routes of a stack that serves MCP alone: `POST /mcp` to [`Mcp`], asked with who sent it, as a layer before
/// attached it. A request on another path is answered `404` by the router, and another method on `/mcp` `405`.
pub struct McpRoutes;

impl LayerForm for McpRoutes {
    type Form = Router<Self>;
}

impl Routes for McpRoutes {
    type Attached = Caller;
    type Handlers = (Route<Mcp, ()>,);

    fn routes(router: Router<Self>) -> Result<Router<Self>, Unroutable> {
        router.route(Method::Post, "/mcp", |()| path::<Mcp>(()))
    }
}
