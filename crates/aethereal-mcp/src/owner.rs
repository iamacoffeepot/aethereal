//! The pure layer of a stack whose port is the owner's (ADR-0035, ADR-0057).

use aethereal_data::Plain;
use aethereal_http::stack::{Head, Layer, LayerForm, Step};
use aethereal_tools::Caller;

/// A stack's one layer when its port is on the loopback: every request on it is the owner's, so it attaches
/// [`Caller::Owner`] to each and asks no one, whatever credentials the request bears.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
pub struct MarkOwner;

impl Layer for MarkOwner {
    type In = ();
    type Out = Caller;

    fn inbound(&mut self, _: &Head, (): ()) -> Step<Caller> {
        Step::Pass(Caller::Owner)
    }
}

impl LayerForm for MarkOwner {
    type Form = Self;
}
