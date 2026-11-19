//! The capability as an actor layer (ADR-0035): the authenticator of the sessions' port, which the container stack asks
//! about every request, on the way in and on the way out (ADR-0057).

use super::Sessions;
use aethereal_actor::{Fault, Turn};
use aethereal_http::stack::{FromStack, Inbound, Inward, Outbound, Outward, Response};
use aethereal_tools::Caller;

/// The realm a request refused for want of a bearer is challenged in.
const REALM: &str = "aethereal";

impl Sessions {
    /// A request bearing a bearer it minted and holds, as a `Bearer` token or a `Basic` password, is passed on as that
    /// session's; any other, bearing none, a bearer it does not hold, or credentials of another scheme, is answered
    /// `401` with a `Basic` challenge in the realm `aethereal`, which git's credential helper answers with `Basic`
    /// credentials, and goes no further.
    #[allow(
        clippy::needless_pass_by_value,
        clippy::unnecessary_wraps,
        reason = "a handler's body, it takes and answers what its handler does"
    )]
    pub(super) fn handle_inbound(
        &mut self,
        inbound: Inbound<()>,
        _: &mut Turn<'_, Self, FromStack>,
    ) -> Result<Inward<Caller>, Fault> {
        let inward = self.bearers.session_of(&inbound.head.credentials).map_or_else(
            || Inward::Answer(Response::challenge("Basic", REALM)),
            |session| Inward::Pass(Caller::Session { session }),
        );

        Ok(inward)
    }

    /// Nothing to do on the way out: the head passes on unchanged.
    #[allow(
        clippy::unnecessary_wraps,
        clippy::unused_self,
        reason = "a handler's body, it takes and answers what its handler does"
    )]
    pub(super) fn handle_outbound(
        &mut self,
        outbound: Outbound,
        _: &mut Turn<'_, Self, FromStack>,
    ) -> Result<Outward, Fault> {
        Ok(Outward::Pass(outbound.response))
    }
}
