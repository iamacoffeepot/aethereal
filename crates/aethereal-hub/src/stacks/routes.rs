//! The last layer of the sessions' stack (ADR-0035, ADR-0057): `Mcp`'s route and git's.

use aethereal_actor::path;
use aethereal_git::Git;
use aethereal_http::stack::LayerForm;
use aethereal_http::{Method, Route, Router, Routes, Unroutable};
use aethereal_mcp::Mcp;
use aethereal_tools::Caller;

/// The routes of the sessions' port, each asked with the session `Sessions` attached: `POST /mcp` to `Mcp`, the hub's
/// tools; and git's smart HTTP to `Git`, the hub's one repository, under `/git`: the refs a service advertises, `GET
/// /git/info/refs`, and an exchange of each service, `POST /git/git-upload-pack` and `POST /git/git-receive-pack`. The
/// owner's port has no git route: the owner reaches the repository over ssh.
pub struct ContainerRoutes;

impl LayerForm for ContainerRoutes {
    type Form = Router<Self>;
}

impl Routes for ContainerRoutes {
    type Attached = Caller;
    type Handlers = (Route<Mcp, ()>, Route<Git, ()>);

    fn routes(router: Router<Self>) -> Result<Router<Self>, Unroutable> {
        router
            .route(Method::Post, "/mcp", |()| path::<Mcp>(()))?
            .route(Method::Get, "/git/info/refs", |()| path::<Git>(()))?
            .route(Method::Post, "/git/git-upload-pack", |()| path::<Git>(()))?
            .route(Method::Post, "/git/git-receive-pack", |()| path::<Git>(()))
    }
}
