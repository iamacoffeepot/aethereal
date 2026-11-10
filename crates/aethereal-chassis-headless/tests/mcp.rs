//! The MCP capability served on the native host over real HTTP, the test its tools' worker (ADR-0037, ADR-0036).

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Path,
    ProtocolRef, ToSender, Top, Turn, Unanswered, path,
};
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_chassis_headless::{FileRoots, Headless};
use aethereal_data::{Budget, Plain, Schema, SchemaNodes, from_bytes, from_json, to_bytes, to_json};
use aethereal_files::{Files, ReadTool, WriteTool};
use aethereal_http::{Http, HttpSettings};
use aethereal_link::{
    Down, EngineList, EngineTools, Engines, EnginesSettings, ListEngines, MintToken, Minted, Outcome, REVISION, Up,
    Within,
};
use aethereal_mcp::{Mcp, McpSettings};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{EngineSettings, Host, Lifecycle, Program};
use aethereal_tcp::Tcp;
use aethereal_tools::{
    Call, Called, Offer, Offered, Output, Tool, ToolSheet, Toolbox, ToolboxConfig, Tooling, Withdraw,
};
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::{env, fs, process};

/// What the test's tools read.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.say")]
struct Say {
    text: String,
}

/// What the test's tools answer.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.said")]
struct Said {
    length: u32,
}

/// A value with no object form, as the input of a tool it cannot be.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.positional")]
struct Positional(u32, u32);

/// What a worker of one call is made from: nothing.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.once_config")]
struct OnceConfig;

/// A worker that answers one call and ends.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.once")]
struct Once;

impl ChildOf for Once {
    type Parent = Top;
}

impl Actor for Once {
    type Config = OnceConfig;
    type Key = ();
    type Handles = (Call,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: OnceConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Call> for Once {
    type Sends = (ToSender<Called>,);

    fn handle(&mut self, _: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Called::Refused(String::from("once is enough")))?;
        turn.close();

        Ok(())
    }
}

/// The sheet of a tool `name` from `Say` to `Said`.
fn sheet(name: &str) -> ToolSheet {
    ToolSheet {
        name: String::from(name),
        description: String::from("Says how long a text is"),
        input: SchemaNodes::of(&Say::SHAPE),
        flags: Vec::new(),
        output: Output::Value(SchemaNodes::of(&Said::SHAPE)),
    }
}

/// A host serving MCP on a port of its choosing, and that port.
fn served(settings: McpSettings) -> Result<(Harness<Headless>, ActorRef<Mcp>, u16), Unheard> {
    served_over(HttpSettings::default(), settings)
}

/// The engine `http` needs and no more: what `Http` itself says of it.
fn engine_for(http: &HttpSettings) -> EngineSettings {
    let read_limit = http.most_read_bytes().try_into().unwrap_or(u32::MAX);

    EngineSettings { read_limit, mail_bytes: 1 << 24, ..EngineSettings::default() }
}

/// A host serving MCP over `http` on a port of its choosing, and that port.
fn served_over(http: HttpSettings, settings: McpSettings) -> Result<(Harness<Headless>, ActorRef<Mcp>, u16), Unheard> {
    let settings = McpSettings { port: 0, ..settings };
    let mut harness = Harness::of(
        Headless::program()
            .with_engine(engine_for(&http))
            .root::<Tcp>()
            .root_with::<Http>(http)
            .root_with::<Mcp>(settings),
    )?;
    let mcp = harness.root::<Mcp>();
    for _ in 0..1000 {
        if let Some(port) = harness.state(mcp)?.port() {
            return Ok((harness, mcp, port));
        }
        harness.step();
    }
    panic!("`Mcp` is served");
}

/// A host holding the tool `say`, the test its worker.
fn holding_say() -> Result<(Harness<Headless>, u16), Unheard> {
    let (mut harness, mcp, port) = served(McpSettings::default())?;
    let worker = harness.protocol::<Tooling>();
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    Ok((harness, port))
}

/// A method of the revision, as a test sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rpc {
    Discover,
    List,
    ListFrom,
    CallSay,
    CallShout,
    CallSayBadly,
    CallNothing,
    Initialize,
    Ping,
}

impl Rpc {
    /// The method's name.
    const fn method(self) -> &'static str {
        match self {
            Self::Discover => "server/discover",
            Self::List | Self::ListFrom => "tools/list",
            Self::CallSay | Self::CallShout | Self::CallSayBadly | Self::CallNothing => "tools/call",
            Self::Initialize => "initialize",
            Self::Ping => "ping",
        }
    }

    /// The params beside `_meta`.
    const fn fields(self) -> &'static str {
        match self {
            Self::CallSay => "\"name\":\"say\",\"arguments\":{\"text\":\"hello\"},",
            Self::CallShout => "\"name\":\"shout\",\"arguments\":{\"text\":\"hello\"},",
            Self::CallSayBadly => "\"name\":\"say\",\"arguments\":{\"text\":5},",
            Self::CallNothing => "\"name\":\"nothing\",\"arguments\":{},",
            Self::ListFrom => "\"cursor\":\"more\",",
            Self::Initialize => "\"protocolVersion\":\"2025-06-18\",",
            Self::Discover | Self::List | Self::Ping => "",
        }
    }

    /// The tool it calls, by `Mcp-Name`.
    const fn tool(self) -> Option<&'static str> {
        match self {
            Self::CallSay | Self::CallSayBadly => Some("say"),
            Self::CallShout => Some("shout"),
            Self::CallNothing => Some("nothing"),
            Self::Discover | Self::List | Self::ListFrom | Self::Initialize | Self::Ping => None,
        }
    }
}

/// `_meta`, whole or wanting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Meta {
    Whole,
    WithoutCapabilities,
    OfVersion(&'static str),
}

impl Meta {
    /// Its JSON.
    fn text(self) -> String {
        let version = |version: &str| format!("\"io.modelcontextprotocol/protocolVersion\":\"{version}\"");
        let capabilities = "\"io.modelcontextprotocol/clientCapabilities\":{}";
        match self {
            Self::Whole => format!("{{{},{capabilities}}}", version("2026-07-28")),
            Self::WithoutCapabilities => format!("{{{}}}", version("2026-07-28")),
            Self::OfVersion(stated) => format!("{{{},{capabilities}}}", version(stated)),
        }
    }
}

/// One POST of a test, built as a conforming client writes it, its departures named.
#[derive(Debug, Clone)]
struct Post {
    rpc: Rpc,
    verb: &'static str,
    path: &'static str,
    meta: Meta,
    /// The `MCP-Protocol-Version` sent: none leaves it out.
    version: Option<&'static str>,
    /// The `Mcp-Method` sent: none leaves it out.
    method: Option<&'static str>,
    /// The `Mcp-Name` sent: none leaves it out.
    name: Option<&'static str>,
    /// The `Origin` sent: none leaves it out.
    origin: Option<&'static str>,
    /// A body in place of the request's own.
    body: Option<&'static str>,
    /// How many empty headers to send, of the fewest bytes one can be.
    fillers: usize,
    /// The most bytes of each write.
    write_bytes: usize,
}

impl Post {
    /// `rpc` as a conforming client sends it.
    const fn of(rpc: Rpc) -> Self {
        Self {
            rpc,
            verb: "POST",
            path: "/mcp",
            meta: Meta::Whole,
            version: Some("2026-07-28"),
            method: Some(rpc.method()),
            name: rpc.tool(),
            origin: None,
            body: None,
            fillers: 0,
            write_bytes: usize::MAX,
        }
    }

    /// Its bytes on the wire.
    fn bytes(&self) -> Vec<u8> {
        let (rpc, meta) = (self.rpc, self.meta.text());
        let body = self.body.map_or_else(
            || {
                let (method, fields) = (rpc.method(), rpc.fields());

                format!(
                    "{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"{method}\",\"params\":{{{fields}\"_meta\":{meta}}}}}"
                )
            },
            String::from,
        );
        let mut text =
            format!("{} {} HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\n", self.verb, self.path, body.len());
        let headers = [
            ("MCP-Protocol-Version", self.version),
            ("Mcp-Method", self.method),
            ("Mcp-Name", self.name),
            ("Origin", self.origin),
        ];
        let lines = headers.iter().filter_map(|(name, value)| value.map(|value| format!("{name}: {value}\r\n")));
        text.extend(lines);
        text.push_str(&"a:\r\n".repeat(self.fillers));
        text.push_str("\r\n");
        text.push_str(&body);

        text.into_bytes()
    }
}

/// A response as the client read it.
#[derive(Debug)]
struct Read {
    status: u16,
    body: String,
}

/// A client on a thread of its own, outside the engine, which sends `request` to `port` and reads to the close.
fn client(port: u16, request: &Post) -> JoinHandle<Read> {
    let sent = request.bytes();
    let write_bytes = request.write_bytes;
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
            panic!("the client connects");
        };
        assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");
        assert!(stream.set_nodelay(true).is_ok(), "each write goes as it is made");
        for piece in sent.chunks(write_bytes) {
            assert!(stream.write_all(piece).is_ok(), "the client writes");
            thread::sleep(Duration::from_millis(u64::from(write_bytes < sent.len()) * 5));
        }
        let mut whole = Vec::new();
        assert!(stream.read_to_end(&mut whole).is_ok(), "a response comes within ten seconds");
        let text = String::from_utf8_lossy(&whole).into_owned();
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            panic!("a whole response: {text}");
        };
        let Some(status) = head.split(' ').nth(1).and_then(|status| status.parse().ok()) else {
            panic!("a status: {head}");
        };

        Read { status, body: String::from(body) }
    })
}

/// What `client` read, the harness stepping the host until it has.
fn heard(harness: &mut Harness<Headless>, client: JoinHandle<Read>) -> Read {
    while !client.is_finished() {
        harness.step();
    }
    let Ok(read) = client.join() else {
        panic!("the client finished");
    };

    read
}

/// What `request` is answered by a host with nothing asked of its worker.
fn answered(harness: &mut Harness<Headless>, port: u16, request: &Post) -> Read {
    let client = client(port, request);

    heard(harness, client)
}

#[test]
fn a_call_reaches_the_worker_as_bytes_and_its_output_comes_back_as_json() -> Result<(), Unheard> {
    // Catches arguments not read by the input schema, the wrong tool named, the output not written by the output
    // schema, and a request not held open for the worker.
    let (mut harness, port) = holding_say()?;
    let client = client(port, &Post::of(Rpc::CallSay));

    let call = harness.asked::<Call>()?;
    assert_eq!(call.request.tool, "say");
    let schema = SchemaNodes::of(&Say::SHAPE);
    let Some(input) = harness.read(&call.request.input) else {
        panic!("the input's blob is held");
    };
    let Ok(input) = to_json(&schema, input, &mut Budget::new(1024)) else {
        panic!("the input is a value of its schema");
    };
    assert_eq!(input, "{\"text\":\"hello\"}");
    let Ok(output) = from_json(&SchemaNodes::of(&Said::SHAPE), "{\"length\":5}", &mut Budget::new(1024)) else {
        panic!("the output is a value of its schema");
    };
    let output = harness.put(output)?;
    harness.reply(call, &Called::Output(output))?;

    let read = heard(&mut harness, client);
    assert_eq!(read.status, 200, "{read:?}");
    assert!(read.body.contains("\"id\":7"), "{read:?}");
    assert!(read.body.contains("\"resultType\":\"complete\""), "{read:?}");
    assert!(read.body.contains("\"structuredContent\":{\"length\":5}"), "{read:?}");
    assert!(read.body.contains("\"text\":\"{\\\"length\\\":5}\""), "{read:?}");
    assert!(read.body.contains("\"isError\":false"), "{read:?}");

    Ok(())
}

#[test]
fn what_a_tool_could_not_do_is_a_result_the_agent_reads() -> Result<(), Unheard> {
    // Catches a refusal or a worker that failed made a protocol error, or left hanging, and output that is no value of
    // its schema written as if it were.
    let (mut harness, port) = holding_say()?;

    let client_of = client(port, &Post::of(Rpc::CallSay));
    let call = harness.asked::<Call>()?;
    harness.reply(call, &Called::Refused(String::from("no ink")))?;
    let read = heard(&mut harness, client_of);
    assert!(read.body.contains("\"text\":\"no ink\""), "{read:?}");
    assert!(read.body.contains("\"isError\":true"), "{read:?}");

    let client_of = client(port, &Post::of(Rpc::CallSay));
    let call = harness.asked::<Call>()?;
    harness.unanswered(call, Unanswered::Failed)?;
    let read = heard(&mut harness, client_of);
    assert!(read.body.contains("\"isError\":true"), "{read:?}");

    let client_of = client(port, &Post::of(Rpc::CallSay));
    let call = harness.asked::<Call>()?;
    let output = harness.put(vec![1])?;
    harness.reply(call, &Called::Output(output))?;
    let read = heard(&mut harness, client_of);
    assert!(read.body.contains("no value of its schema"), "{read:?}");
    assert!(read.body.contains("\"isError\":true"), "{read:?}");

    Ok(())
}

#[test]
fn arguments_that_are_no_input_are_a_result_and_never_reach_the_worker() -> Result<(), Unheard> {
    // Catches bad arguments forwarded to the worker, or made a protocol error, which the revision says they are not,
    // and the path of the fault lost.
    let (mut harness, port) = holding_say()?;

    let read = answered(&mut harness, port, &Post::of(Rpc::CallSayBadly));

    assert_eq!(read.status, 200, "{read:?}");
    assert!(read.body.contains("\"isError\":true"), "{read:?}");
    assert!(read.body.contains("text"), "{read:?}");
    assert_eq!(harness.rest(), Vec::new());

    Ok(())
}

#[test]
fn each_request_is_answered_as_the_table_says() -> Result<(), Unheard> {
    // Catches a row of ADR-0037's table dropped or moved, each case one a conforming client never sends or a server
    // that took it would misread.
    let (mut harness, port) = holding_say()?;
    let conforming = Post::of(Rpc::List);
    let cases = [
        (Post { origin: Some("http://evil.test"), ..conforming.clone() }, 403, ""),
        (Post { origin: Some("http://evil.test"), path: "/other", ..conforming.clone() }, 403, ""),
        (Post { path: "/other", ..conforming.clone() }, 404, ""),
        (Post { verb: "GET", ..conforming.clone() }, 405, ""),
        (Post { verb: "DELETE", ..conforming.clone() }, 405, ""),
        (Post { body: Some("[]"), ..conforming.clone() }, 400, ""),
        (Post { body: Some("not json"), ..conforming.clone() }, 400, ""),
        (Post { body: Some("{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}"), ..conforming.clone() }, 400, ""),
        (
            Post { body: Some("{\"jsonrpc\":\"2.0\",\"id\":null,\"method\":\"tools/list\"}"), ..conforming.clone() },
            400,
            "",
        ),
        (
            Post {
                body: Some("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/anything\"}"),
                version: None,
                method: None,
                ..conforming.clone()
            },
            202,
            "",
        ),
        (Post::of(Rpc::Initialize), 400, "-32022"),
        (Post { version: None, method: None, ..Post::of(Rpc::Initialize) }, 400, "\"supported\":[\"2026-07-28\"]"),
        (Post { version: Some("2025-06-18"), ..conforming.clone() }, 400, "-32022"),
        (Post { version: None, ..conforming.clone() }, 400, "-32020"),
        (Post { method: None, ..conforming.clone() }, 400, "-32020"),
        (Post { method: Some("tools/call"), ..conforming.clone() }, 400, "-32020"),
        (Post { name: None, ..Post::of(Rpc::CallSay) }, 400, "-32020"),
        (Post { name: Some("other"), ..Post::of(Rpc::CallSay) }, 400, "-32020"),
        (Post { meta: Meta::OfVersion("2025-11-25"), ..conforming.clone() }, 400, "-32020"),
        (Post { meta: Meta::WithoutCapabilities, ..conforming.clone() }, 400, "-32602"),
        (Post::of(Rpc::Ping), 404, "-32601"),
        (Post::of(Rpc::ListFrom), 200, "-32602"),
        (Post::of(Rpc::CallNothing), 200, "-32602"),
        (Post::of(Rpc::Discover), 200, "\"supportedVersions\":[\"2026-07-28\"]"),
        (Post { origin: Some("http://localhost:3000"), ..conforming }, 200, "\"name\":\"say\""),
    ];

    for (post, status, holds) in cases {
        let read = answered(&mut harness, port, &post);

        assert_eq!(read.status, status, "{post:?}: {read:?}");
        assert!(read.body.contains(holds), "{post:?}: {read:?}");
    }

    Ok(())
}

#[test]
fn a_name_in_base64_is_compared_decoded() -> Result<(), Unheard> {
    // Catches the header compared as it was sent, which refuses a conforming client.
    let (mut harness, port) = holding_say()?;

    let client_of = client(port, &Post { name: Some("=?base64?c2F5?="), ..Post::of(Rpc::CallSay) });
    let call = harness.asked::<Call>()?;
    harness.reply(call, &Called::Refused(String::from("heard")))?;

    let read = heard(&mut harness, client_of);
    assert!(read.body.contains("heard"), "{read:?}");

    Ok(())
}

#[test]
fn the_list_is_every_tool_by_name_with_its_schemas_and_stale_at_once() -> Result<(), Unheard> {
    // Catches an order that varies, a schema left out, a tool list a client may keep, and fields the revision requires
    // left off.
    let (mut harness, mcp, port) = served(McpSettings::default())?;
    let worker = harness.protocol::<Tooling>();
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("zebra"), sheet("ant")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    let read = answered(&mut harness, port, &Post::of(Rpc::List));

    let (ant, zebra) = (read.body.find("\"ant\""), read.body.find("\"zebra\""));
    assert!(ant < zebra, "{read:?}");
    assert!(read.body.contains("\"inputSchema\":{\"type\":\"object\""), "{read:?}");
    assert!(read.body.contains("\"outputSchema\":{\"type\":\"object\""), "{read:?}");
    assert!(read.body.contains("\"ttlMs\":0,\"cacheScope\":\"public\""), "{read:?}");
    assert!(read.body.contains("io.modelcontextprotocol/serverInfo"), "{read:?}");

    Ok(())
}

#[test]
fn an_image_is_answered_as_an_image_block_beside_its_text_and_lists_no_output_schema() -> Result<(), Unheard> {
    // Catches an image written as text or as `structuredContent`, its bytes or text lost, its MIME type left off, an
    // output schema listed for a tool that answers an image, and an image from a tool whose sheet says a value
    // written as if it allowed one.
    let (mut harness, mcp, port) = served(McpSettings::default())?;
    let worker = harness.protocol::<Tooling>();
    let shout = ToolSheet { output: Output::Image, ..sheet("shout") };
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say"), shout], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    let read = answered(&mut harness, port, &Post::of(Rpc::List));
    assert_eq!(read.body.matches("\"outputSchema\"").count(), 1, "{read:?}");

    let client_of = client(port, &Post::of(Rpc::CallShout));
    let call = harness.asked::<Call>()?;
    let png = harness.put(b"foobar".to_vec())?;
    harness.reply(call, &Called::Image { png, text: String::from("6 \"bytes\"") })?;
    let read = heard(&mut harness, client_of);
    let image = "{\"type\":\"image\",\"data\":\"Zm9vYmFy\",\"mimeType\":\"image/png\"}";
    assert!(read.body.contains(image), "{read:?}");
    assert!(read.body.contains("{\"type\":\"text\",\"text\":\"6 \\\"bytes\\\"\"}"), "{read:?}");
    assert!(!read.body.contains("structuredContent"), "{read:?}");
    assert!(read.body.contains("\"isError\":false"), "{read:?}");

    let client_of = client(port, &Post::of(Rpc::CallSay));
    let call = harness.asked::<Call>()?;
    let png = harness.put(b"foobar".to_vec())?;
    harness.reply(call, &Called::Image { png, text: String::new() })?;
    let read = heard(&mut harness, client_of);
    assert!(read.body.contains("its sheet says a value"), "{read:?}");
    assert!(read.body.contains("\"isError\":true"), "{read:?}");

    Ok(())
}

#[test]
fn an_offer_is_refused_whole_and_says_why() -> Result<(), Unheard> {
    // Catches part of a refused batch kept, a name held twice, a table past its limit, and an input with no object
    // form taken, as a struct with positional fields would be if the root's type alone were tested.
    let (mut harness, mcp, _) = served(McpSettings { most_tools: 2, ..McpSettings::default() })?;
    let worker = harness.protocol::<Tooling>();
    let positional = ToolSheet { input: SchemaNodes::of(&Positional::SHAPE), ..sheet("positional") };
    let cases = [
        (vec![sheet("one")], Offered::Yes),
        (vec![sheet("two"), sheet("one")], Offered::Taken),
        (vec![sheet("two"), sheet("two")], Offered::Taken),
        (vec![sheet("bad name")], Offered::BadName),
        (vec![sheet("")], Offered::BadName),
        (vec![sheet(&"x".repeat(129))], Offered::BadName),
        (vec![positional], Offered::NotAnObject),
        (vec![sheet("two"), sheet("three")], Offered::Full),
        (vec![sheet("two")], Offered::Yes),
    ];

    for (tools, offered) in cases {
        let offering = harness.ask(mcp, &Offer { tools, worker })?;

        assert_eq!(harness.answer(offering)?, Answered::Yes(offered));
    }

    Ok(())
}

#[test]
fn a_tool_withdrawn_is_no_longer_held_and_its_name_is_free() -> Result<(), Unheard> {
    // Catches a withdraw that leaves the tool listed or its name taken.
    let (mut harness, mcp, port) = served(McpSettings::default())?;
    let worker = harness.protocol::<Tooling>();
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    harness.tell(mcp, &Withdraw { tool: String::from("say"), worker })?;

    let read = answered(&mut harness, port, &Post::of(Rpc::CallSay));
    assert!(read.body.contains("-32602"), "{read:?}");
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    Ok(())
}

#[test]
fn another_workers_withdraw_leaves_a_tool_held() -> Result<(), Unheard> {
    // Catches a withdraw by name alone, with which a failed toolbox would take a name that was never its own.
    let (mut harness, mcp, port) = served(McpSettings::default())?;
    harness.add_root::<Once>(&(), OnceConfig)?;
    let worker = harness.protocol::<Tooling>();
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    let other = harness.root::<Once>().protocol::<Tooling, _>();
    harness.tell(mcp, &Withdraw { tool: String::from("say"), worker: other })?;

    let read = answered(&mut harness, port, &Post::of(Rpc::List));
    assert!(read.body.contains("\"name\":\"say\""), "{read:?}");

    Ok(())
}

#[test]
fn a_worker_that_ends_takes_its_tools_with_it() -> Result<(), Unheard> {
    // Catches a worker not watched, whose tools stay listed and whose calls would go nowhere.
    let (mut harness, mcp, port) = served(McpSettings::default())?;
    harness.add_root::<Once>(&(), OnceConfig)?;
    let worker = harness.root::<Once>().protocol::<Tooling, _>();
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    let read = answered(&mut harness, port, &Post::of(Rpc::CallSay));
    assert!(read.body.contains("once is enough"), "{read:?}");
    for _ in 0..16 {
        harness.step();
    }

    let read = answered(&mut harness, port, &Post::of(Rpc::CallSay));
    assert!(read.body.contains("-32602"), "{read:?}");

    Ok(())
}

#[test]
fn a_call_past_the_most_out_is_a_result_and_a_slot_comes_back() -> Result<(), Unheard> {
    // Catches `most_calls` not held to, a slot never given back, and a call past it dropped with no answer.
    let (mut harness, mcp, port) = served(McpSettings { most_calls: 1, ..McpSettings::default() })?;
    let worker = harness.protocol::<Tooling>();
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    let first = client(port, &Post::of(Rpc::CallSay));
    let call = harness.asked::<Call>()?;
    let read = answered(&mut harness, port, &Post::of(Rpc::CallSay));
    assert!(read.body.contains("too many calls"), "{read:?}");
    harness.reply(call, &Called::Refused(String::from("first")))?;
    let read = heard(&mut harness, first);
    assert!(read.body.contains("first"), "{read:?}");

    let again = client(port, &Post::of(Rpc::CallSay));
    let call = harness.asked::<Call>()?;
    harness.reply(call, &Called::Refused(String::from("again")))?;
    let read = heard(&mut harness, again);
    assert!(read.body.contains("again"), "{read:?}");

    Ok(())
}

#[test]
fn a_call_its_tool_never_answers_frees_its_slot_at_the_http_deadline() -> Result<(), Unheard> {
    // Catches #355: `Mcp`'s ask to a tool that holds the call and never answers kept a slot of `most_calls` after the
    // client was answered `504`, so with one slot every call after the first was refused. Its ask inherits the
    // exchange's deadline, and expires with it.
    let http = HttpSettings { answer_within_millis: 200, ..HttpSettings::default() };
    let (mut harness, mcp, port) = served_over(http, McpSettings { most_calls: 1, ..McpSettings::default() })?;
    let worker = harness.protocol::<Tooling>();
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    let first = client(port, &Post::of(Rpc::CallSay));
    let _never_answered = harness.asked::<Call>()?;
    let read = heard(&mut harness, first);
    assert_eq!(read.status, 504, "{read:?}");

    let again = client(port, &Post::of(Rpc::CallSay));
    let call = harness.asked::<Call>()?;
    harness.reply(call, &Called::Refused(String::from("again")))?;
    let read = heard(&mut harness, again);
    assert!(read.body.contains("again"), "the slot came back: {read:?}");

    Ok(())
}

#[test]
fn the_largest_request_http_takes_is_read_within_the_limit_it_asks_for() -> Result<(), Unheard> {
    // Catches `most_read_bytes` counting a head by its bytes alone, as a list charges each element whole: a head of the
    // most headers that fit would be mail `Mcp` cannot read, and never handles, however it is written.
    let (mut harness, _, port) = served_over(modest(), McpSettings::default())?;
    let largest = Post { verb: "GET", fillers: 480, write_bytes: 200, ..Post::of(Rpc::List) };

    let read = answered(&mut harness, port, &largest);

    assert_eq!(read.status, 405, "{read:?}");
    let read = answered(&mut harness, port, &Post::of(Rpc::List));
    assert_eq!(read.status, 200, "{read:?}");

    Ok(())
}

/// An HTTP server of small requests, whose engine's read limit, as [`engine_for`] gives it, is modest.
fn modest() -> HttpSettings {
    HttpSettings { most_head_bytes: 2048, most_body_bytes: 256, chunk_bytes: 256, ..HttpSettings::default() }
}

#[test]
fn a_tool_answer_past_the_read_limit_refuses_that_call_and_mcp_serves_the_next() -> Result<(), Unheard> {
    // Catches #357: a tool's answer `Mcp` could not read failed its turn, which ended it, so one large answer took the
    // whole MCP front down until the program restarted. It reaches `Mcp` as no answer, `Unreadable`, which refuses
    // that call, and `Mcp` serves the next. An output is a blob, which no mail's limit weighs (ADR-0042), so the
    // answer too large to read is a refusal's text.
    let past = modest().most_read_bytes() + 1;
    let (mut harness, mcp, port) = served_over(modest(), McpSettings::default())?;
    let worker = harness.protocol::<Tooling>();
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say"), sheet("shout")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    let too_large = client(port, &Post::of(Rpc::CallSay));
    let call = harness.asked::<Call>()?;
    harness.reply(call, &Called::Refused("7".repeat(past)))?;
    let refused = heard(&mut harness, too_large);
    let next = client(port, &Post::of(Rpc::CallShout));
    let call = harness.asked::<Call>()?;
    let tool = call.request.tool.clone();
    harness.reply(call, &Called::Refused(String::from("served")))?;
    let served = heard(&mut harness, next);

    assert_eq!(refused.status, 200, "{refused:?}");
    assert!(refused.body.contains("\"isError\":true"), "{refused:?}");
    assert!(refused.body.contains("Unreadable"), "{refused:?}");
    assert_eq!(tool, "shout");
    assert!(served.body.contains("\"text\":\"served\""), "`Mcp` lived to serve the next call: {served:?}");

    Ok(())
}

#[test]
fn an_offer_past_the_read_limit_is_answered_unreadable_and_mcp_takes_the_next() -> Result<(), Unheard> {
    // Catches #357's other half: an offer whose sheets `Mcp` could not read failed its turn, which ended it. Its
    // offerer is answered `Unreadable`, nothing of it is held, and `Mcp` takes the next offer.
    let past = modest().most_read_bytes() + 1;
    let (mut harness, mcp, port) = served_over(modest(), McpSettings::default())?;
    let worker = harness.protocol::<Tooling>();
    let large = ToolSheet { description: "x".repeat(past), ..sheet("large") };

    let offering = harness.ask(mcp, &Offer { tools: vec![large], worker })?;
    let refused = harness.answer(offering)?;
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    let taken = harness.answer(offering)?;
    let read = answered(&mut harness, port, &Post::of(Rpc::List));

    assert_eq!(refused, Answered::No(Unanswered::Unreadable));
    assert_eq!(taken, Answered::Yes(Offered::Yes));
    assert!(read.body.contains("\"name\":\"say\""), "{read:?}");
    assert!(!read.body.contains("\"name\":\"large\""), "{read:?}");

    Ok(())
}

/// What a listed tool is made from: nothing.
#[derive(Debug, Default, Plain)]
#[kind(name = "aether.test.listed_config")]
struct ListedConfig;

/// A tool to list, of the kind `$kind`, that offers each of `$name` and says how long a text is.
macro_rules! listed {
    ($actor:ident, $kind:literal, $( $name:literal ),+) => {
        #[derive(Debug, Plain)]
        #[kind(name = $kind)]
        struct $actor;

        impl ChildOf for $actor {
            type Parent = Anywhere;
        }

        impl Actor for $actor {
            type Config = ListedConfig;
            type Key = ();
            type Handles = (Call,);
            type BeginSends = ();
            type EndSends = ();

            fn begin(_: ListedConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
                Ok(Self)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Handles<Call> for $actor {
            type Sends = (ToSender<Called>,);

            fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                let say = from_bytes::<Say>(turn.read(&call.input), &mut Budget::new(1024))?;
                let length = u32::try_from(say.text.len()).unwrap_or(u32::MAX);
                let said = Called::output(turn, to_bytes(&Said { length })?);
                turn.reply(&said)?;

                Ok(())
            }
        }

        impl Tool for $actor {
            fn sheets() -> Vec<ToolSheet> {
                vec![$(sheet($name)),+]
            }

            fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
                me.protocol()
            }
        }
    };
}

listed!(Say1, "aether.test.say1", "say");
listed!(Say2, "aether.test.say2", "say");
listed!(Other, "aether.test.other", "other");

/// A root with no place in any program of this test, for a tool to reach.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.absent")]
struct Absent;

/// What is asked of [`Absent`].
#[derive(Debug, Plain)]
#[kind(name = "aether.test.absent_ask")]
struct AbsentAsk;

impl ChildOf for Absent {
    type Parent = Top;
}

impl Carries<AbsentAsk> for Absent {
    type Answers = (AbsentAsk,);
}

impl Actor for Absent {
    type Config = ListedConfig;
    type Key = ();
    type Handles = (AbsentAsk,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: ListedConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<AbsentAsk> for Absent {
    type Sends = (ToSender<AbsentAsk>,);

    fn handle(&mut self, _: AbsentAsk, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&AbsentAsk)?;

        Ok(())
    }
}

/// A tool that reaches a root the program does not have.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.reaching")]
struct Reaching;

impl ChildOf for Reaching {
    type Parent = Anywhere;
}

impl Actor for Reaching {
    type Config = ListedConfig;
    type Key = ();
    type Handles = (Call, Answer<AbsentAsk, ListedConfig>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: ListedConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Call> for Reaching {
    type Sends = (Asks<Path<Absent>, AbsentAsk, ListedConfig>, ToSender<Called>);

    fn handle(&mut self, _: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(path::<Absent>(())).ask(&AbsentAsk, &ListedConfig)?;
        turn.reply(&Called::Refused(String::new()))?;

        Ok(())
    }
}

impl Handles<Answer<AbsentAsk, ListedConfig>> for Reaching {
    type Sends = ();

    fn handle(&mut self, _: (Answered<AbsentAsk>, ListedConfig), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for Reaching {
    fn sheets() -> Vec<ToolSheet> {
        vec![sheet("reaching")]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// The roots of every toolbox test's program, the last made first.
type Served = (Mcp, (Http, (Tcp, ())));

/// The program every toolbox test starts from: `Mcp` served, on a port of its choosing.
fn program() -> Program<Headless, Served> {
    let http = HttpSettings::default();

    Headless::program()
        .with_engine(engine_for(&http))
        .root::<Tcp>()
        .root_with::<Http>(http)
        .root_with::<Mcp>(McpSettings { port: 0, ..McpSettings::default() })
}

/// The port `harness` serves on, once `Http` has bound it.
fn port_of(harness: &mut Harness<Headless>) -> Result<u16, Unheard> {
    let mcp = harness.root::<Mcp>();
    for _ in 0..1000 {
        if let Some(port) = harness.state(mcp)?.port() {
            return Ok(port);
        }
        harness.step();
    }
    panic!("`Mcp` is served");
}

#[test]
fn a_toolbox_stands_up_each_tool_and_a_call_reaches_it() -> Result<(), Unheard> {
    // Catches a tool not spawned, not offered, or offered under the wrong worker, and a generic actor's seat or mail
    // going astray, which no other test can as it is the first of them.
    let mut harness = Harness::of(program().root::<Toolbox<(Say1, Other), Mcp>>())?;
    let port = port_of(&mut harness)?;
    for _ in 0..16 {
        harness.step();
    }

    let read = answered(&mut harness, port, &Post::of(Rpc::List));
    assert!(read.body.contains("\"name\":\"say\""), "{read:?}");
    assert!(read.body.contains("\"name\":\"other\""), "{read:?}");

    let read = answered(&mut harness, port, &Post::of(Rpc::CallSay));
    assert!(read.body.contains("\"structuredContent\":{\"length\":5}"), "{read:?}");

    Ok(())
}

#[test]
fn a_name_listed_twice_stops_the_program_as_it_starts() {
    // Catches two tools of one name found only by `Mcp`, whose refusal comes too late to stop anything.
    let started = Harness::of(program().root::<Toolbox<(Say1, Say2), Mcp>>());

    assert!(matches!(started, Err(Unheard::Program(_))));
}

#[test]
fn a_list_in_groups_stands_up_the_tools_of_each() -> Result<(), Unheard> {
    // Catches a group's tools left unspawned or unoffered, which leaves a list past eight short with nothing said.
    let mut harness = Harness::of(program().root::<Toolbox<((Say1,), ((Other,),)), Mcp>>())?;
    let port = port_of(&mut harness)?;
    for _ in 0..16 {
        harness.step();
    }

    let read = answered(&mut harness, port, &Post::of(Rpc::List));
    assert!(read.body.contains("\"name\":\"say\""), "{read:?}");
    assert!(read.body.contains("\"name\":\"other\""), "{read:?}");

    Ok(())
}

#[test]
fn a_name_listed_in_two_groups_stops_the_program_as_it_starts() {
    // Catches names compared within a group but not across groups.
    let started = Harness::of(program().root::<Toolbox<((Say1,), (Say2,)), Mcp>>());

    assert!(matches!(started, Err(Unheard::Program(_))));
}

#[test]
fn a_tool_that_reaches_what_the_program_lacks_stops_it_as_it_starts() {
    // Catches a refused spawn passed over, which leaves a tool missing with nothing said.
    let started = Harness::of(program().root::<Toolbox<(Reaching,), Mcp>>());

    assert!(matches!(started, Err(Unheard::Program(_))));
}

#[test]
fn a_name_held_already_stops_the_program() -> Result<(), Unheard> {
    // Catches a refused offer ignored, and one that ends only the toolbox, which left its tools running, reached by
    // nothing, for as long as the program ran (#356).
    let mut harness = Harness::of(program())?;
    let (mcp, worker) = (harness.root::<Mcp>(), harness.protocol::<Tooling>());
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    harness.add_root::<Toolbox<(Other, Say1), Mcp>>(&(), ToolboxConfig)?;
    waiting::until(BUDGET, || {
        harness.step();
        harness.host().engine().lifecycle() == Lifecycle::Stopped
    });

    assert_eq!(harness.host().engine().lifecycle(), Lifecycle::Stopped, "the program stops");

    Ok(())
}

/// A directory of files, named for `test`, removed when dropped.
struct Assets(PathBuf);

impl Assets {
    /// A directory holding `files`, each a name in it and its bytes.
    fn new(test: &str, files: &[(&str, &[u8])]) -> Self {
        let root = env::temp_dir().join(format!("aethereal-mcp-{}-{test}", process::id()));
        assert!(fs::create_dir_all(&root).is_ok(), "the directory is made");
        for (name, bytes) in files {
            assert!(fs::write(root.join(name), bytes).is_ok(), "the file is written");
        }

        Self(root)
    }
}

impl Drop for Assets {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A POST calling `read_file` for `path`.
fn read_file(path: &'static str) -> Post {
    read_in("Assets", path)
}

/// A POST calling `read_file` for `path` in `namespace`.
fn read_in(namespace: &str, path: &str) -> Post {
    let arguments = format!("{{\"namespace\":\"{namespace}\",\"path\":\"{path}\"}}");

    call_tool("read_file", &arguments)
}

/// A POST calling `write_file` to write `text`, a JSON string as written, as the file at `path` in `namespace`.
fn write_file(namespace: &str, path: &str, text: &str) -> Post {
    let arguments = format!("{{\"namespace\":\"{namespace}\",\"path\":\"{path}\",\"text\":{text}}}");

    call_tool("write_file", &arguments)
}

/// A POST calling the tool `name` with `arguments`, a JSON object as written.
fn call_tool(name: &'static str, arguments: &str) -> Post {
    let body = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/call\",\"params\":{{\"name\":\"{name}\",\
         \"arguments\":{arguments},\"_meta\":{}}}}}",
        Meta::Whole.text()
    );
    let body: &'static str = Box::leak(body.into_boxed_str());

    Post { body: Some(body), method: Some("tools/call"), name: Some(name), ..Post::of(Rpc::CallSay) }
}

#[test]
fn read_tool_answers_a_file_over_mcp_and_says_what_it_could_not_read() -> Result<(), Unheard> {
    // Catches the tool's decode, hold, ask, and encode each going wrong, a path past the namespace taken, a read that
    // ends `Mcp` by answering more than it can read, and a blob answered as text it is not. `large` is past the
    // tool's limit, which a tool that did not hold to it would answer whole. `full` is at it, and catches an engine
    // whose blobs cannot hold the reply under the program's own settings, which answered it `503` with no body.
    let full = vec![b'a'; 256 * 1024];
    let large = vec![b'a'; 256 * 1024 + 1];
    let files: [(&str, &[u8]); 4] =
        [("a.txt", b"hello \"there\"\n"), ("b.bin", &[0xFF, 0xFE]), ("full.txt", &full), ("large.txt", &large)];
    let assets = Assets::new("read", &files);
    let roots = FileRoots {
        assets: assets.0.display().to_string(),
        save: assets.0.join("save").display().to_string(),
        readers: 2,
    };
    let program = program().root_with::<Files>(roots).root::<Toolbox<(ReadTool,), Mcp>>();
    let mut harness = Harness::of(program)?;
    let port = port_of(&mut harness)?;
    for _ in 0..16 {
        harness.step();
    }

    let read = answered(&mut harness, port, &read_file("a.txt"));
    assert!(read.body.contains("\"structuredContent\":{\"text\":\"hello \\\"there\\\"\\n\"}"), "{read:?}");

    let read = answered(&mut harness, port, &read_file("full.txt"));
    let text = String::from_utf8(full).unwrap_or_default();
    assert_eq!(read.status, 200, "the reply was held: {}", read.status);
    assert!(read.body.contains(&format!("\"structuredContent\":{{\"text\":\"{text}\"}}")), "the file came back whole");

    let cases = [
        ("missing.txt", "no file is at that path"),
        ("b.bin", "not text"),
        ("large.txt", "too large"),
        ("../a.txt", "no path"),
    ];
    for (path, says) in cases {
        let read = answered(&mut harness, port, &read_file(path));

        assert!(read.body.contains(says), "{path}: {read:?}");
        assert!(read.body.contains("\"isError\":true"), "{path}: {read:?}");
    }

    let read = answered(&mut harness, port, &Post::of(Rpc::List));
    assert!(read.body.contains("\"name\":\"read_file\""), "{read:?}");

    Ok(())
}

#[test]
fn write_tool_writes_a_save_over_mcp_that_read_file_reads_back() -> Result<(), Unheard> {
    // Catches the write tool's decode, hold, ask, and encode each going wrong, its text written as anything but its
    // bytes, a write to `Assets` or past the namespace taken, and a write the disk refused answered as done.
    let assets = Assets::new("write", &[("a.txt", b"an asset")]);
    let save = assets.0.join("save");
    let roots = FileRoots { assets: assets.0.display().to_string(), save: save.display().to_string(), readers: 2 };
    let program = program().root_with::<Files>(roots).root::<Toolbox<(ReadTool, WriteTool), Mcp>>();
    let mut harness = Harness::of(program)?;
    let port = port_of(&mut harness)?;
    for _ in 0..16 {
        harness.step();
    }

    let written = answered(&mut harness, port, &write_file("Save", "slots/game.sav", "\"hello \\\"there\\\"\\n\""));
    assert!(written.body.contains("\"structuredContent\":{}"), "{written:?}");
    assert_eq!(fs::read(save.join("slots/game.sav")).ok(), Some(b"hello \"there\"\n".to_vec()));
    let read = answered(&mut harness, port, &read_in("Save", "slots/game.sav"));
    assert!(read.body.contains("\"structuredContent\":{\"text\":\"hello \\\"there\\\"\\n\"}"), "{read:?}");

    let cases = [
        ("Assets", "a.txt", "read-only"),
        ("Save", "../a.txt", "file path does not conform"),
        ("Save", "slots/game.sav/inner.sav", "could not be written"),
    ];
    for (namespace, path, says) in cases {
        let written = answered(&mut harness, port, &write_file(namespace, path, "\"over\""));

        assert!(written.body.contains(says), "{path}: {written:?}");
        assert!(written.body.contains("\"isError\":true"), "{path}: {written:?}");
    }
    assert_eq!(fs::read(assets.0.join("a.txt")).ok(), Some(b"an asset".to_vec()));

    let listed = answered(&mut harness, port, &Post::of(Rpc::List));
    assert!(listed.body.contains("\"name\":\"write_file\""), "{listed:?}");

    Ok(())
}

/// The most bytes of a frame of the link, each way.
const LINK_BYTES: u32 = 64 * 1024;

/// Writes `frame` to `stream` as the link frames it: its length, big-endian, then its bytes.
fn send_frame<T: Plain>(stream: &mut TcpStream, frame: &T) {
    let Ok(bytes) = to_bytes(frame) else {
        panic!("the frame encodes");
    };
    let Ok(length) = u32::try_from(bytes.len()) else {
        panic!("the frame has a length");
    };
    assert!(stream.write_all(&length.to_be_bytes()).is_ok(), "the length writes");
    assert!(stream.write_all(&bytes).is_ok(), "the frame writes");
}

/// The next frame of the link on `stream`, read as `T`.
fn receive_frame<T: Plain>(stream: &mut TcpStream) -> T {
    let mut prefix = [0; 4];
    assert!(stream.read_exact(&mut prefix).is_ok(), "a frame comes within ten seconds");
    let mut bytes = vec![0; usize::try_from(u32::from_be_bytes(prefix)).unwrap_or(usize::MAX)];
    assert!(stream.read_exact(&mut bytes).is_ok(), "the frame comes whole");
    let Ok(frame) = from_bytes(&bytes, &mut Budget::new(1 << 24)) else {
        panic!("the frame reads as what was expected");
    };

    frame
}

/// What `Engines` lists.
fn engine_list(harness: &mut Harness<Headless>) -> Result<EngineList, Unheard> {
    let engines = harness.root::<Engines>();
    let listing = harness.ask(engines, &ListEngines)?;
    let Answered::Yes(list) = harness.answer(listing)? else {
        panic!("`Engines` answers its list");
    };

    Ok(list)
}

/// A hub over `http`, serving MCP with the hub's tools, with one call out to each engine, and an engine linked to it
/// holding `say`, played by a socket: the hub, its MCP port, and the engine's connection.
fn hub_with_an_engine(http: HttpSettings) -> Result<(Harness<Headless>, u16, TcpStream), Unheard> {
    let engines = EnginesSettings { port: 0, most_calls: 1, most_bytes: LINK_BYTES, ..EnginesSettings::default() };
    let read_limit = engine_for(&http).read_limit.max(LINK_BYTES);
    let program = Headless::program()
        .with_engine(EngineSettings { read_limit, ..engine_for(&http) })
        .root::<Tcp>()
        .root_with::<Http>(http)
        .root_with::<Mcp>(McpSettings { port: 0, ..McpSettings::default() })
        .root_with::<Engines>(engines)
        .root::<Toolbox<(EngineTools,), Mcp>>();
    let mut harness = Harness::of(program)?;
    let port = port_of(&mut harness)?;
    let engines = harness.root::<Engines>();
    waiting::until(BUDGET, || {
        harness.step();
        harness.state(engines).is_ok_and(|engines| engines.port().is_some())
    });
    let Some(link_port) = harness.state(engines)?.port() else {
        panic!("`Engines` listens");
    };
    harness.settle();

    let minting = harness.ask(engines, &MintToken)?;
    let Answered::Yes(Minted { token, .. }) = harness.answer(minting)? else {
        panic!("`Engines` mints a token");
    };
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", link_port)) else {
        panic!("the engine's side connects");
    };
    assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");
    send_frame(&mut stream, &Up::Hello { revision: REVISION, name: String::from("scene"), token });
    send_frame(&mut stream, &Up::Sheets { revision: 1, tools: vec![sheet("say")] });
    waiting::until(BUDGET, || engine_list(&mut harness).is_ok_and(|list| list.engines.len() == 1));
    assert_eq!(engine_list(&mut harness)?.engines.len(), 1, "the engine is linked");

    Ok((harness, port, stream))
}

/// `call_tool` of `say` on engine 0.
fn call_say_on_the_engine() -> Post {
    call_tool("call_tool", "{\"engine\":0,\"tool\":\"say\",\"input\":{\"text\":\"hello\"}}")
}

#[test]
fn call_tool_of_a_tool_that_never_answers_is_answered_504_and_frees_the_engines_slot() -> Result<(), Unheard> {
    // Catches the link's hop left out of HTTP's deadline: `EngineTools` asks the engine from the turn that reads its
    // tools, not the one that holds the call, so an ask there that states no deadline is due never, `Engine` keeps
    // the call after the client was answered `504`, and with one call out every `call_tool` of the engine after it is
    // refused. Catches too a call written with no time left, or more than HTTP gave it.
    let http = HttpSettings { answer_within_millis: 300, ..HttpSettings::default() };
    let (mut harness, port, stream) = hub_with_an_engine(http)?;

    let first = client(port, &call_say_on_the_engine());
    #[allow(clippy::disallowed_methods)] // The engine's side is a native thread of its own, outside the engine.
    let silent = thread::spawn(move || {
        let mut stream = stream;
        let down: Down = receive_frame(&mut stream);

        (down, stream)
    });
    let read = heard(&mut harness, first);
    assert_eq!(read.status, 504, "{read:?}");
    let Ok((written, stream)) = silent.join() else {
        panic!("the engine's side read the call");
    };
    let Down::Call { call: 0, within: Within::Millis(left), .. } = written else {
        panic!("the call is written with the time it has left: {written:?}");
    };
    assert!(left <= 300, "{left} milliseconds left of 300");

    let engines = harness.root::<Engines>();
    let Some(engine) = harness.state(engines)?.engine(0) else {
        panic!("the engine stands under its id");
    };
    waiting::until(BUDGET, || {
        harness.step();
        harness.state(engine).is_ok_and(|engine| engine.calls_out() == 0)
    });
    assert_eq!(harness.state(engine)?.calls_out(), 0, "the call's slot is freed at HTTP's deadline");

    let again = client(port, &call_say_on_the_engine());
    #[allow(clippy::disallowed_methods)] // The engine's side is a native thread of its own, outside the engine.
    let answering = thread::spawn(move || {
        let mut stream = stream;
        let down: Down = receive_frame(&mut stream);
        let Ok(output) = to_bytes(&Said { length: 5 }) else {
            panic!("the output encodes");
        };
        if let Down::Call { call, .. } = down {
            send_frame(&mut stream, &Up::Called { call, called: Outcome::Output(output) });
        }

        stream
    });
    let read = heard(&mut harness, again);
    assert!(read.body.contains("\"length\":5"), "the engine's slot came back: {read:?}");
    assert!(answering.join().is_ok(), "the engine's side answered the call");

    Ok(())
}
