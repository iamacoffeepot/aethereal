//! The sessions capability on the native host (ADR-0057): a Docker daemon faked by a Unix socket whose containers run
//! a Claude the test speaks for, and, run by hand, the build host's own daemon and Claude Code.
#![cfg(unix)]

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Path, Top, Turn, path,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Plain};
use aethereal_http::{Http, HttpSettings};
use aethereal_sessions::{
    Archive, Archived, Docker, DockerSettings, Ended, Entry, Interrupt, Interrupted, ListSessions, NewSession, Note,
    Numbered, Opened, ReadTranscript, Refusal, Said, Say, Sessions, SessionsListed, SessionsSettings, Speaker, State,
    Transcribed, Transcript,
};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::Tcp;
use core::fmt::Write as _;
use core::mem;
use core::time::Duration;
use std::collections::BTreeMap;
use std::io::{ErrorKind, Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::{env, fs, process};

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
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Unset, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self {
            opened: Vec::new(),
            said: Vec::new(),
            read: Vec::new(),
            listed: Vec::new(),
            interrupted: Vec::new(),
            archived: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Caller {
    const SECTION: &'static str = "caller";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Unset)
    }
}

impl Handles<Order> for Caller {
    type Sends = (
        Asks<Path<Sessions>, NewSession, Asked>,
        Asks<Path<Sessions>, Say, Asked>,
        Asks<Path<Sessions>, ReadTranscript, Asked>,
        Asks<Path<Sessions>, ListSessions, Asked>,
        Asks<Path<Sessions>, Interrupt, Asked>,
        Asks<Path<Sessions>, Archive, Asked>,
    );

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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
        impl Handles<Answer<$answer, Asked>> for Caller {
            type Sends = ();

            fn handle(&mut self, (answered, Asked): (Answered<$answer>, Asked), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
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
            root: self.dir.join("sessions").display().to_string(),
            credential: self.dir.join("claude.env").display().to_string(),
            claude: String::from("/opt/claude"),
            label: String::from("test"),
            labels: vec![String::from("aethereal.test=1")],
            most_running,
            idle_secs,
            ..SessionsSettings::default()
        }
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
                let id = format!("{:064x}", self.made.len() + 1);
                let body = String::from_utf8_lossy(body).into_owned();
                let container = Container { name, body, running: false, output: Vec::new(), stdin: Vec::new() };
                self.containers.insert(id.clone(), container);
                self.made.push(id.clone());
                respond(peer, "201 Created", &format!(r#"{{"Id":"{id}","Warnings":[]}}"#));
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

/// A hub of `Tcp`, `Http`, the Docker client on the fake's socket, the sessions as `sessions` say, and the caller.
fn hub(daemon: &Daemon, sessions: SessionsSettings) -> Result<Harness<Headless>, Unheard> {
    let http = HttpSettings::default();
    let engine = EngineSettings {
        read_limit: http.most_read_bytes().try_into().unwrap_or(u32::MAX),
        ..EngineSettings::default()
    };
    let program = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root_with::<Http>(http)
        .root_with::<Docker>(daemon.docker())
        .root_with::<Sessions>(sessions)
        .root::<Caller>();

    Harness::of(program)
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
    let file = PathBuf::from(&settings.root).join("1").join("transcript");
    until(&mut harness, &mut daemon, |_, _| Transcript::read(&fs::read(&file).unwrap_or_default()).last() == 5);
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

/// How long the live run waits on Claude: a container's start and a model's turn take far longer than the fake's.
const LIVE_BUDGET: Duration = Duration::from_mins(4);

/// What a step of the live run found, written for whoever runs it.
#[allow(clippy::print_stderr)] // A run by hand against a real daemon and Claude reports what it saw, for a person.
fn report(step: &str, found: &str) {
    eprintln!("{step}: {found}");
}

/// Removes every container that carries the live run's label, and the run's directory, with requests of its own
/// written by hand on Docker's socket, whatever became of the run: nothing the run made outlives it.
struct Cleanup {
    label: String,
    root: PathBuf,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        let listed = raw(&listing(&self.label));
        for id in listed.split(r#""Id":""#).skip(1).filter_map(|rest| rest.get(..64)) {
            raw(&format!("DELETE /v1.44/containers/{id}?force=1&v=1"));
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The request that lists every container carrying the live run's `label`.
fn listing(label: &str) -> String {
    let filters = format!(r#"{{"label":["aethereal.sessions={label}"]}}"#);
    let encoded = filters.bytes().fold(String::new(), |mut encoded, byte| {
        let _ = write!(encoded, "%{byte:02X}");
        encoded
    });

    format!("GET /v1.44/containers/json?all=1&filters={encoded}")
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

/// How many of `entries` are Claude's events whose text holds `marker`.
fn holding(entries: &[Numbered], marker: &str) -> usize {
    let held = entries.iter().filter(|numbered| match &numbered.entry {
        Entry::Claude(json) => json.as_str().contains(marker),
        _ => false,
    });

    held.count()
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
#[ignore = "runs Claude Code in a container on this machine's Docker, on the owner's subscription: run by hand with --ignored, naming the run's directory, the Claude binary, the credential's file, and the container's user in AETHEREAL_SESSIONS_LIVE_ROOT, _CLAUDE, _CREDENTIAL, and _USER"]
fn a_session_runs_on_the_machines_own_daemon_and_claude() -> Result<(), Unheard> {
    // Catches what a fake cannot: Claude Code reading the lines the hub writes, writing what the hub reads, taking the
    // interrupt, and the container made from the settings running at all.
    let variable = |name: &str| env::var(format!("AETHEREAL_SESSIONS_LIVE_{name}")).unwrap_or_default();
    let root = PathBuf::from(variable("ROOT"));
    let label = format!("live-{}", process::id());
    let _cleanup = Cleanup { label: label.clone(), root: root.clone() };
    let settings = SessionsSettings {
        root: root.display().to_string(),
        credential: variable("CREDENTIAL"),
        claude: variable("CLAUDE"),
        user: variable("USER"),
        model: String::from("haiku"),
        hub_url: String::new(),
        label: label.clone(),
        labels: vec![String::from("aethereal.test=1")],
        ..SessionsSettings::default()
    };
    let docker = DockerSettings::default();
    let http = HttpSettings::default();
    let engine = EngineSettings {
        read_limit: http.most_read_bytes().try_into().unwrap_or(u32::MAX),
        ..EngineSettings::default()
    };
    let program = Headless::program()
        .with_engine(engine)
        .root::<Tcp>()
        .root_with::<Http>(http)
        .root_with::<Docker>(docker)
        .root_with::<Sessions>(settings)
        .root::<Caller>();
    let mut harness = Harness::of(program)?;
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

    harness.tell(caller_ref, &Order::Say(Say { session, text: String::from("Reply with the word ok.") }))?;
    read_until(&mut harness, &mut entries, &|entries| holding(entries, r#""type":"result""#) >= 1);
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
    let left = raw(&listing(&label));
    let containers = left.rsplit("\r\n\r\n").next().unwrap_or_default().trim().to_owned();
    report("containers left with the run's label", &containers);
    assert!(!left.contains(r#""Id":""#), "no container of the session is left");
    report_transcript(&entries);

    Ok(())
}
