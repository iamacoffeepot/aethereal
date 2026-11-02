//! The hub's tools: the agent lists engines, reads an engine's tools, and calls one (ADR-0038).

mod kinds;

pub use kinds::{
    CallToolInput, CallToolOutput, DescribeEngineInput, EngineDescribed, EngineListed, EnginesListed, ListEnginesInput,
    ToolDescribed,
};

use crate::{CallEngine, DescribeEngine, Described, Engine, EngineList, Engines, ListEngines};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{
    Bound, Budget, Json, JsonError, Plain, Schema, SchemaNodes, from_bytes, from_json, json_schema, to_bytes, to_json,
};
use aethereal_tools::{Call, Called, Tool, ToolSheet, Tooling};
use std::collections::BTreeMap;

/// The tool that lists engines.
const LIST_ENGINES: &str = "list_engines";
/// The tool that describes an engine's tools.
const DESCRIBE_ENGINE: &str = "describe_engine";
/// The tool that calls an engine's tool.
const CALL_TOOL: &str = "call_tool";

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
    /// `call_tool`: the engine is asked to call `tool`, whose output is of `output`.
    Calling { held: Held<Called>, tool: String, output: SchemaNodes },
}

impl Pending {
    /// The call held, whatever ask it waits on.
    fn into_held(self) -> Held<Called> {
        match self {
            Self::Listing { held }
            | Self::Describing { held, .. }
            | Self::Finding { held, .. }
            | Self::Calling { held, .. } => held,
        }
    }
}

/// What a call asks for, read from its input.
enum Order {
    List,
    Describe { engine: u32 },
    Call(CallToolInput),
}

/// A call of an engine's tool, ready to ask: the set it was converted with, the input's bytes, and the output schema.
struct Converted {
    revision: u64,
    input: Vec<u8>,
    output: SchemaNodes,
}

/// The hub's three tools for the agent (ADR-0038), served by one worker: `list_engines` asks [`Engines`] for every
/// engine linked, `describe_engine` asks an [`Engine`] for its tools and writes each one's input and output as JSON
/// Schema, and `call_tool` converts its JSON input to bytes by the tool's input schema, asks the engine to call it,
/// and converts the bytes it answers back to JSON by the tool's output schema. Input that is not of the schema is
/// refused before anything is sent, so an engine receives only bytes its schema reads.
///
/// An engine's id is a `u32` in these tools, where the link counts in `u64`, since a 64-bit integer's JSON form is a
/// string (ADR-0007) and an agent writes the `3` it was shown. An engine whose id is past `u32::MAX` cannot be named,
/// and is not listed.
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
    type Handles = (Call, Answer<EngineList, Asking>, Answer<Described, Asking>, Answer<Called, Asking>);
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
                    "Lists the engines linked to this hub: each one's id, name, and count of tools.",
                ),
                input: SchemaNodes::of(&ListEnginesInput::SHAPE),
                output: SchemaNodes::of(&EnginesListed::SHAPE),
            },
            ToolSheet {
                name: String::from(DESCRIBE_ENGINE),
                description: String::from(
                    "Describes an engine's tools: each one's name, what it does, and its input and output as JSON \
                     Schema.",
                ),
                input: SchemaNodes::of(&DescribeEngineInput::SHAPE),
                output: SchemaNodes::of(&EngineDescribed::SHAPE),
            },
            ToolSheet {
                name: String::from(CALL_TOOL),
                description: String::from(
                    "Calls an engine's tool with input of its input schema, and answers its output. Describe the \
                     engine again when a call is refused because its tools changed.",
                ),
                input: SchemaNodes::of(&CallToolInput::SHAPE),
                output: SchemaNodes::of(&CallToolOutput::SHAPE),
            },
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for EngineTools {
    type Sends =
        (Asks<Path<Engines>, ListEngines, Asking>, Asks<Path<Engine>, DescribeEngine, Asking>, ToSender<Called>);

    /// Reads the call's input, or refuses it; holds the call, and asks `Engines` or the engine named.
    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let order = match order(&call) {
            Ok(order) => order,
            Err(refusal) => {
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
            Order::Call(CallToolInput { engine, tool, input }) => {
                turn.to(path::<Engine>(u64::from(engine))).ask(&DescribeEngine, &asking)?;
                Pending::Finding { held, engine, tool, input }
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
            (Pending::Listing { held }, Answered::Yes(list)) => (held, listed(list)?),
            (Pending::Listing { held }, Answered::No(_)) => (held, refused("`Engines` did not answer")),
            (other @ (Pending::Describing { .. } | Pending::Finding { .. } | Pending::Calling { .. }), _) => {
                (other.into_held(), lost())
            }
        };
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
                    let call = CallEngine { revision, tool: tool.clone(), input };
                    turn.to(path::<Engine>(u64::from(engine))).ask(&call, &asking)?;
                    self.calls.insert(asking.number, Pending::Calling { held, tool, output });
                    return Ok(());
                }
                Err(refusal) => (held, refusal),
            },
            other @ (Pending::Listing { .. } | Pending::Calling { .. }) => (other.into_held(), lost()),
        };
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
            Pending::Calling { held, tool, output } => (held, output_of(answered, &tool, &output)?),
            other @ (Pending::Listing { .. } | Pending::Describing { .. } | Pending::Finding { .. }) => {
                (other.into_held(), lost())
            }
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

/// The refusal of a call whose answer came to it at the wrong stage, which one ask out for each call rules out.
fn lost() -> Called {
    refused("the hub's tools lost track of this call")
}

/// A refusal saying `why`.
fn refused(why: impl Into<String>) -> Called {
    Called::Refused(why.into())
}

/// What `call` asks for, or its refusal: a tool these are not, or input that is not of the tool's.
fn order(call: &Call) -> Result<Order, Called> {
    let budget = &mut Budget::new(WALK_BYTES);
    let read = match call.tool.as_str() {
        LIST_ENGINES => from_bytes::<ListEnginesInput>(&call.input, budget).map(|_| Order::List),
        DESCRIBE_ENGINE => {
            from_bytes::<DescribeEngineInput>(&call.input, budget).map(|input| Order::Describe { engine: input.engine })
        }
        CALL_TOOL => from_bytes::<CallToolInput>(&call.input, budget).map(Order::Call),
        other => return Err(refused(format!("there is no tool `{other}` here"))),
    };

    read.map_err(|_| refused(format!("that is no input of `{}`", call.tool)))
}

/// What `list_engines` answers of `list`.
fn listed(list: EngineList) -> Result<Called, Fault> {
    let engines = list
        .engines
        .into_iter()
        .filter_map(|listing| {
            let engine = u32::try_from(listing.engine).ok()?;
            Some(EngineListed { engine, name: listing.name, tools: listing.tools })
        })
        .collect();

    Ok(Called::Output(to_bytes(&EnginesListed { engines })?))
}

/// The refusal of an engine's tools that could not be read: no such engine, or one that has not sent them.
fn unread(answered: Answered<Described>, engine: u32) -> Result<(u64, Vec<ToolSheet>), Called> {
    match answered {
        Answered::Yes(Described::Tools { revision, tools }) => Ok((revision, tools)),
        Answered::Yes(Described::NotYet) => Err(refused(format!("engine {engine} has not sent its tools yet"))),
        Answered::No(_) => Err(refused(format!("there is no engine {engine}"))),
    }
}

/// What `describe_engine` answers of the engine's tools.
fn described(answered: Answered<Described>, engine: u32) -> Result<Called, Fault> {
    let tools = match unread(answered, engine) {
        Ok((_, tools)) => tools,
        Err(refusal) => return Ok(refusal),
    };
    let described: Result<Vec<ToolDescribed>, JsonError> = tools.into_iter().map(tool_described).collect();
    let Ok(tools) = described else {
        return Ok(refused("a tool's schema has no JSON Schema"));
    };

    Ok(Called::Output(to_bytes(&EngineDescribed { tools })?))
}

/// `sheet` as `describe_engine` shows it.
fn tool_described(sheet: ToolSheet) -> Result<ToolDescribed, JsonError> {
    let input = schema_json(&sheet.input)?;
    let output = schema_json(&sheet.output)?;
    let ToolSheet { name, description, .. } = sheet;

    Ok(ToolDescribed { name, description, input, output })
}

/// `schema` written as JSON Schema.
fn schema_json(schema: &SchemaNodes) -> Result<Json, JsonError> {
    let budget = &mut Budget::new(WALK_BYTES);

    Json::parse(json_schema(schema, budget)?, budget)
}

/// The call of `tool` on `engine` with `input`, converted by the latest set, or its refusal: no such engine, one that
/// has not sent its tools, no such tool, or input not of its schema.
fn converted(answered: Answered<Described>, engine: u32, tool: &str, input: &Json) -> Result<Converted, Called> {
    let (revision, tools) = unread(answered, engine)?;
    let Some(sheet) = tools.into_iter().find(|sheet| sheet.name == tool) else {
        return Err(refused(format!("engine {engine} has no tool `{tool}`")));
    };
    let read = from_json(&sheet.input, input.as_str(), &mut Budget::new(WALK_BYTES));
    let input = read.map_err(|why| refused(format!("the input is not of `{tool}`'s input schema: {why}")))?;

    Ok(Converted { revision, input, output: sheet.output })
}

/// What `call_tool` answers of the engine's answer to a call of `tool`, whose output is of `output`.
fn output_of(answered: Answered<Called>, tool: &str, output: &SchemaNodes) -> Result<Called, Fault> {
    let bytes = match answered {
        Answered::Yes(Called::Output(bytes)) => bytes,
        Answered::Yes(Called::Refused(why)) => return Ok(Called::Refused(why)),
        Answered::No(_) => return Ok(refused("the engine went before it answered")),
    };
    let budget = &mut Budget::new(WALK_BYTES);
    let written = to_json(output, &bytes, budget).and_then(|text| Json::parse(text, budget));
    let Ok(output) = written else {
        return Ok(refused(format!("the engine's output is not of `{tool}`'s output schema")));
    };

    Ok(Called::Output(to_bytes(&CallToolOutput { output })?))
}
