//! A stack served on the native host (ADR-0035): its pure layers, its actor layers, its handler, its bodies streamed
//! either way, and its limits, to clients on threads of their own over real loopback TCP.

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anyone, Beginning, Carries, ChildOf, Ended, Ending, Enough, Fault, Handles,
    Held, More, Number, Open, Part, Receives, Serving, Spawn, StreamKey, Streams, Taken, Top, Turn, Unsent, path,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, KindId, Plain};
use aethereal_http::stack::{
    BodyHandle, BodyPart, BodyRead, Credentials, FromStack, Gathered, Head, Inbound, InboundBody, InboundPart, Inward,
    Layer, LayerForm, Leaving, Outbound, OutboundBody, OutboundPart, Outward, PartStep, Refused, Request, Response,
    ResponseBody, ResponseBytes, ResponseLength, SourceEnd, StackSettings, Step, StreamedBody,
};
use aethereal_http::{
    BlobBytesTooLow, Captures, Header, Method, ReadLimitTooLow, Route, Router, Routes, Status, Unroutable,
};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Bootstrap, EngineSettings, Host};
use aethereal_tcp::Tcp;
use core::mem;
use std::io::{Read as _, Write as _};
use std::num::NonZeroU32;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

mod peer;
mod stacks;

use peer::{Read, client, closes, connect, heard, response};
use stacks::{bound, stack};

/// What [`Count`] attaches: the number of the request on its connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
struct Seen {
    request: u32,
}

/// A pure layer that counts its connection's requests, attaches each one's number, and writes it on the way out as
/// `x-request`.
#[derive(Debug, Default, Clone, Plain)]
struct Count {
    requests: u32,
}

impl LayerForm for Count {
    type Form = Self;
}

impl Layer for Count {
    type In = ();
    type Out = Seen;

    fn inbound(&mut self, _: &Head, (): ()) -> Step<Seen> {
        self.requests += 1;

        Step::Pass(Seen { request: self.requests })
    }

    fn outbound(&mut self, _: &Head, leaving: &mut Leaving<'_>) {
        leaving.head().headers.push(Header::new("x-request", &self.requests.to_string()));
    }
}

/// A pure layer that passes a request bearing the token `open` and answers any other with a challenge.
#[derive(Debug, Default, Clone, Plain)]
struct Admit;

impl LayerForm for Admit {
    type Form = Self;
}

impl Layer for Admit {
    type In = Seen;
    type Out = Seen;

    fn inbound(&mut self, head: &Head, seen: Seen) -> Step<Seen> {
        match &head.credentials {
            Credentials::Bearer { token } if token == "open" => Step::Pass(seen),
            _ => Step::Answer(Response::challenge("Bearer", "test")),
        }
    }
}

/// [`Echo`]'s settings: the most bytes of a body it gathers.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.echo_settings")]
#[settings]
struct EchoSettings {
    most_bytes: u32,
}

/// A handler that gathers each request's body and answers it with the body, and the request's number in
/// `x-attached`; it counts the requests it is asked.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.echo")]
struct Echo {
    most_bytes: u32,
    asked: u32,
}

/// What [`Echo`] keeps while its request's body is gathered: the request, held, and its number.
#[derive(Bound)]
#[kind(name = "test.http_stack.echoing")]
struct Echoing {
    held: Held<Response>,
    request: u32,
}

impl ChildOf for Echo {
    type Parent = Top;
}

impl Actor for Echo {
    type Config = EchoSettings;
    type Key = ();
    type Handles = (Request<Seen, ()>, Answer<Gathered, Echoing>);
    type Depends = ();
    type Spawns = ();

    fn begin(settings: EchoSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { most_bytes: settings.most_bytes, asked: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<Seen, ()>> for Echo {
    type Answers = (Response,);
}

impl Handles<Request<Seen, ()>> for Echo {
    type From = FromStack;

    fn handle(&mut self, request: Request<Seen, ()>, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        self.asked += 1;
        let echoing = Echoing { held: turn.hold()?, request: request.attached.request };
        turn.to(request.body.gatherer).ask(&request.body.whole(self.most_bytes), &echoing)?;

        Ok(())
    }
}

impl Receives<Answer<Gathered, Echoing>> for Echo {
    fn handle(
        &mut self,
        (gathered, Echoing { held, request }): (Answered<Gathered>, Echoing),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let response = match gathered {
            Answered::Yes(Gathered::Whole(body)) => {
                let mut echoed = Response::new(Status::Ok, "text/plain", body);
                echoed.headers.push(Header::new("x-attached", &request.to_string()));
                echoed
            }
            Answered::Yes(Gathered::TooLarge) => Response::empty(Status::ContentTooLarge),
            Answered::Yes(_) | Answered::No(_) => Response::empty(Status::BadRequest),
        };

        Ok(turn.answer(held, &response)?)
    }
}

/// [`Silent`]'s settings: none.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.silent_settings")]
#[settings]
struct SilentSettings {}

/// A handler that holds every request it is asked, and never answers.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.silent")]
struct Silent {
    held: Vec<Held<Response>>,
}

impl ChildOf for Silent {
    type Parent = Top;
}

impl Actor for Silent {
    type Config = SilentSettings;
    type Key = ();
    type Handles = (Request<(), ()>,);
    type Depends = ();
    type Spawns = ();

    fn begin(_: SilentSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { held: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<(), ()>> for Silent {
    type Answers = (Response,);
}

impl Handles<Request<(), ()>> for Silent {
    type From = FromStack;

    fn handle(&mut self, _: Request<(), ()>, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        self.held.push(turn.hold()?);

        Ok(())
    }
}

/// How [`Fickle`] fails each request it is asked.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "test.http_stack.fails")]
enum Fails {
    /// Its turn fails.
    #[default]
    Faults,
    /// It lets the ask go, answering nothing.
    Drops,
}

/// [`Fickle`]'s settings: how it fails each request.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.fickle_settings")]
#[settings]
struct FickleSettings {
    fails: Fails,
}

/// What [`Fickle`] is told to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "test.http_stack.fickle_order")]
enum FickleOrder {
    /// Ends.
    Close,
}

/// A handler that answers no request: it fails each as its settings say, or has ended before it is asked.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.fickle")]
struct Fickle {
    fails: Fails,
}

impl ChildOf for Fickle {
    type Parent = Top;
}

impl Actor for Fickle {
    type Config = FickleSettings;
    type Key = ();
    type Handles = (Request<(), ()>, FickleOrder);
    type Depends = ();
    type Spawns = ();

    fn begin(settings: FickleSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { fails: settings.fails })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<(), ()>> for Fickle {
    type Answers = (Response,);
}

impl Handles<Request<(), ()>> for Fickle {
    type From = FromStack;

    fn handle(&mut self, _: Request<(), ()>, _: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        match self.fails {
            Fails::Faults => Err(Fault::of_parts(KindId::of::<Self>(), Vec::new())),
            Fails::Drops => Ok(()),
        }
    }
}

impl Handles<FickleOrder> for Fickle {
    type From = Anyone;

    fn handle(&mut self, FickleOrder::Close: FickleOrder, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// Who [`Gate`] says sent a request: the request's number on its connection, which [`Count`] attached before it, and
/// the user its bearer names.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
struct Caller {
    request: u32,
    user: String,
}

/// [`Gate`]'s settings: none.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.gate_settings")]
#[settings]
struct GateSettings {}

/// An actor layer that reads each request's bearer: `open` passes, naming its user; `wait` is held until it is told
/// [`GateOrder::Release`]; any other is answered with a challenge. On the way out it writes `x-gate: passed`, in place
/// of any `x-gate` the response had.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.gate")]
struct Gate {
    held: Vec<Waiting>,
}

/// A request [`Gate`] holds: the ask, and what [`Count`] attached to it.
#[derive(Bound)]
#[kind(name = "test.http_stack.waiting")]
struct Waiting {
    held: Held<Inward<Caller>>,
    seen: Seen,
}

/// What [`Gate`] is told to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "test.http_stack.gate_order")]
enum GateOrder {
    /// Passes every request it holds.
    Release,
    /// Ends.
    Close,
}

impl ChildOf for Gate {
    type Parent = Top;
}

impl Actor for Gate {
    type Config = GateSettings;
    type Key = ();
    type Handles = (Inbound<Seen>, Outbound, GateOrder);
    type Depends = ();
    type Spawns = ();

    fn begin(_: GateSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { held: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Inbound<Seen>> for Gate {
    type Answers = (Inward<Caller>,);
}

impl Carries<Outbound> for Gate {
    type Answers = (Outward,);
}

impl Handles<Inbound<Seen>> for Gate {
    type From = FromStack;

    fn handle(
        &mut self,
        Inbound { head, attached: seen }: Inbound<Seen>,
        turn: &mut Turn<'_, Self, Self::From>,
    ) -> Result<(), Fault> {
        let inward = match &head.credentials {
            Credentials::Bearer { token } if token == "open" => {
                Inward::Pass(Caller { request: seen.request, user: String::from("open") })
            }
            Credentials::Bearer { token } if token == "wait" => {
                self.held.push(Waiting { held: turn.hold()?, seen });
                return Ok(());
            }
            _ => Inward::Answer(Response::challenge("Bearer", "gate")),
        };

        Ok(turn.reply(&inward)?)
    }
}

impl Handles<Outbound> for Gate {
    type From = FromStack;

    fn handle(&mut self, outbound: Outbound, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        let mut response = outbound.response;
        response.headers.retain(|header| header.name != "x-gate");
        response.headers.push(Header::new("x-gate", "passed"));

        Ok(turn.reply(&Outward::Pass(response))?)
    }
}

impl Handles<GateOrder> for Gate {
    type From = Anyone;

    fn handle(&mut self, order: GateOrder, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        match order {
            GateOrder::Release => {
                for Waiting { held, seen } in self.held.drain(..) {
                    let caller = Caller { request: seen.request, user: String::from("wait") };
                    turn.answer(held, &Inward::Pass(caller))?;
                }
            }
            GateOrder::Close => turn.close(),
        }

        Ok(())
    }
}

/// A pure layer that marks who [`Gate`] said sent a request, `+tag` after its user, and ends the request's body with
/// `!`.
#[derive(Debug, Default, Clone, Plain)]
struct Tag;

impl LayerForm for Tag {
    type Form = Self;
}

impl Layer for Tag {
    type In = Caller;
    type Out = Caller;

    fn inbound(&mut self, _: &Head, caller: Caller) -> Step<Caller> {
        Step::Pass(Caller { user: format!("{}+tag", caller.user), ..caller })
    }

    fn inbound_body(&mut self, part: &mut BodyPart<'_>) {
        if part.is_end() {
            part.out().push(b'!');
        }
    }
}

/// [`Upper`]'s settings: none.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.upper_settings")]
#[settings]
struct UpperSettings {}

/// An actor layer that passes every head as it came, and takes the request's body, each part of which it passes in
/// upper case.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.upper")]
struct Upper;

impl ChildOf for Upper {
    type Parent = Top;
}

impl Actor for Upper {
    type Config = UpperSettings;
    type Key = ();
    type Handles = (Inbound<Caller>, Outbound, InboundPart);
    type Depends = ();
    type Spawns = ();

    fn begin(_: UpperSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Inbound<Caller>> for Upper {
    type Answers = (Inward<Caller>,);
}

impl Carries<Outbound> for Upper {
    type Answers = (Outward,);
}

impl Handles<Inbound<Caller>> for Upper {
    type From = FromStack;

    fn handle(&mut self, inbound: Inbound<Caller>, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        Ok(turn.reply(&Inward::Pass(inbound.attached))?)
    }
}

impl Handles<Outbound> for Upper {
    type From = FromStack;

    fn handle(&mut self, outbound: Outbound, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        Ok(turn.reply(&Outward::Pass(outbound.response))?)
    }
}

impl Handles<InboundPart> for Upper {
    type From = FromStack;

    fn handle(&mut self, part: InboundPart, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        let upper = turn.read(&part.bytes).to_ascii_uppercase();
        let step = turn.put(upper).map_or(PartStep::Refuse(Refused::TooLarge), PartStep::Pass);

        Ok(turn.reply(&step)?)
    }
}

/// [`Mirror`]'s settings: none.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.mirror_settings")]
#[settings]
struct MirrorSettings {}

/// A handler that gathers each request's body and answers it with the body, who sent it in `x-user` and `x-attached`,
/// and `x-gate: unseen`; it counts the requests it is asked.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.mirror")]
struct Mirror {
    asked: u32,
}

/// What [`Mirror`] keeps while its request's body is gathered: the request, held, and who sent it.
#[derive(Bound)]
#[kind(name = "test.http_stack.mirroring")]
struct Mirroring {
    held: Held<Response>,
    caller: Caller,
}

impl ChildOf for Mirror {
    type Parent = Top;
}

impl Actor for Mirror {
    type Config = MirrorSettings;
    type Key = ();
    type Handles = (Request<Caller, ()>, Answer<Gathered, Mirroring>);
    type Depends = ();
    type Spawns = ();

    fn begin(_: MirrorSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { asked: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<Caller, ()>> for Mirror {
    type Answers = (Response,);
}

impl Handles<Request<Caller, ()>> for Mirror {
    type From = FromStack;

    fn handle(&mut self, request: Request<Caller, ()>, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        self.asked += 1;
        let mirroring = Mirroring { held: turn.hold()?, caller: request.attached };
        turn.to(request.body.gatherer).ask(&request.body.whole(64), &mirroring)?;

        Ok(())
    }
}

impl Receives<Answer<Gathered, Mirroring>> for Mirror {
    fn handle(
        &mut self,
        (gathered, Mirroring { held, caller }): (Answered<Gathered>, Mirroring),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let response = match gathered {
            Answered::Yes(Gathered::Whole(body)) => {
                let mut mirrored = Response::new(Status::Ok, "text/plain", body);
                mirrored.headers.push(Header::new("x-user", &caller.user));
                mirrored.headers.push(Header::new("x-attached", &caller.request.to_string()));
                mirrored.headers.push(Header::new("x-gate", "unseen"));
                mirrored
            }
            Answered::Yes(_) | Answered::No(_) => Response::empty(Status::BadRequest),
        };

        Ok(turn.answer(held, &response)?)
    }
}

/// [`Shelf`]'s settings: none.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.shelf_settings")]
#[settings]
struct ShelfSettings {}

/// A handler routed on `/shelf`, which answers each request it is asked at once, with `x-handler: shelf`, and counts
/// them.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.shelf")]
struct Shelf {
    asked: u32,
}

impl ChildOf for Shelf {
    type Parent = Top;
}

impl Actor for Shelf {
    type Config = ShelfSettings;
    type Key = ();
    type Handles = (Request<Seen, ()>,);
    type Depends = ();
    type Spawns = ();

    fn begin(_: ShelfSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { asked: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<Seen, ()>> for Shelf {
    type Answers = (Response,);
}

impl Handles<Request<Seen, ()>> for Shelf {
    type From = FromStack;

    fn handle(&mut self, _: Request<Seen, ()>, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        self.asked += 1;
        let mut response = Response::empty(Status::Ok);
        response.headers.push(Header::new("x-handler", "shelf"));

        Ok(turn.reply(&response)?)
    }
}

/// An item's route's captures: its number, read from `{number}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
struct Numbered {
    number: u64,
}

impl Captures for Numbered {
    const NAMES: &'static [&'static str] = &["number"];

    fn read(slices: &[&str]) -> Option<Self> {
        let [number] = slices else {
            return None;
        };

        number.parse().ok().map(|number| Self { number })
    }
}

/// [`Items`]' settings: none.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.items_settings")]
#[settings]
struct ItemsSettings {}

/// A root that stands up [`Item`] `1` and [`Item`] `2` beneath it as it begins, and nothing else.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.items")]
struct Items;

impl ChildOf for Items {
    type Parent = Top;
}

impl Actor for Items {
    type Config = ItemsSettings;
    type Key = ();
    type Handles = ();
    type Depends = ();
    type Spawns = (Item,);

    fn begin(_: ItemsSettings, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        // A child that is not stood up fails the begin, so the program does not start.
        for number in [1, 2] {
            let Ok(_) = turn.spawn::<Item>(&number, ItemSettings { number }) else {
                return Err(Fault::of_parts(KindId::of::<Self>(), Vec::new()));
            };
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

/// [`Item`]'s settings: its own number.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.item_settings")]
struct ItemSettings {
    number: u64,
}

/// An instanced handler, keyed by its number, routed on `/items/{number}`: it answers each request with its own number
/// in `x-item` and the number its route's captures read in `x-captured`.
#[derive(Bound)]
#[kind(name = "test.http_stack.item")]
struct Item {
    number: u64,
}

impl ChildOf for Item {
    type Parent = Items;
}

impl Actor for Item {
    type Config = ItemSettings;
    type Key = u64;
    type Handles = (Request<Seen, Numbered>,);
    type Depends = ();
    type Spawns = ();

    fn begin(settings: ItemSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { number: settings.number })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<Seen, Numbered>> for Item {
    type Answers = (Response,);
}

impl Handles<Request<Seen, Numbered>> for Item {
    type From = FromStack;

    fn handle(&mut self, request: Request<Seen, Numbered>, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        let mut response = Response::empty(Status::Ok);
        response.headers.push(Header::new("x-item", &self.number.to_string()));
        response.headers.push(Header::new("x-captured", &request.captures.number.to_string()));

        Ok(turn.reply(&response)?)
    }
}

/// The routes of [`ShelfStack`], after [`Count`]: `GET` and `POST` on `/shelf` to [`Shelf`], and `GET` on
/// `/items/{number}` to the [`Item`] of that number.
struct ShelfRoutes;

impl LayerForm for ShelfRoutes {
    type Form = Router<Self>;
}

impl Routes for ShelfRoutes {
    type Attached = Seen;
    type Handlers = (Route<Shelf, ()>, Route<Item, Numbered>);

    fn routes(router: Router<Self>) -> Result<Router<Self>, Unroutable> {
        router
            .route(Method::Get, "/shelf", |()| path::<Shelf>(()))?
            .route(Method::Post, "/shelf", |()| path::<Shelf>(()))?
            .route(Method::Get, "/items/{number}", |numbered: Numbered| path::<Item>(numbered.number))
    }
}

/// Routes that overlap: `GET /shelf/{number}` and `GET /shelf/1` both match `/shelf/1`.
struct OverlappingRoutes;

impl LayerForm for OverlappingRoutes {
    type Form = Router<Self>;
}

impl Routes for OverlappingRoutes {
    type Attached = Seen;
    type Handlers = (Route<Shelf, ()>, Route<Item, Numbered>);

    fn routes(router: Router<Self>) -> Result<Router<Self>, Unroutable> {
        router.route(Method::Get, "/shelf/{number}", |numbered: Numbered| path::<Item>(numbered.number))?.route(
            Method::Get,
            "/shelf/1",
            |()| path::<Shelf>(()),
        )
    }
}

stack!(EchoStack, "test.http_stack.echo_stack", "echo_stack", (Count, Admit, Echo), [], depends(Echo));
stack!(SilentStack, "test.http_stack.silent_stack", "silent_stack", (Silent,), [], depends(Silent));
stack!(FickleStack, "test.http_stack.fickle_stack", "fickle_stack", (Fickle,), [], depends(Fickle));
stack!(
    GateStack,
    "test.http_stack.gate_stack",
    "gate_stack",
    (Count, Gate, Tag, Upper, Mirror),
    [Caller],
    depends(Gate, Upper, Mirror)
);
stack!(ShelfStack, "test.http_stack.shelf_stack", "shelf_stack", (Count, ShelfRoutes), [], depends(Shelf, Item));
stack!(
    OverlappingStack,
    "test.http_stack.overlapping_stack",
    "overlapping_stack",
    (Count, OverlappingRoutes),
    [],
    depends(Shelf, Item)
);

/// The engine's settings for a stack of `settings`: a head of its most bytes is read whole.
fn engine(settings: &StackSettings) -> EngineSettings {
    EngineSettings {
        read_limit: settings.most_read_bytes().try_into().unwrap_or(u32::MAX),
        ..EngineSettings::default()
    }
}

/// A host serving an [`EchoStack`] of `settings`, whose handler gathers at most `most_bytes`, and the port it listens
/// on.
fn echoing(settings: StackSettings, most_bytes: u32) -> Result<(Harness<Headless>, u16), Unheard> {
    let program = Headless::program()
        .with_engine(engine(&settings))
        .root::<Tcp>()
        .root_with::<Echo>(EchoSettings { most_bytes })
        .root_with::<EchoStack>(settings);
    let mut harness = Harness::of(program)?;
    let port = bound(&mut harness, |stack: &EchoStack| stack.serving.port());

    Ok((harness, port))
}

/// A client on a thread of its own that writes each of `requests` once the response to the one before has come, on one
/// connection, and reads each response, and whether the server then closed.
fn conversation(port: u16, requests: &'static [&'static [u8]]) -> JoinHandle<(Vec<Read>, bool)> {
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let mut stream = connect(port);
        let mut buffer = Vec::new();
        let read = requests
            .iter()
            .map(|request| {
                assert!(stream.write_all(request).is_ok(), "the client writes");
                response(&mut stream, &mut buffer)
            })
            .collect();
        let closed = closes(&mut stream);

        (read, closed)
    })
}

#[test]
fn a_request_passes_each_layer_to_its_handler_and_the_response_passes_them_back() -> Result<(), Unheard> {
    // Catches what one layer attaches not handed to the next, or to the handler; a layer's way out not run, or run on
    // a response that skips it; a body not gathered for the handler; a response's `Content-Length` not its body's;
    // and the credentials handed on to the handler among its headers.
    let (mut harness, port) = echoing(StackSettings::default(), 64)?;
    let sent =
        b"POST /echo HTTP/1.1\r\nAuthorization: Bearer open\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
    let client = client(port, sent, 1);

    let (answered, closed) = heard(&mut harness, client);

    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{answered:?}");
    assert!(answered[0].head.contains("x-attached: 1"), "{answered:?}");
    assert!(answered[0].head.contains("x-request: 1"), "{answered:?}");
    assert_eq!(answered[0].body, b"hello");
    assert!(closed, "a request that asked to close is closed once answered");

    Ok(())
}

#[test]
fn a_layer_that_answers_turns_the_request_back_through_the_layers_before_it_and_its_handler_is_not_asked()
-> Result<(), Unheard> {
    // Catches a request a layer answered handed on to the handler anyway; the answer not passed back out through the
    // layers before the one that answered; an empty body written with no `Content-Length`, so the client waits for
    // one; and a body a layer answered before left in the connection, to be read as the next request.
    let (mut harness, port) = echoing(StackSettings::default(), 64)?;
    let client = client(port, b"POST /echo HTTP/1.1\r\nContent-Length: 4\r\n\r\nbody", 1);

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 401"), "{refused:?}");
    assert!(refused[0].head.contains("www-authenticate: Bearer realm=\"test\""), "{refused:?}");
    assert!(refused[0].head.contains("x-request: 1"), "{refused:?}");
    assert!(refused[0].head.contains("content-length: 0"), "{refused:?}");
    assert!(closed, "a request answered with its body unread closes its connection");
    let echo = harness.root::<Echo>();
    assert_eq!(harness.state(echo)?.asked, 0, "the handler is not asked a request a layer answered");

    Ok(())
}

#[test]
fn a_connection_kept_alive_keeps_its_layers_values_and_a_new_one_starts_from_the_template() -> Result<(), Unheard> {
    // Catches a connection closed after its first response though it asked to stay; a pure layer's value made anew for
    // each request, or shared between connections, so what it keeps across one connection's requests is lost or
    // leaks into another's.
    const FIRST: &[u8] = b"GET /one HTTP/1.1\r\nAuthorization: Bearer open\r\n\r\n";
    const SECOND: &[u8] = b"GET /two HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";
    let (mut harness, port) = echoing(StackSettings::default(), 64)?;

    let (kept, closed) = heard(&mut harness, conversation(port, &[FIRST, SECOND]));
    let (fresh, _) = heard(&mut harness, client(port, SECOND, 1));

    assert!(kept[0].head.contains("x-request: 1"), "{kept:?}");
    assert!(kept[1].head.contains("x-request: 2"), "{kept:?}");
    assert!(closed, "the second request asked to close");
    assert!(fresh[0].head.contains("x-request: 1"), "{fresh:?}");

    Ok(())
}

#[test]
fn requests_sent_ahead_are_answered_in_order_once_the_one_before_is_written() -> Result<(), Unheard> {
    // Catches a body framed past its `Content-Length` into the next request; bytes read past a request lost; credit
    // withheld while a request is out and never granted back, so the second request is never read; and responses
    // written out of order.
    // Chunks of 16 bytes, one granted at a time, so the second request is read only once the credit withheld while the
    // first was out is granted back.
    let settings = StackSettings { chunk_bytes: 16, window: 1, ..StackSettings::default() };
    let (mut harness, port) = echoing(settings, 64)?;
    let ahead = b"POST /one HTTP/1.1\r\nAuthorization: Bearer open\r\nContent-Length: 5\r\n\r\nhello\
        GET /two HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";
    let client = client(port, ahead, 2);

    let (answered, closed) = heard(&mut harness, client);

    assert_eq!(answered[0].body, b"hello", "{answered:?}");
    assert!(answered[0].head.contains("x-request: 1"), "{answered:?}");
    assert!(answered[1].body.is_empty(), "{answered:?}");
    assert!(answered[1].head.contains("x-request: 2"), "{answered:?}");
    assert!(closed, "the second request asked to close");

    Ok(())
}

#[test]
fn a_body_framed_two_ways_is_refused_before_its_layers() -> Result<(), Unheard> {
    // Catches a `Transfer-Encoding` beside a `Content-Length` settled in favour of one, which is how a request is
    // smuggled past a reader that settles it the other way, and the refused request handed to the layers.
    let (mut harness, port) = echoing(StackSettings::default(), 64)?;
    let sent =
        b"POST /echo HTTP/1.1\r\nAuthorization: Bearer open\r\nContent-Length: 5\r\nTransfer-Encoding: chunked\r\n\r\n\
        5\r\nhello\r\n0\r\n\r\n";
    let client = client(port, sent, 1);

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 400"), "{refused:?}");
    assert!(!refused[0].head.contains("x-request"), "no layer saw it: {refused:?}");
    assert!(closed, "a refused request's connection closes once its response is sent");

    Ok(())
}

#[test]
fn a_head_past_its_most_bytes_is_refused_before_it_is_whole() -> Result<(), Unheard> {
    // Catches a head bounded only once it ends, so a client that never ends one is buffered without limit, and a head
    // too large answered as one that does not parse.
    let settings = StackSettings { most_head_bytes: 64, ..StackSettings::default() };
    let (mut harness, port) = echoing(settings, 64)?;
    let client = client(port, b"GET /echo HTTP/1.1\r\nx-padding: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 1);

    let (refused, closed) = heard(&mut harness, client);

    assert!(refused[0].head.starts_with("HTTP/1.1 431"), "{refused:?}");
    assert!(closed, "a refused request's connection closes once its response is sent");

    Ok(())
}

#[test]
fn a_handler_that_never_answers_is_timed_out_once_its_body_is_read() -> Result<(), Unheard> {
    // Catches a handler that never answers left waiting without a `504`, and a `504` written before
    // `answer_within_millis` has passed.
    let settings = StackSettings { answer_within_millis: 200, ..StackSettings::default() };
    let program = Headless::program()
        .with_engine(engine(&settings))
        .root::<Tcp>()
        .root::<Silent>()
        .root_with::<SilentStack>(settings);
    let mut harness = Harness::of(program)?;
    let port = bound(&mut harness, |stack: &SilentStack| stack.serving.port());
    #[allow(clippy::disallowed_methods)] // The test times the request against its deadline on real time.
    let sent = Instant::now();

    let (timed_out, closed) = heard(&mut harness, client(port, b"GET /silent HTTP/1.1\r\n\r\n", 1));

    assert!(timed_out[0].head.starts_with("HTTP/1.1 504"), "{timed_out:?}");
    assert!(sent.elapsed() >= Duration::from_millis(200), "it waited for its deadline: {:?}", sent.elapsed());
    assert!(closed, "a request timed out closes its connection");

    Ok(())
}

#[test]
fn a_body_gathered_past_its_most_bytes_is_refused_whole_and_the_connection_closes() -> Result<(), Unheard> {
    // Catches a body whose `Content-Length` is past the gather's most bytes read before it is refused; a chunked body,
    // which has no length to check, gathered past them and handed on, or handed on cut short; and the connection kept
    // with the rest of the body unread, to be parsed as the next request.
    let (mut harness, port) = echoing(StackSettings::default(), 8)?;
    let sized = b"POST /echo HTTP/1.1\r\nAuthorization: Bearer open\r\nContent-Length: 10\r\n\r\n0123456789";
    let chunked = b"POST /echo HTTP/1.1\r\nAuthorization: Bearer open\r\nTransfer-Encoding: chunked\r\n\r\n\
        5\r\nhello\r\n5\r\nworld\r\n0\r\n\r\n";

    let (sized, sized_closed) = heard(&mut harness, client(port, sized, 1));
    let (chunked, chunked_closed) = heard(&mut harness, client(port, chunked, 1));

    assert!(sized[0].head.starts_with("HTTP/1.1 413"), "{sized:?}");
    assert!(sized_closed, "the rest of a body refused is not read, so its connection closes");
    assert!(chunked[0].head.starts_with("HTTP/1.1 413"), "{chunked:?}");
    assert!(chunked_closed, "the rest of a body refused is not read, so its connection closes");

    Ok(())
}

#[test]
fn an_idle_connection_is_closed_once_its_wait_for_a_request_runs_out() -> Result<(), Unheard> {
    // Catches a connection whose first wait is never timed: a peer that connects and sends nothing would hold a
    // connection, and one of the stack's `most_connections`, for good.
    let settings = StackSettings { idle_within_millis: 200, ..StackSettings::default() };
    let (mut harness, port) = echoing(settings, 64)?;

    let (_, closed) = heard(&mut harness, client(port, b"", 0));

    assert!(closed, "the stack closes a connection that sent nothing in time");

    Ok(())
}

#[test]
fn a_deadline_of_a_request_already_answered_does_not_end_the_next() -> Result<(), Unheard> {
    // Catches a sleep for a phase the connection has left still acting when it comes: the first request's deadline,
    // passed while the connection waits between requests, would answer the next `504` or close the connection.
    const FIRST: &[u8] = b"GET /first HTTP/1.1\r\nAuthorization: Bearer open\r\n\r\n";
    const SECOND: &[u8] = b"GET /second HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";
    let settings = StackSettings { answer_within_millis: 200, ..StackSettings::default() };
    let (mut harness, port) = echoing(settings, 64)?;
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    let client = thread::spawn(move || {
        let mut stream = connect(port);
        let mut buffer = Vec::new();
        assert!(stream.write_all(FIRST).is_ok(), "the client writes");
        let first = response(&mut stream, &mut buffer);
        // Past the first request's deadline, so a sleep left from it has come.
        #[allow(clippy::disallowed_methods)] // The client waits on real time, outside the engine.
        thread::sleep(Duration::from_millis(400));
        assert!(stream.write_all(SECOND).is_ok(), "the client writes");
        let second = response(&mut stream, &mut buffer);

        (vec![first, second], closes(&mut stream))
    });

    let (answered, closed) = heard(&mut harness, client);

    assert!(answered[0].head.starts_with("HTTP/1.1 200"), "{answered:?}");
    assert!(answered[1].head.starts_with("HTTP/1.1 200"), "{answered:?}");
    assert!(closed, "the second request asked to close");

    Ok(())
}

/// A host serving a [`FickleStack`] whose handler fails each request as `fails` says, and the port it listens on.
fn fickle(fails: Fails) -> Result<(Harness<Headless>, u16), Unheard> {
    let settings = StackSettings::default();
    let program = Headless::program()
        .with_engine(engine(&settings))
        .root::<Tcp>()
        .root_with::<Fickle>(FickleSettings { fails })
        .root_with::<FickleStack>(settings);
    let mut harness = Harness::of(program)?;
    let port = bound(&mut harness, |stack: &FickleStack| stack.serving.port());

    Ok((harness, port))
}

#[test]
fn a_handler_that_ended_is_a_503_and_one_that_failed_or_let_its_request_go_a_500() -> Result<(), Unheard> {
    // Catches a stack that mistakes why its handler did not answer, or hangs on it.
    const GONE: &[u8] = b"GET /gone HTTP/1.1\r\nConnection: close\r\n\r\n";
    for fails in [Fails::Faults, Fails::Drops] {
        let (mut harness, port) = fickle(fails)?;

        let (answered, _) = heard(&mut harness, client(port, GONE, 1));

        assert!(answered[0].head.starts_with("HTTP/1.1 500"), "{fails:?}: {answered:?}");
    }

    let (mut harness, port) = fickle(Fails::Faults)?;
    let fickle = harness.root::<Fickle>();
    harness.tell(fickle, &FickleOrder::Close)?;
    waiting::until(BUDGET, || {
        harness.step();

        harness.state(fickle).is_err()
    });

    let (answered, _) = heard(&mut harness, client(port, GONE, 1));

    assert!(answered[0].head.starts_with("HTTP/1.1 503"), "{answered:?}");

    Ok(())
}

#[test]
fn a_stack_does_not_start_where_a_request_could_not_be_read() {
    // Catches the check dropped, which lets one client end a handler with a request whose head it cannot read.
    let settings = StackSettings::default();
    let needed = settings.most_read_bytes();
    let program = Headless::program()
        .with_engine(EngineSettings::default())
        .root::<Tcp>()
        .root_with::<Echo>(EchoSettings { most_bytes: 64 })
        .root_with::<EchoStack>(settings);

    let Err(Unheard::Program(why)) = Harness::of(program) else {
        panic!("the program does not start");
    };

    assert!(why.contains(&ReadLimitTooLow { read_limit: 1024, needed }.to_string()), "{why}");
}

#[test]
fn a_stack_does_not_start_where_a_chunk_its_connections_read_could_not_be_held() {
    // Catches the check dropped, which lets every read of every connection fail for want of room to hold it.
    let settings = StackSettings { chunk_bytes: 2 << 20, ..StackSettings::default() };
    let engine = EngineSettings { mail_bytes: 1 << 24, blob_bytes: 1 << 20, ..engine(&settings) };
    let program = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root_with::<Echo>(EchoSettings { most_bytes: 64 })
        .root_with::<EchoStack>(settings);

    let Err(Unheard::Program(why)) = Harness::of(program) else {
        panic!("the program does not start");
    };

    assert!(why.contains(&BlobBytesTooLow { blob_bytes: 1 << 20, needed: 2 << 20 }.to_string()), "{why}");
}

/// A host serving a [`GateStack`] of `settings`, and the port it listens on.
fn gated(settings: StackSettings) -> Result<(Harness<Headless>, u16), Unheard> {
    let program = Headless::program()
        .with_engine(engine(&settings))
        .root::<Tcp>()
        .root::<Gate>()
        .root::<Upper>()
        .root::<Mirror>()
        .root_with::<GateStack>(settings);
    let mut harness = Harness::of(program)?;
    let port = bound(&mut harness, |stack: &GateStack| stack.serving.port());

    Ok((harness, port))
}

/// How many requests `gate` holds, the harness stepping the host until it holds `count`.
fn holding(harness: &mut Harness<Headless>, gate: ActorRef<Gate>, count: usize) -> usize {
    let mut held = 0;
    waiting::until(BUDGET, || {
        harness.step();
        held = harness.state(gate).map_or(0, |gate| gate.held.len());

        held == count
    });

    held
}

#[test]
fn an_actor_layer_attaches_who_sent_a_request_and_its_handler_reads_it() -> Result<(), Unheard> {
    // Catches an actor layer's answer not handed on as what the layers after it take, so the handler reads a principal
    // the layer did not attach, or the request stays parked; and what a pure layer before it attached not sent to it.
    let (mut harness, port) = gated(StackSettings::default())?;
    let sent = b"GET /who HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";

    let (answered, closed) = heard(&mut harness, client(port, sent, 1));

    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{answered:?}");
    assert!(answered[0].head.contains("x-user: open+tag"), "{answered:?}");
    assert!(answered[0].head.contains("x-attached: 1"), "{answered:?}");
    assert!(closed, "a request that asked to close is closed once answered");

    Ok(())
}

#[test]
fn an_actor_layer_that_answers_turns_the_request_back_and_its_handler_is_not_asked() -> Result<(), Unheard> {
    // Catches an actor layer's answer handed on to the handler anyway; the answer not passed back out through the
    // layers before it; and the layer that answered handed its own response on the way out.
    let (mut harness, port) = gated(StackSettings::default())?;
    let sent = b"GET /who HTTP/1.1\r\nAuthorization: Bearer nope\r\nConnection: close\r\n\r\n";

    let (refused, _) = heard(&mut harness, client(port, sent, 1));

    assert!(refused[0].head.starts_with("HTTP/1.1 401"), "{refused:?}");
    assert!(refused[0].head.contains("www-authenticate: Bearer realm=\"gate\""), "{refused:?}");
    assert!(refused[0].head.contains("x-request: 1"), "the layer before it passes it out: {refused:?}");
    assert!(!refused[0].head.contains("x-gate"), "the layer is not handed its own answer: {refused:?}");
    let mirror = harness.root::<Mirror>();
    assert_eq!(harness.state(mirror)?.asked, 0, "the handler is not asked a request a layer answered");

    Ok(())
}

#[test]
fn an_actor_layer_changes_a_response_on_its_way_out() -> Result<(), Unheard> {
    // Catches an actor layer not asked on the way out, or its answer's head not the one written, so a header it
    // changes goes out as the handler wrote it.
    let (mut harness, port) = gated(StackSettings::default())?;
    let sent = b"GET /who HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";

    let (answered, _) = heard(&mut harness, client(port, sent, 1));

    assert!(answered[0].head.contains("x-gate: passed"), "{answered:?}");
    assert!(!answered[0].head.contains("x-gate: unseen"), "{answered:?}");
    assert!(answered[0].head.contains("x-request: 1"), "the layer before it still passes it out: {answered:?}");

    Ok(())
}

#[test]
fn a_request_parked_at_an_actor_layer_holds_up_no_other_connection() -> Result<(), Unheard> {
    // Catches a stack that waits for an actor layer's answer before it serves anything else, so one slow ask holds up
    // every connection; and a parked request not taken up again once its layer answers.
    let (mut harness, port) = gated(StackSettings::default())?;
    let gate = harness.root::<Gate>();
    let waiting = client(port, b"GET /wait HTTP/1.1\r\nAuthorization: Bearer wait\r\nConnection: close\r\n\r\n", 1);
    assert_eq!(holding(&mut harness, gate, 1), 1, "the gate holds the first request");

    let open = b"GET /open HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";
    let (passed, _) = heard(&mut harness, client(port, open, 1));

    assert!(passed[0].head.starts_with("HTTP/1.1 200 OK"), "{passed:?}");
    assert_eq!(harness.state(gate)?.held.len(), 1, "the first request is still parked");

    harness.tell(gate, &GateOrder::Release)?;
    let (released, _) = heard(&mut harness, waiting);

    assert!(released[0].head.starts_with("HTTP/1.1 200 OK"), "{released:?}");
    assert!(released[0].head.contains("x-user: wait+tag"), "{released:?}");

    Ok(())
}

#[test]
fn an_actor_layer_that_never_answers_is_timed_out() -> Result<(), Unheard> {
    // Catches an ask of an actor layer with no deadline, so a layer that never answers holds its request for ever;
    // the late ask answered as anything but `504`; and a `504` written before `answer_within_millis` has passed.
    let settings = StackSettings { answer_within_millis: 200, ..StackSettings::default() };
    let (mut harness, port) = gated(settings)?;
    #[allow(clippy::disallowed_methods)] // The test times the request against its deadline on real time.
    let sent = Instant::now();

    let waiting = b"GET /wait HTTP/1.1\r\nAuthorization: Bearer wait\r\nConnection: close\r\n\r\n";

    let (timed_out, _) = heard(&mut harness, client(port, waiting, 1));

    assert!(timed_out[0].head.starts_with("HTTP/1.1 504"), "{timed_out:?}");
    assert!(timed_out[0].head.contains("x-request: 1"), "the status goes out through the layers before it");
    assert!(sent.elapsed() >= Duration::from_millis(200), "it waited for its deadline: {:?}", sent.elapsed());

    Ok(())
}

#[test]
fn an_actor_layer_that_has_ended_answers_unavailable() -> Result<(), Unheard> {
    // Catches an ask of an actor layer that has ended answered as one that failed, or not answered at all.
    let (mut harness, port) = gated(StackSettings::default())?;
    let gate = harness.root::<Gate>();
    harness.tell(gate, &GateOrder::Close)?;
    waiting::until(BUDGET, || {
        harness.step();

        harness.state(gate).is_err()
    });
    let sent = b"GET /who HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";

    let (unavailable, _) = heard(&mut harness, client(port, sent, 1));

    assert!(unavailable[0].head.starts_with("HTTP/1.1 503"), "{unavailable:?}");
    assert!(unavailable[0].head.contains("x-request: 1"), "the status goes out through the layers before it");

    Ok(())
}

#[test]
fn a_body_passes_an_actor_layer_that_takes_it_rewritten_and_one_that_does_not_untouched() -> Result<(), Unheard> {
    // Catches a part asked of an actor layer whose type does not list it, so it fails; a part an actor layer rewrote
    // not the one gathered; and a pure layer's end of the body lost on its way through an actor layer after it.
    let (mut harness, port) = gated(StackSettings::default())?;
    let sent =
        b"POST /echo HTTP/1.1\r\nAuthorization: Bearer open\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";

    let (answered, _) = heard(&mut harness, client(port, sent, 1));

    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{answered:?}");
    assert_eq!(answered[0].body, b"HELLO!", "{answered:?}");

    Ok(())
}

#[test]
fn what_is_attached_chains_through_pure_and_actor_layers_in_any_order() -> Result<(), Unheard> {
    // Catches a pure layer after an actor layer skipped, or handed what the layer before the actor attached; and an
    // actor layer handed what the pure layer before it attached for another request on the connection.
    const FIRST: &[u8] = b"GET /one HTTP/1.1\r\nAuthorization: Bearer open\r\n\r\n";
    const SECOND: &[u8] = b"GET /two HTTP/1.1\r\nAuthorization: Bearer open\r\nConnection: close\r\n\r\n";
    let (mut harness, port) = gated(StackSettings::default())?;

    let (answered, _) = heard(&mut harness, conversation(port, &[FIRST, SECOND]));

    assert!(answered[0].head.contains("x-attached: 1"), "{answered:?}");
    assert!(answered[0].head.contains("x-user: open+tag"), "{answered:?}");
    assert!(answered[1].head.contains("x-attached: 2"), "{answered:?}");
    assert!(answered[1].head.contains("x-user: open+tag"), "{answered:?}");

    Ok(())
}

/// A host serving a [`ShelfStack`], and the port it listens on.
fn shelved() -> Result<(Harness<Headless>, u16), Unheard> {
    let settings = StackSettings::default();
    let program = Headless::program()
        .with_engine(engine(&settings))
        .root::<Tcp>()
        .root::<Shelf>()
        .root::<Items>()
        .root_with::<ShelfStack>(settings);
    let mut harness = Harness::of(program)?;
    let port = bound(&mut harness, |stack: &ShelfStack| stack.serving.port());

    Ok((harness, port))
}

#[test]
fn a_router_asks_each_route_its_own_handler_and_an_instance_by_its_captures() -> Result<(), Unheard> {
    // Catches a route's request handed to another route's handler; captures not read into the handler's type, or read
    // from the wrong segment; and every capture reaching one instance, so two keys do not reach two.
    const SHELF: &[u8] = b"GET /shelf HTTP/1.1\r\n\r\n";
    const FIRST: &[u8] = b"GET /items/1 HTTP/1.1\r\n\r\n";
    const SECOND: &[u8] = b"GET /items/2 HTTP/1.1\r\nConnection: close\r\n\r\n";
    let (mut harness, port) = shelved()?;

    let (answered, _) = heard(&mut harness, conversation(port, &[SHELF, FIRST, SECOND]));

    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{answered:?}");
    assert!(answered[0].head.contains("x-handler: shelf"), "{answered:?}");
    assert!(answered[1].head.contains("x-item: 1"), "{answered:?}");
    assert!(answered[1].head.contains("x-captured: 1"), "{answered:?}");
    assert!(answered[2].head.contains("x-item: 2"), "{answered:?}");
    assert!(answered[2].head.contains("x-captured: 2"), "{answered:?}");
    let shelf = harness.root::<Shelf>();
    assert_eq!(harness.state(shelf)?.asked, 1, "the shelf is asked only for its own path");

    Ok(())
}

#[test]
fn a_path_no_route_matches_is_not_found_and_turns_back_through_the_layers_before_it() -> Result<(), Unheard> {
    // Catches a request no route matches handed to some handler anyway, or left unanswered; the router's answer not
    // passed back out through the layers before it; and a body left unread in the connection, to be read as the next
    // request.
    let (mut harness, port) = shelved()?;

    let (answered, closed) =
        heard(&mut harness, client(port, b"POST /nowhere HTTP/1.1\r\nContent-Length: 4\r\n\r\nbody", 1));

    assert!(answered[0].head.starts_with("HTTP/1.1 404"), "{answered:?}");
    assert!(answered[0].head.contains("x-request: 1"), "the layer before the router passes it out: {answered:?}");
    assert!(closed, "a request answered with its body unread closes its connection");
    let shelf = harness.root::<Shelf>();
    assert_eq!(harness.state(shelf)?.asked, 0, "no handler is asked a request no route matches");

    Ok(())
}

#[test]
fn a_path_routed_for_other_methods_is_not_allowed_and_names_them() -> Result<(), Unheard> {
    // Catches a method no route takes answered as not found, or handed to the path's handler; and `Allow` naming
    // methods the path's routes do not take, or leaving one out.
    let (mut harness, port) = shelved()?;

    let (answered, _) = heard(&mut harness, client(port, b"DELETE /shelf HTTP/1.1\r\nConnection: close\r\n\r\n", 1));

    assert!(answered[0].head.starts_with("HTTP/1.1 405"), "{answered:?}");
    assert!(answered[0].head.contains("allow: GET, POST"), "{answered:?}");
    assert!(answered[0].head.contains("x-request: 1"), "{answered:?}");

    Ok(())
}

#[test]
fn a_capture_that_does_not_read_or_names_no_live_instance_is_not_found() -> Result<(), Unheard> {
    // Catches a capture that does not read as its type handed on, or answered as anything but `404`; and a path to an
    // instance that is not live answered `503` or `500` as an ask of it would be, or standing the instance up.
    const UNREAD: &[u8] = b"GET /items/one HTTP/1.1\r\n\r\n";
    const ABSENT: &[u8] = b"GET /items/3 HTTP/1.1\r\nConnection: close\r\n\r\n";
    let (mut harness, port) = shelved()?;

    let (answered, _) = heard(&mut harness, conversation(port, &[UNREAD, ABSENT]));

    assert!(answered[0].head.starts_with("HTTP/1.1 404"), "{answered:?}");
    assert!(answered[1].head.starts_with("HTTP/1.1 404"), "{answered:?}");
    assert!(answered[1].head.contains("x-request: 2"), "the connection is kept after either: {answered:?}");

    Ok(())
}

#[test]
fn routes_that_overlap_fail_the_stack_as_it_begins() {
    // Catches routes that overlap compiled anyway, so which one a request reaches depends on their order.
    let settings = StackSettings::default();
    let program = Headless::program()
        .with_engine(engine(&settings))
        .root::<Tcp>()
        .root::<Shelf>()
        .root::<Items>()
        .root_with::<OverlappingStack>(settings);

    let begun = Harness::of(program);

    let Err(Unheard::Program(why)) = begun else {
        panic!("the program starts: {:?}", begun.err());
    };
    assert!(why.contains("overlapping_stack"), "the stack's begin is what failed: {why}");
}

/// A pure layer that writes each part of a request's body in upper case, in place of the part as it came.
#[derive(Debug, Default, Clone, Plain)]
struct Upcase;

impl LayerForm for Upcase {
    type Form = Self;
}

impl Layer for Upcase {
    type In = ();
    type Out = ();

    fn inbound(&mut self, _: &Head, (): ()) -> Step<()> {
        Step::Pass(())
    }

    fn inbound_body(&mut self, part: &mut BodyPart<'_>) {
        if part.is_end() {
            return;
        }
        let upper = part.bytes().to_ascii_uppercase();
        part.out().extend_from_slice(&upper);
    }
}

/// How [`Drain`] reads each request's body.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "test.http_stack.drains")]
enum Drains {
    /// It opens one stream, grants a part back for each it takes, and answers once the stream ends.
    #[default]
    Whole,
    /// It opens one stream and takes each part without granting for it, until it is told [`DrainOrder::Release`].
    Slowly,
    /// It answers as the first part comes, before the body ends.
    Hastily,
    /// It opens two streams on the body.
    Twice,
    /// It answers at once, and opens none.
    Never,
}

/// [`Drain`]'s settings: how it reads each body.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.drain_settings")]
#[settings]
struct DrainSettings {
    drains: Drains,
}

/// What [`Drain`] is told to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "test.http_stack.drain_order")]
enum DrainOrder {
    /// Grants every part it took without granting for, and from then grants a part back for each it takes.
    Release,
}

/// A handler that reads each request's body as a stream, as its settings say, keeping every byte it is sent, the size
/// of each part, and how each stream ended; it answers with what it read.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.drain")]
struct Drain {
    drains: Drains,
    bodies: Streams<InboundBody, ()>,
    owing: Owing,
    withheld: Withheld,
    bytes: Vec<u8>,
    sizes: Vec<u32>,
    read: Vec<BodyRead>,
}

/// Whether [`Drain`] owes a request its answer.
#[derive(Bound)]
enum Owing {
    /// It owes none.
    Nothing,
    /// It holds the request, to answer once its body is read.
    Answer(Held<Response>),
}

/// What [`Drain`] has taken of a stream without granting for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Withheld {
    /// Nothing.
    Nothing,
    /// `parts` of the stream `stream`.
    Parts { stream: Number, parts: u32 },
}

impl ChildOf for Drain {
    type Parent = Top;
}

impl Actor for Drain {
    type Config = DrainSettings;
    type Key = ();
    type Handles = (Request<(), ()>, Part<InboundBody>, Answer<BodyRead, Number>, DrainOrder);
    type Depends = ();
    type Spawns = ();

    fn begin(settings: DrainSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self {
            drains: settings.drains,
            bodies: Streams::new(NonZeroU32::MIN),
            owing: Owing::Nothing,
            withheld: Withheld::Nothing,
            bytes: Vec::new(),
            sizes: Vec::new(),
            read: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<(), ()>> for Drain {
    type Answers = (Response,);
}

impl Drain {
    /// Answers the request it holds with what it has read of its body.
    fn answer(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Owing::Answer(held) = mem::replace(&mut self.owing, Owing::Nothing) else {
            return Ok(());
        };
        let response = turn.put(self.bytes.clone()).map_or_else(
            |_| Response::empty(Status::InternalServerError),
            |body| Response::new(Status::Ok, "text/plain", body),
        );

        Ok(turn.answer(held, &response)?)
    }
}

impl Handles<Request<(), ()>> for Drain {
    type From = FromStack;

    fn handle(&mut self, request: Request<(), ()>, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        if self.drains == Drains::Never {
            return Ok(turn.reply(&Response::empty(Status::Ok))?);
        }
        self.owing = Owing::Answer(turn.hold()?);
        let BodyHandle { reader, .. } = request.body;
        self.bodies.open(turn, reader, request.body.read(), ())?;
        if self.drains == Drains::Twice {
            self.bodies.open(turn, reader, request.body.read(), ())?;
        }

        Ok(())
    }
}

impl Handles<Part<InboundBody>> for Drain {
    type From = Anyone;

    fn handle(&mut self, part: Part<InboundBody>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let taken = match self.drains {
            Drains::Slowly => self.bodies.part_held(turn, part),
            Drains::Whole | Drains::Hastily | Drains::Twice | Drains::Never => self.bodies.part(turn, part)?,
        };
        let Some(Taken { number, value, .. }) = taken else {
            return Ok(());
        };
        let bytes = turn.read(&value.bytes);
        self.sizes.push(u32::try_from(bytes.len()).unwrap_or(u32::MAX));
        self.bytes.extend_from_slice(bytes);
        match (self.drains, self.withheld) {
            (Drains::Slowly, Withheld::Parts { parts, .. }) => {
                self.withheld = Withheld::Parts { stream: number, parts: parts + 1 }
            }
            (Drains::Slowly, Withheld::Nothing) => self.withheld = Withheld::Parts { stream: number, parts: 1 },
            (Drains::Hastily, _) => self.answer(turn)?,
            (Drains::Whole | Drains::Twice | Drains::Never, _) => {}
        }

        Ok(())
    }
}

impl Receives<Answer<BodyRead, Number>> for Drain {
    fn handle(&mut self, (read, number): (Answered<BodyRead>, Number), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.bodies.answered(number);
        let Answered::Yes(read) = read else {
            return Err(Fault::of_parts(KindId::of::<Self>(), Vec::new()));
        };
        self.read.push(read);
        if !self.bodies.is_empty() {
            return Ok(());
        }

        self.answer(turn)
    }
}

impl Handles<DrainOrder> for Drain {
    type From = Anyone;

    fn handle(&mut self, DrainOrder::Release: DrainOrder, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.drains = Drains::Whole;
        let Withheld::Parts { stream, parts } = mem::replace(&mut self.withheld, Withheld::Nothing) else {
            return Ok(());
        };
        if let Some(parts) = NonZeroU32::new(parts) {
            self.bodies.grant(turn, stream, parts)?;
        }

        Ok(())
    }
}

/// How [`Serve`]'s source ends each body it sends once every part of its plan is sent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "test.http_stack.plan_end")]
enum PlanEnd {
    /// It ends it `Whole`.
    #[default]
    Whole,
    /// It does not end it, and waits to be stopped.
    Hold,
    /// It ends it `Failed`.
    Fail,
}

/// What [`Serve`] answers each request with: a body streamed from itself, of `length`, which it sends as `parts`,
/// each as credit allows, then ends as `end` says.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "test.http_stack.plan")]
struct Plan {
    length: ResponseLength,
    parts: Vec<String>,
    end: PlanEnd,
}

/// [`Serve`]'s settings: none.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.serve_settings")]
#[settings]
struct ServeSettings {}

/// A handler that answers each request with a body streamed from itself, as the [`Plan`] it was last told says, and
/// the source of that body: it keeps how each body it was asked for ended, and the most credit it held of any.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.serve")]
struct Serve {
    plan: Plan,
    serving: Serving<OutboundBody>,
    making: Making,
    ended: Vec<SourceEnd>,
    most_credit: u32,
}

/// Where [`Serve`] stands in the body it sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Making {
    /// It sends none.
    Idle,
    /// It sends the body of the stream `stream`, the part of its plan at `next` the next to send.
    Parts { stream: StreamKey, next: u32 },
}

impl ChildOf for Serve {
    type Parent = Top;
}

impl Actor for Serve {
    type Config = ServeSettings;
    type Key = ();
    type Handles = (Request<(), ()>, Plan, Open<OutboundBody>, More, Enough, Ended);
    type Depends = ();
    type Spawns = ();

    fn begin(_: ServeSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let plan = Plan { length: ResponseLength::Unknown, parts: Vec::new(), end: PlanEnd::Whole };

        Ok(Self { plan, serving: Serving::new(), making: Making::Idle, ended: Vec::new(), most_credit: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Request<(), ()>> for Serve {
    type Answers = (Response,);
}

impl Serve {
    /// Sends what is left of its plan's parts while the stream has credit, then ends the body as its plan says.
    fn send_planned(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Making::Parts { stream, mut next } = self.making else {
            return Ok(());
        };
        let Some(streaming) = self.serving.get_mut(stream) else {
            return Ok(());
        };
        while let Some(part) = self.plan.parts.get(usize::try_from(next).unwrap_or(usize::MAX)) {
            self.most_credit = self.most_credit.max(streaming.credit());
            let Ok(bytes) = turn.put(part.clone().into_bytes()) else {
                return Err(Fault::of_parts(KindId::of::<Self>(), Vec::new()));
            };
            match streaming.send(turn, ResponseBytes { bytes }) {
                Ok(()) => next += 1,
                Err(Unsent::NoCredit(_)) => break,
                Err(Unsent::Unwritable(error)) => return Err(error.into()),
            }
        }
        self.making = Making::Parts { stream, next };
        let is_sent = usize::try_from(next).is_ok_and(|next| next == self.plan.parts.len());
        if !is_sent {
            return Ok(());
        }
        let end = match self.plan.end {
            PlanEnd::Whole => SourceEnd::Whole,
            PlanEnd::Fail => SourceEnd::Failed,
            PlanEnd::Hold => return Ok(()),
        };

        let Some(streaming) = self.serving.take(stream) else {
            return Ok(());
        };
        streaming.finish(turn, &end)?;
        self.ended.push(end);
        self.making = Making::Idle;

        Ok(())
    }
}

impl Handles<Request<(), ()>> for Serve {
    type From = FromStack;

    fn handle(&mut self, _: Request<(), ()>, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        let source = turn.me().protocol();
        let body = ResponseBody::Streamed(StreamedBody { source, body: 1, length: self.plan.length });
        let headers = vec![Header::new("content-type", "text/plain")];

        Ok(turn.reply(&Response { status: Status::Ok, headers, body })?)
    }
}

impl Handles<Plan> for Serve {
    type From = Anyone;

    fn handle(&mut self, plan: Plan, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.plan = plan;

        Ok(())
    }
}

impl Handles<Open<OutboundBody>> for Serve {
    type From = Anyone;

    fn handle(&mut self, open: Open<OutboundBody>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let (stream, _) = self.serving.open(turn, open)?;
        self.making = Making::Parts { stream, next: 0 };

        self.send_planned(turn)
    }
}

impl Handles<More> for Serve {
    type From = Anyone;

    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if self.serving.more(turn, &more).is_none() {
            return Ok(());
        }

        self.send_planned(turn)
    }
}

impl Handles<Enough> for Serve {
    type From = Anyone;

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(stopped) = self.serving.stop(turn, &enough) else {
            return Ok(());
        };
        stopped.finish(turn, &SourceEnd::Stopped)?;
        self.ended.push(SourceEnd::Stopped);
        self.making = Making::Idle;

        Ok(())
    }
}

impl Receives<Ended> for Serve {
    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        drop(self.serving.ended(&ended));

        Ok(())
    }
}

/// [`Shout`]'s settings: none.
#[derive(Debug, Default, Plain)]
#[kind(name = "test.http_stack.shout_settings")]
#[settings]
struct ShoutSettings {}

/// An actor layer that passes every head as it came, and takes a response's body, each part of which it passes in upper
/// case.
#[derive(Bound, Bootstrap)]
#[kind(name = "test.http_stack.shout")]
struct Shout;

impl ChildOf for Shout {
    type Parent = Top;
}

impl Actor for Shout {
    type Config = ShoutSettings;
    type Key = ();
    type Handles = (Inbound<()>, Outbound, OutboundPart);
    type Depends = ();
    type Spawns = ();

    fn begin(_: ShoutSettings, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Inbound<()>> for Shout {
    type Answers = (Inward<()>,);
}

impl Carries<Outbound> for Shout {
    type Answers = (Outward,);
}

impl Handles<Inbound<()>> for Shout {
    type From = FromStack;

    fn handle(&mut self, _: Inbound<()>, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        Ok(turn.reply(&Inward::Pass(()))?)
    }
}

impl Handles<Outbound> for Shout {
    type From = FromStack;

    fn handle(&mut self, outbound: Outbound, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        Ok(turn.reply(&Outward::Pass(outbound.response))?)
    }
}

impl Handles<OutboundPart> for Shout {
    type From = FromStack;

    fn handle(&mut self, part: OutboundPart, turn: &mut Turn<'_, Self, Self::From>) -> Result<(), Fault> {
        let upper = turn.read(&part.bytes).to_ascii_uppercase();
        let step = turn.put(upper).map_or(PartStep::Refuse(Refused::TooLarge), PartStep::Pass);

        Ok(turn.reply(&step)?)
    }
}

stack!(ReadStack, "test.http_stack.read_stack", "read_stack", (Upcase, Drain), [], depends(Drain));
stack!(SourceStack, "test.http_stack.source_stack", "source_stack", (Serve,), [], depends(Serve));
stack!(ShoutStack, "test.http_stack.shout_stack", "shout_stack", (Shout, Serve), [()], depends(Shout, Serve));

/// A host serving a [`ReadStack`] of `settings`, whose handler reads each body as `drains` says, and the port it
/// listens on.
fn reading(settings: StackSettings, drains: Drains) -> Result<(Harness<Headless>, u16), Unheard> {
    let program = Headless::program()
        .with_engine(engine(&settings))
        .root::<Tcp>()
        .root_with::<Drain>(DrainSettings { drains })
        .root_with::<ReadStack>(settings);
    let mut harness = Harness::of(program)?;
    let port = bound(&mut harness, |stack: &ReadStack| stack.serving.port());

    Ok((harness, port))
}

/// A host serving a [`SourceStack`], or a [`ShoutStack`] when `shouts`, whose handler streams each response as `plan`
/// says, and the port it listens on.
fn sourcing(plan: &Plan, shouts: Shouts) -> Result<(Harness<Headless>, u16), Unheard> {
    let settings = StackSettings::default();
    let program = Headless::program().with_engine(engine(&settings)).root::<Tcp>().root::<Serve>();
    let (mut harness, port) = match shouts {
        Shouts::No => {
            let mut harness = Harness::of(program.root_with::<SourceStack>(settings))?;
            let port = bound(&mut harness, |stack: &SourceStack| stack.serving.port());
            (harness, port)
        }
        Shouts::Yes => {
            let mut harness = Harness::of(program.root::<Shout>().root_with::<ShoutStack>(settings))?;
            let port = bound(&mut harness, |stack: &ShoutStack| stack.serving.port());
            (harness, port)
        }
    };
    let serve = harness.root::<Serve>();
    harness.tell(serve, plan)?;

    Ok((harness, port))
}

/// Whether a host's stack has [`Shout`] before its handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shouts {
    No,
    Yes,
}

/// A client on a thread of its own that sends `sent` to `port` and reads everything the server writes until it closes.
fn raw(port: u16, sent: impl AsRef<[u8]> + Send + 'static) -> JoinHandle<Vec<u8>> {
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let mut stream = connect(port);
        assert!(stream.write_all(sent.as_ref()).is_ok(), "the client writes");
        let mut read = Vec::new();
        assert!(stream.read_to_end(&mut read).is_ok(), "the server closes within ten seconds");

        read
    })
}

/// What `client` answered, the harness stepping the host until it has.
fn joined<R>(harness: &mut Harness<Headless>, client: JoinHandle<R>) -> R {
    while !client.is_finished() {
        harness.step();
    }
    let Ok(answered) = client.join() else {
        panic!("the client finished");
    };

    answered
}

/// The head and the body of what a server wrote, split at the blank line that ends the head.
fn split(written: &[u8]) -> (String, &[u8]) {
    let Some(end) = written.windows(4).position(|window| window == b"\r\n\r\n") else {
        panic!("a head was written: {:?}", String::from_utf8_lossy(written));
    };

    (String::from_utf8_lossy(&written[..end]).into_owned(), &written[end + 4..])
}

/// `body` in chunks of `sizes` bytes in turn, chunked as HTTP/1.1 frames it, its last chunk after them.
fn chunked(body: &[u8], sizes: &[usize]) -> Vec<u8> {
    let mut framed = Vec::new();
    let mut at = 0;
    for &size in sizes.iter().cycle() {
        if at == body.len() {
            break;
        }
        let end = (at + size).min(body.len());
        framed.extend_from_slice(format!("{:x}\r\n", end - at).as_bytes());
        framed.extend_from_slice(&body[at..end]);
        framed.extend_from_slice(b"\r\n");
        at = end;
    }
    framed.extend_from_slice(b"0\r\n\r\n");

    framed
}

#[test]
fn a_large_chunked_body_reaches_its_reader_whole_and_in_order_through_a_layer_that_rewrites_it() -> Result<(), Unheard>
{
    // Catches a part lost, sent twice, or out of order; a chunk's size line or data cut between reads decoded wrong; a
    // pure layer's rewrite not the part the reader is sent; and the stream ended before its last chunk, or never.
    let settings = StackSettings { chunk_bytes: 1024, ..StackSettings::default() };
    let (mut harness, port) = reading(settings, Drains::Whole)?;
    let body: Vec<u8> = (0..256 * 1024).map(|at| b"abcdefghijklmnopqrstuvwxyz"[at % 26]).collect();
    let mut sent = b"POST /up HTTP/1.1\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
    sent.extend_from_slice(&chunked(&body, &[1, 700, 4096, 33, 9000]));

    let (answered, closed) = heard(&mut harness, client(port, sent, 1));

    let drain = harness.root::<Drain>();
    let drain = harness.state(drain)?;
    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{:?}", answered[0].head);
    assert_eq!(drain.read, [BodyRead::Whole]);
    assert!(drain.bytes == body.to_ascii_uppercase(), "the reader is sent every byte, rewritten, in order");
    assert!(drain.sizes.len() > 1, "the body comes in parts: {}", drain.sizes.len());
    assert!(closed, "a request that asked to close is closed once answered");

    Ok(())
}

#[test]
fn a_reader_that_grants_slowly_bounds_what_the_stack_reads_ahead_of_it() -> Result<(), Unheard> {
    // Catches a connection granted credit for its frames while its body's reader has none, so the stack buffers the
    // client's body without bound; and the credit withheld never granted back once the reader takes more.
    let settings = StackSettings { chunk_bytes: 64, window: 2, ..StackSettings::default() };
    let bound: u32 = 2 * 64;
    let (mut harness, port) = reading(settings, Drains::Slowly)?;
    let body = vec![b'a'; 64 * 1024];
    let mut sent =
        format!("POST /up HTTP/1.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    sent.extend_from_slice(&body);
    let client = client(port, sent, 1);
    let drain = harness.root::<Drain>();
    let stack = harness.root::<ReadStack>();

    let mut held = 0;
    waiting::until(BUDGET, || {
        harness.step();
        held = harness.state(stack).map_or(0, |stack| u32::try_from(stack.serving.held_bytes()).unwrap_or(u32::MAX));
        let parts = harness.state(drain).map_or(0, |drain| drain.sizes.len());

        parts == 1 && held == bound
    });
    assert_eq!(held, bound, "the stack holds no more than its read window while the reader takes nothing");

    harness.tell(drain, &DrainOrder::Release)?;
    let (answered, _) = heard(&mut harness, client);

    let drain = harness.state(drain)?;
    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{:?}", answered[0].head);
    assert_eq!((drain.read.as_slice(), drain.bytes.len()), (&[BodyRead::Whole][..], body.len()));
    // The frame that carried the head is granted back as it is read, so its rest stands beside the window.
    let most = bound + 64;
    assert!(
        drain.sizes.iter().all(|&size| size <= most),
        "no part is more than the stack read ahead: {:?}",
        drain.sizes
    );

    Ok(())
}

#[test]
fn a_handler_that_answers_before_the_body_ends_cuts_it_and_the_connection_closes_after_the_response()
-> Result<(), Unheard> {
    // Catches a stream left open once its handler answers, never ended; the rest of the body read as the next request;
    // and the connection kept, though where its next request starts is not known.
    let (mut harness, port) = reading(StackSettings::default(), Drains::Hastily)?;
    let sent = b"POST /up HTTP/1.1\r\nContent-Length: 1000\r\n\r\nonly ten b";

    let (answered, closed) = heard(&mut harness, client(port, sent, 1));

    let drain = harness.root::<Drain>();
    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{:?}", answered[0].head);
    assert!(answered[0].head.contains("connection: close"), "{:?}", answered[0].head);
    assert!(!answered[0].body.is_empty(), "it answers with the part it read");
    assert!(b"ONLY TEN B".starts_with(&answered[0].body), "{:?}", answered[0].body);
    assert_eq!(harness.state(drain)?.read, [BodyRead::Cut]);
    assert!(closed, "the rest of a cut body is not read, so its connection closes");

    Ok(())
}

#[test]
fn a_second_open_of_a_body_or_one_of_a_body_already_read_is_busy() -> Result<(), Unheard> {
    // Catches two streams handed one body's parts; and a body of no bytes read again by a second stream, which a stale
    // stream would read as the next request's.
    let (mut harness, port) = reading(StackSettings::default(), Drains::Twice)?;
    let drain = harness.root::<Drain>();

    let posted = b"POST /up HTTP/1.1\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello";
    let (posted, _) = heard(&mut harness, client(port, posted, 1));
    let read = harness.state(drain)?.read.clone();
    let (got, _) = heard(&mut harness, client(port, b"GET /up HTTP/1.1\r\nConnection: close\r\n\r\n", 1));

    assert_eq!(posted[0].body, b"HELLO");
    assert!(read.contains(&BodyRead::Whole), "one stream reads the body: {read:?}");
    assert!(read.contains(&BodyRead::Busy), "the other is busy: {read:?}");
    assert_eq!(harness.state(drain)?.read[2..], [BodyRead::Whole, BodyRead::Busy]);
    assert!(got[0].head.starts_with("HTTP/1.1 200 OK"), "{:?}", got[0].head);

    Ok(())
}

#[test]
fn a_body_that_stops_moving_ends_stalled_and_its_connection_closes_unanswered() -> Result<(), Unheard> {
    // Catches a body that stops coming held for ever, its stream never ended; a stalled request answered; and the
    // stall timed before `stall_within_millis` has passed.
    let settings = StackSettings { stall_within_millis: 200, ..StackSettings::default() };
    let (mut harness, port) = reading(settings, Drains::Whole)?;
    #[allow(clippy::disallowed_methods)] // The test times the stall against its limit on real time.
    let sent = Instant::now();

    let written = joined(&mut harness, raw(port, b"POST /up HTTP/1.1\r\nContent-Length: 10\r\n\r\nfour"));

    let drain = harness.root::<Drain>();
    assert!(written.is_empty(), "nothing is written: {:?}", String::from_utf8_lossy(&written));
    assert_eq!(harness.state(drain)?.read, [BodyRead::Stalled]);
    assert!(sent.elapsed() >= Duration::from_millis(200), "it waited for the stall: {:?}", sent.elapsed());

    Ok(())
}

/// A client on a thread of its own that sends `head` to `port`, reads the response that comes, and, when it is
/// `100 Continue`, sends `body` and reads the next: each response it read.
fn expecting(port: u16, head: &'static [u8], body: &'static [u8]) -> JoinHandle<Vec<Read>> {
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let mut stream = connect(port);
        let mut buffer = Vec::new();
        assert!(stream.write_all(head).is_ok(), "the client writes its head");
        let first = response(&mut stream, &mut buffer);
        if !first.head.starts_with("HTTP/1.1 100") {
            return vec![first];
        }
        assert!(stream.write_all(body).is_ok(), "the client writes its body");

        vec![first, response(&mut stream, &mut buffer)]
    })
}

#[test]
fn a_client_that_expects_to_continue_is_told_as_its_body_is_opened_and_not_when_it_is_never_read() -> Result<(), Unheard>
{
    // Catches `100 Continue` never sent, so a client that waits for it never sends its body; and sent to a request its
    // handler answers without reading, so the client sends a body no one reads.
    const HEAD: &[u8] = b"POST /up HTTP/1.1\r\nExpect: 100-continue\r\nContent-Length: 5\r\nConnection: close\r\n\r\n";
    let (mut opened, opened_port) = reading(StackSettings::default(), Drains::Whole)?;
    let (mut unread, unread_port) = reading(StackSettings::default(), Drains::Never)?;

    let continued = joined(&mut opened, expecting(opened_port, HEAD, b"hello"));
    let answered = joined(&mut unread, expecting(unread_port, HEAD, b"hello"));

    assert!(continued[0].head.starts_with("HTTP/1.1 100 Continue"), "{continued:?}");
    assert_eq!(continued[1].body, b"HELLO");
    assert_eq!(answered.len(), 1, "{answered:?}");
    assert!(answered[0].head.starts_with("HTTP/1.1 200 OK"), "{answered:?}");

    Ok(())
}

#[test]
fn a_client_that_expects_to_continue_with_a_chunked_body_is_told_as_its_body_is_opened() -> Result<(), Unheard> {
    // Catches `100 Continue` written only for a body of known length, so a client that waits to send a chunked body
    // stalls.
    const HEAD: &[u8] =
        b"POST /up HTTP/1.1\r\nExpect: 100-continue\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
    let (mut harness, port) = reading(StackSettings::default(), Drains::Whole)?;

    let continued = joined(&mut harness, expecting(port, HEAD, b"5\r\nhello\r\n0\r\n\r\n"));

    assert!(continued[0].head.starts_with("HTTP/1.1 100 Continue"), "{continued:?}");
    assert_eq!(continued[1].body, b"HELLO");

    Ok(())
}

#[test]
fn a_body_of_unknown_length_is_chunked_to_one_one_and_delimited_by_the_close_to_one_zero() -> Result<(), Unheard> {
    // Catches a chunked body's size lines or its last chunk written wrong; an empty part written as a chunk, which
    // ends the body early; a chunked body written to an HTTP/1.0 client, which reads no chunks, or its connection kept
    // with nothing to say where the body ends; and the next request on the connection read before the last chunk.
    let parts = vec![String::from("hello "), String::new(), String::from("world")];
    let plan = Plan { length: ResponseLength::Unknown, parts, end: PlanEnd::Whole };
    let (mut harness, port) = sourcing(&plan, Shouts::No)?;

    let pipelined =
        joined(&mut harness, raw(port, b"GET /a HTTP/1.1\r\n\r\nGET /b HTTP/1.1\r\nConnection: close\r\n\r\n"));
    let old = joined(&mut harness, raw(port, b"GET /a HTTP/1.0\r\n\r\n"));

    let (first, rest) = split(&pipelined);
    let chunks: &[u8] = b"6\r\nhello \r\n5\r\nworld\r\n0\r\n\r\n";
    assert!(first.contains("transfer-encoding: chunked"), "{first}");
    assert!(rest.starts_with(chunks), "{:?}", String::from_utf8_lossy(rest));
    let (second, last) = split(&rest[chunks.len()..]);
    assert!(second.starts_with("HTTP/1.1 200 OK"), "{second}");
    assert_eq!(last, chunks);
    let (head, body) = split(&old);
    assert!(!head.contains("transfer-encoding"), "{head}");
    assert!(!head.contains("content-length"), "{head}");
    assert_eq!(body, b"hello world");

    Ok(())
}

#[test]
fn a_known_length_its_source_comes_short_of_or_runs_past_closes_the_connection_cut_short() -> Result<(), Unheard> {
    // Catches a body that came short of its length ended as if whole, so the client waits for bytes that never come or
    // reads the next response as them; and one that ran past it written whole, so the client reads its end as the
    // next response.
    const ASKED: &[u8] = b"GET /a HTTP/1.1\r\n\r\n";
    let short = Plan { length: ResponseLength::Known(10), parts: vec![String::from("12345")], end: PlanEnd::Whole };
    let long = Plan { length: ResponseLength::Known(3), parts: vec![String::from("12345")], end: PlanEnd::Hold };
    let (mut shorted, short_port) = sourcing(&short, Shouts::No)?;
    let (mut lengthened, long_port) = sourcing(&long, Shouts::No)?;

    let came_short = joined(&mut shorted, raw(short_port, ASKED));
    let ran_past = joined(&mut lengthened, raw(long_port, ASKED));

    let (short_head, short_body) = split(&came_short);
    let (long_head, long_body) = split(&ran_past);
    assert!(short_head.contains("content-length: 10"), "{short_head}");
    assert_eq!(short_body, b"12345");
    assert!(long_head.contains("content-length: 3"), "{long_head}");
    assert!(long_body.len() < 3, "{long_body:?}");
    let serve = lengthened.root::<Serve>();
    assert_eq!(lengthened.state(serve)?.ended, [SourceEnd::Stopped], "the source of a body cut short is stopped");

    Ok(())
}

#[test]
fn a_source_that_fails_midway_closes_the_connection_without_ending_the_body() -> Result<(), Unheard> {
    // Catches a failed source's body ended with its last chunk, which a client would take for the whole body, and a
    // connection kept open after a head whose status can no longer say the body broke.
    let parts = vec![String::from("one "), String::from("two "), String::from("three")];
    let plan = Plan { length: ResponseLength::Unknown, parts, end: PlanEnd::Fail };
    let (mut harness, port) = sourcing(&plan, Shouts::No)?;

    let written = joined(&mut harness, raw(port, b"GET /a HTTP/1.1\r\n\r\n"));

    let (head, body) = split(&written);
    let whole: &[u8] = b"4\r\none \r\n4\r\ntwo \r\n5\r\nthree\r\n0\r\n\r\n";
    assert!(head.contains("transfer-encoding: chunked"), "{head}");
    assert!(whole.starts_with(body), "what came is the body's start: {:?}", String::from_utf8_lossy(body));
    assert!(body.len() < whole.len(), "the body is cut short by the close, not ended");
    let serve = harness.root::<Serve>();
    assert_eq!(harness.state(serve)?.ended, [SourceEnd::Failed]);

    Ok(())
}

#[test]
fn a_streamed_responses_source_runs_no_more_than_its_window_ahead_of_what_is_written() -> Result<(), Unheard> {
    // Catches a part granted back to its source as the stack takes it as well as once it is written, which lets the
    // source run past its window, so a client that reads slowly no longer holds it back.
    let parts = vec!["x".repeat(100); 60];
    let plan = Plan { length: ResponseLength::Unknown, parts, end: PlanEnd::Whole };
    let (mut harness, port) = sourcing(&plan, Shouts::No)?;

    let written = joined(&mut harness, raw(port, b"GET /a HTTP/1.1\r\nConnection: close\r\n\r\n"));

    let (head, body) = split(&written);
    let serve = harness.root::<Serve>();
    let serve = harness.state(serve)?;
    assert!(head.starts_with("HTTP/1.1 200 OK"), "{head}");
    assert!(body.ends_with(b"\r\n0\r\n\r\n"), "the body ended with its last chunk");
    assert_eq!(serve.ended, [SourceEnd::Whole]);
    let window = StackSettings::default().window;
    assert!(serve.most_credit <= window, "the source ran {} parts ahead of a window of {window}", serve.most_credit);

    Ok(())
}

#[test]
fn a_client_that_hangs_up_mid_response_stops_its_source() -> Result<(), Unheard> {
    // Catches a source left sending to a connection that has gone, its stream never stopped, so it holds what it makes
    // the body from for ever.
    let parts = vec![String::from("some of the body"); 4];
    let plan = Plan { length: ResponseLength::Unknown, parts, end: PlanEnd::Hold };
    let (mut harness, port) = sourcing(&plan, Shouts::No)?;
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    let client = thread::spawn(move || {
        let mut stream = connect(port);
        assert!(stream.write_all(b"GET /a HTTP/1.1\r\n\r\n").is_ok(), "the client writes");
        let mut read = Vec::new();
        let mut chunk = [0; 4096];
        while !read.windows(16).any(|window| window == b"some of the body") {
            let Ok(count) = stream.read(&mut chunk) else {
                panic!("the body comes within ten seconds");
            };
            assert_ne!(count, 0, "the server writes the body before it closes");
            read.extend_from_slice(&chunk[..count]);
        }
    });
    joined(&mut harness, client);
    let serve = harness.root::<Serve>();

    let mut ended = Vec::new();
    waiting::until(BUDGET, || {
        harness.step();
        ended = harness.state(serve).map_or_else(|_| Vec::new(), |serve| serve.ended.clone());

        !ended.is_empty()
    });

    assert_eq!(ended, [SourceEnd::Stopped]);

    Ok(())
}

#[test]
fn a_head_request_is_written_its_head_alone_and_its_source_is_stopped_unread() -> Result<(), Unheard> {
    // Catches a body written in answer to a `HEAD`; its length left out of the head; and a source opened and left
    // waiting to be read, or never opened, so it never learns it will not be.
    // The source holds its body open once it has sent it, so it ends only when it is stopped.
    let plan = Plan { length: ResponseLength::Known(5), parts: vec![String::from("hello")], end: PlanEnd::Hold };
    let (mut harness, port) = sourcing(&plan, Shouts::No)?;

    let written = joined(&mut harness, raw(port, b"HEAD /a HTTP/1.1\r\nConnection: close\r\n\r\n"));

    let (head, body) = split(&written);
    assert!(head.contains("content-length: 5"), "{head}");
    assert!(body.is_empty(), "{body:?}");
    let serve = harness.root::<Serve>();
    assert_eq!(harness.state(serve)?.ended, [SourceEnd::Stopped]);

    Ok(())
}

#[test]
fn a_request_sent_after_a_streamed_body_is_read_once_that_body_ends_and_answered_in_order() -> Result<(), Unheard> {
    // Catches a streamed body's decoder that reads past its last chunk into the next request; the bytes after it lost;
    // and the next request answered before the one before it.
    let settings = StackSettings { chunk_bytes: 16, window: 1, ..StackSettings::default() };
    let (mut harness, port) = reading(settings, Drains::Whole)?;
    let ahead = b"POST /one HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nhel\r\n2\r\nlo\r\n0\r\n\r\n\
        POST /two HTTP/1.1\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbye";

    let (answered, closed) = heard(&mut harness, client(port, ahead, 2));

    assert_eq!(answered[0].body, b"HELLO", "{answered:?}");
    assert_eq!(answered[1].body, b"HELLOBYE", "the second is read after the first: {answered:?}");
    assert!(closed, "the second request asked to close");

    Ok(())
}

#[test]
fn an_actor_layer_that_takes_a_streamed_body_rewrites_each_part_before_it_is_written() -> Result<(), Unheard> {
    // Catches a streamed part written before an actor layer that lists `OutboundPart` has passed it, or written as it
    // came rather than as the layer passed it; and the parts written out of order once a layer is asked about each.
    let parts = vec![String::from("hello "), String::from("streamed "), String::from("world")];
    let plan = Plan { length: ResponseLength::Known(20), parts, end: PlanEnd::Whole };
    let (mut harness, port) = sourcing(&plan, Shouts::Yes)?;

    let (answered, _) = heard(&mut harness, client(port, b"GET /a HTTP/1.1\r\nConnection: close\r\n\r\n", 1));

    assert!(answered[0].head.contains("content-length: 20"), "{:?}", answered[0].head);
    assert_eq!(answered[0].body, b"HELLO STREAMED WORLD");

    Ok(())
}
