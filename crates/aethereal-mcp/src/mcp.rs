//! The MCP capability's root (ADR-0037).

use crate::McpSettings;
use crate::front::{self, Reply, Routed};
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ended, Ending, Fault, Handles, Held, Level, Path,
    ToSender, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_http::{Handling, Http, Request, Response, Serve, Served, Status};
use aethereal_substrate::Bootstrap;
use aethereal_tools::{Call, Called, HeldTool, Holder, Offer, Offered, Output, Table, Tooling, Withdraw};
use std::collections::BTreeMap;

/// What the root keeps for its serve: nothing, since it asks one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.serving")]
pub struct Serving;

/// What the root keeps for a call it asked of a worker: which it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.calling")]
pub struct Calling {
    number: u64,
}

/// Why the root failed: `Http` gave it no port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.unserved")]
pub struct Unserved;

/// Why a turn of the root failed: the engine had no room to hold a response's body, not even an empty one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.unstored")]
pub struct Unstored;

/// A request the root holds until its worker answers, its JSON-RPC id, and what the tool's sheet says it answers with.
#[derive(Bound)]
struct Waiting {
    held: Held<Response>,
    id: String,
    output: Output,
}

/// Where the root is in being served.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq)]
enum Port {
    /// `Http` has not answered.
    Asked,
    /// `Http` bound this port.
    Bound(u16),
}

/// The MCP capability: forwards each call of a tool to the worker that offered it (ADR-0037). An actor asks it to
/// [`Offer`] at its typed path, `path::<Mcp>(())`.
///
/// Mail it cannot read never fails it (ADR-0017): an offer past the engine's `read_limit` is answered `Unreadable`, and
/// a tool's answer past it refuses that call. `Http` will not start where a request could be such mail, and a program
/// sets `read_limit` above what its offers hold and what its tools answer, so that each is served.
#[derive(Bound, Bootstrap)]
#[kind(name = "aether.mcp")]
pub struct Mcp {
    settings: McpSettings,
    port: Port,
    table: Table,
    waiting: BTreeMap<u64, Waiting>,
    next: u64,
}

impl Mcp {
    /// The port served: none until `Http` has bound it.
    #[must_use]
    pub const fn port(&self) -> Option<u16> {
        match self.port {
            Port::Asked => None,
            Port::Bound(port) => Some(port),
        }
    }
}

impl ChildOf for Mcp {
    type Parent = Top;
}

/// It holds the tools a hub's own program offers.
impl Holder for Mcp {}

/// Asked at its typed path, it answers an offer in the same turn.
impl Carries<Offer> for Mcp {
    type Answers = (Offered,);
}

impl Actor for Mcp {
    type Config = McpSettings;
    type Key = ();
    type Handles = (Request, Offer, Withdraw, Answer<Called, Calling>, Answer<Served, Serving>, Ended);
    type BeginSends = (Asks<Path<Http>, Serve, Serving>,);
    type EndSends = ();

    fn begin(settings: McpSettings, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let serve = Serve { port: settings.port, handler: turn.me().protocol::<Handling, _>() };
        turn.to(path::<Http>(())).ask(&serve, &Serving)?;

        Ok(Self { settings, port: Port::Asked, table: Table::new(), waiting: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Request> for Mcp {
    type Sends = (Asks<Tooling, Call, Calling>, ToSender<Response>);

    fn handle(&mut self, request: Request, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let routed = front::route(&request, turn.read(&request.body), &self.settings.origins, &self.table);
        let (id, tool, input) = match routed {
            Routed::Respond(reply) => {
                let response = response(turn, reply)?;
                turn.reply(&response)?;
                return Ok(());
            }
            Routed::Call { id, tool, input } => (id, tool, input),
        };
        let room = usize::try_from(self.settings.most_calls).unwrap_or(usize::MAX);
        let is_full = self.waiting.len() >= room;
        if is_full {
            let response = response(turn, front::refused(&id, "too many calls are out"))?;
            turn.reply(&response)?;
            return Ok(());
        }
        let Some(HeldTool { worker, output, .. }) = self.table.tool(&tool) else {
            let response = response(turn, front::refused(&id, "the tool has gone"))?;
            turn.reply(&response)?;
            return Ok(());
        };
        let (worker, output) = (*worker, output.sheet());
        let Ok(input) = turn.put(input) else {
            let response = response(turn, front::refused(&id, "the engine had no room to hold the arguments"))?;
            turn.reply(&response)?;
            return Ok(());
        };
        let held = turn.hold()?;
        let number = self.next;
        self.next += 1;
        turn.to(worker).ask(&Call { tool, input }, &Calling { number })?;
        self.waiting.insert(number, Waiting { held, id, output });

        Ok(())
    }
}

impl Handles<Offer> for Mcp {
    type Sends = (ToSender<Offered>,);

    fn handle(&mut self, offer: Offer, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let room = usize::try_from(self.settings.most_tools).unwrap_or(usize::MAX);
        let offered = self.table.take(offer.tools, offer.worker, room);
        if offered == Offered::Yes {
            turn.watch(offer.worker);
        }
        turn.reply(&offered)?;

        Ok(())
    }
}

impl Handles<Withdraw> for Mcp {
    type Sends = ();

    fn handle(&mut self, withdraw: Withdraw, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.table.withdraw(&withdraw.tool, withdraw.worker);

        Ok(())
    }
}

impl Handles<Answer<Called, Calling>> for Mcp {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Calling { number }): (Answered<Called>, Calling),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(Waiting { held, id, output }) = self.waiting.remove(&number) else {
            return Ok(());
        };
        let reply = match (answered, output) {
            (Answered::Yes(Called::Output(bytes)), Output::Value(schema) | Output::ValueOrImage(schema)) => {
                match front::value(&id, &schema, turn.read(&bytes)) {
                    Ok(value) => {
                        let buffer = turn.buffer(front::output_room(&id, &value));
                        front::output(&id, &value, buffer)
                    }
                    Err(refused) => refused,
                }
            }
            (Answered::Yes(Called::Image { png, text }), Output::Image | Output::ValueOrImage(_)) => {
                let length = turn.read(&png).len();
                let buffer = turn.buffer(front::image_room(&id, length, &text));
                front::image(&id, turn.read(&png), &text, buffer)
            }
            (Answered::Yes(Called::Output(_)), Output::Image) => {
                front::refused(&id, "the tool answered a value, and its sheet says an image")
            }
            (Answered::Yes(Called::Image { .. }), Output::Value(_)) => {
                front::refused(&id, "the tool answered an image, and its sheet says a value")
            }
            (Answered::Yes(Called::Refused(why)), _) => front::refused(&id, &why),
            (Answered::No(why), _) => front::refused(&id, &format!("the tool did not answer: {why:?}")),
        };
        let response = response(turn, reply)?;
        turn.answer(held, &response)?;

        Ok(())
    }
}

impl Handles<Answer<Served, Serving>> for Mcp {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Serving): (Answered<Served>, Serving),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Served::Bound { port, .. }) = answered else {
            turn.log(Level::Error, format_args!("no port to serve on: {answered:?}"));
            return Err(Fault::new(&Unserved)?);
        };
        self.port = Port::Bound(port);

        Ok(())
    }
}

impl Handles<Ended> for Mcp {
    type Sends = ();

    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.table.ended(ended.actor());

        Ok(())
    }
}

/// `reply` as a response, its body put as a blob: one the engine has no room to hold is answered `503` with no body
/// (ADR-0042).
fn response<S>(turn: &mut Turn<'_, Mcp, S>, reply: Reply) -> Result<Response, Fault> {
    let put = match reply {
        Reply::Empty(status) => turn.put(Vec::new()).map(|body| Response::bare(status, body)),
        Reply::Json { status, body } => {
            turn.put(body.into_bytes()).map(|body| Response::new(status, "application/json", body))
        }
    };
    if let Ok(response) = put {
        return Ok(response);
    }
    let Ok(empty) = turn.put(Vec::new()) else {
        return Err(Fault::new(&Unstored)?);
    };

    Ok(Response::bare(Status::ServiceUnavailable, empty))
}
