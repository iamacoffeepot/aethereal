//! The tools root's tests: an engine with no link holds the tools its toolbox offers, lists them, and calls one for
//! any actor that asks; a subscriber hears the set as it stands, and again after each change; and a call of a set
//! changed since is refused.

use super::*;
use crate::{Output, Tool, ToolSheet, Toolbox, ToolboxConfig};
use aethereal_actor::{ActorRef, Anywhere, ProtocolRef};
use aethereal_data::{Budget, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Unheard};
use std::num::NonZeroUsize;

/// What the test's tool reads.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.say")]
struct Say {
    text: String,
}

/// What the test's tool answers.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.said")]
struct Said {
    length: u32,
}

/// What the test's tool is made from: nothing.
#[derive(Default, Plain)]
#[kind(name = "aether.test.length_config")]
struct LengthConfig;

/// A tool, `length`, that answers how long the text it is given is.
#[derive(Plain)]
#[kind(name = "aether.test.length")]
struct Length;

impl ChildOf for Length {
    type Parent = Anywhere;
}

impl Actor for Length {
    type Config = LengthConfig;
    type Key = ();
    type Handles = (Call,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: LengthConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for Length {
    fn sheets() -> Vec<ToolSheet> {
        vec![sheet("length")]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for Length {
    type Sends = (ToSender<Called>,);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let said = from_bytes::<Say>(turn.read(&call.input), &mut Budget::new(1 << 16))
            .map(|say| Said { length: u32::try_from(say.text.len()).unwrap_or(u32::MAX) });
        let called = match said {
            Ok(said) => Called::output(turn, to_bytes(&said)?),
            Err(why) => Called::Refused(format!("the input did not read: {why:?}")),
        };
        turn.reply(&called)?;

        Ok(())
    }
}

/// The sheet of a tool `name` from `Say` to `Said`.
fn sheet(name: &str) -> ToolSheet {
    ToolSheet {
        name: String::from(name),
        description: String::from("Says how long a text is"),
        input: SchemaNodes::of(&Say::SHAPE),
        output: Output::Value(SchemaNodes::of(&Said::SHAPE)),
    }
}

/// A harness over an engine of the tools root alone.
fn held() -> Result<Harness<Bench>, Unheard> {
    let setup = Engine::new(NonZeroUsize::MIN, 1 << 16, 0, 1 << 20, 0).with_cycle::<()>();

    Harness::bare(setup.add_root::<Tools>(&(), ToolsSettings::default())?.ready())
}

/// A harness over an engine of the tools root and a toolbox of `length`, and no link, settled once the toolbox's offer
/// has been answered.
fn offered() -> Result<Harness<Bench>, Unheard> {
    let setup = Engine::new(NonZeroUsize::MIN, 1 << 16, 0, 1 << 20, 0).with_cycle::<()>();
    let engine = setup
        .add_root::<Tools>(&(), ToolsSettings::default())?
        .add_root::<Toolbox<(Length,), Tools>>(&(), ToolboxConfig)?
        .ready();
    let mut harness = Harness::bare(engine)?;
    harness.settle();

    Ok(harness)
}

/// What calling `tool` of the tools root with `input`, read from the set of `revision`, came to: what it answered, its
/// output read out of its blob.
fn call(harness: &mut Harness<Bench>, tool: &str, revision: u64, input: &Say) -> Result<Result<Said, String>, Unheard> {
    let input = harness.put(to_bytes(input)?)?;
    let tools = harness.root::<Tools>();
    let asked = harness.ask(tools, &CallTool { tool: String::from(tool), revision, input })?;
    let called = match harness.answer(asked)? {
        Answered::Yes(Called::Output(output)) => {
            let bytes = harness.read(&output).map(<[u8]>::to_vec).unwrap_or_default();
            Ok(from_bytes::<Said>(&bytes, &mut Budget::new(1 << 16))?)
        }
        Answered::Yes(Called::Refused(why)) => Err(why),
        Answered::Yes(Called::Image { .. }) | Answered::No(_) => Err(String::from("not an answer the tool gives")),
    };

    Ok(called)
}

#[test]
fn an_engine_with_no_link_lists_its_toolbox_s_tools_and_calls_one_for_any_actor() -> Result<(), Unheard> {
    // Catches tools held only where a link runs, a list that leaves out a tool its toolbox offered, a call asked of no
    // worker or answered before the worker has, and a call of a tool not held left unanswered.
    let mut harness = offered()?;
    let tools = harness.root::<Tools>();

    let listed = harness.ask(tools, &ListTools)?;
    assert_eq!(harness.answer(listed)?, Answered::Yes(Sheets { revision: 1, tools: vec![sheet("length")] }));

    assert_eq!(call(&mut harness, "length", 1, &Say { text: String::from("hello") })?, Ok(Said { length: 5 }));
    let unknown = call(&mut harness, "nothing", 1, &Say { text: String::from("hello") })?;
    assert_eq!(unknown, Err(String::from("the engine holds no tool named nothing")));

    Ok(())
}

#[test]
fn a_subscriber_hears_the_set_as_it_stands_and_again_after_each_change() -> Result<(), Unheard> {
    // Catches a subscriber not sent the set until it next changes, an offer or a withdrawal sent to no one, a set sent
    // as a change instead of whole, and a revision not raised, with which a hub would convert a call by a set no longer
    // held.
    let mut harness = held()?;
    let (tools, worker) = (harness.root::<Tools>(), harness.protocol::<Tooling>());

    harness.tell(tools, &Subscribe)?;
    assert_eq!(harness.heard::<Sheets>()?, Sheets { revision: 0, tools: Vec::new() });

    let offering = harness.ask(tools, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));
    assert_eq!(harness.heard::<Sheets>()?, Sheets { revision: 1, tools: vec![sheet("say")] });

    harness.tell(tools, &Withdraw { tool: String::from("say"), worker })?;
    assert_eq!(harness.heard::<Sheets>()?, Sheets { revision: 2, tools: Vec::new() });

    Ok(())
}

#[test]
fn a_call_of_a_set_changed_since_is_refused_and_one_of_the_set_held_is_served() -> Result<(), Unheard> {
    // Catches a revision checked anywhere but in the turn that looks the tool up, where a tool withdrawn and offered
    // again under its name would be called with bytes written for the schema it had before.
    let mut harness = held()?;
    let (tools, worker) = (harness.root::<Tools>(), harness.protocol::<Tooling>());
    let input = harness.put(to_bytes(&Say { text: String::from("hello") })?)?;

    let offering = harness.ask(tools, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));
    harness.tell(tools, &Withdraw { tool: String::from("say"), worker })?;
    let offering = harness.ask(tools, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    let stale = harness.ask(tools, &CallTool { tool: String::from("say"), revision: 1, input: input.clone() })?;
    let changed = String::from("the engine's tools have changed since revision 1: read them again");
    assert_eq!(harness.answer(stale)?, Answered::Yes(Called::Refused(changed)));

    let current = harness.ask(tools, &CallTool { tool: String::from("say"), revision: 3, input })?;
    let call = harness.asked::<Call>()?;
    assert_eq!(call.request.tool, "say");
    harness.reply(call, &Called::Refused(String::from("served")))?;
    assert_eq!(harness.answer(current)?, Answered::Yes(Called::Refused(String::from("served"))));

    Ok(())
}
