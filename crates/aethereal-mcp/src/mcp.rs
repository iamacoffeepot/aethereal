//! The MCP capability's root (ADR-0037): a handler a stack asks each request on `/mcp` of (ADR-0035).

use crate::McpSettings;
use crate::front::{self, Reply, Routed};
use aethereal_actor::{Answered, Ended, Fault, Held, Turn, actor};
use aethereal_data::{Bound, Plain};
use aethereal_http::Status;
use aethereal_http::stack::{FromStack, Gathered, Request, Response};
use aethereal_substrate::Bootstrap;
use aethereal_tools::{Call, Called, Caller, HeldTool, Holder, Offer, Offered, Output, Table, Withdraw};
use core::time::Duration;
use std::collections::BTreeMap;

/// What the root keeps for a call it asked of a worker: which it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.mcp.calling")]
pub struct Calling {
    number: u64,
}

/// What the root keeps while a request's body is gathered: the answer it holds for the request, and the request itself,
/// whose headers the answer is checked against and whose caller its call names.
#[derive(Bound)]
#[kind(name = "aether.mcp.reading")]
pub struct Reading {
    held: Held<Response>,
    request: Request<Caller, ()>,
}

/// A request the root holds until its worker answers, its JSON-RPC id, and what the tool's sheet says it answers with.
#[derive(Bound)]
struct Waiting {
    held: Held<Response>,
    id: String,
    output: Output,
}

/// The MCP capability: forwards each call of a tool to the worker that offered it (ADR-0037). An actor asks it to
/// [`Offer`] at its typed path, `path::<Mcp>(())`.
///
/// It is a handler of a stack (ADR-0035): asked each `POST /mcp` a stack's layers pass, with who sent it attached as a
/// [`Caller`], which it forwards with each call. A worker reads who called from [`Call::caller`], which no caller
/// writes, and decides what that caller may do. It never sees a credential: the stack's layers read them, and a request
/// carries none.
///
/// Mail it cannot read never fails it (ADR-0017): an offer past the engine's `read_limit` is answered `Unreadable`, and
/// a tool's answer past it refuses that call. A program sets `read_limit` above what its offers hold and what its tools
/// answer, so that each is served.
#[derive(Bound, Bootstrap)]
#[kind(name = "aether.mcp")]
pub struct Mcp {
    settings: McpSettings,
    table: Table,
    waiting: BTreeMap<u64, Waiting>,
    next: u64,
}

/// It holds the tools a hub's own program offers.
impl Holder for Mcp {}

#[actor]
impl Mcp {
    fn begin(settings: McpSettings) -> Result<Self, Fault> {
        Ok(Self { settings, table: Table::new(), waiting: BTreeMap::new(), next: 0 })
    }

    /// A request from an origin it does not take is answered `403` unread; any other is held, and its body gathered
    /// whole.
    #[request]
    fn request(
        &mut self,
        request: Request<Caller, ()>,
        turn: &mut Turn<'_, Self, FromStack>,
    ) -> Result<aethereal_actor::Reply<Response>, Fault> {
        if !front::is_from_allowed(&request, &self.settings.origins) {
            return Ok(aethereal_actor::Reply::Now(Response::empty(Status::Forbidden)));
        }
        let gather = request.body.whole(self.settings.most_body_bytes);
        let gatherer = request.body.gatherer;
        let (held, later) = turn.hold();
        let reading = Reading { held, request };
        turn.to(gatherer).ask(&gather, &reading)?;

        Ok(aethereal_actor::Reply::Later(later))
    }

    /// The body is read: the request is answered now, or its call is forwarded to its tool's worker and the request
    /// held until the worker answers. A body past `most_body_bytes` is answered `413`; one the stack could not read is
    /// answered `400`, which the stack passes over when it has closed the connection or answers in its place.
    #[answer]
    fn gathered(
        &mut self,
        (gathered, Reading { held, request }): (Answered<Gathered>, Reading),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let body = match gathered {
            Answered::Yes(Gathered::Whole(body)) => body,
            Answered::Yes(Gathered::TooLarge) => {
                return Ok(turn.answer(held, &Response::empty(Status::ContentTooLarge))?);
            }
            Answered::Yes(_) | Answered::No(_) => return Ok(turn.answer(held, &Response::empty(Status::BadRequest))?),
        };
        let (id, tool, input) = match front::route(&request, turn.read(&body), &self.table) {
            Routed::Respond(reply) => {
                let response = response(turn, reply);
                return Ok(turn.answer(held, &response)?);
            }
            Routed::Call { id, tool, input } => (id, tool, input),
        };
        let room = usize::try_from(self.settings.most_calls).unwrap_or(usize::MAX);
        if self.waiting.len() >= room {
            let response = response(turn, front::refused(&id, "too many calls are out"));
            return Ok(turn.answer(held, &response)?);
        }
        let Some(HeldTool { worker, output, .. }) = self.table.tool(&tool) else {
            let response = response(turn, front::refused(&id, "the tool has gone"));
            return Ok(turn.answer(held, &response)?);
        };
        let (worker, output) = (*worker, output.sheet());
        let Ok(input) = turn.put(input) else {
            let response = response(turn, front::refused(&id, "the engine had no room to hold the arguments"));
            return Ok(turn.answer(held, &response)?);
        };
        let number = self.next;
        self.next += 1;
        let due = turn.now() + Duration::from_millis(u64::from(self.settings.call_within_millis));
        let call = Call { tool, input, caller: request.attached };
        turn.to(worker).within(due).ask(&call, &Calling { number })?;
        self.waiting.insert(number, Waiting { held, id, output });

        Ok(())
    }

    #[request]
    fn offer(&mut self, offer: Offer, turn: &mut Turn<'_, Self>) -> Result<Offered, Fault> {
        let room = usize::try_from(self.settings.most_tools).unwrap_or(usize::MAX);
        let offered = self.table.take(offer.tools, offer.worker, room);
        if offered == Offered::Yes {
            turn.watch(offer.worker);
        }

        Ok(offered)
    }

    #[tell]
    fn withdraw(&mut self, withdraw: Withdraw) -> Result<(), Fault> {
        self.table.withdraw(&withdraw.tool, withdraw.worker);

        Ok(())
    }

    #[answer]
    fn called(
        &mut self,
        (answered, Calling { number }): (Answered<Called>, Calling),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Some(Waiting { held, id, output }) = self.waiting.remove(&number) else {
            return Ok(());
        };
        let reply = match (answered, output) {
            (Answered::Yes(Called::Output(bytes)), Output::Value(schema) | Output::ValueOrImage(schema)) => {
                match front::value(&id, &schema, turn.read(&bytes)) {
                    Ok(value) => {
                        let buffer = Vec::from(turn.buffer(front::output_room(&id, &value)));
                        front::output(&id, &value, buffer)
                    }
                    Err(refused) => refused,
                }
            }
            (Answered::Yes(Called::Image { png, text }), Output::Image | Output::ValueOrImage(_)) => {
                let length = turn.read(&png).len();
                let buffer = Vec::from(turn.buffer(front::image_room(&id, length, &text)));
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
        let response = response(turn, reply);
        turn.answer(held, &response)?;

        Ok(())
    }

    #[ended]
    fn ended(&mut self, ended: Ended) -> Result<(), Fault> {
        self.table.ended(ended.actor());

        Ok(())
    }
}

/// `reply` as a response, its JSON put as a blob: one the engine has no room to hold is answered `503` with no body,
/// which needs none (ADR-0042).
fn response(turn: &mut Turn<'_, Mcp>, reply: Reply) -> Response {
    let (status, body) = match reply {
        Reply::Empty(status) => return Response::empty(status),
        Reply::Json { status, body } => (status, body),
    };

    turn.put(body.into_bytes()).map_or_else(
        |_| Response::empty(Status::ServiceUnavailable),
        |body| Response::new(status, "application/json", body),
    )
}
