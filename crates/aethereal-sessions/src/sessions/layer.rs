//! The capability as an actor layer (ADR-0035): the authenticator of the sessions' port, which the container stack asks
//! about every request, on the way in and on the way out (ADR-0057).

use super::Sessions;
use aethereal_actor::{Carries, Fault, Handles, ToSender, Turn};
use aethereal_http::stack::{FromStack, Inbound, Inward, Outbound, Outward, Response};
use aethereal_tools::Caller;

/// The realm a request refused for want of a bearer is challenged in.
const REALM: &str = "aethereal";

/// Asked a request's head by the container stack, it answers who sent it, or a challenge.
impl Carries<Inbound<()>> for Sessions {
    type Answers = (Inward<Caller>,);
}

/// Asked a response's head by the container stack, it passes it on.
impl Carries<Outbound> for Sessions {
    type Answers = (Outward,);
}

impl Handles<Inbound<()>> for Sessions {
    type Sends = (ToSender<Inward<Caller>>,);
    type From = FromStack;

    /// A request bearing a bearer it minted and holds, as a `Bearer` token or a `Basic` password, is passed on as that
    /// session's; any other, bearing none, a bearer it does not hold, or credentials of another scheme, is answered
    /// `401` with a `Bearer` challenge, and goes no further.
    fn handle(
        &mut self,
        inbound: Inbound<()>,
        turn: &mut Turn<'_, Self, Self::Sends, Self::From>,
    ) -> Result<(), Fault> {
        let inward = self.bearers.session_of(&inbound.head.credentials).map_or_else(
            || Inward::Answer(Response::challenge("Bearer", REALM)),
            |session| Inward::Pass(Caller::Session { session }),
        );

        Ok(turn.reply(&inward)?)
    }
}

impl Handles<Outbound> for Sessions {
    type Sends = (ToSender<Outward>,);
    type From = FromStack;

    /// Nothing to do on the way out: the head passes on unchanged.
    fn handle(&mut self, outbound: Outbound, turn: &mut Turn<'_, Self, Self::Sends, Self::From>) -> Result<(), Fault> {
        Ok(turn.reply(&Outward::Pass(outbound.response))?)
    }
}
