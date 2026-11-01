//! The MCP capability served on the native host over real HTTP, the test its tools' worker (ADR-0037, ADR-0036).

use aethereal_actor::{
    Actor, ActorRef, Answered, Beginning, ChildOf, Ending, Fault, Handles, Tick, ToSender, Top, Turn, Unanswered,
};
use aethereal_chassis_headless::Headless;
use aethereal_data::{Budget, Plain, Schema, SchemaNodes, from_json, to_json};
use aethereal_http::{Http, HttpSettings};
use aethereal_mcp::{Call, Called, Mcp, McpSettings, Offer, Offered, ToolSheet, Tooling, Withdraw};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{EngineSettings, Host};
use aethereal_tcp::Tcp;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::thread::{self, JoinHandle};
use std::time::Duration;

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
        output: SchemaNodes::of(&Said::SHAPE),
    }
}

/// The engine an MCP server needs: mail as large as a request or an offer, which the default's would fail to read.
fn roomy() -> EngineSettings {
    EngineSettings { read_limit: 1 << 20, mail_bytes: 1 << 23, ..EngineSettings::default() }
}

/// A host serving MCP on a port of its choosing, and that port.
fn served(settings: McpSettings) -> Result<(Harness<Headless>, ActorRef<Mcp>, u16), Unheard> {
    let settings = McpSettings { port: 0, ..settings };
    let mut harness = Harness::of(
        Headless::program()
            .with_engine(roomy())
            .with_cycle::<(Tick,)>()
            .root::<Tcp>()
            .root_with::<Http>(HttpSettings::default())
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
            Self::CallSay | Self::CallSayBadly | Self::CallNothing => "tools/call",
            Self::Initialize => "initialize",
            Self::Ping => "ping",
        }
    }

    /// The params beside `_meta`.
    const fn fields(self) -> &'static str {
        match self {
            Self::CallSay => "\"name\":\"say\",\"arguments\":{\"text\":\"hello\"},",
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
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
            panic!("the client connects");
        };
        assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");
        assert!(stream.write_all(&sent).is_ok(), "the client writes");
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
    let Ok(input) = to_json(&schema, &call.request.input, &mut Budget::new(1024)) else {
        panic!("the input is a value of its schema");
    };
    assert_eq!(input, "{\"text\":\"hello\"}");
    let Ok(output) = from_json(&SchemaNodes::of(&Said::SHAPE), "{\"length\":5}", &mut Budget::new(1024)) else {
        panic!("the output is a value of its schema");
    };
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
    harness.reply(call, &Called::Output(vec![1]))?;
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

    harness.tell(mcp, &Withdraw { tool: String::from("say") })?;

    let read = answered(&mut harness, port, &Post::of(Rpc::CallSay));
    assert!(read.body.contains("-32602"), "{read:?}");
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

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
