//! `list_tuning` and `tune` (ADR-0050): the tuning of the roots a program lists, read and set by an agent, each
//! field checked against the schema of the root's tuning.

mod overlay;
#[cfg(test)]
mod tests;

use crate::{Call, Called, Output, Tool, ToolSheet, Tooling};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, PathTo,
    ProtocolRef, ReadTuning, ToSender, Tunable, Tune, Tuned, Tunes, Tuning, Turn, path,
};
use aethereal_data::config::{Declared, Settings};
use aethereal_data::{
    ActorPath, Bound, Boundary, Budget, Decode, DecodeError, Encode, EncodeError, Field, Json, JsonError, Kind,
    KindName, Plain, Reader, Schema, SchemaNodes, Shape, Writer, from_bytes, from_json, json_schema, to_bytes, to_json,
};
use core::marker::PhantomData;
use overlay::{Unlaid, named, overlay, picked};
use std::collections::BTreeMap;

/// The most bytes reading an input may take: an actor's path, and the fields to set.
const INPUT_BYTES: usize = 64 * 1024;

/// The most bytes turning a tuning into JSON and back may take.
const JSON_BYTES: usize = 1 << 20;

/// What the agent asks `list_tuning` for: nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.list_tuning")]
pub struct ListTuning;

/// The tuning of each root the tool lists that is live, in the order it lists them.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.tuning_list")]
pub struct TuningList {
    pub actors: Vec<ActorTuning>,
}

/// One actor's tuning as it stands: the actor, its values, their JSON Schema, and what each field does.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.actor_tuning")]
pub struct ActorTuning {
    pub actor: ActorPath,
    pub values: Json,
    pub schema: Json,
    pub fields: Vec<About>,
}

/// What a `tune` answers: the actor, and only the fields it set, as the actor's tuning then holds them, an object
/// of them.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.fields_set")]
pub struct FieldsSet {
    pub actor: ActorPath,
    pub values: Json,
}

/// A field of a tuning, and its doc comment.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct About {
    pub name: String,
    pub doc: String,
}

/// What the agent asks `tune` for: the fields of `actor`'s tuning to set, each to the value given, an object of
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.tune_fields")]
pub struct TuneFields {
    /// The path of the actor whose tuning is set.
    #[flag(position)]
    pub actor: ActorPath,
    /// An object of the fields to set, each to the value given.
    #[flag(position)]
    pub values: Json,
}

/// A root the tool lists: its path, its tuning's schema, and its tuning's fields with their doc comments.
pub struct Listed {
    pub actor: ActorPath,
    pub schema: SchemaNodes,
    pub fields: &'static [Declared],
}

/// The roots a [`TuningTool`] lists: a tuple of [`Tunable`] roots.
pub trait TunableList: Send + 'static {
    /// Each root, in the order listed.
    fn listed() -> Vec<Listed>;

    /// The root at `at` in the list, as one tuned, as of the step's start: none when it is past the last, or not
    /// live.
    fn found<M, S>(turn: &Turn<'_, M, S>, at: usize) -> Option<ProtocolRef<Tunes>>;
}

/// The root `A`, as listed.
fn listed<A: Tunable + ChildOf + PathTo<Keys = ()>>() -> Listed {
    Listed {
        actor: path::<A>(()).names().clone(),
        schema: SchemaNodes::of(&A::Tuning::SHAPE),
        fields: <A::Tuning as Settings>::FIELDS,
    }
}

macro_rules! tunables {
    ($( ($($name:ident)*) )*) => {$(
        impl<$($name: Tunable + ChildOf + PathTo<Keys = ()> + Send),*> TunableList for ($($name,)*) {
            fn listed() -> Vec<Listed> {
                vec![$(listed::<$name>()),*]
            }

            #[allow(unused_variables, unused_mut, unused_assignments)] // An empty list finds nothing.
            fn found<M, S>(turn: &Turn<'_, M, S>, at: usize) -> Option<ProtocolRef<Tunes>> {
                let mut place = 0;
                $(
                    if place == at {
                        return turn.found(&path::<$name>(())).map($name::tunes);
                    }
                    place += 1;
                )*

                None
            }
        }
    )*};
}

tunables! {
    ()
    (A)
    (A B)
    (A B C)
    (A B C D)
    (A B C D E)
    (A B C D E F)
    (A B C D E F G)
    (A B C D E F G H)
}

/// What the tool keeps for a read of a root's tuning: the call it is for, and the root's place in the list, with
/// the fields to set when the read is the first half of a `tune`.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.reading")]
pub enum Reading {
    Listing { call: u64, at: u32 },
    Tuning { call: u64, at: u32, values: Json },
}

/// What the tool keeps for a tuning sent whole: the call it is for, the root's place in the list, and the names of
/// the fields the `tune` set, which it answers.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.setting")]
pub struct Setting {
    call: u64,
    at: u32,
    names: Vec<String>,
}

/// What the tool is made from: nothing, as its list is its type.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.tuning.tuning_tool_config")]
pub struct TuningToolConfig;

/// A call not yet answered.
#[derive(Bound)]
enum Pending {
    /// A `list_tuning`, waiting on `left` reads, with the tuning of each root read so far by its place in the list.
    Listing { held: Held<Called>, read: BTreeMap<u32, ActorTuning>, left: u32 },
    /// A `tune`, waiting on its read or its tuning sent.
    Tuning { held: Held<Called> },
}

impl Pending {
    /// The call it answers.
    fn into_held(self) -> Held<Called> {
        match self {
            Self::Listing { held, .. } | Self::Tuning { held } => held,
        }
    }
}

/// The calls the tool holds, by number, and the number of the next.
#[derive(Bound)]
struct Calls {
    pending: BTreeMap<u64, Pending>,
    next: u64,
}

/// The tools `list_tuning` and `tune`, over the tunable roots `L`: each asks the roots through [`Tunes`], and answers
/// their tuning as JSON, with its schema and what each field does.
pub struct TuningTool<L> {
    calls: Calls,
    list: PhantomData<fn() -> L>,
}

impl<L> Schema for TuningTool<L> {
    const SHAPE: Shape = Shape::Struct { name: "TuningTool", fields: &[Field { name: "calls", shape: Calls::SHAPE }] };
}

/// Named as one, whatever its list.
impl<L> Kind for TuningTool<L> {
    const NAME: KindName = KindName::new("aether.tuning.tuning_tool");
}

impl<L, B: Boundary> Encode<B> for TuningTool<L>
where
    Calls: Encode<B>,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.calls.encode(out)
    }
}

impl<L, B: Boundary> Decode<B> for TuningTool<L>
where
    Calls: Decode<B>,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Ok(Self { calls: Calls::decode(input)?, list: PhantomData })
    }
}

/// What every turn of the tool may send: a read of a root's tuning, a tuning sent whole, and an answer to its caller.
type Sends = (Asks<Tunes, ReadTuning, Reading>, Asks<Tunes, Tune, Setting>, ToSender<Called>);

impl<L: TunableList> ChildOf for TuningTool<L> {
    type Parent = Anywhere;
}

impl<L: TunableList> Actor for TuningTool<L> {
    type Config = TuningToolConfig;
    type Key = ();
    type Handles = (Call, Answer<Tuning, Reading>, Answer<Tuned, Setting>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: TuningToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: Calls { pending: BTreeMap::new(), next: 0 }, list: PhantomData })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl<L: TunableList> Tool for TuningTool<L> {
    fn sheets() -> Vec<ToolSheet> {
        let named: Vec<String> = L::listed().iter().map(|listed| listed.actor.to_string()).collect();
        let listed = named.join(", ");

        vec![
            ToolSheet::json::<ListTuning>(
                "list_tuning",
                &format!(
                    "Lists the tuning of each tunable root. Tuning is the values a root takes while the program \
                     runs, unlike the settings it starts with. Each is answered with its actor's path, which names \
                     it to `tune`, its values, their JSON Schema, and each field's doc comment, which says what the \
                     field does and what it keeps to. The roots tuned here: {listed}."
                ),
                Output::Value(SchemaNodes::of(&TuningList::SHAPE)),
            ),
            ToolSheet::of::<TuneFields>(
                "tune",
                "Sets fields of one actor's tuning. `values` is an object of the fields to set, each replaced whole \
                 by the value given and every other kept, checked against the tuning's schema before anything is \
                 sent. The actor may refuse a value past what it can take, saying why. Answers only the fields it \
                 set, as the actor then holds them; `list_tuning` reads the whole tuning. The next frame drawn is \
                 drawn with it.",
                Output::Value(SchemaNodes::of(&FieldsSet::SHAPE)),
            ),
        ]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// Reads each root's tuning for `list_tuning`, or the one root's for `tune`, and holds the call.
impl<L: TunableList> Handles<Call> for TuningTool<L> {
    type Sends = Sends;

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let number = self.calls.next;
        let input = turn.read(&call.input);
        let mut budget = Budget::new(INPUT_BYTES);
        let pending = match call.tool.as_str() {
            "list_tuning" => {
                let Ok(ListTuning) = from_bytes::<ListTuning>(input, &mut budget) else {
                    return refuse(turn, String::from("that is not what `list_tuning` takes"));
                };
                let mut left = 0;
                for (at, place) in (0..L::listed().len()).zip(0..) {
                    let Some(tuned) = L::found(turn, at) else {
                        continue;
                    };
                    turn.to(tuned).ask(&ReadTuning, &Reading::Listing { call: number, at: place })?;
                    left += 1;
                }
                if left == 0 {
                    let called = Called::output(turn, to_bytes(&TuningList { actors: Vec::new() })?);
                    turn.reply(&called)?;
                    return Ok(());
                }
                Pending::Listing { held: turn.hold()?, read: BTreeMap::new(), left }
            }
            "tune" => {
                let Ok(TuneFields { actor, values }) = from_bytes::<TuneFields>(input, &mut budget) else {
                    return refuse(turn, String::from("that is not what `tune` takes"));
                };
                let listed = L::listed();
                let Some(at) = listed.iter().position(|listed| listed.actor == actor) else {
                    let named: Vec<String> = listed.iter().map(|listed| listed.actor.to_string()).collect();
                    return refuse(
                        turn,
                        format!("no root at `{actor}` is tuned here; these are: {}", named.join(", ")),
                    );
                };
                let Some(tuned) = L::found(turn, at) else {
                    return refuse(turn, format!("`{actor}` is not live"));
                };
                let place = u32::try_from(at).unwrap_or(u32::MAX);
                turn.to(tuned).ask(&ReadTuning, &Reading::Tuning { call: number, at: place, values })?;
                Pending::Tuning { held: turn.hold()? }
            }
            _ => return refuse(turn, String::from("this worker serves list_tuning and tune")),
        };
        self.calls.pending.insert(number, pending);
        self.calls.next += 1;

        Ok(())
    }
}

/// A root's tuning, read: kept toward a `list_tuning`, or laid under the fields a `tune` sets and sent back whole.
impl<L: TunableList> Handles<Answer<Tuning, Reading>> for TuningTool<L> {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, reading): (Answered<Tuning>, Reading),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        match reading {
            Reading::Listing { call, at } => self.listing(call, at, answered, turn),
            Reading::Tuning { call, at, values } => self.tuning(call, at, answered, &values, turn),
        }
    }
}

/// A tuning sent whole was taken, or refused: the `tune` is answered with the fields it set as they stand, or why.
impl<L: TunableList> Handles<Answer<Tuned, Setting>> for TuningTool<L> {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Setting { call, at, names }): (Answered<Tuned>, Setting),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(pending) = self.calls.pending.remove(&call) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(Tuned::Tuned(tuning)) => match set::<L>(at, turn.read(&tuning.bytes), &names) {
                Ok(set) => Called::output(turn, to_bytes(&set)?),
                Err(why) => Called::Refused(why),
            },
            Answered::Yes(Tuned::Refused(why)) => Called::Refused(why),
            Answered::Yes(Tuned::Unread) => Called::Refused(String::from("the actor did not read the tuning as its")),
            Answered::No(why) => Called::Refused(format!("the actor did not answer: {why:?}")),
        };
        turn.answer(pending.into_held(), &called)?;

        Ok(())
    }
}

impl<L: TunableList> TuningTool<L> {
    /// Keeps the root at `at`'s tuning toward the `list_tuning` `call`, and answers it once every root has answered.
    fn listing(
        &mut self,
        call: u64,
        at: u32,
        answered: Answered<Tuning>,
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(Pending::Listing { read, left, .. }) = self.calls.pending.get_mut(&call) else {
            return Ok(());
        };
        // A root that did not answer, or answered what is not its tuning, is left out of the list.
        if let Answered::Yes(tuning) = answered
            && let Ok(about) = about::<L>(at, turn.read(&tuning.bytes))
        {
            read.insert(at, about);
        }
        *left -= 1;
        if *left > 0 {
            return Ok(());
        }
        let Some(Pending::Listing { read, held, .. }) = self.calls.pending.remove(&call) else {
            return Ok(());
        };
        let list = TuningList { actors: read.into_values().collect() };
        let called = Called::output(turn, to_bytes(&list)?);
        turn.answer(held, &called)?;

        Ok(())
    }

    /// Lays `values` over the root at `at`'s tuning, as read, and sends it back whole: the `tune` `call` is refused
    /// when what it sets is not of the tuning's schema.
    fn tuning(
        &mut self,
        call: u64,
        at: u32,
        answered: Answered<Tuning>,
        values: &Json,
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(tuning) = answered else {
            return self.refused(call, format!("the actor did not answer: {answered:?}"), turn);
        };
        let listed = L::listed();
        let Some(listed) = listed.get(at as usize) else {
            return Ok(());
        };
        let laid = laid(&listed.schema, turn.read(&tuning.bytes), values);
        let (bytes, names) = match laid {
            Ok(laid) => laid,
            Err(why) => return self.refused(call, why, turn),
        };
        let Some(tuned) = L::found(turn, at as usize) else {
            return self.refused(call, format!("`{}` is not live", listed.actor), turn);
        };
        let Ok(bytes) = turn.put(bytes) else {
            return self.refused(call, String::from("the engine had no room to hold the tuning"), turn);
        };
        turn.to(tuned).ask(&Tune { bytes }, &Setting { call, at, names })?;

        Ok(())
    }

    /// Answers the call `call` with `why`, when it is held.
    fn refused(&mut self, call: u64, why: String, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Some(pending) = self.calls.pending.remove(&call) else {
            return Ok(());
        };
        turn.answer(pending.into_held(), &Called::Refused(why))?;

        Ok(())
    }
}

/// The bytes of the tuning of `schema` whose bytes are `bytes`, with each field `values` names set to the value
/// given there, and the names of those fields: why not, for the agent to read, when `values` is not an object of
/// fields of the tuning's schema.
fn laid(schema: &SchemaNodes, bytes: &[u8], values: &Json) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut budget = Budget::new(JSON_BYTES);
    let standing = to_json(schema, bytes, &mut budget).map_err(|why| format!("the tuning was not read: {why}"))?;
    let laid = overlay(&standing, values.as_str(), &mut budget).map_err(|why| match why {
        Unlaid::Unread(why) => format!("`values` was not read: {why}"),
        Unlaid::NotFields => String::from("`values` is an object of the fields to set"),
    })?;

    let bytes = from_json(schema, &laid, &mut budget)
        .map_err(|why| format!("`values` is not of the tuning's schema: {why}"))?;
    let names =
        named(values.as_str(), &mut budget).map_err(|_| String::from("`values` is an object of the fields to set"))?;

    Ok((bytes, names))
}

/// The fields `names` names of the tuning of the root at `at` of `L`, whose bytes are `bytes`, as the actor then
/// holds them: why not, when it is not of the root's tuning.
fn set<L: TunableList>(at: u32, bytes: &[u8], names: &[String]) -> Result<FieldsSet, String> {
    let listed = L::listed();
    let Some(listed) = listed.get(at as usize) else {
        return Err(String::from("the tool lists no root there"));
    };
    let mut budget = Budget::new(JSON_BYTES);
    let standing =
        to_json(&listed.schema, bytes, &mut budget).map_err(|why| format!("the tuning was not read: {why}"))?;
    let picked = picked(&standing, names, &mut budget).map_err(|_| String::from("the tuning was not an object"))?;
    let values = Json::parse(picked, &mut budget).map_err(|why| format!("the tuning was not read: {why}"))?;

    Ok(FieldsSet { actor: listed.actor.clone(), values })
}

/// The tuning of the root at `at` of `L`, whose bytes are `bytes`, as the agent reads it: why not, when it is not
/// of the root's tuning.
fn about<L: TunableList>(at: u32, bytes: &[u8]) -> Result<ActorTuning, String> {
    let listed = L::listed();
    let Some(listed) = listed.get(at as usize) else {
        return Err(String::from("the tool lists no root there"));
    };
    let mut budget = Budget::new(JSON_BYTES);
    let unread = |why: JsonError| format!("the tuning was not read: {why}");
    let values =
        Json::parse(to_json(&listed.schema, bytes, &mut budget).map_err(unread)?, &mut budget).map_err(unread)?;
    let schema = Json::parse(json_schema(&listed.schema, &mut budget).map_err(unread)?, &mut budget).map_err(unread)?;
    let fields = listed
        .fields
        .iter()
        .map(|declared| About { name: String::from(declared.name), doc: String::from(declared.doc) })
        .collect();

    Ok(ActorTuning { actor: listed.actor.clone(), values, schema, fields })
}

/// Refuses the call being handled, saying `why`.
fn refuse<L: TunableList>(turn: &mut Turn<'_, TuningTool<L>, Sends>, why: String) -> Result<(), Fault> {
    turn.reply(&Called::Refused(why))?;

    Ok(())
}
