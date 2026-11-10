//! The link served on the native host (ADR-0038, ADR-0040, ADR-0036): each end driven by the harness, and the other
//! played by a real socket speaking the link's own typed frames, on a thread of its own where it waits on the end it
//! plays to.

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Held, Path, ToSender,
    Top, Turn, Unanswered, path,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_link::{
    CallEngine, DescribeEngine, Described, Down, Engine, EngineList, EngineListing, Engines, EnginesSettings, Link,
    LinkSettings, ListEngines, MintToken, Minted, Origin, Outcome, REVISION, Refusal, Token, Up, Within,
};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{EngineSettings, Host, Lifecycle};
use aethereal_tcp::Tcp;
use aethereal_tools::{Call, Called, Offer, Offered, Output, ToolSheet, Tooling, Tools, Withdraw};
use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// The most bytes of a frame, each way, at both ends.
const MOST_BYTES: u32 = 64 * 1024;

/// What the test's tool reads.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.say")]
struct Say {
    text: String,
}

/// What the test's tool answers.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.said")]
struct Said {
    length: u32,
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

/// `value` as bytes.
fn bytes_of<T: Plain>(value: &T) -> Vec<u8> {
    let Ok(bytes) = to_bytes(value) else {
        panic!("the value encodes");
    };

    bytes
}

/// What `answered` came to, its output read out of its blob in `harness`: to compare with what the engine sent.
fn outcome(harness: &mut Harness<Headless>, answered: Answered<Called>) -> Answered<Outcome> {
    match answered {
        Answered::Yes(Called::Output(bytes)) => {
            let Some(bytes) = harness.read(&bytes) else {
                panic!("the output's blob is held");
            };
            Answered::Yes(Outcome::Output(bytes.to_vec()))
        }
        Answered::Yes(Called::Image { png, text }) => {
            let Some(png) = harness.read(&png) else {
                panic!("the image's blob is held");
            };
            Answered::Yes(Outcome::Image { png: png.to_vec(), text })
        }
        Answered::Yes(Called::Refused(why)) => Answered::Yes(Outcome::Refused(why)),
        Answered::No(why) => Answered::No(why),
    }
}

/// The engine both ends run on: it reads a frame whole.
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

/// Whether the other end closes `stream`: the next read finds the end.
fn closes(stream: &mut TcpStream) -> bool {
    let mut byte = [0];

    stream.read(&mut byte).is_ok_and(|count| count == 0)
}

/// `stream`, which gives up on a read after ten seconds.
fn timed(stream: TcpStream) -> TcpStream {
    assert!(stream.set_nonblocking(false).is_ok(), "the stream blocks");
    assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");

    stream
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

/// The next `count` frames on `stream`, read on a thread of their own as the harness steps, and the stream.
fn frames<T: Plain + Send + 'static>(
    harness: &mut Harness<Headless>,
    mut stream: TcpStream,
    count: usize,
) -> (Vec<T>, TcpStream) {
    #[allow(clippy::disallowed_methods)] // The socket's side is a native thread of its own, outside the engine.
    let reader = thread::spawn(move || {
        let frames = (0..count).map(|_| receive(&mut stream)).collect();

        (frames, stream)
    });

    heard(harness, reader)
}

/// A hub listening for engines on a port of its choosing, and that port.
fn hub() -> Result<(Harness<Headless>, u16), Unheard> {
    hub_with(EnginesSettings::default())
}

/// A hub with `settings`, listening for engines on a port of its choosing, and that port.
fn hub_with(settings: EnginesSettings) -> Result<(Harness<Headless>, u16), Unheard> {
    let settings = EnginesSettings { port: 0, most_bytes: MOST_BYTES, ..settings };
    let program = Headless::program().with_engine(engine()).root::<Tcp>().root_with::<Engines>(settings);
    let mut harness = Harness::of(program)?;
    let engines = harness.root::<Engines>();
    waiting::until(BUDGET, || {
        harness.step();
        harness.state(engines).is_ok_and(|engines| engines.port().is_some())
    });
    let Some(port) = harness.state(engines)?.port() else {
        panic!("`Engines` listens");
    };

    Ok((harness, port))
}

/// A connection to the hub at `port`, as an engine dials it.
fn dialed(port: u16) -> TcpStream {
    let Ok(stream) = TcpStream::connect(("127.0.0.1", port)) else {
        panic!("the engine's side connects");
    };

    timed(stream)
}

/// A token `Engines` minted for an engine started outside the hub.
fn minted(harness: &mut Harness<Headless>) -> Result<Token, Unheard> {
    let engines = harness.root::<Engines>();
    let minting = harness.ask(engines, &MintToken)?;
    let Answered::Yes(Minted { token, .. }) = harness.answer(minting)? else {
        panic!("`Engines` mints a token");
    };

    Ok(token)
}

/// The `Hello` of an engine named `name`, of this crate's revision, bringing `token`.
fn hello(name: &str, token: Token) -> Up {
    Up::Hello { revision: REVISION, name: String::from(name), token }
}

/// The frame the hub answers on `stream`, and whether it then hangs up, the harness stepping the host until it has.
fn refusal(harness: &mut Harness<Headless>, mut stream: TcpStream) -> (Down, bool) {
    #[allow(clippy::disallowed_methods)] // The engine's side is a native thread of its own, outside the engine.
    let peer = thread::spawn(move || {
        let down: Down = receive(&mut stream);

        (down, closes(&mut stream))
    });

    heard(harness, peer)
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

/// What `Engines` lists once `done` holds of it.
fn listed_once(harness: &mut Harness<Headless>, done: impl Fn(&EngineList) -> bool) -> Result<EngineList, Unheard> {
    waiting::until(BUDGET, || listed(harness).is_ok_and(|list| done(&list)));

    listed(harness)
}

/// Whether `list` holds one engine, with `tools` tools.
fn holds_one_with(list: &EngineList, tools: u32) -> bool {
    let is_one = list.engines.len() == 1;
    let has_tools = list.engines.iter().all(|listing| listing.tools == tools);

    is_one && has_tools
}

/// An engine linked as `name`, with the set `tools` of `revision`, once `Engines` lists it, and its connection.
fn linked_engine(
    harness: &mut Harness<Headless>,
    port: u16,
    revision: u64,
    tools: Vec<ToolSheet>,
) -> Result<TcpStream, Unheard> {
    let count = u32::try_from(tools.len()).unwrap_or(u32::MAX);
    let token = minted(harness)?;
    let mut stream = dialed(port);
    send(&mut stream, &hello("scene", token));
    send(&mut stream, &Up::Sheets { revision, tools });
    listed_once(harness, |list| holds_one_with(list, count))?;

    Ok(stream)
}

#[test]
fn an_engines_hello_with_a_minted_token_and_latest_set_are_listed_and_described() -> Result<(), Unheard> {
    // Catches a minted token not kept, so no engine could link, a `Hello`'s name or a set not kept, a set sent while
    // the token is taken lost, a tool count never told to `Engines`, an engine listed under another id, and a
    // description that is not the latest set with its revision.
    let (mut harness, port) = hub()?;
    let token = minted(&mut harness)?;
    let mut stream = dialed(port);
    send(&mut stream, &hello("scene", token));
    send(&mut stream, &Up::Sheets { revision: 3, tools: vec![sheet("say")] });
    send(&mut stream, &Up::Sheets { revision: 7, tools: vec![sheet("say"), sheet("look")] });

    let list = listed_once(&mut harness, |list| holds_one_with(list, 2))?;
    assert_eq!(
        list.engines,
        vec![EngineListing { engine: 0, name: String::from("scene"), tools: 2, origin: Origin::External }]
    );

    let engines = harness.root::<Engines>();
    let Some(engine) = harness.state(engines)?.engine(0) else {
        panic!("the engine stands under its id");
    };
    let describing = harness.ask(engine, &DescribeEngine)?;
    let described = harness.answer(describing)?;
    let latest = Described::Tools { revision: 7, tools: vec![sheet("say"), sheet("look")] };
    assert_eq!(described, Answered::Yes(latest));

    Ok(())
}

#[test]
fn a_call_is_written_down_and_answered_with_what_the_engine_sends_up() -> Result<(), Unheard> {
    // Catches a call written with the wrong number, revision, tool, or input, a request not held for the engine's
    // answer, an answer matched to the wrong call, and a call asked with no deadline written with a time left.
    let (mut harness, port) = hub()?;
    let stream = linked_engine(&mut harness, port, 3, vec![sheet("say")])?;
    let engines = harness.root::<Engines>();
    let Some(engine) = harness.state(engines)?.engine(0) else {
        panic!("the engine stands under its id");
    };
    let input = bytes_of(&Say { text: String::from("hello") });
    let output = bytes_of(&Said { length: 5 });

    let held = harness.put(input.clone())?;
    let calling = harness.ask(engine, &CallEngine { revision: 3, tool: String::from("say"), input: held })?;
    let answer = output.clone();
    #[allow(clippy::disallowed_methods)] // The engine's side is a native thread of its own, outside the engine.
    let peer = thread::spawn(move || {
        let mut stream = stream;
        let down: Down = receive(&mut stream);
        if let Down::Call { call, .. } = down {
            send(&mut stream, &Up::Called { call, called: Outcome::Output(answer) });
        }

        (down, stream)
    });
    let (down, _stream) = heard(&mut harness, peer);

    assert_eq!(down, Down::Call { call: 0, revision: 3, tool: String::from("say"), input, within: Within::Unbounded });
    let answered = harness.answer(calling)?;
    assert_eq!(outcome(&mut harness, answered), Answered::Yes(Outcome::Output(output)));

    Ok(())
}

#[test]
fn an_engine_of_another_revision_is_refused_and_hung_up_on() -> Result<(), Unheard> {
    // Catches a revision not checked, a refusal that does not name the hub's revision, a connection left open after
    // it, and a refused engine listed.
    let (mut harness, port) = hub()?;
    let token = minted(&mut harness)?;
    let mut stream = dialed(port);
    send(&mut stream, &Up::Hello { revision: REVISION + 1, name: String::from("future"), token });

    let (down, closed) = refusal(&mut harness, stream);

    assert_eq!(down, Down::Refused(Refusal::Revision { revision: REVISION }));
    assert!(closed, "the hub hangs up once its refusal is sent");
    assert_eq!(listed(&mut harness)?.engines, Vec::new());

    Ok(())
}

#[test]
fn an_engine_of_the_revision_before_calls_carried_their_time_is_refused() -> Result<(), Unheard> {
    // Catches `REVISION` left at 1 when `Down::Call` gained `within`: a build of revision 1, which the store keeps,
    // would be linked and misread every call the hub writes it.
    let (mut harness, port) = hub()?;
    let token = minted(&mut harness)?;
    let mut stream = dialed(port);
    send(&mut stream, &Up::Hello { revision: 1, name: String::from("stored"), token });

    let (down, closed) = refusal(&mut harness, stream);

    assert_eq!(down, Down::Refused(Refusal::Revision { revision: REVISION }));
    assert!(closed, "the hub hangs up once its refusal is sent");

    Ok(())
}

#[test]
fn a_token_the_hub_never_minted_is_refused_and_hung_up_on() -> Result<(), Unheard> {
    // Catches a token not checked, which lets any process on the machine link, a refusal that does not say it was the
    // token, a connection left open after it, and an engine listed before its token was taken.
    let (mut harness, port) = hub()?;
    let mut stream = dialed(port);
    send(&mut stream, &hello("stranger", Token([7; 16])));
    send(&mut stream, &Up::Sheets { revision: 1, tools: vec![sheet("say")] });

    let (down, closed) = refusal(&mut harness, stream);

    assert_eq!(down, Down::Refused(Refusal::Token));
    assert!(closed, "the hub hangs up once its refusal is sent");
    assert_eq!(listed(&mut harness)?.engines, Vec::new());

    Ok(())
}

#[test]
fn a_token_is_good_for_one_hello_and_refused_the_second_time() -> Result<(), Unheard> {
    // Catches a token left in the table once a `Hello` brought it, which a process that read it from the process list
    // after the engine linked could link with.
    let (mut harness, port) = hub()?;
    let token = minted(&mut harness)?;
    let mut first = dialed(port);
    send(&mut first, &hello("scene", token));
    send(&mut first, &Up::Sheets { revision: 1, tools: vec![sheet("say")] });
    listed_once(&mut harness, |list| holds_one_with(list, 1))?;

    let mut second = dialed(port);
    send(&mut second, &hello("copy", token));
    let (down, closed) = refusal(&mut harness, second);

    assert_eq!(down, Down::Refused(Refusal::Token));
    assert!(closed, "the hub hangs up once its refusal is sent");
    assert_eq!(
        listed(&mut harness)?.engines,
        vec![EngineListing { engine: 0, name: String::from("scene"), tools: 1, origin: Origin::External }]
    );

    Ok(())
}

#[test]
fn a_token_brought_once_its_time_has_passed_is_refused() -> Result<(), Unheard> {
    // Catches a token that never expires, so one minted for an engine never started stays good for good. The hub's
    // tokens are good for no time at all, so the `Hello` comes after its time whatever the machine's speed.
    let (mut harness, port) = hub_with(EnginesSettings { token_within_millis: 0, ..EnginesSettings::default() })?;
    let token = minted(&mut harness)?;
    let mut stream = dialed(port);
    send(&mut stream, &hello("late", token));

    let (down, closed) = refusal(&mut harness, stream);

    assert_eq!(down, Down::Refused(Refusal::Token));
    assert!(closed, "the hub hangs up once its refusal is sent");

    Ok(())
}

#[test]
fn an_engine_whose_connection_closes_leaves_the_list_and_its_call_is_answered_no() -> Result<(), Unheard> {
    // Catches an engine left listed after its connection went, a held call left waiting for good, and an ask of a
    // gone engine that is not answered `No`, which the hub's tools read as no such engine.
    let (mut harness, port) = hub()?;
    let stream = linked_engine(&mut harness, port, 1, vec![sheet("say")])?;
    let engines = harness.root::<Engines>();
    let Some(engine) = harness.state(engines)?.engine(0) else {
        panic!("the engine stands under its id");
    };

    let input = harness.put(bytes_of(&Say { text: String::from("hello") }))?;
    let calling = harness.ask(engine, &CallEngine { revision: 1, tool: String::from("say"), input })?;
    #[allow(clippy::disallowed_methods)] // The engine's side is a native thread of its own, outside the engine.
    let peer = thread::spawn(move || {
        let mut stream = stream;
        let _: Down = receive(&mut stream);
    });
    heard(&mut harness, peer);

    assert_eq!(harness.answer(calling)?, Answered::No(Unanswered::Ended));
    let list = listed_once(&mut harness, |list| list.engines.is_empty())?;
    assert_eq!(list.engines, Vec::new());
    let describing = harness.ask(engine, &DescribeEngine)?;
    assert_eq!(harness.answer(describing)?, Answered::No(Unanswered::Ended));

    Ok(())
}

/// What the test asks its asker: to ask engine 0 to call `call`, stating a deadline `within_millis` after it asks.
#[derive(Debug, Bound)]
#[kind(name = "aether.test.call_within")]
struct CallWithin {
    call: CallEngine,
    within_millis: u64,
}

/// What became of the asker's call, as it answers the test.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.test.heard")]
enum Heard {
    Called(Called),
    No(Unanswered),
}

/// What the asker keeps for its call: the test's request, answered with what became of it.
#[derive(Debug, Bound)]
#[kind(name = "aether.test.relaying")]
struct Relaying {
    held: Held<Heard>,
}

/// What the asker is made from: nothing.
#[derive(Debug, Default, Plain)]
#[kind(name = "aether.test.asker_config")]
struct AskerConfig;

/// An asker standing in for the hub's tools, which ask an engine on behalf of a call they hold: it states a deadline of
/// its own, as HTTP does where a request enters the hub, which the outside cannot.
#[derive(Debug, Plain)]
#[kind(name = "aether.test.asker")]
struct Asker;

impl ChildOf for Asker {
    type Parent = Top;
}

impl Carries<CallWithin> for Asker {
    type Answers = (Heard,);
}

impl Actor for Asker {
    type Config = AskerConfig;
    type Key = ();
    type Handles = (CallWithin, Answer<Called, Relaying>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: AskerConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<CallWithin> for Asker {
    type Sends = (ToSender<Heard>, Asks<Path<Engine>, CallEngine, Relaying>);

    fn handle(&mut self, asked: CallWithin, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        let due = turn.now() + Duration::from_millis(asked.within_millis);
        turn.to(path::<Engine>(0)).within(due).ask(&asked.call, &Relaying { held })?;

        Ok(())
    }
}

impl Handles<Answer<Called, Relaying>> for Asker {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Relaying { held }): (Answered<Called>, Relaying),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let heard = match answered {
            Answered::Yes(called) => Heard::Called(called),
            Answered::No(why) => Heard::No(why),
        };
        turn.answer(held, &heard)?;

        Ok(())
    }
}

/// How long the asker gives its call.
const WITHIN_MILLIS: u64 = 300;

/// The bytes of the input of the call of `say` the hub-side tests ask.
fn say_input() -> Vec<u8> {
    bytes_of(&Say { text: String::from("hello") })
}

/// The call of `say` the hub-side tests ask, of the set of revision 1, its input put in `harness`.
fn say_call(harness: &mut Harness<Headless>) -> Result<CallEngine, Unheard> {
    Ok(CallEngine { revision: 1, tool: String::from("say"), input: harness.put(say_input())? })
}

/// The engine linked under id 0.
fn first_engine(harness: &mut Harness<Headless>) -> Result<ActorRef<Engine>, Unheard> {
    let engines = harness.root::<Engines>();
    let Some(engine) = harness.state(engines)?.engine(0) else {
        panic!("the engine stands under its id");
    };

    Ok(engine)
}

/// Whether `heard` says the asker's call ran out of time: the engine's `Expired`, or `Engine`'s refusal, which comes
/// first when its sleep is handled in a step that began before the deadline.
fn out_of_time(heard: &Answered<Heard>) -> bool {
    let expired = matches!(heard, Answered::Yes(Heard::No(Unanswered::Expired)));
    let refused = matches!(heard, Answered::Yes(Heard::Called(Called::Refused(why))) if why.contains("in time"));

    expired || refused
}

/// A hub with one call out to each engine and an asker, an engine linked with `say`, and a call the asker gave
/// `WITHIN_MILLIS` that the engine never answers, once its slot is freed: the frame written for it, the engine's
/// connection, and the engine.
fn lapsed() -> Result<(Harness<Headless>, Down, TcpStream, ActorRef<Engine>), Unheard> {
    let (mut harness, port) = hub_with(EnginesSettings { most_calls: 1, ..EnginesSettings::default() })?;
    harness.add_root::<Asker>(&(), AskerConfig)?;
    let stream = linked_engine(&mut harness, port, 1, vec![sheet("say")])?;
    let (engine, asker) = (first_engine(&mut harness)?, harness.root::<Asker>());

    let call = say_call(&mut harness)?;
    let asking = harness.ask(asker, &CallWithin { call, within_millis: WITHIN_MILLIS })?;
    let (mut written, stream) = frames::<Down>(&mut harness, stream, 1);
    let heard = harness.answer(asking)?;
    assert!(out_of_time(&heard), "{heard:?}");
    waiting::until(BUDGET, || {
        harness.step();
        harness.state(engine).is_ok_and(|engine| engine.calls_out() == 0)
    });
    assert_eq!(harness.state(engine)?.calls_out(), 0, "the call's slot is freed at its deadline");
    let Some(written) = written.pop() else {
        unreachable!("one frame was read");
    };

    Ok((harness, written, stream, engine))
}

/// The next call the hub writes on `stream`, answered with each of `answers` in turn, under the number each names or
/// the call's own, the harness stepping the host until they are written, and the stream.
fn answered_call(harness: &mut Harness<Headless>, stream: TcpStream, answers: Vec<Answering>) -> (Down, TcpStream) {
    #[allow(clippy::disallowed_methods)] // The engine's side is a native thread of its own, outside the engine.
    let peer = thread::spawn(move || {
        let mut stream = stream;
        let down: Down = receive(&mut stream);
        if let Down::Call { call, .. } = down {
            for answering in answers {
                let up = match answering {
                    Answering::This(called) => Up::Called { call, called },
                    Answering::Lapsed { call, called } => Up::Called { call, called },
                };
                send(&mut stream, &up);
            }
        }

        (down, stream)
    });

    heard(harness, peer)
}

/// What the engine's side sends once it reads a call.
enum Answering {
    /// The call's answer.
    This(Outcome),
    /// An answer of the earlier call `call`, whose deadline has passed.
    Lapsed { call: u64, called: Outcome },
}

#[test]
fn a_call_the_engine_never_answers_frees_its_slot_at_its_deadline() -> Result<(), Unheard> {
    // Catches `Engine` keeping a call the engine never answers past its deadline, which with one call out refuses
    // every call after it, "too many calls of the engine are out", for good: #355 at the link. Catches too a call
    // written with no time left, or with more than its asker gave it, as a deadline written as the time on the hub's
    // clock is.
    let (mut harness, written, stream, engine) = lapsed()?;
    let Down::Call { call: 0, within: Within::Millis(left), .. } = written else {
        panic!("the call is written with the time it has left: {written:?}");
    };
    assert!(left <= WITHIN_MILLIS, "{left} milliseconds left of {WITHIN_MILLIS}");
    let output = bytes_of(&Said { length: 5 });

    let call = say_call(&mut harness)?;
    let calling = harness.ask(engine, &call)?;
    let (down, _stream) = answered_call(&mut harness, stream, vec![Answering::This(Outcome::Output(output.clone()))]);

    let input = say_input();
    assert_eq!(down, Down::Call { call: 1, revision: 1, tool: String::from("say"), input, within: Within::Unbounded });
    let answered = harness.answer(calling)?;
    assert_eq!(outcome(&mut harness, answered), Answered::Yes(Outcome::Output(output)));

    Ok(())
}

#[test]
fn a_called_that_comes_once_its_call_has_lapsed_is_passed_over() -> Result<(), Unheard> {
    // Catches a late `Called` matched to the call out now, which would answer it with the lapsed call's output, a
    // late frame read as out of turn, which would end the engine's link, and an answer that reaches anyone twice.
    let (mut harness, _, stream, engine) = lapsed()?;
    let (late, output) = (bytes_of(&Said { length: 0 }), bytes_of(&Said { length: 5 }));

    let call = say_call(&mut harness)?;
    let calling = harness.ask(engine, &call)?;
    let answers = vec![
        Answering::Lapsed { call: 0, called: Outcome::Output(late) },
        Answering::This(Outcome::Output(output.clone())),
    ];
    let _ = answered_call(&mut harness, stream, answers);

    let answered = harness.answer(calling)?;
    assert_eq!(outcome(&mut harness, answered), Answered::Yes(Outcome::Output(output)));
    assert_eq!(harness.rest(), Vec::new());

    Ok(())
}

/// A hub played by a listener on a port of its choosing, and that port.
fn hub_socket() -> (TcpListener, u16) {
    let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
        panic!("the hub's side binds");
    };
    let Ok(address) = listener.local_addr() else {
        panic!("the hub's side has a port");
    };
    assert!(listener.set_nonblocking(true).is_ok(), "the hub's side accepts without waiting");

    (listener, address.port())
}

/// The token the tests' engines link with.
const TOKEN: Token = Token([0xA5; 16]);

/// An engine linking to the hub at `port` with `settings`, and the tests' token.
fn linking(port: u16, settings: LinkSettings) -> Result<Harness<Headless>, Unheard> {
    let settings = LinkSettings {
        hub: String::from("127.0.0.1"),
        port,
        token: Vec::from(TOKEN.0),
        most_bytes: MOST_BYTES,
        ..settings
    };

    Harness::of(Headless::program().with_engine(engine()).root::<Tcp>().root::<Tools>().root_with::<Link>(settings))
}

/// The next connection `Link` dials to `listener`, the harness stepping the host until it has.
fn accepted(harness: &mut Harness<Headless>, listener: &TcpListener) -> TcpStream {
    let mut accepted = None;
    waiting::until(BUDGET, || {
        harness.step();
        if accepted.is_none() {
            accepted = listener.accept().ok();
        }
        accepted.is_some()
    });
    let Some((stream, _)) = accepted else {
        panic!("`Link` dials the hub");
    };

    timed(stream)
}

/// Offers the engine's `Tools` the tool `say`, the test its worker.
fn offer_say(harness: &mut Harness<Headless>) -> Result<(), Unheard> {
    let tools = harness.root::<Tools>();
    let worker = harness.protocol::<Tooling>();
    let offering = harness.ask(tools, &Offer { tools: vec![sheet("say")], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    Ok(())
}

/// The frames of a connection's start: `Hello` from `name` with the tests' token, then the set `tools` of `revision`.
fn start(name: &str, revision: u64, tools: Vec<ToolSheet>) -> Vec<Up> {
    vec![hello(name, TOKEN), Up::Sheets { revision, tools }]
}

/// Whether the program `harness` steps has stopped, once it has, the harness stepping the host until then.
fn stops(harness: &mut Harness<Headless>) -> bool {
    waiting::until(BUDGET, || {
        harness.step();
        harness.host().engine().lifecycle() == Lifecycle::Stopped
    });

    harness.host().engine().lifecycle() == Lifecycle::Stopped
}

#[test]
fn link_dials_its_hub_and_says_hello_with_its_token_then_its_set() -> Result<(), Unheard> {
    // Catches a dial not made as `Link` begins, `Hello` not first or naming another revision or name, a `Hello` that
    // does not bring the token the engine was started with, and a set not written once linked.
    let (listener, port) = hub_socket();
    let mut harness = linking(port, LinkSettings { name: String::from("scene"), ..LinkSettings::default() })?;

    let stream = accepted(&mut harness, &listener);
    let (frames, _stream) = frames::<Up>(&mut harness, stream, 2);

    assert_eq!(frames, start("scene", 0, Vec::new()));

    Ok(())
}

#[test]
fn each_change_while_linked_writes_the_whole_set_at_a_higher_revision() -> Result<(), Unheard> {
    // Catches an offer to `Tools` or a withdrawal from it, once `Link` has written its first set, that `Link` never
    // hears and so never writes, a set written as a change instead of whole, and a revision not raised, with which the
    // hub would convert a call by a set the engine no longer holds. A withdrawal's turn may send nothing, so it catches
    // the set it changed never written too.
    let (listener, port) = hub_socket();
    let mut harness = linking(port, LinkSettings::default())?;
    let stream = accepted(&mut harness, &listener);
    let (_, stream) = frames::<Up>(&mut harness, stream, 2);

    offer_say(&mut harness)?;
    let (offered, stream) = frames::<Up>(&mut harness, stream, 1);
    assert_eq!(offered, vec![Up::Sheets { revision: 1, tools: vec![sheet("say")] }]);

    let (tools, worker) = (harness.root::<Tools>(), harness.protocol::<Tooling>());
    harness.tell(tools, &Withdraw { tool: String::from("say"), worker })?;
    let (withdrawn, _stream) = frames::<Up>(&mut harness, stream, 1);
    assert_eq!(withdrawn, vec![Up::Sheets { revision: 2, tools: Vec::new() }]);

    Ok(())
}

#[test]
fn a_call_from_the_hub_reaches_its_worker_and_the_answer_goes_back_up() -> Result<(), Unheard> {
    // Catches a call asked of no worker or with its input changed, and an answer written under another call's number
    // or not at all.
    let (listener, port) = hub_socket();
    let mut harness = linking(port, LinkSettings::default())?;
    let stream = accepted(&mut harness, &listener);
    let (_, stream) = frames::<Up>(&mut harness, stream, 2);
    offer_say(&mut harness)?;
    let (_, mut stream) = frames::<Up>(&mut harness, stream, 1);
    let input = bytes_of(&Say { text: String::from("hello") });
    let output = bytes_of(&Said { length: 5 });

    let within = Within::Unbounded;
    send(&mut stream, &Down::Call { call: 9, revision: 1, tool: String::from("say"), input: input.clone(), within });
    let call = harness.asked::<Call>()?;
    assert_eq!(call.request.tool, "say");
    assert_eq!(harness.read(&call.request.input), Some(&input[..]));
    let held = harness.put(output.clone())?;
    harness.reply(call, &Called::Output(held))?;

    let (answered, _stream) = frames::<Up>(&mut harness, stream, 1);
    assert_eq!(answered, vec![Up::Called { call: 9, called: Outcome::Output(output) }]);

    Ok(())
}

#[test]
fn a_call_of_a_changed_set_or_no_tool_is_refused_without_reaching_a_worker() -> Result<(), Unheard> {
    // Catches a call of an older set handed to the worker, which would read bytes written for another schema, and a
    // call of a tool the engine does not hold left unanswered.
    let (listener, port) = hub_socket();
    let mut harness = linking(port, LinkSettings::default())?;
    let stream = accepted(&mut harness, &listener);
    let (_, stream) = frames::<Up>(&mut harness, stream, 2);
    offer_say(&mut harness)?;
    let (_, mut stream) = frames::<Up>(&mut harness, stream, 1);
    let input = bytes_of(&Say { text: String::from("hello") });

    let within = Within::Unbounded;
    send(&mut stream, &Down::Call { call: 4, revision: 0, tool: String::from("say"), input: input.clone(), within });
    send(&mut stream, &Down::Call { call: 5, revision: 1, tool: String::from("nothing"), input, within });
    let (answered, _stream) = frames::<Up>(&mut harness, stream, 2);

    let refused =
        |frame: &Up, number: u64| matches!(frame, Up::Called { call, called: Outcome::Refused(_) } if *call == number);
    assert!(refused(&answered[0], 4), "{answered:?}");
    assert!(refused(&answered[1], 5), "{answered:?}");
    assert_eq!(harness.rest(), Vec::new());

    Ok(())
}

#[test]
fn a_call_out_of_time_is_refused_and_an_unbounded_one_waits() -> Result<(), Unheard> {
    // Catches `Link` asking its worker with no deadline, so a tool that never answers holds the engine's slot and the
    // hub's for good, a deadline shorter than the time the frame gave, a refusal that does not say the tool ran out of
    // time, an unbounded call given a deadline of none at all, and an answer that comes once its call was
    // refused written as a second answer.
    let (listener, port) = hub_socket();
    let mut harness = linking(port, LinkSettings::default())?;
    let stream = accepted(&mut harness, &listener);
    let (_, stream) = frames::<Up>(&mut harness, stream, 2);
    offer_say(&mut harness)?;
    let (_, mut stream) = frames::<Up>(&mut harness, stream, 1);
    let (quick, patient) =
        (bytes_of(&Say { text: String::from("quick") }), bytes_of(&Say { text: String::from("patient") }));
    let (late, output) = (bytes_of(&Said { length: 5 }), bytes_of(&Said { length: 7 }));

    #[allow(clippy::disallowed_methods)] // A lower bound on real time: the refusal comes no sooner than its time.
    let sent = Instant::now();
    let within = Within::Millis(WITHIN_MILLIS);
    send(&mut stream, &Down::Call { call: 9, revision: 1, tool: String::from("say"), input: quick.clone(), within });
    let within = Within::Unbounded;
    send(&mut stream, &Down::Call { call: 10, revision: 1, tool: String::from("say"), input: patient.clone(), within });
    let first = harness.asked::<Call>()?;
    let second = harness.asked::<Call>()?;
    assert_eq!(harness.read(&first.request.input), Some(&quick[..]));
    assert_eq!(harness.read(&second.request.input), Some(&patient[..]));

    let (refused, stream) = frames::<Up>(&mut harness, stream, 1);
    let waited = sent.elapsed();
    let in_time = Outcome::Refused(String::from("the tool did not answer in time"));
    assert_eq!(refused, vec![Up::Called { call: 9, called: in_time }]);
    assert!(waited >= Duration::from_millis(WITHIN_MILLIS), "refused after {waited:?}");

    let (late, held) = (harness.put(late)?, harness.put(output.clone())?);
    harness.reply(first, &Called::Output(late))?;
    harness.reply(second, &Called::Output(held))?;
    let (answered, _stream) = frames::<Up>(&mut harness, stream, 1);
    assert_eq!(answered, vec![Up::Called { call: 10, called: Outcome::Output(output) }]);

    Ok(())
}

#[test]
fn when_the_hub_hangs_up_link_ends_its_program() -> Result<(), Unheard> {
    // Catches an engine that outlives its link: one left running with a spent token, which no hub can take, and so
    // linked to nothing for good.
    let (listener, port) = hub_socket();
    let mut harness = linking(port, LinkSettings::default())?;
    let stream = accepted(&mut harness, &listener);
    let (_, stream) = frames::<Up>(&mut harness, stream, 2);

    drop(stream);

    assert!(stops(&mut harness), "the program stops once the hub hangs up");

    Ok(())
}

#[test]
fn when_the_hub_refuses_its_token_link_ends_its_program() -> Result<(), Unheard> {
    // Catches a refusal read and passed over, which leaves the engine running unlinked, waiting for a hub that will
    // never take it.
    let (listener, port) = hub_socket();
    let mut harness = linking(port, LinkSettings::default())?;
    let stream = accepted(&mut harness, &listener);
    let (_, mut stream) = frames::<Up>(&mut harness, stream, 2);

    send(&mut stream, &Down::Refused(Refusal::Token));

    assert!(stops(&mut harness), "the program stops once the hub refuses it");

    Ok(())
}

#[test]
fn an_engine_started_without_a_token_fails_as_it_begins_naming_the_setting() {
    // Catches an engine that starts with no token and dials a hub that can only refuse it, and a failure that does
    // not say which setting to give.
    let link = LinkSettings { most_bytes: MOST_BYTES, ..LinkSettings::default() };
    let program = Headless::program().with_engine(engine()).root::<Tcp>().root::<Tools>().root_with::<Link>(link);

    let built = Harness::of(program);

    let Err(Unheard::Program(why)) = built else {
        panic!("the program does not start");
    };
    assert!(why.contains("--hub-token"), "{why}");
}
