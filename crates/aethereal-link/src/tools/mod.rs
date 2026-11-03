//! The hub's tools: the agent lists engines, reads an engine's tools, calls one, mints a token for an engine it starts
//! itself, spawns an engine from the store, and terminates one (ADR-0038, ADR-0040).

mod kinds;

pub use kinds::{EngineAnswer, EngineCall, EngineId, EngineSheets, JsonSheet, SpawnedEngine};

use crate::{
    CallEngine, DescribeEngine, Described, Engine, EngineList, Engines, ListEngines, MintToken, Minted, Outcome, Spawn,
    SpawnEngine, TerminateEngine, Terminated,
};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{
    Bound, Budget, Json, JsonError, Plain, Schema, SchemaNodes, from_bytes, from_json, json_schema, to_bytes, to_json,
};
use aethereal_process::ExitStatus;
use aethereal_tools::{Call, Called, Done, Output, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The tool that lists engines.
const LIST_ENGINES: &str = "list_engines";
/// The tool that describes an engine's tools.
const DESCRIBE_ENGINE: &str = "describe_engine";
/// The tool that calls an engine's tool.
const CALL_TOOL: &str = "call_tool";
/// The tool that mints a token for an engine started outside the hub.
const MINT_TOKEN: &str = "mint_token";
/// The tool that spawns an engine from the store.
const SPAWN_ENGINE: &str = "spawn_engine";
/// The tool that terminates an engine.
const TERMINATE_ENGINE: &str = "terminate_engine";

/// The most bytes one decode of an input, or one walk between JSON and bytes or a schema and JSON Schema, may take.
const WALK_BYTES: usize = 16 * 1024 * 1024;

/// What the tools keep for an ask they made: the call it was made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.tools_asking")]
pub struct Asking {
    number: u64,
}

/// What the tools are made from: nothing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.engine_tools_config")]
pub struct EngineToolsConfig;

/// Where a call held is: which ask it waits on, and what its answer needs.
#[derive(Bound)]
enum Pending {
    /// `list_engines`: `Engines` is asked for its list.
    Listing { held: Held<Called> },
    /// `describe_engine`: the engine is asked for its tools.
    Describing { held: Held<Called>, engine: u32 },
    /// `call_tool`: the engine is asked for its tools, to find `tool` and convert `input` by its input schema.
    Finding { held: Held<Called>, engine: u32, tool: String, input: Json },
    /// `call_tool`: the engine is asked to call `tool`, whose sheet says it answers with `output`.
    Calling { held: Held<Called>, tool: String, output: Output },
    /// `mint_token`: `Engines` is asked for a token.
    Minting { held: Held<Called> },
    /// `spawn_engine`: `Engines` is asked to spawn an engine.
    Spawning { held: Held<Called> },
    /// `terminate_engine`: `Engines` is asked to terminate engine `engine`.
    Terminating { held: Held<Called>, engine: u32 },
}

impl Pending {
    /// The call held, whatever ask it waits on.
    fn into_held(self) -> Held<Called> {
        match self {
            Self::Listing { held }
            | Self::Describing { held, .. }
            | Self::Finding { held, .. }
            | Self::Calling { held, .. }
            | Self::Minting { held }
            | Self::Spawning { held }
            | Self::Terminating { held, .. } => held,
        }
    }
}

/// What a call asks for, read from its input.
enum Order {
    List,
    Describe { engine: u32 },
    Call(EngineCall),
    Mint,
    Spawn(SpawnEngine),
    Terminate(TerminateEngine),
}

/// A call of an engine's tool, ready to ask: the set it was converted with, the input's bytes, and what its sheet says
/// it answers with.
struct Converted {
    revision: u64,
    input: Vec<u8>,
    output: Output,
}

/// The hub's tools for the agent (ADR-0038, ADR-0040), served by one worker: `list_engines` asks [`Engines`] for every
/// engine linked, `describe_engine` asks an [`Engine`] for its tools and writes each one's input and output as JSON
/// Schema, and `call_tool` converts its JSON input to bytes by the tool's input schema, asks the engine to call it,
/// and converts the bytes it answers back to JSON by the tool's output schema. Input that is not of the schema is
/// refused before anything is sent, so an engine receives only bytes its schema reads. `mint_token` asks `Engines`
/// for a token, and answers it with the arguments to start an engine with, for an engine the agent starts itself.
/// `spawn_engine` asks `Engines` to spawn an engine from the store, and answers its id once it has linked, or refuses
/// saying how the spawn failed; `terminate_engine` asks `Engines` to terminate an engine, spawned or not.
///
/// An engine's id is a `u32`, as `Engines` counts them: a 64-bit integer's JSON form is a string (ADR-0007), and an
/// agent writes the `3` it was shown. `list_engines` takes and answers the link's own `ListEngines` and `EngineList`,
/// `mint_token` its `MintToken` and `Minted`, whose token JSON writes as 32 hexadecimal digits, `spawn_engine` takes
/// its `SpawnEngine`, whose binary is one string, a hash when it spells one and a name otherwise, and
/// `terminate_engine` takes its `TerminateEngine`. Each case short of done is refused with a reason the agent reads.
///
/// `describe_engine` does not show the set's revision. The agent never passes one: `call_tool` reads the engine's
/// latest set when it is called, and converts by it.
#[derive(Bound)]
#[kind(name = "aether.link.engine_tools")]
pub struct EngineTools {
    /// Each call held, by a number of its own, with where it is.
    calls: BTreeMap<u64, Pending>,
    next: u64,
}

impl ChildOf for EngineTools {
    type Parent = Anywhere;
}

impl Actor for EngineTools {
    type Config = EngineToolsConfig;
    type Key = ();
    type Handles = (
        Call,
        Answer<EngineList, Asking>,
        Answer<Described, Asking>,
        Answer<Called, Asking>,
        Answer<Minted, Asking>,
        Answer<Spawn, Asking>,
        Answer<Terminated, Asking>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EngineToolsConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for EngineTools {
    fn sheets() -> Vec<ToolSheet> {
        vec![
            ToolSheet {
                name: String::from(LIST_ENGINES),
                description: String::from(
                    "Lists the engines linked to this hub: each one's id, name, count of tools, and origin, the \
                     binary and hash it was spawned from or `External` for one started by hand. Also lists the last \
                     engines the hub spawned that have ended, each with its binary, exit status, and the end of its \
                     stderr.",
                ),
                input: SchemaNodes::of(&ListEngines::SHAPE),
                output: Output::Value(SchemaNodes::of(&EngineList::SHAPE)),
            },
            ToolSheet {
                name: String::from(DESCRIBE_ENGINE),
                description: String::from(
                    "Describes an engine's tools: each one's name, what it does, and its input and output as JSON \
                     Schema.",
                ),
                input: SchemaNodes::of(&EngineId::SHAPE),
                output: Output::Value(SchemaNodes::of(&EngineSheets::SHAPE)),
            },
            ToolSheet {
                name: String::from(CALL_TOOL),
                description: String::from(
                    "Calls an engine's tool with input of its input schema, and answers its output, or the image and \
                     text it answers when its output is `\"image\"`. Describe the engine again when a call is refused \
                     because its tools changed.",
                ),
                input: SchemaNodes::of(&EngineCall::SHAPE),
                output: Output::ValueOrImage(SchemaNodes::of(&EngineAnswer::SHAPE)),
            },
            ToolSheet {
                name: String::from(MINT_TOKEN),
                description: String::from(
                    "Mints a token for an engine started outside the hub, and answers the arguments to start it with. \
                     The token links one engine once, and expires if it goes unused.",
                ),
                input: SchemaNodes::of(&MintToken::SHAPE),
                output: Output::Value(SchemaNodes::of(&Minted::SHAPE)),
            },
            ToolSheet {
                name: String::from(SPAWN_ENGINE),
                description: String::from(
                    "Spawns an engine from a binary checked into the store, named by a name or by its 64-digit hash, \
                     started with `args` and then the arguments that link it to this hub. Answers the engine's id \
                     once it has linked; refused if nothing is stored by that binary, the hub would not start it, it \
                     exited before linking (with its exit status and the end of its stderr), or it did not link in \
                     time, when it is stopped.",
                ),
                input: SchemaNodes::of(&SpawnEngine::SHAPE),
                output: Output::Value(SchemaNodes::of(&SpawnedEngine::SHAPE)),
            },
            ToolSheet {
                name: String::from(TERMINATE_ENGINE),
                description: String::from(
                    "Terminates an engine: a spawned engine's process is stopped, and an engine started by hand has \
                     its link shut, so it ends. Refused if no engine of that id is linked.",
                ),
                input: SchemaNodes::of(&TerminateEngine::SHAPE),
                output: Output::Value(SchemaNodes::of(&Done::SHAPE)),
            },
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for EngineTools {
    type Sends = (
        Asks<Path<Engines>, ListEngines, Asking>,
        Asks<Path<Engine>, DescribeEngine, Asking>,
        Asks<Path<Engines>, MintToken, Asking>,
        Asks<Path<Engines>, SpawnEngine, Asking>,
        Asks<Path<Engines>, TerminateEngine, Asking>,
        ToSender<Called>,
    );

    /// Reads the call's input, or refuses it; holds the call, and asks `Engines` or the engine named.
    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let order = match order(&call.tool, turn.read(&call.input)) {
            Ok(order) => order,
            Err(refusal) => {
                let refusal = refusal.into_called(turn);
                turn.reply(&refusal)?;
                return Ok(());
            }
        };
        let held = turn.hold()?;
        let asking = Asking { number: self.next };
        self.next += 1;
        let pending = match order {
            Order::List => {
                turn.to(path::<Engines>(())).ask(&ListEngines, &asking)?;
                Pending::Listing { held }
            }
            Order::Describe { engine } => {
                turn.to(path::<Engine>(u64::from(engine))).ask(&DescribeEngine, &asking)?;
                Pending::Describing { held, engine }
            }
            Order::Call(EngineCall { engine, tool, input }) => {
                turn.to(path::<Engine>(u64::from(engine))).ask(&DescribeEngine, &asking)?;
                Pending::Finding { held, engine, tool, input }
            }
            Order::Mint => {
                turn.to(path::<Engines>(())).ask(&MintToken, &asking)?;
                Pending::Minting { held }
            }
            Order::Spawn(spawn) => {
                turn.to(path::<Engines>(())).ask(&spawn, &asking)?;
                Pending::Spawning { held }
            }
            Order::Terminate(terminate) => {
                turn.to(path::<Engines>(())).ask(&terminate, &asking)?;
                Pending::Terminating { held, engine: terminate.engine }
            }
        };
        self.calls.insert(asking.number, pending);

        Ok(())
    }
}

impl Handles<Answer<EngineList, Asking>> for EngineTools {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Asking { number }): (Answered<EngineList>, Asking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(pending) = self.calls.remove(&number) else {
            return Ok(());
        };
        let (held, called) = match (pending, answered) {
            (Pending::Listing { held }, Answered::Yes(list)) => (held, listed(&list)?),
            (Pending::Listing { held }, Answered::No(_)) => (held, refused("`Engines` did not answer")),
            (
                other @ (Pending::Describing { .. }
                | Pending::Finding { .. }
                | Pending::Calling { .. }
                | Pending::Minting { .. }
                | Pending::Spawning { .. }
                | Pending::Terminating { .. }),
                _,
            ) => (other.into_held(), lost()),
        };
        let called = called.into_called(turn);
        turn.answer(held, &called)?;

        Ok(())
    }
}

impl Handles<Answer<Described, Asking>> for EngineTools {
    type Sends = (Asks<Path<Engine>, CallEngine, Asking>,);

    /// For `describe_engine`, answers the engine's tools; for `call_tool`, converts the input by the tool's sheet and
    /// asks the engine to call it, or refuses.
    fn handle(
        &mut self,
        (answered, asking): (Answered<Described>, Asking),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Some(pending) = self.calls.remove(&asking.number) else {
            return Ok(());
        };
        let (held, called) = match pending {
            Pending::Describing { held, engine } => (held, described(answered, engine)?),
            Pending::Finding { held, engine, tool, input } => match converted(answered, engine, &tool, &input) {
                Ok(Converted { revision, input, output }) => {
                    let Ok(input) = turn.put(input) else {
                        let unheld = Called::Refused(String::from("the hub had no room to hold the input"));
                        turn.answer(held, &unheld)?;
                        return Ok(());
                    };
                    let call = CallEngine { revision, tool: tool.clone(), input };
                    // This turn handles the engine's tools, not the call held, so it states the call's deadline.
                    turn.to(path::<Engine>(u64::from(engine))).within(held.due()).ask(&call, &asking)?;
                    self.calls.insert(asking.number, Pending::Calling { held, tool, output });
                    return Ok(());
                }
                Err(refusal) => (held, refusal),
            },
            other @ (Pending::Listing { .. }
            | Pending::Calling { .. }
            | Pending::Minting { .. }
            | Pending::Spawning { .. }
            | Pending::Terminating { .. }) => (other.into_held(), lost()),
        };
        let called = called.into_called(turn);
        turn.answer(held, &called)?;

        Ok(())
    }
}

impl Handles<Answer<Called, Asking>> for EngineTools {
    type Sends = ();

    /// The engine's answer: its output converted to JSON by the tool's output schema, or its refusal as it gave it.
    fn handle(
        &mut self,
        (answered, Asking { number }): (Answered<Called>, Asking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(pending) = self.calls.remove(&number) else {
            return Ok(());
        };
        let (held, called) = match pending {
            Pending::Calling { held, tool, output } => (held, output_of(turn, answered, &tool, &output)?),
            other @ (Pending::Listing { .. }
            | Pending::Describing { .. }
            | Pending::Finding { .. }
            | Pending::Minting { .. }
            | Pending::Spawning { .. }
            | Pending::Terminating { .. }) => (other.into_held(), lost()),
        };
        let called = called.into_called(turn);
        turn.answer(held, &called)?;

        Ok(())
    }
}

impl Handles<Answer<Minted, Asking>> for EngineTools {
    type Sends = ();

    /// `Engines`' token, answered as it gave it.
    fn handle(
        &mut self,
        (answered, Asking { number }): (Answered<Minted>, Asking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(pending) = self.calls.remove(&number) else {
            return Ok(());
        };
        let (held, called) = match (pending, answered) {
            (Pending::Minting { held }, Answered::Yes(minted)) => (held, Outcome::Output(to_bytes(&minted)?)),
            (Pending::Minting { held }, Answered::No(_)) => (held, refused("`Engines` minted no token")),
            (
                other @ (Pending::Listing { .. }
                | Pending::Describing { .. }
                | Pending::Finding { .. }
                | Pending::Calling { .. }
                | Pending::Spawning { .. }
                | Pending::Terminating { .. }),
                _,
            ) => (other.into_held(), lost()),
        };
        let called = called.into_called(turn);
        turn.answer(held, &called)?;

        Ok(())
    }
}

impl Handles<Answer<Spawn, Asking>> for EngineTools {
    type Sends = ();

    /// The spawned engine's id once it has linked, or the refusal saying how the spawn failed.
    fn handle(
        &mut self,
        (answered, Asking { number }): (Answered<Spawn>, Asking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(pending) = self.calls.remove(&number) else {
            return Ok(());
        };
        let (held, called) = match pending {
            Pending::Spawning { held } => (held, spawned(answered)?),
            other @ (Pending::Listing { .. }
            | Pending::Describing { .. }
            | Pending::Finding { .. }
            | Pending::Calling { .. }
            | Pending::Minting { .. }
            | Pending::Terminating { .. }) => (other.into_held(), lost()),
        };
        let called = called.into_called(turn);
        turn.answer(held, &called)?;

        Ok(())
    }
}

impl Handles<Answer<Terminated, Asking>> for EngineTools {
    type Sends = ();

    /// That the engine is being stopped, or the refusal of an id no engine is linked under.
    fn handle(
        &mut self,
        (answered, Asking { number }): (Answered<Terminated>, Asking),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(pending) = self.calls.remove(&number) else {
            return Ok(());
        };
        let (held, called) = match (pending, answered) {
            (Pending::Terminating { held, .. }, Answered::Yes(Terminated::Stopping)) => {
                (held, Outcome::Output(to_bytes(&Done {})?))
            }
            (Pending::Terminating { held, engine }, Answered::Yes(Terminated::NotLinked)) => {
                (held, refused(format!("no engine {engine} is linked")))
            }
            (Pending::Terminating { held, .. }, Answered::No(_)) => (held, refused("`Engines` did not answer")),
            (
                other @ (Pending::Listing { .. }
                | Pending::Describing { .. }
                | Pending::Finding { .. }
                | Pending::Calling { .. }
                | Pending::Minting { .. }
                | Pending::Spawning { .. }),
                _,
            ) => (other.into_held(), lost()),
        };
        let called = called.into_called(turn);
        turn.answer(held, &called)?;

        Ok(())
    }
}

/// The refusal of a call whose answer came to it at the wrong stage, which one ask out for each call rules out.
fn lost() -> Outcome {
    refused("the hub's tools lost track of this call")
}

/// A refusal saying `why`.
fn refused(why: impl Into<String>) -> Outcome {
    Outcome::Refused(why.into())
}

/// What a call of `tool` with `input`, the bytes of its input, asks for, or its refusal: a tool these are not, or
/// input that is not of the tool's.
fn order(tool: &str, input: &[u8]) -> Result<Order, Outcome> {
    let budget = &mut Budget::new(WALK_BYTES);
    let read = match tool {
        LIST_ENGINES => from_bytes::<ListEngines>(input, budget).map(|_| Order::List),
        DESCRIBE_ENGINE => from_bytes::<EngineId>(input, budget).map(|EngineId { engine }| Order::Describe { engine }),
        CALL_TOOL => from_bytes::<EngineCall>(input, budget).map(Order::Call),
        MINT_TOKEN => from_bytes::<MintToken>(input, budget).map(|_| Order::Mint),
        SPAWN_ENGINE => from_bytes(input, budget).map(Order::Spawn),
        TERMINATE_ENGINE => from_bytes(input, budget).map(Order::Terminate),
        other => return Err(refused(format!("there is no tool `{other}` here"))),
    };

    read.map_err(|fault| refused(format!("the input of `{tool}`: {fault}")))
}

/// What `spawn_engine` answers of what the spawn came to: the engine's id once it has linked, and otherwise a refusal
/// saying how it failed.
fn spawned(answered: Answered<Spawn>) -> Result<Outcome, Fault> {
    let called = match answered {
        Answered::Yes(Spawn::Linked { engine }) => Outcome::Output(to_bytes(&SpawnedEngine { engine })?),
        Answered::Yes(Spawn::NotStored) => refused("no binary of that name or hash is stored"),
        Answered::Yes(Spawn::Refused) => refused("the hub would not start the binary"),
        Answered::Yes(Spawn::Exited { status, stderr }) => {
            refused(format!("the engine exited before it linked, {}; its stderr ended: {stderr}", ended(status)))
        }
        Answered::Yes(Spawn::Unlinked) => refused("the engine did not link in time, and was stopped"),
        Answered::No(_) => refused("`Engines` did not answer"),
    };

    Ok(called)
}

/// How a program ended, as a refusal says it.
fn ended(status: ExitStatus) -> String {
    match status {
        ExitStatus::Code(code) => format!("with code {code}"),
        ExitStatus::Signal(signal) => format!("ended by signal {signal}"),
    }
}

/// What `list_engines` answers of `list`: the list itself.
fn listed(list: &EngineList) -> Result<Outcome, Fault> {
    Ok(Outcome::Output(to_bytes(list)?))
}

/// The refusal of an engine's tools that could not be read: no such engine, or one that has not sent them.
fn unread(answered: Answered<Described>, engine: u32) -> Result<(u64, Vec<ToolSheet>), Outcome> {
    match answered {
        Answered::Yes(Described::Tools { revision, tools }) => Ok((revision, tools)),
        Answered::Yes(Described::NotYet) => Err(refused(format!("engine {engine} has not sent its tools yet"))),
        Answered::No(_) => Err(refused(format!("there is no engine {engine}"))),
    }
}

/// What `describe_engine` answers of the engine's tools.
fn described(answered: Answered<Described>, engine: u32) -> Result<Outcome, Fault> {
    let tools = match unread(answered, engine) {
        Ok((_, tools)) => tools,
        Err(refusal) => return Ok(refusal),
    };
    let described: Result<Vec<JsonSheet>, JsonError> = tools.into_iter().map(json_sheet).collect();
    let Ok(tools) = described else {
        return Ok(refused("a tool's schema has no JSON Schema"));
    };

    Ok(Outcome::Output(to_bytes(&EngineSheets { tools })?))
}

/// `sheet` as `describe_engine` shows it: an output that is an image as `"image"`, and one that may be either as the
/// schema of its value beside `"image"`.
fn json_sheet(sheet: ToolSheet) -> Result<JsonSheet, JsonError> {
    let input = schema_json(&sheet.input)?;
    let output = match &sheet.output {
        Output::Value(schema) => schema_json(schema)?,
        Output::Image => image_json()?,
        Output::ValueOrImage(schema) => {
            let budget = &mut Budget::new(WALK_BYTES);
            Json::parse(format!("{{\"value\":{},\"or\":\"image\"}}", json_schema(schema, budget)?), budget)?
        }
    };
    let ToolSheet { name, description, .. } = sheet;

    Ok(JsonSheet { name, description, input, output })
}

/// What `describe_engine` shows of an output that is an image.
fn image_json() -> Result<Json, JsonError> {
    Json::parse(String::from("\"image\""), &mut Budget::new(WALK_BYTES))
}

/// `schema` written as JSON Schema.
fn schema_json(schema: &SchemaNodes) -> Result<Json, JsonError> {
    let budget = &mut Budget::new(WALK_BYTES);

    Json::parse(json_schema(schema, budget)?, budget)
}

/// The call of `tool` on `engine` with `input`, converted by the latest set, or its refusal: no such engine, one that
/// has not sent its tools, no such tool, or input not of its schema.
fn converted(answered: Answered<Described>, engine: u32, tool: &str, input: &Json) -> Result<Converted, Outcome> {
    let (revision, tools) = unread(answered, engine)?;
    let Some(sheet) = tools.into_iter().find(|sheet| sheet.name == tool) else {
        return Err(refused(format!("engine {engine} has no tool `{tool}`")));
    };
    let read = from_json(&sheet.input, input.as_str(), &mut Budget::new(WALK_BYTES));
    let input = read.map_err(|why| refused(format!("the input is not of `{tool}`'s input schema: {why}")))?;

    Ok(Converted { revision, input, output: sheet.output })
}

/// What `call_tool` answers of the engine's answer to a call of `tool`, whose sheet says it answers with `output`, read
/// in `turn`: a value as JSON of its schema, and an image as it is (ADR-0043).
fn output_of<S>(
    turn: &Turn<'_, EngineTools, S>,
    answered: Answered<Called>,
    tool: &str,
    output: &Output,
) -> Result<Outcome, Fault> {
    let (bytes, schema) = match (answered, output) {
        (Answered::Yes(Called::Output(bytes)), Output::Value(schema) | Output::ValueOrImage(schema)) => (bytes, schema),
        (Answered::Yes(Called::Image { png, text }), Output::Image | Output::ValueOrImage(_)) => {
            return Ok(Outcome::Image { png: turn.read(&png).to_vec(), text });
        }
        (Answered::Yes(Called::Output(_)), Output::Image) => {
            return Ok(refused(format!("`{tool}` answered a value, and its sheet says an image")));
        }
        (Answered::Yes(Called::Image { .. }), Output::Value(_)) => {
            return Ok(refused(format!("`{tool}` answered an image, and its sheet says a value")));
        }
        (Answered::Yes(Called::Refused(why)), _) => return Ok(Outcome::Refused(why)),
        (Answered::No(_), _) => return Ok(refused("the engine went before it answered")),
    };
    let budget = &mut Budget::new(WALK_BYTES);
    let written = to_json(schema, turn.read(&bytes), budget).and_then(|text| Json::parse(text, budget));
    let Ok(output) = written else {
        return Ok(refused(format!("the engine's output is not of `{tool}`'s output schema")));
    };

    Ok(Outcome::Output(to_bytes(&EngineAnswer { output })?))
}
