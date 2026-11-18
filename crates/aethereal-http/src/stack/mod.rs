//! A stack (ADR-0035): a program's own actor, one for each port, which alone serves that port. It asks `Tcp` to
//! listen, accepts each connection, reads and writes HTTP/1.1 on every connection itself, and passes each request
//! through its layers, in order, to the last, which answers it: its handler, or its routes' handler. The response goes
//! back through the layers in reverse.
//!
//! A pure layer, a [`Layer`], is a value the stack keeps for each open connection and calls as a function in its own
//! turn. An actor layer is an actor of its own, whose state is shared between connections: it handles [`Inbound`] and
//! [`Outbound`], and the stack asks it and parks the request until it answers. The last layer is a handler, or a routes
//! type whose [`Router`](crate::Router) picks the handler by method and path. A handler is an actor that lists
//! [`Request`], asked through its reference, and reads the body through the request's [`BodyHandle`]. The stack lists
//! its layers as one tuple of types, read by [`layers`]; what it holds and does is [`Serving`], into which each of its
//! handlers of HTTP's and TCP's kinds, and of its actor layers' answers, is one call.

mod actor;
mod gather;
mod head;
mod last;
mod layer;
mod layers;
mod message;
mod serving;
mod settings;
mod walks;

pub use actor::{
    Inbound, InboundPart, InboundParts, Inward, Outbound, OutboundPart, OutboundParts, Outward, Parked, ParkedPart,
    ParkedResponse, PartStep,
};
pub use gather::{Gather, Gathered, Gathering};
pub use head::{Credentials, Head};
pub use last::{AsksEach, Handed, HandlerList, LastLayer, Picked, Reach, Slot12, Unrouted};
pub use layer::{BodyPart, Layer, LayerForm, Leaving, Refused, Step};
pub use layers::{Entered, LayerList, Layers, Slot, Unlayered, layers};
pub use message::{BodyHandle, Request, Response, ResponseBody, ResponseHead};
pub use serving::{
    Answering, Binding, Deadline, GatherSends, ListenSends, Listened, ReadSends, Sending, Serving, StackSends,
};
pub use settings::StackSettings;
pub use walks::{Ask, Formed, Returned, Taker, Unasked, Walked, Walks};

use aethereal_actor::{Actor, Handles, Speaks};
use layer::Passed;

/// What an actor that serves one port is: one that answers a request's body whole, which only a stack does.
pub trait Stack: Actor + Handles<Gather> {}

/// Who a handler takes its request from, and an actor layer its ask: an actor whose type answers [`Gather`], which is
/// what a stack does (ADR-0058). A stack's type names its handlers, so a handler cannot name its stacks back; this
/// names what they do.
pub type FromStack = Speaks<Gathering>;
