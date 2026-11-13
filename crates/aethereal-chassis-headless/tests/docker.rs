//! The Docker client on the native host (ADR-0057): a daemon faked by a Unix socket that writes Docker's canned
//! replies, and, run by hand, the build host's own daemon.
#![cfg(unix)]

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, More, Number, Open, Part,
    ProtocolRef, Streamed, Streams, ToAny, Top, Turn,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Plain};
use aethereal_http::{Http, HttpSettings, Unreached};
use aethereal_sessions::{
    Access, Acted, Attach, Attached, Attaching, Bind, ContainerId, ContainerSpec, ContainerState, Create, Created,
    Detached, Docker, DockerError, DockerSettings, Event, Events, EventsDone, Inspect, Inspected, Label, List, Listed,
    Logging, Logs, LogsDone, Network, Phase, Remove, Start, Std, Stdin, Stop, Until, Watching,
};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{HungUp, Shut, Tcp, Write, Writing, Wrote};
use core::fmt::Debug;
use std::io::{ErrorKind, Read as _, Write as _};
use std::num::NonZeroU32;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs, process};

/// How many parts a session's streams are kept ahead of it.
const WINDOW: u32 = 2;

/// An id as Docker writes one.
const ID: &str = "4f9a0c1e2b3d4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f90";

/// How a session takes the parts of its logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Pace {
    /// It holds back the credit of each part it takes: a reader that has fallen behind.
    Holding,
    /// It grants the credit of each part as it takes it.
    Taking,
}

/// What the test tells a session.
#[derive(Bound)]
#[kind(name = "aether.test.docker.order")]
enum Order {
    /// Attach to a container through `docker`.
    Attach { docker: ActorRef<Docker>, attach: Attach },
    /// Read a container's logs through `docker`, taking their parts at `pace`.
    Logs { docker: ActorRef<Docker>, logs: Logs, pace: Pace },
    /// Follow the events through `docker`.
    Events { docker: ActorRef<Docker>, events: Events },
    /// Grant the credit held back, and take every later part as it comes.
    Release,
}

/// A part a session took.
#[derive(Debug, Bound)]
enum Heard {
    /// An attach's stdin.
    Stdin(ProtocolRef<Writing>),
    /// Output, of an attach or of the logs.
    Output(Std, Vec<u8>),
    Event(Event),
}

/// What a session's begin is given: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.docker.unset")]
struct Unset;

/// A root that opens the streams the test has it open, and keeps what they carry, as the sessions capability will: a
/// stream is opened and its parts taken only by [`Streams`], which only an actor holds.
#[derive(Bound)]
#[kind(name = "aether.test.docker.session")]
struct Session {
    attaching: Streams<Attaching, ()>,
    logging: Streams<Logging, ()>,
    watching: Streams<Watching, ()>,
    pace: Pace,
    /// The stream of each part of the logs whose credit it holds back.
    held: Vec<Number>,
    heard: Vec<Heard>,
    detached: Vec<Detached>,
    logs_done: Vec<LogsDone>,
    events_done: Vec<EventsDone>,
}

impl ChildOf for Session {
    type Parent = Top;
}

impl Actor for Session {
    type Config = Unset;
    type Key = ();
    type Handles = (
        Order,
        Part<Attaching>,
        Part<Logging>,
        Part<Watching>,
        Answer<Detached, Number>,
        Answer<LogsDone, Number>,
        Answer<EventsDone, Number>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Unset, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let Some(window) = NonZeroU32::new(WINDOW) else {
            return Err(Fault::new(&Unset)?);
        };

        Ok(Self {
            attaching: Streams::new(window),
            logging: Streams::new(window),
            watching: Streams::new(window),
            pace: Pace::Taking,
            held: Vec::new(),
            heard: Vec::new(),
            detached: Vec::new(),
            logs_done: Vec::new(),
            events_done: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Session {
    const SECTION: &'static str = "session";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Unset)
    }
}

impl Handles<Order> for Session {
    type Sends = (
        Asks<Streamed<Attaching>, Open<Attaching>, Number>,
        ToAny<Streamed<Attaching>, More>,
        Asks<Streamed<Logging>, Open<Logging>, Number>,
        ToAny<Streamed<Logging>, More>,
        Asks<Streamed<Watching>, Open<Watching>, Number>,
        ToAny<Streamed<Watching>, More>,
    );

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match order {
            Order::Attach { docker, attach } => {
                self.attaching.open(turn, docker.protocol(), attach, ())?;
            }
            Order::Logs { docker, logs, pace } => {
                self.pace = pace;
                self.logging.open(turn, docker.protocol(), logs, ())?;
            }
            Order::Events { docker, events } => {
                self.watching.open(turn, docker.protocol(), events, ())?;
            }
            Order::Release => {
                self.pace = Pace::Taking;
                for number in self.held.drain(..) {
                    self.logging.grant(turn, number, NonZeroU32::MIN)?;
                }
            }
        }

        Ok(())
    }
}

impl Handles<Part<Attaching>> for Session {
    type Sends = (ToAny<Streamed<Attaching>, More>,);

    fn handle(&mut self, part: Part<Attaching>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(taken) = self.attaching.part(turn, part)? else {
            return Ok(());
        };
        let heard = match taken.value {
            Attached::Stdin(writing) => Heard::Stdin(writing),
            Attached::Output(output) => Heard::Output(output.stream, turn.read(&output.bytes).to_vec()),
        };
        self.heard.push(heard);

        Ok(())
    }
}

impl Handles<Part<Logging>> for Session {
    type Sends = (ToAny<Streamed<Logging>, More>,);

    fn handle(&mut self, part: Part<Logging>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let taken = match self.pace {
            Pace::Holding => self.logging.part_held(turn, part),
            Pace::Taking => self.logging.part(turn, part)?,
        };
        let Some(taken) = taken else {
            return Ok(());
        };
        if self.pace == Pace::Holding {
            self.held.push(taken.number);
        }
        let output = taken.value;
        self.heard.push(Heard::Output(output.stream, turn.read(&output.bytes).to_vec()));

        Ok(())
    }
}

impl Handles<Part<Watching>> for Session {
    type Sends = (ToAny<Streamed<Watching>, More>,);

    fn handle(&mut self, part: Part<Watching>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(taken) = self.watching.part(turn, part)? else {
            return Ok(());
        };
        self.heard.push(Heard::Event(taken.value));

        Ok(())
    }
}

impl Handles<Answer<Detached, Number>> for Session {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, number): (Answered<Detached>, Number),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.attaching.answered(number);
        if let Answered::Yes(detached) = answered {
            self.detached.push(detached);
        }

        Ok(())
    }
}

impl Handles<Answer<LogsDone, Number>> for Session {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, number): (Answered<LogsDone>, Number),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.logging.answered(number);
        if let Answered::Yes(done) = answered {
            self.logs_done.push(done);
        }

        Ok(())
    }
}

impl Handles<Answer<EventsDone, Number>> for Session {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, number): (Answered<EventsDone>, Number),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.watching.answered(number);
        if let Answered::Yes(done) = answered {
            self.events_done.push(done);
        }

        Ok(())
    }
}

impl Session {
    /// Every byte of output it has heard on `stream`, in order.
    fn output(&self, stream: Std) -> Vec<u8> {
        let pieces = self.heard.iter().filter_map(|heard| match heard {
            Heard::Output(on, bytes) if *on == stream => Some(bytes.as_slice()),
            Heard::Output(..) | Heard::Stdin(_) | Heard::Event(_) => None,
        });

        pieces.flatten().copied().collect()
    }

    /// Every event it has heard, in order.
    fn events(&self) -> Vec<Event> {
        let events = self.heard.iter().filter_map(|heard| match heard {
            Heard::Event(event) => Some(event.clone()),
            Heard::Output(..) | Heard::Stdin(_) => None,
        });

        events.collect()
    }

    /// The stdin of the latest attach it has heard of.
    fn stdin(&self) -> Option<ProtocolRef<Writing>> {
        self.heard.iter().rev().find_map(|heard| match heard {
            Heard::Stdin(writing) => Some(*writing),
            Heard::Output(..) | Heard::Event(_) => None,
        })
    }
}

/// A host running `Tcp`, `Http`, a Docker client on `docker`'s socket, and a session.
fn host(http: HttpSettings, docker: DockerSettings) -> Result<Harness<Headless>, Unheard> {
    let engine = EngineSettings {
        read_limit: http.most_read_bytes().try_into().unwrap_or(u32::MAX),
        ..EngineSettings::default()
    };
    let program = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root_with::<Http>(http)
        .root_with::<Docker>(docker)
        .root::<Session>();

    Harness::of(program)
}

/// The session's state once `done` holds of it, the harness stepping the host until it does or the wait is spent.
fn heard(harness: &mut Harness<Headless>, mut done: impl FnMut(&Session) -> bool) -> Result<&Session, Unheard> {
    let session = harness.root::<Session>();
    waiting::until(BUDGET, || {
        harness.step();

        harness.state(session).is_ok_and(&mut done)
    });

    harness.state(session)
}

/// A daemon faked on a Unix socket in a directory named for the test, removed when dropped.
struct Daemon {
    dir: PathBuf,
    listener: UnixListener,
}

impl Daemon {
    fn new(test: &str) -> Self {
        let dir = env::temp_dir().join(format!("aethereal-docker-{}-{test}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(fs::create_dir_all(&dir).is_ok(), "the test makes its directory");
        let Ok(listener) = UnixListener::bind(dir.join("docker.sock")) else {
            panic!("the test binds its socket");
        };
        assert!(listener.set_nonblocking(true).is_ok(), "the listener's accepts return at once");

        Self { dir, listener }
    }

    /// The Docker client's settings, naming the socket.
    fn settings(&self) -> DockerSettings {
        DockerSettings { socket: self.dir.join("docker.sock").display().to_string(), ..DockerSettings::default() }
    }

    /// The connection the client dials next, the harness stepping the host until it comes, its reads and writes
    /// returning at once.
    fn accept(&self, harness: &mut Harness<Headless>) -> UnixStream {
        let mut accepted = None;
        waiting::until(BUDGET, || {
            harness.step();
            accepted = self.listener.accept().ok().map(|(peer, _)| peer);

            accepted.is_some()
        });
        let Some(peer) = accepted else {
            panic!("the client dials the daemon");
        };
        assert!(peer.set_nonblocking(true).is_ok(), "the peer's calls return at once");

        peer
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A request the daemon read: its head, and its body by its `Content-Length`.
struct Asked {
    head: String,
    body: Vec<u8>,
}

/// The request the peer reads, the harness stepping the host until it has all come.
fn request(harness: &mut Harness<Headless>, peer: &mut UnixStream) -> Asked {
    let mut read = Vec::new();
    let mut chunk = [0; 4096];
    waiting::until(BUDGET, || {
        harness.step();
        if let Ok(count) = peer.read(&mut chunk) {
            read.extend_from_slice(&chunk[..count]);
        }

        split(&read).is_some()
    });
    let Some(asked) = split(&read) else {
        panic!("the request came whole: {}", String::from_utf8_lossy(&read));
    };

    asked
}

/// The request `read` holds, once its head and its body have all come.
fn split(read: &[u8]) -> Option<Asked> {
    let end = read.windows(4).position(|window| window == b"\r\n\r\n")? + 4;
    let head = String::from_utf8_lossy(&read[..end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .map_or(Some(0), |length| length.trim().parse::<usize>().ok())?;
    let body = read.get(end..end + length)?.to_vec();

    Some(Asked { head, body })
}

/// Writes `bytes` to the peer, whose socket holds far more than any test writes this way.
fn send(peer: &mut UnixStream, bytes: &[u8]) {
    assert!(peer.write_all(bytes).is_ok(), "the peer writes");
}

/// A response of `status` with a JSON `body`, as Docker writes one.
fn json(status: &str, body: &str) -> Vec<u8> {
    let length = body.len();

    format!("HTTP/1.1 {status}\r\nApi-Version: 1.55\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n\r\n{body}")
        .into_bytes()
}

/// Whether the client hung up the peer's connection, the harness stepping the host until it does or the wait is
/// spent.
fn hung_up(harness: &mut Harness<Headless>, peer: &mut UnixStream) -> bool {
    let mut byte = [0; 1];
    let mut closed = false;
    waiting::until(BUDGET, || {
        harness.step();
        closed = matches!(peer.read(&mut byte), Ok(0));

        closed
    });

    closed
}

/// A frame of Docker's on `stream`, carrying `payload`.
fn frame(stream: u8, payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(payload.len()).unwrap_or(u32::MAX).to_be_bytes();

    [&[stream, 0, 0, 0][..], &size, payload].concat()
}

/// `bytes` as one chunk of a chunked body.
fn chunk(bytes: &[u8]) -> Vec<u8> {
    [format!("{:x}\r\n", bytes.len()).as_bytes(), bytes, b"\r\n"].concat()
}

fn id() -> ContainerId {
    ContainerId::read(ID).unwrap_or_else(|| panic!("the id is 64 hex digits"))
}

/// A container a test makes: `cat`, its stdin kept, labelled `aethereal.test=1` and `label`.
fn spec(name: &str, label: &Label) -> ContainerSpec {
    ContainerSpec {
        name: String::from(name),
        image: String::from("alpine:latest"),
        user: String::from("65534:65534"),
        working_dir: String::from("/"),
        command: vec![String::from("cat")],
        env: Vec::new(),
        binds: Vec::new(),
        network: Network::Isolated,
        millicpus: 500,
        memory_bytes: 64 << 20,
        labels: vec![Label { key: String::from("aethereal.test"), value: String::from("1") }, label.clone()],
        stdin: Stdin::Kept,
    }
}

/// The label of the containers the tests make.
fn probe() -> Label {
    Label { key: String::from("aethereal.probe"), value: String::from("fake") }
}

#[test]
fn requests_go_to_the_pinned_api_each_on_a_connection_hung_up_once_its_reply_is_read() -> Result<(), Unheard> {
    // Catches a request written to another version, method, or endpoint than Docker documents, a create's body or its
    // content type not sent, a reply read as the wrong answer, and a connection kept once its reply is read, which
    // would hold one of `Http`'s clients for every request ever made.
    let daemon = Daemon::new("whole");
    let mut harness = host(HttpSettings::default(), daemon.settings())?;
    let docker = harness.root::<Docker>();

    let creating = harness.ask(docker, &Create { spec: spec("probe", &probe()) })?;
    let mut peer = daemon.accept(&mut harness);
    let asked = request(&mut harness, &mut peer);
    assert!(asked.head.starts_with("POST /v1.44/containers/create?name=probe HTTP/1.1\r\n"), "{}", asked.head);
    assert!(asked.head.contains("\r\ncontent-type: application/json\r\n"), "{}", asked.head);
    assert!(asked.body.starts_with(br#"{"Image":"alpine:latest","User":"65534:65534""#), "{:?}", asked.body);
    send(&mut peer, &json("201 Created", &format!(r#"{{"Id":"{ID}","Warnings":[]}}"#)));
    assert_eq!(harness.answer(creating)?, Answered::Yes(Created::Made(id())));
    assert!(hung_up(&mut harness, &mut peer), "the create's connection is hung up");

    let starting = harness.ask(docker, &Start { container: id() })?;
    let mut peer = daemon.accept(&mut harness);
    let asked = request(&mut harness, &mut peer);
    assert!(asked.head.starts_with(&format!("POST /v1.44/containers/{ID}/start HTTP/1.1\r\n")), "{}", asked.head);
    send(&mut peer, b"HTTP/1.1 204 No Content\r\nApi-Version: 1.55\r\n\r\n");
    assert_eq!(harness.answer(starting)?, Answered::Yes(Acted::Done));
    assert!(hung_up(&mut harness, &mut peer), "the start's connection is hung up");

    let removing = harness.ask(docker, &Remove { container: id() })?;
    let mut peer = daemon.accept(&mut harness);
    let asked = request(&mut harness, &mut peer);
    let line = format!("DELETE /v1.44/containers/{ID}?force=1&v=1 HTTP/1.1\r\n");
    assert!(asked.head.starts_with(&line), "{}", asked.head);
    send(&mut peer, &json("404 Not Found", &format!(r#"{{"message":"No such container: {ID}"}}"#)));
    let missing = DockerError::NotFound { message: format!("No such container: {ID}") };
    assert_eq!(harness.answer(removing)?, Answered::Yes(Acted::Failed(missing)));

    Ok(())
}

#[test]
fn a_daemon_not_there_is_named_missing_and_one_not_listening_refused() -> Result<(), Unheard> {
    // Catches a socket that cannot be reached answered as some other failure, or never answered, so a hub cannot tell
    // a daemon not installed from one stopped.
    let daemon = Daemon::new("unreached");
    let settings = daemon.settings();
    let socket = PathBuf::from(&settings.socket);
    assert!(fs::remove_file(&socket).is_ok(), "the test takes its socket's file away");
    let mut harness = host(HttpSettings::default(), settings)?;
    let docker = harness.root::<Docker>();

    let listing = harness.ask(docker, &List { label: probe() })?;
    let missing = Listed::Failed(DockerError::Unreachable(Unreached::Missing));
    assert_eq!(harness.answer(listing)?, Answered::Yes(missing));

    drop(UnixListener::bind(&socket));
    let listing = harness.ask(docker, &List { label: probe() })?;
    let refused = Listed::Failed(DockerError::Unreachable(Unreached::Refused));
    assert_eq!(harness.answer(listing)?, Answered::Yes(refused));

    Ok(())
}

#[test]
fn an_attach_hands_over_stdin_and_takes_its_output_apart_however_the_frames_are_cut() -> Result<(), Unheard> {
    // Catches an attach that does not ask to take the connection over; output that came in the read with the head
    // lost; a frame cut between reads lost or misread; stdout and stderr crossed; a write through the stdin handed
    // over not reaching the container; and an attach that does not end when Docker hangs up.
    let daemon = Daemon::new("attach");
    let mut harness = host(HttpSettings::default(), daemon.settings())?;
    let docker = harness.root::<Docker>();
    let session = harness.root::<Session>();
    harness.tell(session, &Order::Attach { docker, attach: Attach { container: id() } })?;
    let mut peer = daemon.accept(&mut harness);
    let asked = request(&mut harness, &mut peer);
    let line = format!("POST /v1.44/containers/{ID}/attach?stream=1&stdin=1&stdout=1&stderr=1 HTTP/1.1\r\n");
    assert!(asked.head.starts_with(&line), "{}", asked.head);
    assert!(asked.head.contains("\r\nconnection: upgrade\r\nupgrade: tcp\r\n"), "{}", asked.head);

    let frames = [frame(1, b"hello\n"), frame(2, b"oops\n"), frame(1, b"world\n")].concat();
    let upgraded = b"HTTP/1.1 101 UPGRADED\r\nContent-Type: application/vnd.docker.multiplexed-stream\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n";
    // The head, and the first frame's header cut after its third byte.
    send(&mut peer, &[upgraded.as_slice(), &frames[..3]].concat());
    heard(&mut harness, |session| session.stdin().is_some())?;
    // The rest of the first frame, and the second cut two bytes into its payload.
    send(&mut peer, &frames[3..24]);
    let early = heard(&mut harness, |session| session.output(Std::Stderr).len() == 2)?.output(Std::Stderr);
    assert_eq!(early, b"oo", "a payload is handed on as it comes");
    send(&mut peer, &frames[24..]);
    let session_state = heard(&mut harness, |session| session.output(Std::Stdout) == b"hello\nworld\n")?;
    assert!(matches!(session_state.heard.first(), Some(Heard::Stdin(_))), "stdin comes first");
    assert_eq!(session_state.output(Std::Stdout), b"hello\nworld\n");
    assert_eq!(session_state.output(Std::Stderr), b"oops\n");
    let Some(stdin) = session_state.stdin() else {
        panic!("the attach hands over stdin");
    };

    let parts = vec![harness.put(b"typed\n".to_vec())?];
    let wrote = harness.ask(stdin, &Write { parts })?;
    assert_eq!(harness.answer(wrote)?, Answered::Yes(Wrote::Sent));
    let mut written = [0; 6];
    waiting::until(BUDGET, || peer.read_exact(&mut written).is_ok());
    assert_eq!(&written, b"typed\n", "a write to stdin reaches the container");

    drop(peer);
    let session_state = heard(&mut harness, |session| !session.detached.is_empty())?;
    assert_eq!(session_state.detached, [Detached::HungUp(HungUp::Peer)]);

    Ok(())
}

#[test]
fn an_attach_docker_refuses_before_taking_the_connection_fails_with_its_error() -> Result<(), Unheard> {
    // Catches an error Docker answers before the switch read as the container's output, or the attach left open.
    let daemon = Daemon::new("refused");
    let mut harness = host(HttpSettings::default(), daemon.settings())?;
    let docker = harness.root::<Docker>();
    let session = harness.root::<Session>();
    harness.tell(session, &Order::Attach { docker, attach: Attach { container: id() } })?;
    let mut peer = daemon.accept(&mut harness);
    request(&mut harness, &mut peer);

    let message = format!("container {ID} is not running");
    send(&mut peer, &json("409 Conflict", &format!(r#"{{"message":"{message}"}}"#)));
    let session_state = heard(&mut harness, |session| !session.detached.is_empty())?;
    assert_eq!(session_state.detached, [Detached::Failed(DockerError::Conflict { message })]);
    assert!(session_state.heard.is_empty(), "nothing of the error is output: {:?}", session_state.heard);

    Ok(())
}

#[test]
fn events_are_followed_as_their_lines_arrive_and_end_when_docker_ends_them() -> Result<(), Unheard> {
    // Catches events asked without their filters, so a hub would follow every container on the daemon; a line cut
    // between chunks lost or read early; and the stream left open once Docker ends it.
    let daemon = Daemon::new("events");
    let mut harness = host(HttpSettings::default(), daemon.settings())?;
    let docker = harness.root::<Docker>();
    let session = harness.root::<Session>();
    harness.tell(session, &Order::Events { docker, events: Events { label: probe() } })?;
    let mut peer = daemon.accept(&mut harness);
    let asked = request(&mut harness, &mut peer);
    assert!(asked.head.starts_with("GET /v1.44/events?filters=%7B%22type%22"), "{}", asked.head);

    let started = format!(r#"{{"Type":"container","Action":"start","Actor":{{"ID":"{ID}","Attributes":{{}}}}}}"#);
    let died =
        format!(r#"{{"Type":"container","Action":"die","Actor":{{"ID":"{ID}","Attributes":{{"exitCode":"137"}}}}}}"#);
    let lines = format!("{started}\n{died}\n");
    let (first, second) = lines.as_bytes().split_at(started.len() + 20);
    send(
        &mut peer,
        &[
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n",
            &chunk(first)[..],
        ]
        .concat(),
    );
    heard(&mut harness, |session| session.events().len() == 1)?;
    send(&mut peer, &[chunk(second), b"0\r\n\r\n".to_vec()].concat());
    let session_state = heard(&mut harness, |session| !session.events_done.is_empty())?;

    let expected = [Event::Started { container: id() }, Event::Died { container: id(), code: 137 }];
    assert_eq!(session_state.events(), expected);
    assert_eq!(session_state.events_done, [EventsDone::Closed]);

    Ok(())
}

/// Whether a peer's write went in, or would wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Written {
    /// It wrote some bytes, or had none left to write.
    Took,
    /// The socket would wait: the kernel holds all it will.
    Blocked,
}

/// Writes what the socket takes of `payload` from `sent` on, counting in `sent` what it took.
fn write_some(peer: &mut UnixStream, payload: &[u8], sent: &mut usize) -> Written {
    match peer.write(&payload[*sent..]) {
        Ok(count) => {
            *sent += count;
            Written::Took
        }
        Err(error) if error.kind() == ErrorKind::WouldBlock => Written::Blocked,
        Err(error) => panic!("the peer writes: {error}"),
    }
}

#[test]
fn logs_whose_reader_holds_its_credit_hold_docker_back_until_it_takes_more() -> Result<(), Unheard> {
    // Catches a client that keeps reading Docker while its asker has no credit, gathering the logs itself, so a
    // session that falls behind does not slow Docker and the client holds what Docker wrote without bound; one that,
    // held, never reads again once credit comes; and output lost or reordered across the parts between.
    let http = HttpSettings { chunk_bytes: 1024, window: 1, ..HttpSettings::default() };
    let daemon = Daemon::new("logs");
    let settings = DockerSettings { window: 1, ..daemon.settings() };
    let mut harness = host(http, settings)?;
    let docker = harness.root::<Docker>();
    let session = harness.root::<Session>();
    let logs = Logs { container: id(), since_nanos: 0, until: Until::Now };
    harness.tell(session, &Order::Logs { docker, logs, pace: Pace::Holding })?;
    let mut peer = daemon.accept(&mut harness);
    request(&mut harness, &mut peer);
    // Frames in a pattern no read's length divides, so output lost or read twice shows.
    let output: Vec<u8> = (0..1024 * 1024_u32).map(|index| u8::try_from(index % 251).unwrap_or(0)).collect();
    let frames: Vec<u8> = output.chunks(1000).flat_map(|piece| frame(1, piece)).collect();
    let response = [
        b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.docker.multiplexed-stream\r\nTransfer-Encoding: chunked\r\n\r\n".as_slice(),
        &chunk(&frames),
        b"0\r\n\r\n",
    ]
    .concat();
    let mut sent = 0;

    // The engine runs all it can before each write, so a client that read on without credit would have emptied the
    // socket before Docker is seen held back.
    waiting::until(BUDGET, || {
        harness.settle();
        let blocked = write_some(&mut peer, &response, &mut sent) == Written::Blocked;
        let parts = harness.state(session).map_or(0, |state| state.held.len());
        let out_of_credit = parts == usize::try_from(WINDOW).unwrap_or(usize::MAX);

        blocked && out_of_credit
    });
    let held = harness.state(session)?.held.len();
    assert_eq!(held, usize::try_from(WINDOW).unwrap_or(usize::MAX), "the session took its window, and holds it");
    assert!(sent < response.len(), "Docker is held back once the session's credit is spent: it sent {sent} bytes");

    harness.tell(session, &Order::Release)?;
    waiting::until(BUDGET, || {
        harness.step();
        write_some(&mut peer, &response, &mut sent);

        harness.state(session).is_ok_and(|state| !state.logs_done.is_empty())
    });
    let session_state = harness.state(session)?;
    assert!(session_state.output(Std::Stdout) == output, "every byte is read, in order, once credit comes");
    assert_eq!(session_state.logs_done, [LogsDone::Read]);

    Ok(())
}

/// What a step of the live run found, written for whoever runs it.
#[allow(clippy::print_stderr)] // A run by hand against a real daemon reports what it answered, for a person.
fn report(step: &str, found: &impl Debug) {
    eprintln!("{step}: {found:?}");
}

/// Removes the container named `name`, with a request of its own written by hand on Docker's socket, whatever became
/// of the run: a container the run made never outlives it.
struct Removing(String);

impl Drop for Removing {
    fn drop(&mut self) {
        let Ok(mut socket) = UnixStream::connect("/var/run/docker.sock") else {
            return;
        };
        let request = format!(
            "DELETE /v1.44/containers/{}?force=1&v=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            self.0
        );
        let _ = socket.write_all(request.as_bytes());
        let mut answer = Vec::new();
        let _ = socket.read_to_end(&mut answer);
    }
}

#[test]
#[ignore = "reaches the Docker daemon on this machine's socket, which CI has none of: run by hand with --ignored"]
fn a_container_runs_through_its_life_on_the_machines_own_daemon() -> Result<(), Unheard> {
    // Catches what a fake daemon cannot: a request or a field the real daemon reads otherwise than its documentation
    // says, at the pinned version.
    let name = format!("aethereal-test-{}", process::id());
    let _removing = Removing(name.clone());
    let mounted = env::temp_dir().join(&name);
    assert!(fs::create_dir_all(&mounted).is_ok(), "the run makes the directory it binds");
    let run = Label { key: String::from("aethereal.probe"), value: name.clone() };
    let mut harness = host(HttpSettings::default(), DockerSettings::default())?;
    let docker = harness.root::<Docker>();
    let session = harness.root::<Session>();

    harness.tell(session, &Order::Events { docker, events: Events { label: run.clone() } })?;
    let mut made = spec(&name, &run);
    let target = String::from("/probe");
    made.binds.push(Bind { source: mounted.display().to_string(), target, access: Access::ReadOnly });
    let creating = harness.ask(docker, &Create { spec: made })?;
    let created = harness.answer(creating)?;
    report("create", &created);
    let Answered::Yes(Created::Made(container)) = created else {
        panic!("the container is made");
    };
    let listing = harness.ask(docker, &List { label: run.clone() })?;
    report("list before start", &harness.answer(listing)?);

    #[allow(clippy::disallowed_methods)] // The run asks Docker for the logs from its own start, on the real clock.
    let began = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let since_nanos = u64::try_from(began.as_nanos()).unwrap_or(u64::MAX);
    let starting = harness.ask(docker, &Start { container: container.clone() })?;
    let started = harness.answer(starting)?;
    report("start", &started);
    assert_eq!(started, Answered::Yes(Acted::Done));

    harness.tell(session, &Order::Attach { docker, attach: Attach { container: container.clone() } })?;
    let stdin = heard(&mut harness, |session| session.stdin().is_some())?.stdin();
    let Some(stdin) = stdin else {
        panic!("the attach hands over stdin");
    };
    let parts = vec![harness.put(b"hello\n".to_vec())?];
    let wrote = harness.ask(stdin, &Write { parts })?;
    harness.answer(wrote)?;
    let echoed = heard(&mut harness, |session| session.output(Std::Stdout) == b"hello\n")?.output(Std::Stdout);
    report("attach, wrote hello", &String::from_utf8_lossy(&echoed));
    assert_eq!(echoed, b"hello\n");
    harness.tell(stdin, &Shut)?;
    let detached = heard(&mut harness, |session| !session.detached.is_empty())?.detached.clone();
    report("detach", &detached);

    harness.tell(session, &Order::Attach { docker, attach: Attach { container: container.clone() } })?;
    let stdin = heard(&mut harness, |session| {
        session.heard.iter().filter(|heard| matches!(heard, Heard::Stdin(_))).count() == 2
    })?
    .stdin();
    let Some(stdin) = stdin else {
        panic!("the second attach hands over stdin");
    };
    let parts = vec![harness.put(b"again\n".to_vec())?];
    let wrote = harness.ask(stdin, &Write { parts })?;
    harness.answer(wrote)?;
    let echoed = heard(&mut harness, |session| session.output(Std::Stdout) == b"hello\nagain\n")?.output(Std::Stdout);
    report("attach again, wrote again", &String::from_utf8_lossy(&echoed));
    assert_eq!(echoed, b"hello\nagain\n", "the same process reads the second attach's stdin");
    harness.tell(stdin, &Shut)?;
    heard(&mut harness, |session| session.detached.len() == 2)?;

    let before = harness.state(session)?.output(Std::Stdout).len();
    let logs = Logs { container: container.clone(), since_nanos, until: Until::Now };
    harness.tell(session, &Order::Logs { docker, logs, pace: Pace::Taking })?;
    let session_state = heard(&mut harness, |session| !session.logs_done.is_empty())?;
    let logged = session_state.output(Std::Stdout).split_off(before);
    report("logs since start", &(String::from_utf8_lossy(&logged), &session_state.logs_done));
    assert_eq!(logged, b"hello\nagain\n");
    assert_eq!(session_state.logs_done, [LogsDone::Read]);

    let inspecting = harness.ask(docker, &Inspect { container: container.clone() })?;
    let inspected = harness.answer(inspecting)?;
    report("inspect running", &inspected);
    assert_eq!(inspected, Answered::Yes(Inspected::State(ContainerState::Running)));
    let listing = harness.ask(docker, &List { label: run })?;
    let listed = harness.answer(listing)?;
    report("list running", &listed);
    let Answered::Yes(Listed::Containers(found)) = listed else {
        panic!("the list is read");
    };
    let is_found = found.iter().any(|listing| listing.id == container && listing.phase == Phase::Running);
    assert!(is_found, "the list finds the container running");

    let stopping = harness.ask(docker, &Stop { container: container.clone(), grace_secs: 1 })?;
    let stopped = harness.answer(stopping)?;
    report("stop", &stopped);
    assert_eq!(stopped, Answered::Yes(Acted::Done));
    let events = heard(&mut harness, |session| session.events().len() >= 2)?.events();
    report("events", &events);
    assert!(matches!(events.as_slice(), [Event::Started { .. }, Event::Died { .. }, ..]), "{events:?}");
    let inspecting = harness.ask(docker, &Inspect { container: container.clone() })?;
    let inspected = harness.answer(inspecting)?;
    report("inspect stopped", &inspected);
    assert!(matches!(inspected, Answered::Yes(Inspected::State(ContainerState::Exited { .. }))), "{inspected:?}");

    let removing = harness.ask(docker, &Remove { container: container.clone() })?;
    let removed = harness.answer(removing)?;
    report("remove", &removed);
    assert_eq!(removed, Answered::Yes(Acted::Done));
    let removing = harness.ask(docker, &Remove { container })?;
    report("remove again", &harness.answer(removing)?);
    let _ = fs::remove_dir_all(&mounted);

    Ok(())
}
