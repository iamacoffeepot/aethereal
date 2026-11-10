//! The hub's tools on the native host (ADR-0038, ADR-0040, ADR-0036): `EngineTools` listed in a toolbox, the test
//! asking it each call as a holder would, and an engine played by a real socket speaking the link's own typed frames,
//! on a thread of its own where it waits on the hub, or by a real `Link` started with what `mint_token` answers.

use aethereal_actor::{
    Actor, Answered, Beginning, Carries, ChildOf, Ending, Fault, Handles, ProtocolRef, ToSender, Top, Turn,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::native::{Invocation, Invoked, NativeLayers};
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Budget, Json, Plain, Schema, SchemaNodes, from_bytes, json_schema, to_bytes, to_json};
use aethereal_link::{
    DescribeTools, DescribedTools, Down, EngineAnswer, EngineCall, EngineDescription, EngineList, EngineListing,
    EngineTools, Engines, EnginesSettings, JsonSheet, Link, LinkSettings, ListEngines, MintToken, Minted, Origin,
    Outcome, REVISION, Token, ToolSummary, ToolsShown, Up, Within,
};
use aethereal_substrate::harness::{Asked, Harness, Unheard};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;
use aethereal_tools::{
    Call, Called, Holder, Offer, Offered, Output, Table, ToolSheet, Toolbox, ToolboxConfig, Tooling, Tools, Withdraw,
};
use std::io::{Read as _, Write as _};
use std::iter;
use std::net::TcpStream;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// The most bytes of a frame, each way.
const MOST_BYTES: u32 = 64 * 1024;

/// The most bytes a test's JSON walk takes.
const WALK_BYTES: usize = 1 << 20;

/// What the engine's tool reads.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.say")]
struct Say {
    text: String,
}

/// What the engine's tool answers.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.said")]
struct Said {
    length: u32,
}

/// What the stand-in holder is made from: nothing.
#[derive(Debug, Default, Plain)]
#[kind(name = "aether.test.stand_config")]
struct StandConfig;

/// A holder standing in for `Mcp`: it checks each offer as `Mcp` does, with the shared table, so the test reaches the
/// tools' worker through it.
#[derive(Bound)]
#[kind(name = "aether.test.stand")]
struct Stand {
    table: Table,
}

impl ChildOf for Stand {
    type Parent = Top;
}

impl Carries<Offer> for Stand {
    type Answers = (Offered,);
}

impl Holder for Stand {}

impl Actor for Stand {
    type Config = StandConfig;
    type Key = ();
    type Handles = (Offer, Withdraw);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: StandConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { table: Table::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Offer> for Stand {
    type Sends = (ToSender<Offered>,);

    fn handle(&mut self, offer: Offer, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let offered = self.table.take(offer.tools, offer.worker, 16);
        turn.reply(&offered)?;

        Ok(())
    }
}

impl Handles<Withdraw> for Stand {
    type Sends = ();

    fn handle(&mut self, withdraw: Withdraw, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.table.withdraw(&withdraw.tool, withdraw.worker);

        Ok(())
    }
}

/// The sheet of the engine's tool `say`, from `Say` to `Said`.
fn sheet() -> ToolSheet {
    ToolSheet {
        name: String::from("say"),
        description: String::from("Says how long a text is"),
        input: SchemaNodes::of(&Say::SHAPE),
        output: Output::Value(SchemaNodes::of(&Said::SHAPE)),
    }
}

/// `value` as bytes.
fn bytes_of<T: Plain>(value: &T) -> Vec<u8> {
    let Ok(bytes) = to_bytes(value) else {
        panic!("the value encodes");
    };

    bytes
}

/// `value` as JSON of its schema.
fn json_of<T: Plain + Schema>(value: &T) -> Json {
    let budget = &mut Budget::new(WALK_BYTES);
    let written =
        to_json(&SchemaNodes::of(&T::SHAPE), &bytes_of(value), budget).and_then(|text| Json::parse(text, budget));
    let Ok(json) = written else {
        panic!("the value writes as JSON");
    };

    json
}

/// `T`'s schema as JSON Schema.
fn schema_of<T: Schema>() -> Json {
    let budget = &mut Budget::new(WALK_BYTES);
    let written = json_schema(&SchemaNodes::of(&T::SHAPE), budget).and_then(|text| Json::parse(text, budget));
    let Ok(json) = written else {
        panic!("the schema writes as JSON Schema");
    };

    json
}

/// The output `called` holds, read out of its blob in `harness` as `T`.
fn output<T: Plain>(harness: &mut Harness<Headless>, called: Answered<Called>) -> T {
    let Answered::Yes(Called::Output(bytes)) = called else {
        panic!("the tool answers an output: {called:?}");
    };
    let Some(bytes) = harness.read(&bytes) else {
        panic!("the output's blob is held");
    };
    let Ok(output) = from_bytes(bytes, &mut Budget::new(WALK_BYTES)) else {
        panic!("the output reads as the tool's output kind");
    };

    output
}

/// Whether `called` is a refusal whose reason says `says`.
fn refuses(called: &Answered<Called>, says: &str) -> bool {
    matches!(called, Answered::Yes(Called::Refused(why)) if why.contains(says))
}

/// The engine the hub runs on: it reads a frame whole.
fn engine() -> EngineSettings {
    EngineSettings { read_limit: MOST_BYTES, mail_bytes: 1 << 24, ..EngineSettings::default() }
}

/// Writes `frame` to `stream` as the link frames it: its length, big-endian, then its bytes.
fn send<T: Plain>(stream: &mut TcpStream, frame: &T) {
    let bytes = bytes_of(frame);
    let Ok(length) = u32::try_from(bytes.len()) else {
        panic!("the frame has a length");
    };
    assert!(stream.write_all(&length.to_be_bytes()).is_ok(), "the length writes");
    assert!(stream.write_all(&bytes).is_ok(), "the frame writes");
}

/// The next frame on `stream`, read as `T`.
fn receive<T: Plain>(stream: &mut TcpStream) -> T {
    let mut prefix = [0; 4];
    assert!(stream.read_exact(&mut prefix).is_ok(), "a frame comes within ten seconds");
    let mut bytes = vec![0; usize::try_from(u32::from_be_bytes(prefix)).unwrap_or(usize::MAX)];
    assert!(stream.read_exact(&mut bytes).is_ok(), "the frame comes whole");
    let Ok(frame) = from_bytes(&bytes, &mut Budget::new(1 << 24)) else {
        panic!("the frame reads as what was expected");
    };

    frame
}

/// What `peer` came to, the harness stepping the host until it has.
fn heard<T>(harness: &mut Harness<Headless>, peer: JoinHandle<T>) -> T {
    while !peer.is_finished() {
        harness.step();
    }
    let Ok(heard) = peer.join() else {
        panic!("the socket's side finished");
    };

    heard
}

/// The next call the hub writes on `stream`, answered with `called`, the harness stepping the host until it is, and the
/// stream.
fn answered_call(harness: &mut Harness<Headless>, stream: TcpStream, called: Outcome) -> (Down, TcpStream) {
    #[allow(clippy::disallowed_methods)] // The engine's side is a native thread of its own, outside the engine.
    let peer = thread::spawn(move || {
        let mut stream = stream;
        let down: Down = receive(&mut stream);
        if let Down::Call { call, .. } = down {
            send(&mut stream, &Up::Called { call, called });
        }

        (down, stream)
    });

    heard(harness, peer)
}

/// A hub: `Tcp`, `Engines` on a port of its choosing, and a toolbox of `EngineTools` offering to the stand-in holder,
/// with that port and the tools' worker.
fn hub() -> Result<(Harness<Headless>, u16, ProtocolRef<Tooling>), Unheard> {
    let settings = EnginesSettings { port: 0, most_bytes: MOST_BYTES, ..EnginesSettings::default() };
    let program = Headless::program().with_engine(engine()).root::<Tcp>().root_with::<Engines>(settings);
    let mut harness = Harness::of(program)?;
    harness.add_root::<Stand>(&(), StandConfig)?;
    harness.add_root::<Toolbox<(EngineTools,), Stand>>(&(), ToolboxConfig)?;
    let (engines, stand) = (harness.root::<Engines>(), harness.root::<Stand>());
    waiting::until(BUDGET, || {
        harness.step();
        let listens = harness.state(engines).is_ok_and(|engines| engines.port().is_some());
        let holds = harness.state(stand).is_ok_and(|stand| stand.table.len() == 6);
        listens && holds
    });
    let Some(port) = harness.state(engines)?.port() else {
        panic!("`Engines` listens");
    };
    let Some(worker) = harness.state(stand)?.table.tool("call_tool").map(|held| held.worker) else {
        panic!("the toolbox offers the hub's six tools, and the holder takes them");
    };

    Ok((harness, port, worker))
}

/// Asks the hub's tools `worker` to call `tool` with `input`.
fn ask<T: Plain>(
    harness: &mut Harness<Headless>,
    worker: ProtocolRef<Tooling>,
    tool: &str,
    input: &T,
) -> Result<Asked<Called>, Unheard> {
    let input = harness.put(bytes_of(input))?;

    harness.ask(worker, &Call { tool: String::from(tool), input })
}

/// What the hub's tools `worker` answer a call of `tool` with `input`, needing nothing of an engine's side.
fn called<T: Plain>(
    harness: &mut Harness<Headless>,
    worker: ProtocolRef<Tooling>,
    tool: &str,
    input: &T,
) -> Result<Answered<Called>, Unheard> {
    let asked = ask(harness, worker, tool, input)?;

    harness.answer(asked)
}

/// A connection to the hub at `port`, as an engine dials it, which gives up on a read after ten seconds.
fn dialed(port: u16) -> TcpStream {
    let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) else {
        panic!("the engine's side connects");
    };
    assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");

    stream
}

/// A token `Engines` minted for an engine started outside the hub.
fn minted(harness: &mut Harness<Headless>) -> Token {
    let engines = harness.root::<Engines>();
    let minting = harness.ask(engines, &MintToken);
    let Ok(Answered::Yes(Minted { token, .. })) = minting.and_then(|minting| harness.answer(minting)) else {
        panic!("`Engines` mints a token");
    };

    token
}

/// The `Hello` of an engine named `name`, of this crate's revision, bringing a token `Engines` minted.
fn hello(harness: &mut Harness<Headless>, name: &str) -> Up {
    Up::Hello { revision: REVISION, name: String::from(name), token: minted(harness) }
}

/// What `Engines` lists.
fn listed(harness: &mut Harness<Headless>) -> Result<EngineList, Unheard> {
    let engines = harness.root::<Engines>();
    let listing = harness.ask(engines, &ListEngines)?;
    let Answered::Yes(list) = harness.answer(listing)? else {
        panic!("`Engines` answers its list");
    };

    Ok(list)
}

/// Whether `list` holds `count` engines, the first with `tools` tools.
fn holds(list: &EngineList, count: usize, tools: u32) -> bool {
    let is_count = list.engines.len() == count;
    let first_has = list.engines.first().is_some_and(|listing| listing.tools == tools);

    is_count && first_has
}

/// An engine linked as `scene`, with `tools` its set of revision 3, once `Engines` lists it, and its connection.
fn linked_engine(harness: &mut Harness<Headless>, port: u16, tools: Vec<ToolSheet>) -> TcpStream {
    let count = u32::try_from(tools.len()).unwrap_or(u32::MAX);
    let hello = hello(harness, "scene");
    let mut stream = dialed(port);
    send(&mut stream, &hello);
    send(&mut stream, &Up::Sheets { revision: 3, tools });
    waiting::until(BUDGET, || listed(harness).is_ok_and(|list| holds(&list, 1, count)));

    stream
}

/// The sheet of a second engine tool, `listen`, from `Said` to `Say`, whose description runs to two sentences.
fn listen_sheet() -> ToolSheet {
    ToolSheet {
        name: String::from("listen"),
        description: String::from("Listens for 0.5 seconds. Then answers what it heard."),
        input: SchemaNodes::of(&Said::SHAPE),
        output: Output::Value(SchemaNodes::of(&Say::SHAPE)),
    }
}

/// The input of `describe_engine` that asks engine `engine` for the full sheets of the tools `names`.
fn sheets_of(engine: u32, names: &[&str]) -> DescribeTools {
    let names = names.iter().map(|name| String::from(*name)).collect();

    DescribeTools { engine, show: ToolsShown::Sheets(names) }
}

/// The input of `call_tool` that calls `say` on engine 0 with `input`.
fn call_say(input: Json) -> EngineCall {
    EngineCall { engine: 0, tool: String::from("say"), input }
}

#[test]
fn list_engines_shows_each_engine_by_its_id_name_and_count_of_tools() -> Result<(), Unheard> {
    // Catches `Engines` not asked, and an engine's id, name, or count of tools lost or swapped converting the link's
    // listing to the agent's.
    let (mut harness, port, worker) = hub()?;
    let _stream = linked_engine(&mut harness, port, vec![sheet()]);

    let listing = called(&mut harness, worker, "list_engines", &ListEngines)?;
    let listed: EngineList = output(&mut harness, listing);

    assert_eq!(
        listed.engines,
        vec![EngineListing { engine: 0, name: String::from("scene"), tools: 1, origin: Origin::External }]
    );

    Ok(())
}

#[test]
fn describe_engine_writes_each_sheet_asked_for_with_its_input_and_output_as_json_schema() -> Result<(), Unheard> {
    // Catches a schema written other than as JSON Schema, the input and output swapped, a sheet's name or description
    // lost or cut, a sheet asked for left out, and sheets answered in the order asked rather than the engine's.
    let (mut harness, port, worker) = hub()?;
    let _stream = linked_engine(&mut harness, port, vec![sheet(), listen_sheet()]);

    let describing = called(&mut harness, worker, "describe_engine", &sheets_of(0, &["listen", "say"]))?;
    let described: EngineDescription = output(&mut harness, describing);

    let say = JsonSheet {
        name: String::from("say"),
        description: String::from("Says how long a text is"),
        input: schema_of::<Say>(),
        output: schema_of::<Said>(),
    };
    let listen = JsonSheet {
        name: String::from("listen"),
        description: String::from("Listens for 0.5 seconds. Then answers what it heard."),
        input: schema_of::<Said>(),
        output: schema_of::<Say>(),
    };
    assert_eq!(described.tools, DescribedTools::Sheets(vec![say, listen]));

    Ok(())
}

#[test]
fn describe_engine_lists_every_tool_by_its_name_and_first_sentence_alone() -> Result<(), Unheard> {
    // Catches a tool left out of the names, a description not cut to its first sentence, and the sheets answered when
    // only names were asked for, which is what the names are there to spare.
    let (mut harness, port, worker) = hub()?;
    let _stream = linked_engine(&mut harness, port, vec![sheet(), listen_sheet()]);

    let names = DescribeTools { engine: 0, show: ToolsShown::Names };
    let describing = called(&mut harness, worker, "describe_engine", &names)?;
    let described: EngineDescription = output(&mut harness, describing);

    let say = ToolSummary { name: String::from("say"), description: String::from("Says how long a text is") };
    let listen = ToolSummary { name: String::from("listen"), description: String::from("Listens for 0.5 seconds.") };
    assert_eq!(described.tools, DescribedTools::Names(vec![say, listen]));

    Ok(())
}

#[test]
fn call_tool_sends_the_bytes_its_json_input_is_and_answers_the_engines_bytes_as_json() -> Result<(), Unheard> {
    // Catches input converted by the wrong schema or not at all, a call written with another revision than the set
    // it was converted with, and output answered as bytes or converted by the input's schema.
    let (mut harness, port, worker) = hub()?;
    let stream = linked_engine(&mut harness, port, vec![sheet()]);
    let say = Say { text: String::from("hello") };
    let said = Said { length: 5 };

    let calling = ask(&mut harness, worker, "call_tool", &call_say(json_of(&say)))?;
    let (down, _stream) = answered_call(&mut harness, stream, Outcome::Output(bytes_of(&said)));
    let answered = harness.answer(calling)?;
    let answer: EngineAnswer = output(&mut harness, answered);

    let input = bytes_of(&say);
    assert_eq!(down, Down::Call { call: 0, revision: 3, tool: String::from("say"), input, within: Within::Unbounded });
    assert_eq!(answer, EngineAnswer { output: json_of(&said) });

    Ok(())
}

#[test]
fn input_not_of_the_tools_schema_is_refused_and_never_written() -> Result<(), Unheard> {
    // Catches input sent unchecked, which the engine would read as bytes its schema does not: the refused call,
    // written, would be the engine's call 0, ahead of the good one.
    let (mut harness, port, worker) = hub()?;
    let stream = linked_engine(&mut harness, port, vec![sheet()]);
    let Ok(wrong) = Json::parse("{\"text\":5}", &mut Budget::new(WALK_BYTES)) else {
        panic!("the input is JSON");
    };

    let refused = called(&mut harness, worker, "call_tool", &call_say(wrong))?;
    assert!(refuses(&refused, "input schema"), "{refused:?}");

    let say = Say { text: String::from("hello") };
    let calling = ask(&mut harness, worker, "call_tool", &call_say(json_of(&say)))?;
    let (down, _stream) = answered_call(&mut harness, stream, Outcome::Output(bytes_of(&Said { length: 5 })));
    let answered = harness.answer(calling)?;
    let _: EngineAnswer = output(&mut harness, answered);

    let input = bytes_of(&say);
    assert_eq!(down, Down::Call { call: 0, revision: 3, tool: String::from("say"), input, within: Within::Unbounded });

    Ok(())
}

#[test]
fn an_unknown_engine_one_with_no_tools_yet_and_an_unknown_tool_are_each_refused_as_such() -> Result<(), Unheard> {
    // Catches an ask answered `No` taken for something else, an engine with no set yet taken for one with no tools,
    // and a tool not found left unanswered, sent anyway, or its sheet silently left out of a description: each
    // refusal says which it is.
    let (mut harness, port, worker) = hub()?;
    let _stream = linked_engine(&mut harness, port, vec![sheet()]);
    let silent_hello = hello(&mut harness, "silent");
    let mut silent = dialed(port);
    send(&mut silent, &silent_hello);
    waiting::until(BUDGET, || listed(&mut harness).is_ok_and(|list| holds(&list, 2, 1)));
    let say = json_of(&Say { text: String::from("hello") });

    let gone = called(&mut harness, worker, "describe_engine", &DescribeTools { engine: 7, show: ToolsShown::Names })?;
    assert!(refuses(&gone, "no engine 7"), "{gone:?}");
    let gone = called(&mut harness, worker, "call_tool", &EngineCall { engine: 7, ..call_say(say.clone()) })?;
    assert!(refuses(&gone, "no engine 7"), "{gone:?}");

    let not_yet =
        called(&mut harness, worker, "describe_engine", &DescribeTools { engine: 1, show: ToolsShown::Names })?;
    assert!(refuses(&not_yet, "not sent its tools"), "{not_yet:?}");

    let unknown = called(&mut harness, worker, "describe_engine", &sheets_of(0, &["say", "nothing"]))?;
    assert!(refuses(&unknown, "no tool `nothing`"), "{unknown:?}");

    let nothing = EngineCall { tool: String::from("nothing"), ..call_say(say) };
    let unknown = called(&mut harness, worker, "call_tool", &nothing)?;
    assert!(refuses(&unknown, "no tool `nothing`"), "{unknown:?}");

    Ok(())
}

#[test]
fn an_engines_refusal_passes_through_as_it_gave_it() -> Result<(), Unheard> {
    // Catches the engine's reason replaced or dropped, which is all the agent has to act on.
    let (mut harness, port, worker) = hub()?;
    let stream = linked_engine(&mut harness, port, vec![sheet()]);

    let calling = ask(&mut harness, worker, "call_tool", &call_say(json_of(&Say { text: String::from("hello") })))?;
    let _ = answered_call(&mut harness, stream, Outcome::Refused(String::from("the tools have changed")));

    assert_eq!(harness.answer(calling)?, Answered::Yes(Called::Refused(String::from("the tools have changed"))));

    Ok(())
}

#[test]
fn an_output_not_of_the_tools_schema_is_refused() -> Result<(), Unheard> {
    // Catches bytes the engine sent answered unchecked, which the agent would read as a value they are not.
    let (mut harness, port, worker) = hub()?;
    let stream = linked_engine(&mut harness, port, vec![sheet()]);

    let calling = ask(&mut harness, worker, "call_tool", &call_say(json_of(&Say { text: String::from("hello") })))?;
    let _ = answered_call(&mut harness, stream, Outcome::Output(vec![0xFF]));
    let answer = harness.answer(calling)?;

    assert!(refuses(&answer, "output schema"), "{answer:?}");

    Ok(())
}

#[test]
fn call_tool_answers_an_engines_image_as_it_is_and_describe_engine_shows_it_as_an_image() -> Result<(), Unheard> {
    // Catches an engine's image written as a value of `EngineAnswer`, dropped, or its text lost on the way, and an
    // image tool described by a schema it does not have.
    let (mut harness, port, worker) = hub()?;
    let looks = ToolSheet { output: Output::Image, ..sheet() };
    let stream = linked_engine(&mut harness, port, vec![looks]);
    let say = json_of(&Say { text: String::from("hello") });

    let describing = called(&mut harness, worker, "describe_engine", &sheets_of(0, &["say"]))?;
    let described: EngineDescription = output(&mut harness, describing);
    let Ok(image) = Json::parse("\"image\"", &mut Budget::new(WALK_BYTES)) else {
        panic!("a string is JSON");
    };
    let DescribedTools::Sheets(sheets) = described.tools else {
        panic!("the sheets asked for are answered");
    };
    let outputs: Vec<Json> = sheets.into_iter().map(|sheet| sheet.output).collect();
    assert_eq!(outputs, vec![image]);

    let calling = ask(&mut harness, worker, "call_tool", &call_say(say))?;
    let answer = Outcome::Image { png: b"a png".to_vec(), text: String::from("1 by 1") };
    let (_, _stream) = answered_call(&mut harness, stream, answer);
    let Answered::Yes(Called::Image { png, text }) = harness.answer(calling)? else {
        panic!("the image is answered as an image");
    };
    assert_eq!(harness.read(&png), Some(&b"a png"[..]));
    assert_eq!(text, "1 by 1");

    Ok(())
}

#[test]
fn an_image_from_a_tool_whose_sheet_says_a_value_is_refused() -> Result<(), Unheard> {
    // Catches an image passed on as if the tool's sheet allowed one, which the agent's client would be handed for a
    // tool it was told answers JSON.
    let (mut harness, port, worker) = hub()?;
    let stream = linked_engine(&mut harness, port, vec![sheet()]);

    let calling = ask(&mut harness, worker, "call_tool", &call_say(json_of(&Say { text: String::from("hello") })))?;
    let answer = Outcome::Image { png: b"a png".to_vec(), text: String::new() };
    let (_, _stream) = answered_call(&mut harness, stream, answer);
    let answered = harness.answer(calling)?;
    assert!(refuses(&answered, "its sheet says a value"), "{answered:?}");

    Ok(())
}

/// An engine of a `Link` alone, over `Tools`, started with `args` as a person would type them after its name, the engine reading
/// frames as large as the hub's.
fn started(args: &[String]) -> Result<Harness<Headless>, Unheard> {
    let link = LinkSettings { most_bytes: MOST_BYTES, ..LinkSettings::default() };
    let program = Headless::program().with_engine(engine()).root::<Tcp>().root::<Tools>().root_with::<Link>(link);
    let invocation =
        Invocation::new(iter::once("engine").chain(args.iter().map(String::as_str)), Vec::<(String, String)>::new());
    let Ok(Invoked::Run(program)) = program.read("engine", invocation) else {
        panic!("the engine reads the arguments it was started with");
    };

    Harness::of(program)
}

#[test]
fn mint_token_answers_the_arguments_that_start_an_engine_that_links() -> Result<(), Unheard> {
    // Catches arguments that name the wrong flag, the wrong port or host, or a token written in a form `--hub-token`
    // does not read, and a token the tool answers that `Engines` does not hold: the engine would never be listed.
    let (mut hub, _, worker) = hub()?;

    let minting = called(&mut hub, worker, "mint_token", &MintToken)?;
    let minted: Minted = output(&mut hub, minting);
    let mut engine = started(&minted.args)?;

    waiting::until(BUDGET, || {
        engine.step();
        listed(&mut hub).is_ok_and(|list| list.engines.len() == 1)
    });
    assert_eq!(
        listed(&mut hub)?.engines,
        vec![EngineListing { engine: 0, name: String::from("engine"), tools: 0, origin: Origin::External }]
    );

    Ok(())
}
