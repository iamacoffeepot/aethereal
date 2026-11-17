//! A stack (ADR-0035): a program's own actor, one for each port, which alone serves that port. It asks `Tcp` to
//! listen, accepts each connection, reads and writes HTTP/1.1 on every connection itself, and passes each request
//! through its layers, in order, to the last, its handler, which answers it. The response goes back through the layers
//! in reverse.
//!
//! A pure layer, a [`Layer`], is a value the stack keeps for each open connection and calls as a function in its own
//! turn. The handler is an actor that lists [`Request`], asked through its reference, and reads the body through the
//! request's [`BodyHandle`]. The stack lists its layers as one tuple of types, read by [`layers`]; what it holds and
//! does is [`Serving`], into which each of its handlers of HTTP's and TCP's kinds is one call.

mod gather;
mod head;
mod layer;
mod layers;
mod message;
mod serving;
mod settings;

pub use gather::{Gather, Gathered, Gathering};
pub use head::{Credentials, Head};
pub use layer::{BodyPart, Layer, LayerForm, Leaving, Refused, Step};
pub use layers::{Entered, LayerList, Layers, Slot, Unlayered, layers};
pub use message::{BodyHandle, Request, Response, ResponseBody, ResponseHead};
pub use serving::{Answering, Binding, Deadline, Listened, Sending, Serving, StackSends};
pub use settings::StackSettings;

use aethereal_actor::{Actor, Handles, Speaks};
use layer::Passed;

/// What an actor that serves one port is: one that answers a request's body whole, which only a stack does.
pub trait Stack: Actor + Handles<Gather> {}

/// Who a handler takes its request from, and an actor layer its ask: an actor whose type answers [`Gather`], which is
/// what a stack does (ADR-0058). A stack's type names its handlers, so a handler cannot name its stacks back; this
/// names what they do.
pub type FromStack = Speaks<Gathering>;
