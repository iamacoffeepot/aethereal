//! The capability as an actor layer (ADR-0035): the authenticator of the sessions' port, which the container stack asks
//! about every request, on the way in and on the way out (ADR-0057).

use super::Sessions;
use aethereal_actor::{Fault, Handles, Turn};
use aethereal_http::stack::{FromStack, Inbound, Inward, Outbound, Outward, Response};
use aethereal_tools::Caller;

/// The realm a request refused for want of a bearer is challenged in.
const REALM: &str = "aethereal";

impl Handles<Inbound<()>> for Sessions {
    type From = FromStack;
    type Returns = Inward<Caller>;

    /// A request bearing a bearer it minted and holds, as a `Bearer` token or a `Basic` password, is passed on as that
    /// session's; any other, bearing none, a bearer it does not hold, or credentials of another scheme, is answered
    /// `401` with a `Bearer` challenge, and goes no further.
    fn handle(&mut self, inbound: Inbound<()>, _: &mut Turn<'_, Self, Self::From>) -> Result<Inward<Caller>, Fault> {
        let inward = self.bearers.session_of(&inbound.head.credentials).map_or_else(
            || Inward::Answer(Response::challenge("Bearer", REALM)),
            |session| Inward::Pass(Caller::Session { session }),
        );

        Ok(inward)
    }
}

impl Handles<Outbound> for Sessions {
    type From = FromStack;
    type Returns = Outward;

    /// Nothing to do on the way out: the head passes on unchanged.
    fn handle(&mut self, outbound: Outbound, _: &mut Turn<'_, Self, Self::From>) -> Result<Outward, Fault> {
        Ok(Outward::Pass(outbound.response))
    }
}
