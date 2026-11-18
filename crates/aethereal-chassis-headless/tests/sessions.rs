//! The sessions capability on the native host (ADR-0057): a Docker daemon faked by a Unix socket whose containers run
//! a Claude the test speaks for, the sessions' files kept through `Files` in the test's directory, and, run by hand,
//! the build host's own daemon and Claude Code.
#![cfg(unix)]

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anyone, Beginning, ChildOf, Ending, Fault, Handles, Receives, Top, Turn, path,
};
use aethereal_actor::{Service, Spares};
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_chassis_headless::{Directories, FileRoots, Headless};
use aethereal_data::{Bound, Budget, FilePath, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_files::{Files, Listing, Namespace, Reading, Storage, Writing};
use aethereal_http::stack::StackSettings;
use aethereal_http::{Http, HttpSettings};
use aethereal_link::{
    ChatTo, ChatToSessions, Down, Engine, Engines, EnginesSettings, HearTrusted, ListEngines, MintFor, Minted,
    REVISION, TrustedEngine, Up,
};
use aethereal_mcp::{Mcp, McpRoutes};
use aethereal_sessions::{
    Archive, Archived, Chat, Chatted, Conversation, Docker, DockerSettings, Ended, Entry, Index, Interrupt,
    Interrupted, Joined, ListSessions, NewSession, Note, Numbered, Opened, ReadTranscript, Record, Refusal, Said, Say,
    Sessions, SessionsListed, SessionsSettings, SessionsTools, ShowAsk, ShowEvents, ShowTabs, Speaker, Standing, State,
    Transcribed, Transcript,
};
use aethereal_store::{Store, StoreSettings};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{Interface, Tcp};
use aethereal_tools::{Call, Called, Caller as By, Offer, Offered, Output, ToolSheet, Toolbox, Tooling};
use core::fmt::Write as _;
use core::mem;
use core::time::Duration;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::future::Future;
use std::io::{ErrorKind, Read as _, Write as _};
use std::net::TcpStream;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path as FsPath, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};
use std::{env, fs, process, thread};

mod stacks;

use stacks::{bound, stack};

// The sessions' port's stack, as the hub's: `Sessions` lets in each request by the bearer it bears, and `Mcp` is asked
// it on `POST /mcp`.
stack!(
    ContainerStack,
    "test.sessions.container_stack",
    "container_stack",
    (Sessions, McpRoutes),
    [By],
    depends(Sessions, Mcp)
);

/// The session id Claude names its conversation by in these tests.
const CONVERSATION: &str = "0f4c2a9e-5b1d-4e3f-8a7b-6c5d4e3f2a1b";

/// What the test tells its caller to ask of the sessions.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.sessions.order")]
enum Order {
    New(NewSession),
    Say(Say),
    Read(ReadTranscript),
    List,
    Interrupt(Interrupt),
    Archive(Archive),
}

/// What the caller is begun from: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.sessions.unset")]
struct Unset;

/// What the caller keeps for an ask: nothing, as each answer kind says which it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.sessions.asked")]
struct Asked;

/// A root that asks the sessions what the test orders, and keeps every answer, in order: as the hub's tools do.
#[derive(Bound)]
#[kind(name = "aether.test.sessions.caller")]
struct Caller {
    opened: Vec<Opened>,
    said: Vec<Said>,
    read: Vec<Transcribed>,
    listed: Vec<SessionsListed>,
    interrupted: Vec<Interrupted>,
    archived: Vec<Archived>,
}

impl ChildOf for Caller {
    type Parent = Top;
}

impl Actor for Caller {
    type Config = Unset;
    type Key = ();
    type Handles = (
        Order,
        Answer<Opened, Asked>,
        Answer<Said, Asked>,
        Answer<Transcribed, Asked>,
        Answer<SessionsListed, Asked>,
        Answer<Interrupted, Asked>,
        Answer<Archived, Asked>,
    );
    type Depends = (Sessions,);

    fn begin(_: Unset, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self {
            opened: Vec::new(),
            said: Vec::new(),
            read: Vec::new(),
            listed: Vec::new(),
            interrupted: Vec::new(),
            archived: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Caller {
    const SECTION: &'static str = "caller";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Unset)?;

        Ok(())
    }
}

impl Handles<Order> for Caller {
    type From = Anyone;

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let sessions = path::<Sessions>(());
        match order {
            Order::New(new) => turn.to(sessions).ask(&new, &Asked)?,
            Order::Say(say) => turn.to(sessions).ask(&say, &Asked)?,
            Order::Read(read) => turn.to(sessions).ask(&read, &Asked)?,
            Order::List => turn.to(sessions).ask(&ListSessions, &Asked)?,
            Order::Interrupt(interrupt) => turn.to(sessions).ask(&interrupt, &Asked)?,
            Order::Archive(archive) => turn.to(sessions).ask(&archive, &Asked)?,
        };

        Ok(())
    }
}

macro_rules! keeps {
    ($($answer:ty => $field:ident),*) => {$(
        impl Receives<Answer<$answer, Asked>> for Caller {
            fn handle(&mut self, (answered, Asked): (Answered<$answer>, Asked), _: &mut Turn<'_, Self>) -> Result<(), Fault> {
                let Answered::Yes(answer) = answered else {
                    panic!("the sessions answer: {answered:?}");
                };
                self.$field.push(answer);

                Ok(())
            }
        }
    )*};
}

keeps!(
    Opened => opened, Said => said, Transcribed => read, SessionsListed => listed, Interrupted => interrupted,
    Archived => archived
);

/// A container the fake daemon made.
struct Container {
    name: String,
    /// The body of the create that made it.
    body: String,
    running: bool,
    /// Everything its process wrote, in Docker's frames.
    output: Vec<u8>,
    /// Everything written to its stdin.
    stdin: Vec<u8>,
}

/// What a connection to the fake daemon is.
enum Role {
    /// It asks a request, not yet whole.
    Asking,
    /// An attach: what comes on it is the container's stdin.
    Attach(String),
    /// A container's logs, followed: how many bytes of its output were written on it.
    Logs { id: String, sent: usize },
    /// The events: how many were written on it.
    Events { sent: usize },
    /// Answered, or hung up.
    Done,
}

/// One connection to the fake daemon.
struct Connection {
    peer: UnixStream,
    read: Vec<u8>,
    role: Role,
}

/// A Docker daemon faked on a Unix socket in a directory named for the test, removed when dropped, whose containers'
/// output the test writes as Claude would.
struct Daemon {
    dir: PathBuf,
    listener: UnixListener,
    connections: Vec<Connection>,
    containers: BTreeMap<String, Container>,
    /// The ids of the containers made, in the order made.
    made: Vec<String>,
    /// The names of the containers made, in the order made.
    names: Vec<String>,
    /// The bodies of the volumes made, by name.
    volumes: BTreeMap<String, String>,
    /// Every event, a JSON line each.
    events: Vec<String>,
}

impl Daemon {
    fn new(test: &str) -> Self {
        let dir = env::temp_dir().join(format!("aethereal-sessions-{}-{test}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(fs::create_dir_all(&dir).is_ok(), "the test makes its directory");
        let Ok(listener) = UnixListener::bind(dir.join("docker.sock")) else {
            panic!("the test binds its socket");
        };
        assert!(listener.set_nonblocking(true).is_ok(), "the listener's accepts return at once");
        assert!(fs::write(dir.join("claude.env"), "# the test's\nCLAUDE_CODE_OAUTH_TOKEN=not-a-token\n").is_ok());

        Self {
            dir,
            listener,
            connections: Vec::new(),
            containers: BTreeMap::new(),
            made: Vec::new(),
            names: Vec::new(),
            volumes: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    fn docker(&self) -> DockerSettings {
        DockerSettings { socket: self.dir.join("docker.sock").display().to_string(), ..DockerSettings::default() }
    }

    /// The sessions' settings: kept in the test's directory, each container's limit and idle time as `most_running`
    /// and `idle_secs` say.
    fn sessions(&self, most_running: u32, idle_secs: u32) -> SessionsSettings {
        SessionsSettings {
            credential: self.dir.join("claude.env").display().to_string(),
            label: String::from("test"),
            labels: vec![String::from("aethereal.test=1")],
            most_running,
            idle_secs,
            ..SessionsSettings::default()
        }
    }

    /// The store's settings: kept in the test's directory, where each session's drop box is made.
    fn store(&self) -> StoreSettings {
        StoreSettings { root: self.dir.join("store").display().to_string(), ..StoreSettings::default() }
    }

    /// `Files`' settings: each namespace a directory in the test's.
    fn files(&self) -> FileRoots {
        let directory = |name: &str| self.dir.join(name).display().to_string();

        FileRoots { assets: directory("assets"), save: directory("save"), code: directory("code"), readers: 2 }
    }

    /// Where the file at `path` in `Files`' `Save` is on the disk.
    fn saved(&self, path: &str) -> PathBuf {
        self.dir.join("save").join(path)
    }

    /// Accepts what dials, reads what came, answers each request whole, and writes each stream what it has to.
    fn pump(&mut self) {
        while let Ok((peer, _)) = self.listener.accept() {
            assert!(peer.set_nonblocking(true).is_ok(), "the peer's calls return at once");
            self.connections.push(Connection { peer, read: Vec::new(), role: Role::Asking });
        }
        let mut connections = mem::take(&mut self.connections);
        for connection in &mut connections {
            self.serve(connection);
        }
        connections.retain(|connection| !matches!(connection.role, Role::Done));
        self.connections = connections;
    }

    /// Serves one connection.
    fn serve(&mut self, connection: &mut Connection) {
        let mut chunk = [0; 4096];
        match connection.peer.read(&mut chunk) {
            Ok(0) => connection.role = Role::Done,
            Ok(count) => connection.read.extend_from_slice(&chunk[..count]),
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(_) => connection.role = Role::Done,
        }
        match &mut connection.role {
            Role::Asking => {
                if let Some((head, body)) = split(&connection.read) {
                    connection.read.clear();
                    connection.role = self.answer(&mut connection.peer, &head, &body);
                }
            }
            Role::Attach(id) => {
                if let Some(container) = self.containers.get_mut(id) {
                    container.stdin.append(&mut connection.read);
                }
            }
            Role::Logs { id, sent } => {
                let Some(container) = self.containers.get(id) else {
                    write(&mut connection.peer, b"0\r\n\r\n");
                    connection.role = Role::Done;
                    return;
                };
                let more = &container.output[*sent..];
                if !more.is_empty() {
                    write(&mut connection.peer, &chunk_of(more));
                    *sent = container.output.len();
                }
                if !container.running {
                    write(&mut connection.peer, b"0\r\n\r\n");
                    connection.role = Role::Done;
                }
            }
            Role::Events { sent } => {
                for event in &self.events[*sent..] {
                    write(&mut connection.peer, &chunk_of(format!("{event}\n").as_bytes()));
                }
                *sent = self.events.len();
            }
            Role::Done => {}
        }
    }

    /// Answers the request of `head` and `body`, and answers what the connection is then.
    fn answer(&mut self, peer: &mut UnixStream, head: &str, body: &[u8]) -> Role {
        let line = head.lines().next().unwrap_or_default();
        let mut words = line.split(' ');
        let (method, target) = (words.next().unwrap_or_default(), words.next().unwrap_or_default());
        let target = target.strip_prefix("/v1.44").unwrap_or(target);
        let (route, query) = target.split_once('?').unwrap_or((target, ""));
        let parts: Vec<&str> = route.trim_start_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("POST", ["containers", "create"]) => {
                let name = query.strip_prefix("name=").unwrap_or_default().to_owned();
                let body = String::from_utf8_lossy(body).into_owned();
                let id = format!("{:064x}", self.made.len() + 1);
                self.made.push(id.clone());
                self.names.push(name.clone());
                let container = Container { name, body, running: false, output: Vec::new(), stdin: Vec::new() };
                self.containers.insert(id.clone(), container);
                respond(peer, "201 Created", &format!(r#"{{"Id":"{id}","Warnings":[]}}"#));
            }
            ("POST", ["volumes", "create"]) => {
                let body = String::from_utf8_lossy(body).into_owned();
                let name = body.split(r#""Name":""#).nth(1).and_then(|rest| rest.split('"').next()).unwrap_or_default();
                self.volumes.entry(name.to_owned()).or_insert_with(|| body.clone());
                respond(peer, "201 Created", &format!(r#"{{"Name":"{name}"}}"#));
            }
            ("POST", ["containers", id, "start"]) => {
                if let Some(container) = self.containers.get_mut(*id) {
                    container.running = true;
                }
                self.events.push(event(id, "start", ""));
                write(peer, b"HTTP/1.1 204 No Content\r\n\r\n");
            }
            ("POST", ["containers", id, "attach"]) => {
                write(peer, b"HTTP/1.1 101 UPGRADED\r\nContent-Type: application/vnd.docker.multiplexed-stream\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n");
                return Role::Attach((*id).to_owned());
            }
            ("GET", ["containers", id, "logs"]) => {
                write(peer, b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
                return Role::Logs { id: (*id).to_owned(), sent: 0 };
            }
            ("GET", ["events"]) => {
                write(peer, b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
                return Role::Events { sent: self.events.len() };
            }
            ("GET", ["containers", "json"]) => {
                let listed: Vec<String> = self
                    .containers
                    .iter()
                    .map(|(id, container)| {
                        let state = if container.running {
                            "running"
                        } else {
                            "exited"
                        };
                        format!(r#"{{"Id":"{id}","Names":["/{}"],"State":"{state}","Labels":{{}}}}"#, container.name)
                    })
                    .collect();
                respond(peer, "200 OK", &format!("[{}]", listed.join(",")));
            }
            ("GET", ["containers", id, "json"]) => {
                let status = self.containers.get(*id).map(|container| container.running);
                match status {
                    Some(running) => {
                        let status = if running {
                            "running"
                        } else {
                            "exited"
                        };
                        respond(
                            peer,
                            "200 OK",
                            &format!(r#"{{"State":{{"Status":"{status}","ExitCode":0,"OOMKilled":false}}}}"#),
                        );
                    }
                    None => respond(peer, "404 Not Found", r#"{"message":"No such container"}"#),
                }
            }
            ("POST", ["containers", id, "stop"]) => {
                self.stop(id);
                write(peer, b"HTTP/1.1 204 No Content\r\n\r\n");
            }
            ("DELETE", ["containers", id]) => {
                self.stop(id);
                let removed = self.containers.remove(*id).is_some();
                if removed {
                    write(peer, b"HTTP/1.1 204 No Content\r\n\r\n");
                } else {
                    respond(peer, "404 Not Found", r#"{"message":"No such container"}"#);
                }
            }
            _ => respond(peer, "500 Internal Server Error", r#"{"message":"the fake does not serve this"}"#),
        }

        Role::Done
    }

    /// Stops the container `id`, if it runs: its process dies.
    fn stop(&mut self, id: &str) {
        let Some(container) = self.containers.get_mut(id) else {
            return;
        };
        if container.running {
            container.running = false;
            self.events.push(event(id, "die", "0"));
        }
    }

    /// Writes `line` as the container `id`'s process writes a line to stdout.
    fn emit(&mut self, id: &str, line: &str) {
        let Some(container) = self.containers.get_mut(id) else {
            panic!("the container {id} is there to write");
        };
        let payload = format!("{line}\n");
        let size = u32::try_from(payload.len()).unwrap_or(u32::MAX).to_be_bytes();
        container.output.extend([&[1, 0, 0, 0][..], &size, payload.as_bytes()].concat());
    }

    /// The lines written to the container `id`'s stdin.
    fn stdin(&self, id: &str) -> Vec<String> {
        let written = self.containers.get(id).map(|container| container.stdin.clone()).unwrap_or_default();

        String::from_utf8_lossy(&written).lines().map(String::from).collect()
    }

    /// The id of the `nth` container made, from 0.
    fn made(&self, nth: usize) -> String {
        self.made.get(nth).cloned().unwrap_or_else(|| panic!("a container {nth} was made"))
    }

    /// The create body of the container `id`.
    fn body(&self, id: &str) -> String {
        self.containers.get(id).map(|container| container.body.clone()).unwrap_or_default()
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A docker event line of `action` for the container `id`.
fn event(id: &str, action: &str, code: &str) -> String {
    format!(
        r#"{{"Type":"container","Action":"{action}","Actor":{{"ID":"{id}","Attributes":{{"exitCode":"{code}"}}}}}}"#
    )
}

/// The head and body `read` holds, once both have come.
fn split(read: &[u8]) -> Option<(String, Vec<u8>)> {
    let end = read.windows(4).position(|window| window == b"\r\n\r\n")? + 4;
    let head = String::from_utf8_lossy(&read[..end]).into_owned();
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length: "))
        .map_or(Some(0), |length| length.trim().parse::<usize>().ok())?;
    let body = read.get(end..end + length)?.to_vec();

    Some((head, body))
}

/// Writes `bytes` to the peer, whose socket holds far more than any test writes this way.
fn write(peer: &mut UnixStream, bytes: &[u8]) {
    let _ = peer.write_all(bytes);
}

/// Answers a JSON `body` of `status`.
fn respond(peer: &mut UnixStream, status: &str, body: &str) {
    let length = body.len();
    write(
        peer,
        format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n\r\n{body}")
            .as_bytes(),
    );
}

/// `bytes` as one chunk of a chunked body.
fn chunk_of(bytes: &[u8]) -> Vec<u8> {
    [format!("{:x}\r\n", bytes.len()).as_bytes(), bytes, b"\r\n"].concat()
}

/// A hub of `Tcp`, `Http`, `Mcp`, `Files` in the test's directory, the Docker client on the fake's socket, the sessions
/// as `sessions` say with their tools, the sessions' port served on the loopback, and the caller.
fn hub(daemon: &Daemon, sessions: SessionsSettings) -> Result<Harness<Headless>, Unheard> {
    let program = Headless::program()
        .with_engine(engine())
        .root::<Tcp>()
        .root_with::<Http>(HttpSettings::default())
        .root::<Mcp>()
        .root_with::<Store>(daemon.store())
        .root_with::<Files>(daemon.files())
        .root_with::<Docker>(daemon.docker())
        .root_with::<Sessions>(sessions)
        .root::<Toolbox<(SessionsTools,), Mcp>>()
        .root_with::<ContainerStack>(StackSettings {
            port: 0,
            interface: Interface::Loopback,
            ..StackSettings::default()
        })
        .root::<Caller>();

    Harness::of(program)
}

/// The engine's settings for a hub: it reads whatever `Http` and the sessions' port read whole.
fn engine() -> EngineSettings {
    let heads = HttpSettings::default().most_read_bytes().max(StackSettings::default().most_read_bytes());
    let read_limit = heads.try_into().unwrap_or(u32::MAX);

    EngineSettings { read_limit, ..EngineSettings::default() }
}

/// Steps the hub and the fake daemon until `done` holds of them, or the wait is spent.
fn until(harness: &mut Harness<Headless>, daemon: &mut Daemon, mut done: impl FnMut(&Caller, &Daemon) -> bool) {
    let caller = harness.root::<Caller>();
    waiting::until(BUDGET, || {
        harness.step();
        daemon.pump();

        harness.state(caller).is_ok_and(|state| done(state, daemon))
    });
}

/// Tells the caller `order`.
fn order(harness: &mut Harness<Headless>, order: &Order) -> Result<(), Unheard> {
    let caller: ActorRef<Caller> = harness.root::<Caller>();

    harness.tell(caller, order)
}

/// The caller's state.
fn caller(harness: &mut Harness<Headless>) -> Result<&Caller, Unheard> {
    let caller = harness.root::<Caller>();

    harness.state(caller)
}

/// Opens a session named `name`, and answers its number once it runs.
fn open(harness: &mut Harness<Headless>, daemon: &mut Daemon, name: &str) -> Result<u32, Unheard> {
    let before = caller(harness)?.opened.len();
    order(harness, &Order::New(NewSession { name: String::from(name) }))?;
    until(harness, daemon, |caller, _| caller.opened.len() > before);
    let Some(Opened::Open(session)) = caller(harness)?.opened.last().cloned() else {
        panic!("the session opens: {:?}", caller(harness)?.opened);
    };

    Ok(session.session)
}

/// Says `text` to `session`, and answers the number of its transcript's entry.
fn say(harness: &mut Harness<Headless>, daemon: &mut Daemon, session: u32, text: &str) -> Result<u64, Unheard> {
    let before = caller(harness)?.said.len();
    order(harness, &Order::Say(Say { session, text: String::from(text) }))?;
    until(harness, daemon, |caller, _| caller.said.len() > before);
    let Some(Said::Taken { number }) = caller(harness)?.said.last().cloned() else {
        panic!("the line is taken: {:?}", caller(harness)?.said);
    };

    Ok(number)
}

/// The entries of `session`'s transcript after `after`.
fn read(
    harness: &mut Harness<Headless>,
    daemon: &mut Daemon,
    session: u32,
    after: u64,
) -> Result<Vec<Numbered>, Unheard> {
    let before = caller(harness)?.read.len();
    order(harness, &Order::Read(ReadTranscript { session, after, most: 0 }))?;
    until(harness, daemon, |caller, _| caller.read.len() > before);
    let Some(Transcribed::Entries { entries, .. }) = caller(harness)?.read.last().cloned() else {
        panic!("the transcript is read: {:?}", caller(harness)?.read);
    };

    Ok(entries)
}

/// The state `session` is listed in.
fn state(harness: &mut Harness<Headless>, daemon: &mut Daemon, session: u32) -> Result<State, Unheard> {
    let before = caller(harness)?.listed.len();
    order(harness, &Order::List)?;
    until(harness, daemon, |caller, _| caller.listed.len() > before);
    let listed = caller(harness)?.listed.last().cloned();
    let found = listed.and_then(|listed| listed.sessions.into_iter().find(|listed| listed.session == session));
    let Some(found) = found else {
        panic!("the session is listed");
    };

    Ok(found.state)
}

/// Waits until `session` is listed in `wanted`.
fn until_state(
    harness: &mut Harness<Headless>,
    daemon: &mut Daemon,
    session: u32,
    wanted: State,
) -> Result<State, Unheard> {
    let mut seen = state(harness, daemon, session)?;
    waiting::until(BUDGET, || {
        seen = state(harness, daemon, session).unwrap_or(seen);

        seen == wanted
    });

    Ok(seen)
}

/// The lines of the container `id`'s stdin once it holds `count` of them.
fn stdin(harness: &mut Harness<Headless>, daemon: &mut Daemon, id: &str, count: usize) -> Vec<String> {
    until(harness, daemon, |_, daemon| daemon.stdin(id).len() >= count);

    daemon.stdin(id)
}

/// Claude's events of one turn it ends, as stream-json writes them.
fn turn_lines(text: &str) -> [String; 3] {
    [
        format!(r#"{{"type":"system","subtype":"init","session_id":"{CONVERSATION}"}}"#),
        format!(
            r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{text}"}}]}},"session_id":"{CONVERSATION}"}}"#
        ),
        format!(r#"{{"type":"result","subtype":"success","session_id":"{CONVERSATION}"}}"#),
    ]
}

/// Writes a turn of Claude's in the container `id`.
fn answer_turn(daemon: &mut Daemon, id: &str, text: &str) {
    for line in turn_lines(text) {
        daemon.emit(id, &line);
    }
}

/// The entries of `session`'s transcript once it holds `events` of Claude's.
fn transcript_with(
    harness: &mut Harness<Headless>,
    daemon: &mut Daemon,
    session: u32,
    events: usize,
) -> Result<Vec<Numbered>, Unheard> {
    let mut entries = read(harness, daemon, session, 0)?;
    waiting::until(BUDGET, || {
        entries = read(harness, daemon, session, 0).unwrap_or_default();

        claude_events(&entries) >= events
    });

    Ok(entries)
}

/// How many lines of `entries` are Claude's events.
fn claude_events(entries: &[Numbered]) -> usize {
    entries.iter().filter(|numbered| matches!(numbered.entry, Entry::Claude(_))).count()
}

#[test]
fn a_transcript_numbers_every_line_in_order_and_keeps_a_turn_the_hub_did_not_start() -> Result<(), Unheard> {
    // Catches a container made from what the caller asked, a line not written to stdin as a user's message, a
    // transcript whose numbers skip or repeat or which a read `after` misses, and a turn Claude starts by itself (a
    // background agent finishing) dropped because no line began it.
    let mut daemon = Daemon::new("transcript");
    let mut harness = hub(&daemon, daemon.sessions(4, 3600))?;
    let hostile = "evil\",\"Binds\":[\"/:/host\"],\"x\":\"";
    let session = open(&mut harness, &mut daemon, hostile)?;
    let id = daemon.made(0);
    let body = daemon.body(&id);
    assert!(!body.contains("evil") && !body.contains("/host"), "nothing of the name reaches the container: {body}");
    assert!(body.contains(r#""aethereal.session":"1""#), "{body}");
    let prompt = format!(r#""--append-system-prompt","{}""#, SessionsSettings::default().prompt.replace('\n', "\\n"));
    assert!(body.contains(&prompt), "the settings' prompt is appended: {body}");
    let volumes: Vec<&String> = daemon.volumes.keys().collect();
    assert_eq!(volumes, ["aethereal-cargo-test", "aethereal-session-test-1", "aethereal-session-test-1-target"]);
    assert!(daemon.volumes.values().all(|volume| !volume.contains("evil")), "nothing of the name reaches a volume");

    let number = say(&mut harness, &mut daemon, session, "hello")?;
    let written = stdin(&mut harness, &mut daemon, &id, 1);
    assert_eq!(written, [r#"{"type":"user","message":{"role":"user","content":"hello"}}"#]);
    answer_turn(&mut daemon, &id, "hi");
    answer_turn(&mut daemon, &id, "my background agent finished");
    let entries = transcript_with(&mut harness, &mut daemon, session, 6)?;

    let numbers: Vec<u64> = entries.iter().map(|numbered| numbered.number).collect();
    assert_eq!(numbers, (1..=8).collect::<Vec<u64>>(), "every entry is numbered in order");
    assert!(matches!(entries[0].entry, Entry::Hub(Note::Started { run: 1, resumed: false })), "{entries:?}");
    assert_eq!(entries[1].entry, Entry::Said { by: Speaker::Caller, text: String::from("hello") });
    assert_eq!(number, 2);
    assert_eq!(claude_events(&entries), 6, "the turn nobody started is kept as any other");
    let after = read(&mut harness, &mut daemon, session, 5)?;
    assert_eq!(after.first().map(|numbered| numbered.number), Some(6));
    assert_eq!(after.len(), 3);

    Ok(())
}

#[test]
fn a_session_past_the_most_running_is_refused_naming_the_limit_and_those_that_run() -> Result<(), Unheard> {
    // Catches a limit not kept, so sessions take the build host past its share, or a refusal that does not say what
    // runs, so the caller cannot tell which to archive.
    let mut daemon = Daemon::new("limit");
    let mut harness = hub(&daemon, daemon.sessions(1, 3600))?;
    let first = open(&mut harness, &mut daemon, "first")?;
    order(&mut harness, &Order::New(NewSession { name: String::from("second") }))?;
    until(&mut harness, &mut daemon, |caller, _| caller.opened.len() == 2);

    let refused = caller(&mut harness)?.opened.last().cloned();
    assert_eq!(refused, Some(Opened::Refused(Refusal::Full { most: 1, running: vec![first] })));
    assert_eq!(daemon.made.len(), 1, "no container is made for a session refused");

    Ok(())
}

#[test]
fn an_idle_session_with_no_turn_since_its_compaction_is_stopped_at_once_and_resumed_by_a_line() -> Result<(), Unheard> {
    // Catches an idle session left running, one told it is about to be compacted when it has nothing new to compact,
    // and a line to a stopped session lost rather than resuming it in a new container.
    let mut daemon = Daemon::new("idle");
    let mut harness = hub(&daemon, daemon.sessions(4, 0))?;
    let session = open(&mut harness, &mut daemon, "idle")?;
    let first = daemon.made(0);
    assert_eq!(until_state(&mut harness, &mut daemon, session, State::Stopped)?, State::Stopped);
    assert!(daemon.stdin(&first).is_empty(), "nothing is written to a session with nothing to compact");
    assert!(!daemon.containers.contains_key(&first), "its container is removed");

    say(&mut harness, &mut daemon, session, "back again")?;
    until(&mut harness, &mut daemon, |_, daemon| daemon.made.len() == 2);
    let second = daemon.made(1);
    let written = stdin(&mut harness, &mut daemon, &second, 1);
    assert_eq!(written, [r#"{"type":"user","message":{"role":"user","content":"back again"}}"#]);
    let entries = read(&mut harness, &mut daemon, session, 0)?;
    let stopped =
        entries.iter().any(|numbered| numbered.entry == Entry::Hub(Note::Stopped { run: 1, why: Ended::Idle }));
    assert!(stopped, "{entries:?}");

    Ok(())
}

#[test]
fn an_idle_session_is_told_it_will_be_compacted_then_compacted_then_stopped_and_resumes_its_conversation()
-> Result<(), Unheard> {
    // Catches the notice or `/compact` not written, or written as the caller's; a session stopped before its
    // compaction ends; and a resumed session that starts a new conversation rather than resuming Claude's.
    let mut daemon = Daemon::new("compact");
    let mut harness = hub(&daemon, daemon.sessions(4, 0))?;
    let session = open(&mut harness, &mut daemon, "compact")?;
    until_state(&mut harness, &mut daemon, session, State::Stopped)?;
    say(&mut harness, &mut daemon, session, "work")?;
    until(&mut harness, &mut daemon, |_, daemon| daemon.made.len() == 2);
    let id = daemon.made(1);
    stdin(&mut harness, &mut daemon, &id, 1);

    answer_turn(&mut daemon, &id, "done");
    let written = stdin(&mut harness, &mut daemon, &id, 2);
    assert!(written[1].contains("will be compacted next"), "the notice follows the idle turn: {written:?}");
    answer_turn(&mut daemon, &id, "my loose threads");
    let written = stdin(&mut harness, &mut daemon, &id, 3);
    assert_eq!(written[2], r#"{"type":"user","message":{"role":"user","content":"/compact"}}"#);
    daemon.emit(&id, r#"{"type":"system","subtype":"compact_boundary","compact_metadata":{"trigger":"manual"}}"#);
    daemon.emit(&id, r#"{"type":"result","subtype":"success"}"#);
    until_state(&mut harness, &mut daemon, session, State::Stopped)?;

    let entries = read(&mut harness, &mut daemon, session, 0)?;
    let hub_lines =
        entries.iter().filter(|numbered| matches!(numbered.entry, Entry::Said { by: Speaker::Hub, .. })).count();
    assert_eq!(hub_lines, 2, "the notice and the compaction are the hub's lines");
    say(&mut harness, &mut daemon, session, "again")?;
    until(&mut harness, &mut daemon, |_, daemon| daemon.made.len() == 3);
    let body = daemon.body(&daemon.made(2));
    assert!(body.contains(&format!(r#""--resume","{CONVERSATION}""#)), "{body}");

    Ok(())
}

#[test]
fn a_line_said_while_the_notice_runs_makes_the_session_active_and_nothing_is_compacted() -> Result<(), Unheard> {
    // Catches a winding-down that goes on to compact and stop a session its owner has spoken to meanwhile.
    let mut daemon = Daemon::new("notice");
    let mut harness = hub(&daemon, daemon.sessions(4, 0))?;
    let session = open(&mut harness, &mut daemon, "notice")?;
    until_state(&mut harness, &mut daemon, session, State::Stopped)?;
    say(&mut harness, &mut daemon, session, "work")?;
    until(&mut harness, &mut daemon, |_, daemon| daemon.made.len() == 2);
    let id = daemon.made(1);
    stdin(&mut harness, &mut daemon, &id, 1);
    answer_turn(&mut daemon, &id, "done");
    stdin(&mut harness, &mut daemon, &id, 2);
    assert_eq!(state(&mut harness, &mut daemon, session)?, State::Noticed);

    say(&mut harness, &mut daemon, session, "wait, one more thing")?;
    assert_eq!(state(&mut harness, &mut daemon, session)?, State::Working);
    // Claude answers the notice and the line it queued in one turn.
    answer_turn(&mut daemon, &id, "my loose threads, and the one more thing");
    let written = stdin(&mut harness, &mut daemon, &id, 4);
    assert!(
        written[3].contains("will be compacted next"),
        "the session idles again, and is noticed again: {written:?}"
    );
    assert!(written.iter().all(|line| !line.contains("/compact")), "nothing is compacted: {written:?}");
    assert!(daemon.containers.contains_key(&id), "its container runs");

    Ok(())
}

/// A response the sessions' port answered: its status, and its head's and body's text.
#[derive(Debug)]
struct Heard {
    status: u16,
    head: String,
    body: String,
}

/// A `POST /mcp` of the JSON-RPC `method`, with `fields` in its params beside `_meta`, naming the tool `tool` for a
/// call, and bearing the `Authorization` header `authorization`: none sends no such header.
fn rpc(method: &str, tool: Option<&str>, fields: &str, authorization: Option<&str>) -> String {
    let meta =
        r#"{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}"#;
    let body = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}","params":{{{fields}"_meta":{meta}}}}}"#);
    let mut request = format!(
        "POST /mcp HTTP/1.1\r\nConnection: close\r\nContent-Length: {}\r\nMCP-Protocol-Version: 2026-07-28\r\n\
         Mcp-Method: {method}\r\n",
        body.len()
    );
    if let Some(tool) = tool {
        let _ = write!(request, "Mcp-Name: {tool}\r\n");
    }
    if let Some(authorization) = authorization {
        let _ = write!(request, "Authorization: {authorization}\r\n");
    }
    request.push_str("\r\n");
    request.push_str(&body);

    request
}

/// A `tools/list` bearing `authorization`.
fn list_tools(authorization: Option<&str>) -> String {
    rpc("tools/list", None, "", authorization)
}

/// A client on a thread of its own, outside the engine, which sends `request` to the hub's sessions' port and reads to
/// the close.
fn sent(harness: &mut Harness<Headless>, request: String) -> thread::JoinHandle<Heard> {
    let port = bound(harness, |stack: &ContainerStack| stack.serving.port());
    #[allow(clippy::disallowed_methods)] // The client is a native thread of its own, outside the engine.
    thread::spawn(move || {
        let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
            panic!("the client connects to the sessions' port");
        };
        assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");
        assert!(stream.write_all(request.as_bytes()).is_ok(), "the client writes");
        let mut whole = Vec::new();
        assert!(stream.read_to_end(&mut whole).is_ok(), "a response comes within ten seconds");
        let text = String::from_utf8_lossy(&whole).into_owned();
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            panic!("a whole response: {text}");
        };
        let Some(status) = head.split(' ').nth(1).and_then(|status| status.parse().ok()) else {
            panic!("a status: {head}");
        };

        Heard { status, head: String::from(head), body: String::from(body) }
    })
}

/// What `client` heard, the hub and the daemon stepped until it has.
fn heard_by(harness: &mut Harness<Headless>, daemon: &mut Daemon, client: thread::JoinHandle<Heard>) -> Heard {
    while !client.is_finished() {
        harness.step();
        daemon.pump();
    }
    let Ok(heard) = client.join() else {
        panic!("the client finished");
    };

    heard
}

/// What the hub's sessions' port answers `request` with.
fn routed(harness: &mut Harness<Headless>, daemon: &mut Daemon, request: String) -> Heard {
    let client = sent(harness, request);

    heard_by(harness, daemon, client)
}

/// The `Authorization` header the container `id` was given in its environment, as its MCP config sends it.
fn authorization_of(daemon: &Daemon, id: &str) -> String {
    let body = daemon.body(id);
    let bearer = body.split("\"AETHEREAL_HUB_BEARER=").nth(1).and_then(|rest| rest.split('"').next());
    let Some(bearer) = bearer else {
        panic!("the container's environment holds its bearer: {body}");
    };
    assert_eq!(body.matches(bearer).count(), 1, "the bearer is in the environment alone");

    format!("Bearer {bearer}")
}

/// Whether `heard` is the sessions' refusal of a request with no bearer they hold: `401`, with the challenge a client
/// answers with its bearer, which only `Sessions` writes, as `Mcp` answers no request `401`.
fn challenged(heard: &Heard) -> bool {
    let is_unauthorized = heard.status == 401;
    let challenges = heard.head.contains("www-authenticate: Bearer realm=\"aethereal\"");

    is_unauthorized && challenges
}

#[test]
fn a_session_s_container_bears_a_bearer_its_port_lets_in_until_its_container_is_removed() -> Result<(), Unheard> {
    // Catches a container given a bearer `Sessions` does not hold, so the session reaches none of the hub's tools; the
    // bearer written into the container's command, where every process list and `docker inspect` show it; a request
    // bearing no bearer, or one never minted, let on to `Mcp`, or refused without the challenge a client answers; and
    // a bearer still let in once its container is gone (ADR-0057).
    let mut daemon = Daemon::new("bearer");
    let mut harness = hub(&daemon, daemon.sessions(4, 3600))?;
    let session = open(&mut harness, &mut daemon, "bearer")?;
    let authorization = authorization_of(&daemon, &daemon.made(0));
    let stranger = format!("Bearer {}", "5a".repeat(16));

    let held = routed(&mut harness, &mut daemon, list_tools(Some(&authorization)));
    let unborne = routed(&mut harness, &mut daemon, list_tools(None));
    let unminted = routed(&mut harness, &mut daemon, list_tools(Some(&stranger)));

    assert_eq!(held.status, 200, "{held:?}");
    assert!(challenged(&unborne), "{unborne:?}");
    assert!(challenged(&unminted), "{unminted:?}");

    order(&mut harness, &Order::Archive(Archive { session }))?;
    until(&mut harness, &mut daemon, |caller, _| !caller.archived.is_empty());
    let dismissed = routed(&mut harness, &mut daemon, list_tools(Some(&authorization)));

    assert!(challenged(&dismissed), "{dismissed:?}");

    Ok(())
}

#[test]
fn a_session_s_calls_bear_only_its_latest_container_s_bearer() -> Result<(), Unheard> {
    // Catches a session's bearer of a container it no longer runs still let in once its next container's is minted,
    // and the next container's bearer not held, so a session that wakes reaches none of the hub's tools (ADR-0057).
    let mut daemon = Daemon::new("rebearer");
    let mut harness = hub(&daemon, daemon.sessions(4, 0))?;
    let session = open(&mut harness, &mut daemon, "rebearer")?;
    let older = authorization_of(&daemon, &daemon.made(0));
    assert_eq!(until_state(&mut harness, &mut daemon, session, State::Stopped)?, State::Stopped);

    say(&mut harness, &mut daemon, session, "back again")?;
    until(&mut harness, &mut daemon, |_, daemon| daemon.made.len() == 2);
    let newer = authorization_of(&daemon, &daemon.made(1));
    let refused = routed(&mut harness, &mut daemon, list_tools(Some(&older)));
    let held = routed(&mut harness, &mut daemon, list_tools(Some(&newer)));

    assert_ne!(older, newer, "each container is given a bearer of its own");
    assert!(challenged(&refused), "{refused:?}");
    assert_eq!(held.status, 200, "{held:?}");

    Ok(())
}

#[test]
fn a_session_s_call_reaches_its_tool_as_that_session_s_and_a_session_tool_refuses_it() -> Result<(), Unheard> {
    // Catches the session `Sessions` names for a bearer not handed on to `Mcp`, or `Mcp` forwarding a call on the
    // sessions' port as the owner's or another session's, so a session could mint a trusted engine or drive another
    // session; and a session's call of a session tool carried out (ADR-0057).
    let mut daemon = Daemon::new("caller");
    let mut harness = hub(&daemon, daemon.sessions(4, 3600))?;
    let session = open(&mut harness, &mut daemon, "caller")?;
    let authorization = authorization_of(&daemon, &daemon.made(0));
    let (mcp, worker) = (harness.root::<Mcp>(), harness.protocol::<Tooling>());
    let offering = harness.ask(mcp, &Offer { tools: vec![sheet()], worker })?;
    assert_eq!(harness.answer(offering)?, Answered::Yes(Offered::Yes));

    let calling = sent(&mut harness, rpc("tools/call", Some("nothing"), r#""name":"nothing","#, Some(&authorization)));
    let call = harness.asked::<Call>()?;
    let named = call.request.caller;
    harness.reply(call, &Called::Refused(String::from("heard")))?;
    let called = heard_by(&mut harness, &mut daemon, calling);
    let fields = r#""name":"session_list","arguments":{},"#;
    let refused =
        routed(&mut harness, &mut daemon, rpc("tools/call", Some("session_list"), fields, Some(&authorization)));

    assert_eq!(named, By::Session { session });
    assert!(called.body.contains("heard"), "{called:?}");
    assert!(refused.body.contains("\"isError\":true"), "{refused:?}");
    assert!(refused.body.contains("the session tools are the owner's"), "{refused:?}");

    Ok(())
}

#[test]
fn an_interrupt_is_written_as_claude_s_control_request_and_an_archive_leaves_no_container() -> Result<(), Unheard> {
    // Catches an interrupt written as a user's message, which Claude would read as text, and an archive that leaves
    // the container running or the session still resumable.
    let mut daemon = Daemon::new("interrupt");
    let mut harness = hub(&daemon, daemon.sessions(4, 3600))?;
    let session = open(&mut harness, &mut daemon, "interrupt")?;
    let id = daemon.made(0);
    say(&mut harness, &mut daemon, session, "count to a million")?;
    daemon.emit(&id, &turn_lines("")[0]);
    order(&mut harness, &Order::Interrupt(Interrupt { session }))?;
    until(&mut harness, &mut daemon, |caller, _| !caller.interrupted.is_empty());
    let Some(Interrupted::Sent { number }) = caller(&mut harness)?.interrupted.first().cloned() else {
        panic!("the interrupt is sent");
    };
    let written = stdin(&mut harness, &mut daemon, &id, 2);
    let expected = format!(
        r#"{{"type":"control_request","request_id":"interrupt-{number}","request":{{"subtype":"interrupt"}}}}"#
    );
    assert_eq!(written[1], expected);

    order(&mut harness, &Order::Archive(Archive { session }))?;
    until(&mut harness, &mut daemon, |caller, _| !caller.archived.is_empty());
    assert_eq!(caller(&mut harness)?.archived, [Archived::Done]);
    assert!(daemon.containers.is_empty(), "no container of it is left");
    let before = caller(&mut harness)?.said.len();
    order(&mut harness, &Order::Say(Say { session, text: String::from("hello?") }))?;
    until(&mut harness, &mut daemon, |caller, _| caller.said.len() > before);
    assert_eq!(caller(&mut harness)?.said.last(), Some(&Said::Refused(Refusal::Archived)));

    Ok(())
}

#[test]
fn a_hub_that_starts_again_reattaches_to_a_running_session_reads_only_what_it_missed_and_reports_strays()
-> Result<(), Unheard> {
    // Catches a restarted hub that loses its sessions, records again what its transcript held, misses what the
    // container wrote while it was away, or removes a container it has no record of.
    let mut daemon = Daemon::new("reconcile");
    let settings = daemon.sessions(4, 3600);
    let mut harness = hub(&daemon, settings.clone())?;
    let session = open(&mut harness, &mut daemon, "kept")?;
    let id = daemon.made(0);
    say(&mut harness, &mut daemon, session, "hello")?;
    answer_turn(&mut daemon, &id, "before");
    transcript_with(&mut harness, &mut daemon, session, 3)?;
    let file = daemon.saved("sessions/1/transcript");
    // A record is written after the append before it, so a hub whose writes are slow can hold every entry on disk while
    // its record still says the session was starting: the hub is stopped only once both say the container runs.
    until(&mut harness, &mut daemon, |_, daemon| {
        let transcribed = Transcript::read(&fs::read(&file).unwrap_or_default()).last() == 5;
        let recorded = matches!(kept(daemon, session), Some(Standing::Running { .. }));

        transcribed && recorded
    });
    drop(harness);
    daemon.connections.clear();

    answer_turn(&mut daemon, &id, "while the hub was away");
    let stray = Container {
        name: String::from("someone-elses"),
        body: String::new(),
        running: true,
        output: Vec::new(),
        stdin: Vec::new(),
    };
    daemon.containers.insert(format!("{:064x}", 99), stray);
    let mut harness = hub(&daemon, settings)?;
    let entries = transcript_with(&mut harness, &mut daemon, session, 6)?;

    assert_eq!(claude_events(&entries), 6, "each event once, the missed ones after the others: {entries:?}");
    let numbers: Vec<u64> = entries.iter().map(|numbered| numbered.number).collect();
    assert_eq!(numbers, (1..=8).collect::<Vec<u64>>());
    order(&mut harness, &Order::List)?;
    until(&mut harness, &mut daemon, |caller, _| !caller.listed.is_empty());
    let listed = caller(&mut harness)?.listed.last().cloned();
    assert_eq!(listed.map(|listed| listed.strays), Some(vec![String::from("someone-elses")]));
    assert!(daemon.containers.contains_key(&format!("{:064x}", 99)), "a stray is never removed");
    say(&mut harness, &mut daemon, session, "still there?")?;
    let written = stdin(&mut harness, &mut daemon, &id, 2);
    assert_eq!(written[1], r#"{"type":"user","message":{"role":"user","content":"still there?"}}"#);

    Ok(())
}

/// Every session listed, by number, with its name and state.
fn listed(harness: &mut Harness<Headless>, daemon: &mut Daemon) -> Result<Vec<(u32, String, State)>, Unheard> {
    let before = caller(harness)?.listed.len();
    order(harness, &Order::List)?;
    until(harness, daemon, |caller, _| caller.listed.len() > before);
    let listed = caller(harness)?.listed.last().cloned().map(|listed| listed.sessions).unwrap_or_default();

    Ok(listed.into_iter().map(|session| (session.session, session.name, session.state)).collect())
}

/// Where the record of `session` says it stood, once it is there.
fn kept(daemon: &Daemon, session: u32) -> Option<Standing> {
    let bytes = fs::read(daemon.saved(&format!("sessions/{session}/record"))).ok()?;

    from_bytes::<Record>(&bytes, &mut Budget::new(4096)).ok().map(|record| record.kept)
}

/// The sessions' index as `Files` keeps it, once it is there.
fn index(daemon: &Daemon) -> Vec<u32> {
    let bytes = fs::read(daemon.saved("sessions/index")).unwrap_or_default();

    from_bytes::<Index>(&bytes, &mut Budget::new(4096)).map(|index| index.sessions).unwrap_or_default()
}

#[test]
fn a_hub_that_starts_again_reads_its_sessions_back_through_files_by_the_index_with_the_same_numbers()
-> Result<(), Unheard> {
    // Catches an index not written as a session is added, or not read back, so a restarted hub loses its sessions or
    // numbers a new one over an old; a record or transcript read from the wrong file; a container run before a
    // session's own, as the step that gave a home to its user once was; a session's volumes named apart from its
    // session's, which no later container would find; and a cargo home made per session, not shared.
    let mut daemon = Daemon::new("index");
    let settings = daemon.sessions(4, 3600);
    let mut harness = hub(&daemon, settings.clone())?;
    let first = open(&mut harness, &mut daemon, "first")?;
    let second = open(&mut harness, &mut daemon, "second")?;
    order(&mut harness, &Order::Archive(Archive { session: second }))?;
    until(&mut harness, &mut daemon, |caller, _| !caller.archived.is_empty());
    say(&mut harness, &mut daemon, first, "hello")?;
    until(&mut harness, &mut daemon, |_, daemon| index(daemon) == [1, 2]);
    until(&mut harness, &mut daemon, |_, daemon| kept(daemon, second) == Some(Standing::Archived));
    let file = daemon.saved("sessions/1/transcript");
    until(&mut harness, &mut daemon, |_, _| Transcript::read(&fs::read(&file).unwrap_or_default()).last() == 2);
    drop(harness);
    daemon.connections.clear();

    let mut harness = hub(&daemon, settings)?;
    // A list is answered at once, before the sessions are read back: it is asked again until they are.
    let mut sessions = Vec::new();
    waiting::until(BUDGET, || {
        sessions = listed(&mut harness, &mut daemon).unwrap_or_default();

        sessions.len() == 2
    });
    let names: Vec<(u32, String)> = sessions.iter().map(|(number, name, _)| (*number, name.clone())).collect();
    assert_eq!(names, [(1, String::from("first")), (2, String::from("second"))]);
    assert_eq!(sessions[1].2, State::Archived);
    let number = say(&mut harness, &mut daemon, first, "again")?;
    assert_eq!(number, 3, "the transcript goes on from its last number");
    let third = open(&mut harness, &mut daemon, "third")?;
    assert_eq!(third, 3, "a new session is numbered after every one read back");
    until(&mut harness, &mut daemon, |_, daemon| index(daemon) == [1, 2, 3]);

    let volumes: Vec<&String> = daemon.volumes.keys().collect();
    let expected = [
        "aethereal-cargo-test",
        "aethereal-session-test-1",
        "aethereal-session-test-1-target",
        "aethereal-session-test-2",
        "aethereal-session-test-2-target",
        "aethereal-session-test-3",
        "aethereal-session-test-3-target",
    ];
    assert_eq!(volumes, expected, "each session's own volumes, and one cargo home they share");
    let names = ["aethereal-session-test-1-1", "aethereal-session-test-2-1", "aethereal-session-test-3-1"];
    assert_eq!(daemon.names, names, "every container made is a session's: no step runs as root before it");
    let body = daemon.body(&daemon.made(0));
    assert!(body.contains(r#""Type":"volume","Source":"aethereal-session-test-1""#), "{body}");
    assert!(daemon.volumes.values().all(|body| body.contains(r#""aethereal.test":"1""#)));

    Ok(())
}

#[test]
fn with_no_index_there_are_no_sessions_whatever_else_save_holds() -> Result<(), Unheard> {
    // Catches a hub that finds its sessions by what lies in `Save` rather than by the index, which `Files` does not
    // list, and one that fails to start when there is no index, as on its first run.
    let mut daemon = Daemon::new("no-index");
    let record =
        Record { name: String::from("orphan"), conversation: Conversation::Unbegun, runs: 1, kept: Standing::Stopped };
    let Ok(bytes) = to_bytes(&record) else {
        panic!("a record encodes");
    };
    let orphan = daemon.saved("sessions/1/record");
    let written = orphan.parent().map(fs::create_dir_all).transpose().and_then(|_| fs::write(&orphan, bytes));
    assert!(written.is_ok(), "the test writes a record no index lists");
    let mut harness = hub(&daemon, daemon.sessions(4, 3600))?;

    // A new session is answered only once the sessions are read back, so the list after it shows what was read.
    let session = open(&mut harness, &mut daemon, "fresh")?;
    assert_eq!(session, 1);
    let names: Vec<String> = listed(&mut harness, &mut daemon)?.into_iter().map(|(_, name, _)| name).collect();
    assert_eq!(names, ["fresh"], "no session is read without the index");
    until(&mut harness, &mut daemon, |_, daemon| index(daemon) == [1]);

    Ok(())
}

/// What a write of `Files`' did, as the recording storage logs it.
fn logged(log: &FsPath, line: &str) {
    let opened = OpenOptions::new().create(true).append(true).open(log);
    let written = opened.and_then(|mut file| file.write_all(format!("{line}\n").as_bytes()));
    assert!(written.is_ok(), "the recording storage logs to its file");
}

/// The host's storage, recording every write and append it is asked as it begins and as it is answered, each append
/// answered only some time after it has landed, so a writer that does not wait for it sends the next meanwhile.
struct Recording {
    directories: Directories,
    log: PathBuf,
}

impl Storage for Recording {
    fn read<'a>(&'a self, namespace: Namespace, path: &'a FilePath, lent: Spares) -> Reading<'a> {
        self.directories.read(namespace, path, lent)
    }

    fn write<'a>(&'a self, namespace: Namespace, path: &'a FilePath, bytes: Vec<u8>) -> Writing<'a> {
        logged(&self.log, &format!("begun {path}"));
        let writing = self.directories.write(namespace, path, bytes);

        Box::pin(async move {
            let written = writing.await;
            logged(&self.log, &format!("done {path}"));
            written
        })
    }

    fn append<'a>(&'a self, namespace: Namespace, path: &'a FilePath, bytes: Vec<u8>) -> Writing<'a> {
        logged(&self.log, &format!("begun {path}"));
        let appending = self.directories.append(namespace, path, bytes);

        Box::pin(async move {
            let appended = appending.await;
            Later::by(Duration::from_millis(50)).await;
            logged(&self.log, &format!("done {path}"));
            appended
        })
    }

    fn list(&self, namespace: Namespace) -> Listing<'_> {
        self.directories.list(namespace)
    }
}

/// A wait of some time, on a thread of its own that wakes whoever polls it.
struct Later {
    shared: Arc<Mutex<Wakes>>,
}

/// Where a wait stands, as its thread and its poller both see it.
enum Wakes {
    Waiting,
    Polled(Waker),
    Passed,
}

impl Later {
    /// A wait of `time`, begun now.
    fn by(time: Duration) -> Self {
        let shared = Arc::new(Mutex::new(Wakes::Waiting));
        let passing = Arc::clone(&shared);
        #[allow(clippy::disallowed_methods)] // The test's storage holds its answer back on a thread of its own.
        thread::spawn(move || {
            thread::sleep(time);
            let was = mem::replace(&mut *passing.lock().unwrap_or_else(PoisonError::into_inner), Wakes::Passed);
            if let Wakes::Polled(waker) = was {
                waker.wake();
            }
        });

        Self { shared }
    }
}

impl Future for Later {
    type Output = ();

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<()> {
        let mut wakes = self.shared.lock().unwrap_or_else(PoisonError::into_inner);
        let has_passed = matches!(*wakes, Wakes::Passed);
        if has_passed {
            return Poll::Ready(());
        }
        *wakes = Wakes::Polled(context.waker().clone());

        Poll::Pending
    }
}

/// A root that serves the recording storage in place of the one `Files` served, as it is listed after it.
#[derive(Bound)]
#[kind(name = "aether.test.sessions.recorder")]
struct Recorder;

impl ChildOf for Recorder {
    type Parent = Top;
}

impl Actor for Recorder {
    type Config = Unset;
    type Key = ();
    type Handles = ();
    type Depends = ();

    fn begin(_: Unset, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Recorder {
    const SECTION: &'static str = "recorder";
    type Settings = FileRoots;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(roots: FileRoots, (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        let log = PathBuf::from(&roots.save).with_file_name("writes.log");
        let recording = Recording { directories: Directories::new(&roots), log };
        cx.serve(Service::<dyn Storage>::of(Arc::new(recording)));
        cx.begin(&(), Unset)?;

        Ok(())
    }
}

#[test]
fn a_transcript_s_append_is_answered_before_its_record_is_written_and_one_write_runs_at_a_time() -> Result<(), Unheard>
{
    // Catches the record's write sent beside the transcript's append, which `Files` may land first, so a crash leaves
    // a record ahead of its transcript; and two appends to one transcript in flight at once, which may land out of
    // order.
    let mut daemon = Daemon::new("order");
    let program = Headless::program()
        .with_engine(engine())
        .root::<Tcp>()
        .root_with::<Http>(HttpSettings::default())
        .root::<Mcp>()
        .root_with::<Store>(daemon.store())
        .root_with::<Files>(daemon.files())
        .root_with::<Recorder>(daemon.files())
        .root_with::<Docker>(daemon.docker())
        .root_with::<Sessions>(daemon.sessions(4, 3600))
        .root::<Caller>();
    let mut harness = Harness::of(program)?;
    let session = open(&mut harness, &mut daemon, "ordered")?;
    let id = daemon.made(0);
    let log = daemon.dir.join("writes.log");
    say(&mut harness, &mut daemon, session, "one")?;
    // The first append finishes before the next line is said, so that line is a second append rather than one batched
    // beside the first.
    until(&mut harness, &mut daemon, |_, _| {
        fs::read_to_string(&log).unwrap_or_default().contains("done sessions/1/transcript\n")
    });
    say(&mut harness, &mut daemon, session, "two")?;
    answer_turn(&mut daemon, &id, "three");
    let file = daemon.saved("sessions/1/transcript");
    until(&mut harness, &mut daemon, |_, _| Transcript::read(&fs::read(&file).unwrap_or_default()).last() == 6);
    let record = daemon.saved("sessions/1/record");
    until(&mut harness, &mut daemon, |_, _| fs::read_to_string(&log).unwrap_or_default().ends_with("record\n"));

    let written = fs::read_to_string(&log).unwrap_or_default();
    let mut running = 0_u32;
    for line in written.lines() {
        let is_begun = line.starts_with("begun");
        if is_begun {
            assert_eq!(running, 0, "nothing else runs as `{line}` begins:\n{written}");
            running += 1;
        } else {
            running = running.saturating_sub(1);
        }
    }
    let appends = written.lines().filter(|line| *line == "begun sessions/1/transcript").count();
    assert!(appends >= 2, "the transcript was appended to at least twice:\n{written}");
    assert!(record.exists());

    Ok(())
}

/// How long the live run waits on Claude: a container's start and a model's turn take far longer than the fake's.
const LIVE_BUDGET: Duration = Duration::from_mins(4);

/// What a step of the live run found, written for whoever runs it.
#[allow(clippy::print_stderr)] // A run by hand against a real daemon and Claude reports what it saw, for a person.
fn report(step: &str, found: &str) {
    eprintln!("{step}: {found}");
}

/// Removes every container and then every volume that carries the live run's label, and the run's directory, with
/// requests of its own written by hand on Docker's socket, whatever became of the run: nothing the run made outlives
/// it, the home an archive keeps among it.
struct Cleanup {
    label: String,
    root: PathBuf,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let listed = raw(&listing("containers/json?all=1&", &self.label));
        for id in listed.split(r#""Id":""#).skip(1).filter_map(|rest| rest.get(..64)) {
            raw(&format!("DELETE /v1.44/containers/{id}?force=1&v=1"));
        }
        for name in volumes(&self.label) {
            raw(&format!("DELETE /v1.44/volumes/{name}"));
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The request that lists from `endpoint`, a path and the start of a query, everything carrying the live run's
/// `label`.
fn listing(endpoint: &str, label: &str) -> String {
    let filters = format!(r#"{{"label":["aethereal.sessions={label}"]}}"#);
    let encoded = filters.bytes().fold(String::new(), |mut encoded, byte| {
        let _ = write!(encoded, "%{byte:02X}");
        encoded
    });

    format!("GET /v1.44/{endpoint}filters={encoded}")
}

/// The names of the volumes carrying the live run's `label`.
fn volumes(label: &str) -> Vec<String> {
    let listed = raw(&listing("volumes?", label));
    let names = listed.split(r#""Name":""#).skip(1).filter_map(|rest| rest.split('"').next());

    names.map(String::from).collect()
}

/// What Docker answers `request`, a method and a target, on its own socket.
fn raw(request: &str) -> String {
    let Ok(mut socket) = UnixStream::connect("/var/run/docker.sock") else {
        return String::new();
    };
    let _ = socket.write_all(format!("{request} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").as_bytes());
    let mut answer = Vec::new();
    let _ = socket.read_to_end(&mut answer);

    String::from_utf8_lossy(&answer).into_owned()
}

/// The live run's setting `name`, from the environment.
fn live(name: &str) -> String {
    env::var(format!("AETHEREAL_SESSIONS_LIVE_{name}")).unwrap_or_default()
}

/// The live run's hub: `Files` in the run's directory `root`, this machine's daemon, and the sessions on Haiku, as the
/// environment names their image and credential, every container and volume labelled `label`.
fn live_hub(root: &FsPath, label: &str) -> Result<Harness<Headless>, Unheard> {
    let files = FileRoots {
        assets: root.join("assets").display().to_string(),
        save: root.join("save").display().to_string(),
        code: root.join("code").display().to_string(),
        readers: 2,
    };
    let settings = SessionsSettings {
        credential: live("CREDENTIAL"),
        image: live("IMAGE"),
        model: String::from("haiku"),
        hub_url: String::new(),
        label: String::from(label),
        labels: vec![String::from("aethereal.test=1")],
        ..SessionsSettings::default()
    };
    let program = Headless::program()
        .with_engine(engine())
        .root::<Tcp>()
        .root_with::<Http>(HttpSettings::default())
        .root::<Mcp>()
        .root_with::<Store>(StoreSettings {
            root: root.join("store").display().to_string(),
            ..StoreSettings::default()
        })
        .root_with::<Files>(files)
        .root_with::<Docker>(DockerSettings::default())
        .root_with::<Sessions>(settings)
        .root::<Caller>();

    Harness::of(program)
}

/// How many of `entries` are Claude's events whose text holds `marker`.
fn holding(entries: &[Numbered], marker: &str) -> usize {
    let held = entries.iter().filter(|numbered| match &numbered.entry {
        Entry::Claude(json) => json.as_str().contains(marker),
        _ => false,
    });

    held.count()
}

/// How many of `entries` are the results of a tool that printed cargo's version and `42written`, which the live run's
/// first command prints only once its `touch` of the home, the target directory, and the cargo home has succeeded and
/// cargo has run. Claude's own words may hold them too, so only a tool's result that is not an error counts.
fn wrote_home(entries: &[Numbered]) -> usize {
    let wrote = entries.iter().filter(|numbered| match &numbered.entry {
        Entry::Claude(json) => {
            let text = json.as_str();
            let is_result = text.contains(r#""type":"tool_result""#);
            let printed = text.contains("42written");
            let cargo_ran = text.contains("cargo 1.");
            let failed = text.contains(r#""is_error":true"#);

            is_result && printed && cargo_ran && !failed
        }
        _ => false,
    });

    wrote.count()
}

/// Reports each entry of `entries`: Claude's events by their type and subtype, the others whole.
fn report_transcript(entries: &[Numbered]) {
    for numbered in entries {
        let line = match &numbered.entry {
            Entry::Claude(json) => {
                let member = |name: &str| {
                    let marker = format!(r#""{name}":""#);
                    let after = json.as_str().split(marker.as_str()).nth(1);
                    after.and_then(|rest| rest.split('"').next()).unwrap_or_default().to_owned()
                };
                let (kind, subtype) = (member("type"), member("subtype"));
                format!("claude {kind} {subtype}")
            }
            other => format!("{other:?}"),
        };
        report(&format!("  {}", numbered.number), &line);
    }
}

#[test]
#[ignore = "runs Claude Code in a container on this machine's Docker, on the owner's subscription: run by hand with --ignored, naming the run's directory, the session image, and the credential's file in AETHEREAL_SESSIONS_LIVE_ROOT, _IMAGE, and _CREDENTIAL"]
fn a_session_runs_on_the_machines_own_daemon_and_claude() -> Result<(), Unheard> {
    // Catches what a fake cannot: Claude Code reading the lines the hub writes, writing what the hub reads, taking the
    // interrupt, the container made from the settings running at all, and its volumes, which Docker makes root's,
    // written by the session's user, as the session image holds their directories owned by that user, with the
    // image's cargo on the shared cargo home.
    let root = PathBuf::from(live("ROOT"));
    let label = format!("live-{}", process::id());
    let _cleanup = Cleanup { label: label.clone(), root: root.clone() };
    let mut harness = live_hub(&root, &label)?;
    let caller_ref = harness.root::<Caller>();
    let wait = |harness: &mut Harness<Headless>, done: &mut dyn FnMut(&Caller) -> bool| {
        waiting::until(LIVE_BUDGET, || {
            harness.step();
            harness.state(caller_ref).is_ok_and(&mut *done)
        });
    };

    harness.tell(caller_ref, &Order::New(NewSession { name: String::from("live") }))?;
    wait(&mut harness, &mut |caller| !caller.opened.is_empty());
    let opened = caller(&mut harness)?.opened.clone();
    report("session_new", &format!("{opened:?}"));
    let Some(Opened::Open(session)) = opened.first().cloned() else {
        panic!("the session opens");
    };
    let session = session.session;
    let mut entries = Vec::new();
    let read_until =
        |harness: &mut Harness<Headless>, entries: &mut Vec<Numbered>, done: &dyn Fn(&[Numbered]) -> bool| {
            waiting::until(LIVE_BUDGET, || {
                let before = caller(harness).map_or(0, |caller| caller.read.len());
                let _ = harness.tell(caller_ref, &Order::Read(ReadTranscript { session, after: 0, most: 0 }));
                waiting::until(BUDGET, || {
                    harness.step();
                    harness.state(caller_ref).is_ok_and(|caller| caller.read.len() > before)
                });
                if let Ok(Some(Transcribed::Entries { entries: read, .. })) =
                    caller(harness).map(|caller| caller.read.last().cloned())
                {
                    *entries = read;
                }
                done(entries)
            });
        };

    let text = "Run the shell command `touch ~/written \"$CARGO_TARGET_DIR/written\" \"$CARGO_HOME/written\" && cargo \
                --version && echo $((6 * 7))written`, then reply with the word ok.";
    harness.tell(caller_ref, &Order::Say(Say { session, text: String::from(text) }))?;
    read_until(&mut harness, &mut entries, &|entries| holding(entries, r#""type":"result""#) >= 1);
    let wrote_home = wrote_home(&entries);
    report("the session's user writes its volumes", &format!("{wrote_home} tool results hold what the touch printed"));
    assert!(wrote_home >= 1, "the session's user wrote its home, its target, and the cargo home, and cargo ran");
    let answered: Vec<&str> = entries
        .iter()
        .filter_map(|numbered| match &numbered.entry {
            Entry::Claude(json) if json.as_str().contains(r#""type":"assistant""#) => Some(json.as_str()),
            _ => None,
        })
        .collect();
    report("session_say, then session_read", &format!("{} entries; Claude answered {answered:?}", entries.len()));
    assert_eq!(holding(&entries, r#""type":"result""#), 1, "the turn ends");

    let text = "Run the shell command `sleep 60`, then reply with the word done.";
    harness.tell(caller_ref, &Order::Say(Say { session, text: String::from(text) }))?;
    read_until(&mut harness, &mut entries, &|entries| holding(entries, r#""type":"tool_use""#) >= 1);
    harness.tell(caller_ref, &Order::Interrupt(Interrupt { session }))?;
    read_until(&mut harness, &mut entries, &|entries| holding(entries, r#""type":"result""#) >= 2);
    let ended = entries.iter().rev().find_map(|numbered| match &numbered.entry {
        Entry::Claude(json) if json.as_str().contains(r#""type":"result""#) => Some(json.as_str().to_owned()),
        _ => None,
    });
    report(
        "session_interrupt",
        &format!(
            "{:?}, the turn ended {:?}",
            caller(&mut harness)?.interrupted,
            ended.as_deref().map(|json| &json[..json.len().min(120)])
        ),
    );
    assert!(ended.is_some_and(|json| json.contains("error_during_execution")), "the interrupted turn ends as an error");

    harness.tell(caller_ref, &Order::Archive(Archive { session }))?;
    wait(&mut harness, &mut |caller| !caller.archived.is_empty());
    report("session_archive", &format!("{:?}", caller(&mut harness)?.archived));
    let left = raw(&listing("containers/json?all=1&", &label));
    let containers = left.rsplit("\r\n\r\n").next().unwrap_or_default().trim().to_owned();
    report("containers left with the run's label", &containers);
    assert!(!left.contains(r#""Id":""#), "no container of the session is left, the home's owner's among them");
    let kept = volumes(&label);
    report("volumes left with the run's label, removed as the run ends", &format!("{kept:?}"));
    let mut kept = kept;
    kept.sort();
    let volumes = [
        format!("aethereal-cargo-{label}"),
        format!("aethereal-session-{label}-1"),
        format!("aethereal-session-{label}-1-target"),
    ];
    assert_eq!(kept, volumes, "the archive keeps the session's volumes and the shared cargo home");
    report_transcript(&entries);

    Ok(())
}

/// The most bytes of a link's frame in the chat's hubs.
const LINK_BYTES: u32 = 1 << 16;

/// What the test tells the impostor to do: send the sessions what engine `engine` would, tell engine `engine` its chat
/// goes to the sessions, or hear of the engines the hub trusts.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.sessions.pose")]
enum Pose {
    Join { engine: u32 },
    Say { engine: u32, session: u32, text: String },
    Point { engine: u32 },
    Hear,
}

/// A root that takes everything an engine is shown, as an `Engine` does, but is none: it joins and chats as whichever
/// engine the test tells it, counts what it is shown, points an engine's chat at the sessions, as any actor may, and
/// keeps each trusted engine it is told of, in order, once it hears of them.
#[derive(Bound)]
#[kind(name = "aether.test.sessions.impostor")]
struct Impostor {
    shown: u32,
    heard: Vec<ActorRef<Engine>>,
}

impl ChildOf for Impostor {
    type Parent = Top;
}

impl Actor for Impostor {
    type Config = Unset;
    type Key = ();
    type Handles = (Pose, ShowTabs, ShowEvents, ShowAsk, TrustedEngine);
    type Depends = (Sessions, Engines);

    fn begin(_: Unset, _: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { shown: 0, heard: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Impostor {
    const SECTION: &'static str = "impostor";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Unset)?;

        Ok(())
    }
}

impl Handles<Pose> for Impostor {
    type From = Anyone;

    fn handle(&mut self, pose: Pose, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let sessions = path::<Sessions>(());
        match pose {
            Pose::Join { engine } => {
                turn.to(sessions).send(&Joined { engine })?;
            }
            Pose::Say { engine, session, text } => {
                turn.to(sessions).send(&Chatted { engine, chat: Chat::Say { session, text } })?;
            }
            Pose::Point { engine } => {
                let Some(to) = turn.found(&sessions) else {
                    panic!("the sessions are live");
                };
                turn.to(path::<Engine>(u64::from(engine))).send(&ChatTo { to: to.protocol() })?;
            }
            Pose::Hear => {
                turn.to(path::<Engines>(())).send(&HearTrusted)?;
            }
        }

        Ok(())
    }
}

impl Handles<TrustedEngine> for Impostor {
    type From = Anyone;

    fn handle(&mut self, trusted: TrustedEngine, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.heard.push(trusted.engine);

        Ok(())
    }
}

macro_rules! counts {
    ($($shown:ty),*) => {$(
        impl Handles<$shown> for Impostor {
            type From = Anyone;

            fn handle(&mut self, _: $shown, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
                self.shown += 1;

                Ok(())
            }
        }
    )*};
}

counts!(ShowTabs, ShowEvents, ShowAsk);

/// Tells the impostor `pose`, and steps until the engine has nothing left to run of it.
fn pose(harness: &mut Harness<Headless>, pose: &Pose) -> Result<(), Unheard> {
    let impostor: ActorRef<Impostor> = harness.root::<Impostor>();
    harness.tell(impostor, pose)?;
    harness.settle();

    Ok(())
}

/// The trusted engines the impostor was told of, in order.
fn impostor_heard(harness: &mut Harness<Headless>) -> Result<Vec<ActorRef<Engine>>, Unheard> {
    let impostor: ActorRef<Impostor> = harness.root::<Impostor>();

    Ok(harness.state(impostor)?.heard.clone())
}

/// How many frames the impostor has been shown.
fn impostor_shown(harness: &mut Harness<Headless>) -> Result<u32, Unheard> {
    let impostor: ActorRef<Impostor> = harness.root::<Impostor>();

    Ok(harness.state(impostor)?.shown)
}

/// A hub as `hub` makes one, with `Engines` too, on a port of its choosing, and the root that puts each trusted
/// engine's chat through to the sessions, so engines link and chat over the real link (ADR-0057), and the impostor.
fn chat_hub(daemon: &Daemon) -> Result<Harness<Headless>, Unheard> {
    let engines = EnginesSettings { port: 0, most_bytes: LINK_BYTES, ..EnginesSettings::default() };
    let program = Headless::program()
        .with_engine(engine())
        .root::<Tcp>()
        .root_with::<Http>(HttpSettings::default())
        .root::<Mcp>()
        .root_with::<Store>(daemon.store())
        .root_with::<Files>(daemon.files())
        .root_with::<Docker>(daemon.docker())
        .root_with::<Engines>(engines)
        .root_with::<Sessions>(daemon.sessions(4, 3600))
        .root::<ChatToSessions>()
        .root::<Caller>()
        .root::<Impostor>();

    Harness::of(program)
}

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

/// The frames the hub writes on `stream` until one `ends` reads, each read on a thread of the engine's side, the hub
/// and the daemon stepped meanwhile; and the stream.
fn downs_until(
    harness: &mut Harness<Headless>,
    daemon: &mut Daemon,
    stream: TcpStream,
    ends: impl Fn(&Down) -> bool + Send + 'static,
) -> (Vec<Down>, TcpStream) {
    #[allow(clippy::disallowed_methods)] // The engine's side is a native thread of its own, outside the engine.
    let peer = thread::spawn(move || {
        let mut stream = stream;
        let mut downs = Vec::new();
        loop {
            let mut prefix = [0; 4];
            assert!(stream.read_exact(&mut prefix).is_ok(), "a frame comes within ten seconds");
            let mut bytes = vec![0; usize::try_from(u32::from_be_bytes(prefix)).unwrap_or(usize::MAX)];
            assert!(stream.read_exact(&mut bytes).is_ok(), "the frame comes whole");
            let Ok(down) = from_bytes::<Down>(&bytes, &mut Budget::new(1 << 24)) else {
                panic!("the frame reads as a `Down`");
            };
            let is_last = ends(&down);
            downs.push(down);
            if is_last {
                return (downs, stream);
            }
        }
    });
    while !peer.is_finished() {
        harness.step();
        daemon.pump();
    }
    let Ok(heard) = peer.join() else {
        panic!("the engine's side finished");
    };

    heard
}

/// An engine's side linked to the hub with a token minted for `by`, named `name` and holding no tools, once `Engines`
/// lists `count` engines.
fn linked_as(
    harness: &mut Harness<Headless>,
    daemon: &mut Daemon,
    by: By,
    name: &str,
    count: usize,
) -> Result<TcpStream, Unheard> {
    let engines = harness.root::<Engines>();
    let minting = harness.ask(engines, &MintFor { by })?;
    let Answered::Yes(Minted { token, .. }) = harness.answer(minting)? else {
        panic!("`Engines` mints a token");
    };
    let Some(port) = harness.state(engines)?.port() else {
        panic!("`Engines` listens");
    };
    let Ok(mut stream) = TcpStream::connect(("127.0.0.1", port)) else {
        panic!("the engine's side connects");
    };
    assert!(stream.set_read_timeout(Some(Duration::from_secs(10))).is_ok(), "the read timeout sets");
    send_frame(&mut stream, &Up::Hello { revision: REVISION, name: String::from(name), token });
    send_frame(&mut stream, &Up::Sheets { revision: 0, tools: Vec::new() });
    let mut listed = 0;
    waiting::until(BUDGET, || {
        harness.step();
        daemon.pump();
        listed = engines_listed(harness);
        listed >= count
    });
    assert_eq!(listed, count, "the engine is listed");

    Ok(stream)
}

/// How many engines `Engines` lists.
fn engines_listed(harness: &mut Harness<Headless>) -> usize {
    let engines = harness.root::<Engines>();
    let Ok(listing) = harness.ask(engines, &ListEngines) else {
        return 0;
    };
    let Ok(Answered::Yes(list)) = harness.answer(listing) else {
        return 0;
    };

    list.engines.len()
}

/// Whether `down` shows `session`'s events up to the one numbered `last`.
fn shows_through(down: &Down, session: u32, last: u64) -> bool {
    let Down::Show { session: shown, events } = down else {
        return false;
    };
    let reaches = events.last().is_some_and(|numbered| numbered.number >= last);

    *shown == session && reaches
}

/// Whether `down` shows `session`'s line said at engine `engine`.
fn shows_said_at(down: &Down, session: u32, engine: u32) -> bool {
    let Down::Show { session: shown, events } = down else {
        return false;
    };
    let by = Speaker::Engine { engine };
    let said_there =
        events.iter().any(|numbered| matches!(&numbered.entry, Entry::Said { by: said, .. } if *said == by));

    *shown == session && said_there
}

#[test]
fn an_untrusted_engine_s_chat_is_dropped_and_it_is_shown_none_of_it() -> Result<(), Unheard> {
    // Catches a chat frame from an engine a session started acted on, so code a session just wrote could type to a
    // real session, and such an engine joined to be shown the sessions, all over the real link (ADR-0057), even once
    // an actor tells it that its chat goes to the sessions, which any actor may.
    let mut daemon = Daemon::new("chat-untrusted");
    let mut harness = chat_hub(&daemon)?;
    let session = open(&mut harness, &mut daemon, "chat")?;
    let id = daemon.made(0);
    let _trusted = linked_as(&mut harness, &mut daemon, By::Owner, "owner's", 1)?;
    let mut untrusted = linked_as(&mut harness, &mut daemon, By::Session { session }, "session's", 2)?;
    pose(&mut harness, &Pose::Point { engine: 1 })?;

    send_frame(&mut untrusted, &Up::Say { session, text: String::from("from code a session wrote") });
    send_frame(&mut untrusted, &Up::Interrupt { session });
    send_frame(&mut untrusted, &Up::New { name: String::from("another") });
    send_frame(&mut untrusted, &Up::Sheets { revision: 1, tools: vec![sheet()] });
    waiting::until(BUDGET, || {
        harness.step();
        daemon.pump();
        tools_of(&mut harness, 1) == 1
    });

    let sessions = harness.root::<Sessions>();
    assert_eq!(harness.state(sessions)?.joined(), vec![0], "only the owner's engine is shown the chat");
    let entries = read(&mut harness, &mut daemon, session, 0)?;
    let said =
        entries.iter().filter(|numbered| matches!(numbered.entry, Entry::Said { .. } | Entry::Interrupted { .. }));
    assert_eq!(said.count(), 0, "nothing the untrusted engine sent reached the session: {entries:?}");
    assert_eq!(daemon.stdin(&id), Vec::<String>::new());
    let listed = caller(&mut harness)?.listed.len();
    order(&mut harness, &Order::List)?;
    until(&mut harness, &mut daemon, |caller, _| caller.listed.len() > listed);
    let sessions_listed = caller(&mut harness)?.listed.last().map(|listed| listed.sessions.len());
    assert_eq!(sessions_listed, Some(1), "no session was made");

    Ok(())
}

#[test]
fn a_trusted_engine_is_shown_the_sessions_and_its_lines_and_commands_reach_claude_tagged_each_its_own()
-> Result<(), Unheard> {
    // Catches a trusted engine not shown the sessions as it links, a tab opened from `after` that misses or repeats its
    // backlog, a command run in `!` mode held back rather than written as a line of its own, without what it printed
    // or its `!`, a line written to Claude untagged, and either kept in the transcript as anyone's but the engine's,
    // over the real link (ADR-0057).
    let mut daemon = Daemon::new("chat-trusted");
    let mut harness = chat_hub(&daemon)?;
    let session = open(&mut harness, &mut daemon, "chat")?;
    let id = daemon.made(0);
    let stream = linked_as(&mut harness, &mut daemon, By::Owner, "owner's", 1)?;

    let (downs, mut stream) =
        downs_until(&mut harness, &mut daemon, stream, |down| matches!(down, Down::Sessions { .. }));
    let Some(Down::Sessions { list }) = downs.last() else {
        panic!("the engine is shown the sessions");
    };
    let tabs: Vec<(u32, &str)> = list.iter().map(|tab| (tab.session, tab.name.as_str())).collect();
    assert_eq!(tabs, vec![(session, "chat")]);
    say(&mut harness, &mut daemon, session, "hello")?;
    answer_turn(&mut daemon, &id, "hello to you");
    until_state(&mut harness, &mut daemon, session, State::Idle)?;
    let backlog = read(&mut harness, &mut daemon, session, 0)?;
    let last = backlog.last().map_or(0, |numbered| numbered.number);
    assert!(last > 2, "the session has a backlog to show: {backlog:?}");
    send_frame(&mut stream, &Up::Open { session, after: 1 });
    let (downs, mut stream) =
        downs_until(&mut harness, &mut daemon, stream, move |down| shows_through(down, session, last));
    let shown: Vec<u64> = downs
        .iter()
        .filter_map(|down| match down {
            Down::Show { events, .. } => Some(events.iter().map(|numbered| numbered.number)),
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(shown, (2..=last).collect::<Vec<u64>>(), "the backlog after event 1, once each");

    send_frame(&mut stream, &Up::Ran { session, line: String::from("capture"), printed: String::from("a frame\n") });
    send_frame(&mut stream, &Up::Say { session, text: String::from("what do you see?") });
    let written = stdin(&mut harness, &mut daemon, &id, 3);
    let (downs, _stream) = downs_until(&mut harness, &mut daemon, stream, move |down| shows_said_at(down, session, 0));

    let expected = [
        r#"{"type":"user","message":{"role":"user","content":"[0] !capture\na frame"}}"#,
        r#"{"type":"user","message":{"role":"user","content":"[0] what do you see?"}}"#,
    ];
    assert_eq!(written[1..3], expected);
    let ran = downs.iter().any(|down| {
        matches!(down, Down::Show { events, .. } if events.iter().any(|numbered| matches!(&numbered.entry, Entry::Ran { engine: 0, .. })))
    });
    assert!(ran, "the command is shown where it was run: {downs:?}");

    Ok(())
}

#[test]
fn an_ask_of_claude_s_is_shown_to_trusted_engines_and_their_answer_is_written_as_its_control_response()
-> Result<(), Unheard> {
    // Catches Claude Code's `can_use_tool` request never shown to the owner, so its turn waits for good, and an answer
    // written for another request, or not as the control response Claude Code reads (ADR-0057).
    let mut daemon = Daemon::new("chat-ask");
    let mut harness = chat_hub(&daemon)?;
    let session = open(&mut harness, &mut daemon, "ask")?;
    let id = daemon.made(0);
    let stream = linked_as(&mut harness, &mut daemon, By::Owner, "owner's", 1)?;
    let (_, stream) = downs_until(&mut harness, &mut daemon, stream, |down| matches!(down, Down::Sessions { .. }));

    let request = r#"{"type":"control_request","request_id":"r-9","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
    daemon.emit(&id, request);
    let (downs, mut stream) = downs_until(&mut harness, &mut daemon, stream, |down| matches!(down, Down::Ask { .. }));
    let Some(Down::Ask { ask, session: asking, what }) = downs.last().cloned() else {
        panic!("the ask is shown");
    };
    assert_eq!((asking, what.as_str()), (session, r#"Bash {"command":"ls"}"#));
    send_frame(&mut stream, &Up::Answer { ask, allow: true });
    let written = stdin(&mut harness, &mut daemon, &id, 1);

    let expected = concat!(
        r#"{"type":"control_response","response":{"subtype":"success","request_id":"r-9","#,
        r#""response":{"behavior":"allow","updatedInput":{"command":"ls"}}}}"#
    );
    assert_eq!(written, vec![String::from(expected)]);

    Ok(())
}

#[test]
fn an_actor_that_hears_of_trusted_engines_is_told_those_linked_before_it_and_each_after_and_no_untrusted_one()
-> Result<(), Unheard> {
    // Catches an engine the hub trusted before the sessions' root heard of trusted engines left out, so its chat never
    // reaches the sessions; an untrusted engine told as trusted, which would then be pointed at the sessions; an engine
    // trusted after it heard not told; and one told twice (ADR-0057).
    let mut daemon = Daemon::new("chat-hear");
    let mut harness = chat_hub(&daemon)?;
    let _owner_s = linked_as(&mut harness, &mut daemon, By::Owner, "owner's", 1)?;
    let _session_s = linked_as(&mut harness, &mut daemon, By::Session { session: 1 }, "session's", 2)?;

    pose(&mut harness, &Pose::Hear)?;
    let before = impostor_heard(&mut harness)?;
    let _after = linked_as(&mut harness, &mut daemon, By::Owner, "owner's after", 3)?;
    harness.settle();

    let engines = harness.root::<Engines>();
    let first = harness.state(engines)?.engine(0);
    let after = harness.state(engines)?.engine(2);
    let heard = impostor_heard(&mut harness)?;
    assert_eq!(before.first().copied(), first, "the engine trusted before it heard is told as it hears");
    assert_eq!(before.len(), 1, "the untrusted engine is not told: {before:?}");
    assert_eq!(heard.last().copied(), after, "the engine trusted after is told as its token is taken");
    assert_eq!(heard.len(), 2, "each trusted engine is told once: {heard:?}");

    Ok(())
}

#[test]
fn an_actor_that_joins_as_a_joined_engine_takes_nothing_of_its_place() -> Result<(), Unheard> {
    // Catches a `Joined` that names an engine another actor joined as replacing it: the impostor would be shown the
    // engine's tabs and asks in its place, the engine shown none, and a chat frame the impostor sends as the engine
    // taken as the engine's owner's (#585).
    let mut daemon = Daemon::new("chat-impostor");
    let mut harness = chat_hub(&daemon)?;
    let session = open(&mut harness, &mut daemon, "chat")?;
    let id = daemon.made(0);
    let stream = linked_as(&mut harness, &mut daemon, By::Owner, "owner's", 1)?;
    let (_, stream) = downs_until(&mut harness, &mut daemon, stream, |down| matches!(down, Down::Sessions { .. }));

    pose(&mut harness, &Pose::Join { engine: 0 })?;
    let request = r#"{"type":"control_request","request_id":"r-1","request":{"subtype":"can_use_tool","tool_name":"Bash","input":{"command":"ls"}}}"#;
    daemon.emit(&id, request);
    let (downs, _stream) = downs_until(&mut harness, &mut daemon, stream, |down| matches!(down, Down::Ask { .. }));
    pose(&mut harness, &Pose::Say { engine: 0, session, text: String::from("as the owner") })?;

    let sessions = harness.root::<Sessions>();
    assert_eq!(harness.state(sessions)?.joined(), vec![0]);
    assert!(matches!(downs.last(), Some(Down::Ask { .. })), "the engine is still shown the ask: {downs:?}");
    assert_eq!(impostor_shown(&mut harness)?, 0, "the impostor is shown nothing");
    let entries = read(&mut harness, &mut daemon, session, 0)?;
    let said = entries.iter().filter(|numbered| matches!(numbered.entry, Entry::Said { .. }));
    assert_eq!(said.count(), 0, "nothing the impostor said reached the session: {entries:?}");

    Ok(())
}

#[test]
fn an_actor_joined_as_one_engine_does_not_join_as_another() -> Result<(), Unheard> {
    // Catches one actor holding two engines' numbers, so it is shown everything twice and its chat frames are taken
    // as either engine's.
    let daemon = Daemon::new("chat-twice");
    let mut harness = chat_hub(&daemon)?;

    pose(&mut harness, &Pose::Join { engine: 7 })?;
    let shown = impostor_shown(&mut harness)?;
    pose(&mut harness, &Pose::Join { engine: 8 })?;

    let sessions = harness.root::<Sessions>();
    assert_eq!(harness.state(sessions)?.joined(), vec![7]);
    assert_eq!(shown, 1, "joined, it is shown the tabs");
    assert_eq!(impostor_shown(&mut harness)?, shown, "joining again as another engine shows it nothing more");

    Ok(())
}

#[test]
fn an_engine_that_ends_is_let_go_and_one_linked_after_it_joins() -> Result<(), Unheard> {
    // Catches an engine joined without being watched, whose entry outlives it, so the sessions show and take frames
    // as a gone engine, and an engine that links after one ended refused or shown nothing.
    let mut daemon = Daemon::new("chat-rejoin");
    let mut harness = chat_hub(&daemon)?;
    open(&mut harness, &mut daemon, "chat")?;
    let stream = linked_as(&mut harness, &mut daemon, By::Owner, "first", 1)?;
    let (_, stream) = downs_until(&mut harness, &mut daemon, stream, |down| matches!(down, Down::Sessions { .. }));

    drop(stream);
    let sessions = harness.root::<Sessions>();
    let mut joined = vec![0];
    waiting::until(BUDGET, || {
        harness.step();
        daemon.pump();
        joined = harness.state(sessions).map(Sessions::joined).unwrap_or_default();
        joined.is_empty()
    });
    assert_eq!(joined, Vec::<u32>::new(), "the engine that ended is let go");
    let stream = linked_as(&mut harness, &mut daemon, By::Owner, "second", 1)?;
    let (downs, _stream) = downs_until(&mut harness, &mut daemon, stream, |down| matches!(down, Down::Sessions { .. }));

    assert!(matches!(downs.last(), Some(Down::Sessions { .. })), "the engine linked after is shown the sessions");
    assert_eq!(harness.state(sessions)?.joined(), vec![1]);

    Ok(())
}

/// The sheet of a tool an engine's side holds: what it is is nothing to the chat.
fn sheet() -> ToolSheet {
    let shape = SchemaNodes::of(&ListSessions::SHAPE);

    ToolSheet {
        name: String::from("nothing"),
        description: String::from("Does nothing"),
        input: shape.clone(),
        flags: Vec::new(),
        output: Output::Value(shape),
    }
}

/// How many tools `Engines` lists engine `engine` holding: 0 for one it does not list.
fn tools_of(harness: &mut Harness<Headless>, engine: u32) -> u32 {
    let engines = harness.root::<Engines>();
    let Ok(listing) = harness.ask(engines, &ListEngines) else {
        return 0;
    };
    let Ok(Answered::Yes(list)) = harness.answer(listing) else {
        return 0;
    };

    list.engines.iter().find(|listed| listed.engine == engine).map_or(0, |listed| listed.tools)
}
