//! The HTTP capability's client on the native host (ADR-0035): asking the capability's own server over loopback TCP,
//! and peers on Unix sockets that write canned bytes, as Docker's Engine API is reached (ADR-0057).
#![cfg(unix)]

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, More, Number, Open, Part,
    ProtocolRef, Streamed, Streams, ToAny, Top, Turn, Unanswered,
};
use aethereal_chassis_headless::Headless;
use aethereal_chassis_headless::waiting::{self, BUDGET};
use aethereal_data::{Bound, Plain};
use aethereal_http::{
    Client, Connect, Connected, Failure, Fetch, Fetched, Finished, Follow, Handling, Header, Http, HttpSettings, Limit,
    Method, Outgoing, Peer, Received, Receiving, Reply, Request, Response, Serve, Served, Status, Switch,
};
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_substrate::{Bootstrap, Bootstrapping, EngineSettings, Host, Unbuilt};
use aethereal_tcp::{HungUp, SocketPath, Tcp, Write, Writing, Wrote};
use std::io::{ErrorKind, Read as _, Write as _};
use std::num::NonZeroU32;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::{env, fs, process};

/// How many parts a follower's stream is kept ahead of it: the head and one part of the body.
const WINDOW: u32 = 2;

/// How a follower takes the parts of the stream it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Pace {
    /// It holds back the credit of each part it takes: a reader that has fallen behind.
    Holding,
    /// It grants the credit of each part as it takes it.
    Taking,
}

/// What the test tells a follower.
#[derive(Bound)]
#[kind(name = "aether.test.http.order")]
enum Order {
    /// Open a stream of `follow` on `client`, taking its parts at `pace`.
    Follow { client: ActorRef<Client>, follow: Follow, pace: Pace },
    /// Grant the credit held back, and take every later part as it comes.
    Release,
}

/// A part a follower took.
#[derive(Debug, Bound)]
enum Heard {
    /// The response's head, of this status.
    Head(u16),
    /// The response's head, of this status, and the connection switched, written through `writing`.
    Switched { status: u16, writing: ProtocolRef<Writing> },
    /// Bytes of the body, or of the switched connection.
    Bytes(Vec<u8>),
}

/// What a follower's begin is given: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.http.unset")]
struct Unset;

/// A root that streams the responses the test has it follow, and keeps what they carry: a stream is opened and its
/// parts taken only by [`Streams`], which only an actor holds.
#[derive(Bound)]
#[kind(name = "aether.test.http.follower")]
struct Follower {
    receiving: Streams<Receiving, ()>,
    pace: Pace,
    /// The stream of each part whose credit it holds back.
    held: Vec<Number>,
    heard: Vec<Heard>,
    finished: Vec<Finished>,
}

impl ChildOf for Follower {
    type Parent = Top;
}

impl Actor for Follower {
    type Config = Unset;
    type Key = ();
    type Handles = (Order, Part<Receiving>, Answer<Finished, Number>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Unset, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let Some(window) = NonZeroU32::new(WINDOW) else {
            return Err(Fault::new(&Unset)?);
        };

        Ok(Self {
            receiving: Streams::new(window),
            pace: Pace::Taking,
            held: Vec::new(),
            heard: Vec::new(),
            finished: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Follower {
    const SECTION: &'static str = "follower";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Unset)
    }
}

impl Handles<Order> for Follower {
    type Sends = (Asks<Streamed<Receiving>, Open<Receiving>, Number>, ToAny<Streamed<Receiving>, More>);

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match order {
            Order::Follow { client, follow, pace } => {
                self.pace = pace;
                self.receiving.open(turn, client.protocol(), follow, ())?;
            }
            Order::Release => {
                self.pace = Pace::Taking;
                for number in self.held.drain(..) {
                    self.receiving.grant(turn, number, NonZeroU32::MIN)?;
                }
            }
        }

        Ok(())
    }
}

impl Handles<Part<Receiving>> for Follower {
    type Sends = (ToAny<Streamed<Receiving>, More>,);

    fn handle(&mut self, part: Part<Receiving>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let taken = match self.pace {
            Pace::Holding => self.receiving.part_held(turn, part),
            Pace::Taking => self.receiving.part(turn, part)?,
        };
        let Some(taken) = taken else {
            return Ok(());
        };
        if self.pace == Pace::Holding {
            self.held.push(taken.number);
        }
        let heard = match taken.value {
            Received::Head(head) => Heard::Head(head.status),
            Received::Switched { head, writing } => Heard::Switched { status: head.status, writing },
            Received::Bytes(bytes) => Heard::Bytes(turn.read(&bytes).to_vec()),
        };
        self.heard.push(heard);

        Ok(())
    }
}

impl Handles<Answer<Finished, Number>> for Follower {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, number): (Answered<Finished>, Number),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.receiving.answered(number);
        if let Answered::Yes(finished) = answered {
            self.finished.push(finished);
        }

        Ok(())
    }
}

impl Heard {
    /// How many bytes of a body or a connection it carries.
    const fn length(&self) -> usize {
        match self {
            Self::Bytes(bytes) => bytes.len(),
            Self::Head(_) | Self::Switched { .. } => 0,
        }
    }
}

impl Follower {
    /// Every byte it has heard, of a body or a switched connection, in order.
    fn bytes(&self) -> Vec<u8> {
        let parts = self.heard.iter().filter_map(|heard| match heard {
            Heard::Bytes(bytes) => Some(bytes.as_slice()),
            Heard::Head(_) | Heard::Switched { .. } => None,
        });

        parts.flatten().copied().collect()
    }
}

/// A host running `settings`, with `Tcp`, `Http`, and a follower.
fn host(settings: HttpSettings) -> Result<Harness<Headless>, Unheard> {
    let engine = EngineSettings {
        read_limit: settings.most_read_bytes().try_into().unwrap_or(u32::MAX),
        ..EngineSettings::default()
    };
    let program = Headless::program().with_engine(engine).root::<Tcp>().root_with::<Http>(settings).root::<Follower>();

    Harness::of(program)
}

/// What the root answers a connect to `peer`.
fn connect(harness: &mut Harness<Headless>, peer: Peer) -> Result<ActorRef<Client>, Unheard> {
    let http = harness.root::<Http>();
    let connecting = harness.ask(http, &Connect { peer })?;
    let Answered::Yes(Connected::Open { client }) = harness.answer(connecting)? else {
        panic!("the client connects");
    };

    Ok(client)
}

/// A request of `method` to `path`, with no header or body.
fn outgoing(harness: &mut Harness<Headless>, method: Method, path: &str, body: &[u8]) -> Result<Outgoing, Unheard> {
    let body = harness.put(body.to_vec())?;

    Ok(Outgoing { method, path: String::from(path), query: String::new(), headers: Vec::new(), body })
}

/// What `client` answers `request` with, whole.
fn fetched(
    harness: &mut Harness<Headless>,
    client: ActorRef<Client>,
    request: Outgoing,
) -> Result<Answered<Fetched>, Unheard> {
    let fetching = harness.ask(client, &Fetch { request })?;

    harness.answer(fetching)
}

/// A directory for a test's socket, named for the test, removed when dropped.
struct Sockets(PathBuf);

impl Sockets {
    fn new(test: &str) -> Self {
        let dir = env::temp_dir().join(format!("aethereal-http-{}-{test}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(fs::create_dir_all(&dir).is_ok(), "the test makes its directory");

        Self(dir)
    }

    /// A listener on the directory's socket, and its path as a peer is named.
    fn listen(&self) -> (UnixListener, Peer) {
        let file = self.0.join("api.sock");
        let Ok(listener) = UnixListener::bind(&file) else {
            panic!("the test binds its socket");
        };
        let Ok(path) = SocketPath::new(file.display().to_string()) else {
            panic!("the test's socket path is absolute and fits a socket's address");
        };

        (listener, Peer::Unix { path })
    }
}

impl Drop for Sockets {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The peer a client dialed, taken from the test's listener, its reads and writes returning at once.
fn peer(listener: &UnixListener) -> UnixStream {
    let Ok((peer, _)) = listener.accept() else {
        panic!("the listener accepts the dial");
    };
    assert!(peer.set_nonblocking(true).is_ok(), "the peer's calls return at once");

    peer
}

/// The head of the request the peer reads, the harness stepping the host until it has all come.
fn request_head(harness: &mut Harness<Headless>, peer: &mut UnixStream) -> String {
    let mut read = Vec::new();
    let mut chunk = [0; 1024];
    waiting::until(BUDGET, || {
        harness.step();
        if let Ok(count) = peer.read(&mut chunk) {
            read.extend_from_slice(&chunk[..count]);
        }

        read.ends_with(b"\r\n\r\n")
    });
    assert!(read.ends_with(b"\r\n\r\n"), "the request's head came: {}", String::from_utf8_lossy(&read));

    String::from_utf8_lossy(&read).into_owned()
}

/// Writes `bytes` to the peer, whose socket holds far more than any test writes this way.
fn send(peer: &mut UnixStream, bytes: &[u8]) {
    assert!(peer.write_all(bytes).is_ok(), "the peer writes");
}

/// The follower's state once `done` holds of it, the harness stepping the host until it does or the wait is spent.
fn heard(harness: &mut Harness<Headless>, mut done: impl FnMut(&Follower) -> bool) -> Result<&Follower, Unheard> {
    let follower = harness.root::<Follower>();
    waiting::until(BUDGET, || {
        harness.step();

        harness.state(follower).is_ok_and(&mut done)
    });

    harness.state(follower)
}

#[test]
fn a_response_is_read_whole_by_its_length_and_the_connection_is_kept_for_the_next() -> Result<(), Unheard> {
    // Catches a request written without `Host` or its body's length, so the server waits for a body or refuses it; a
    // response's body cut short of its `Content-Length` or run into the next; and a connection closed or opened again
    // between requests: both come from one peer address, so they came on one connection.
    let mut harness = host(HttpSettings::default())?;
    let http = harness.root::<Http>();
    let handler = harness.protocol::<Handling>();
    let serving = harness.ask(http, &Serve { port: 0, handler })?;
    let Answered::Yes(Served::Bound { port, .. }) = harness.answer(serving)? else {
        panic!("the port is bound");
    };
    let client = connect(&mut harness, Peer::Tcp { host: String::from("127.0.0.1"), port })?;
    let mut peers = Vec::new();

    for body in [b"hello".as_slice(), b"again, longer"] {
        let request = outgoing(&mut harness, Method::Post, "/echo", body)?;
        let fetching = harness.ask(client, &Fetch { request })?;
        let asked = harness.asked::<Request>()?;
        assert_eq!(asked.request.header("host"), Some(format!("127.0.0.1:{port}").as_str()));
        assert_eq!(harness.read(&asked.request.body), Some(body));
        peers.push(asked.request.peer);
        let mut response = Response::new(Status::Ok, "text/plain", asked.request.body.clone());
        response.headers.push(Header::new("x-echo", "yes"));
        harness.reply(asked, &response)?;

        let Answered::Yes(Fetched::Whole(Reply { status, headers, body: echoed })) = harness.answer(fetching)? else {
            panic!("the response is read whole");
        };
        assert_eq!(status, 200);
        assert!(headers.contains(&Header::new("x-echo", "yes")), "{headers:?}");
        assert_eq!(harness.read(&echoed), Some(body));
    }
    assert_eq!(peers[0], peers[1], "both requests came on one connection");

    Ok(())
}

#[test]
fn a_chunked_response_is_streamed_as_it_arrives_however_its_reads_are_cut() -> Result<(), Unheard> {
    // Catches a streamed body held until it is whole, so a stream that never ends (`GET /events`) delivers nothing; a
    // chunk's size line, its data, or the line ending after it cut between reads and lost, read twice, or misread;
    // and a body that ends without its stream being finished, or finished before its last chunk.
    let sockets = Sockets::new("chunked");
    let (listener, at) = sockets.listen();
    let mut harness = host(HttpSettings::default())?;
    let client = connect(&mut harness, at)?;
    let mut peer = peer(&listener);
    let request = outgoing(&mut harness, Method::Get, "/events", b"")?;
    let follower = harness.root::<Follower>();
    harness.tell(
        follower,
        &Order::Follow { client, follow: Follow { request, switch: Switch::Stay }, pace: Pace::Taking },
    )?;
    let head = request_head(&mut harness, &mut peer);
    assert!(head.starts_with("GET /events HTTP/1.1\r\nhost: localhost\r\n"), "{head}");

    // The head whole, then a size line cut after its first digit.
    send(&mut peer, b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1");
    heard(&mut harness, |follower| !follower.heard.is_empty())?;
    // The rest of the size line, sixteen bytes, and the data cut after six of them.
    send(&mut peer, b"0\r\nfirst-");
    let first = heard(&mut harness, |follower| follower.bytes() == b"first-")?.bytes();
    assert_eq!(first, b"first-", "each piece is handed on as it arrives");
    // The rest of the data, and its line ending cut between its two bytes.
    send(&mut peer, b"chunk!!!!!\r");
    heard(&mut harness, |follower| follower.bytes().len() == 16)?;
    send(&mut peer, b"\n6\r\nsecond\r\n0\r\n\r\n");
    let follower = heard(&mut harness, |follower| !follower.finished.is_empty())?;

    assert!(matches!(follower.heard.first(), Some(Heard::Head(200))), "{:?}", follower.heard);
    assert_eq!(follower.bytes(), b"first-chunk!!!!!second");
    assert_eq!(follower.finished, [Finished::Whole]);

    Ok(())
}

#[test]
fn a_streamed_reader_that_holds_its_credit_holds_the_server_back_until_it_grants_more() -> Result<(), Unheard> {
    // Catches a client that keeps reading its connection, gathering the body, while its reader has no credit, so a
    // reader that falls behind does not slow the server and the body is held whole after all: the client then hands
    // on more at once than its window of chunks holds. Also one that, held for want of credit, never reads again once
    // credit comes, and bytes lost or reordered across the parts between.
    // One chunk of a kilobyte granted at a time, so what the client may hold while its reader has no credit is small
    // beside what the socket holds.
    let settings = HttpSettings { chunk_bytes: 1024, window: 1, ..HttpSettings::default() };
    let most_part = usize::try_from(settings.chunk_bytes * settings.window).unwrap_or(usize::MAX);
    let sockets = Sockets::new("credit");
    let (listener, at) = sockets.listen();
    let mut harness = host(settings)?;
    let client = connect(&mut harness, at)?;
    let mut peer = peer(&listener);
    let request = outgoing(&mut harness, Method::Get, "/containers/7/logs", b"")?;
    let follower = harness.root::<Follower>();
    let follow = Follow { request, switch: Switch::Stay };
    harness.tell(follower, &Order::Follow { client, follow, pace: Pace::Holding })?;
    request_head(&mut harness, &mut peer);
    // One chunk far larger than the socket holds, in a pattern no read's length divides, so a part lost or read twice
    // shows.
    let payload: Vec<u8> = (0..1024 * 1024_u32).map(|index| u8::try_from(index % 251).unwrap_or(0)).collect();
    let mut response =
        format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n", payload.len()).into_bytes();
    response.extend_from_slice(&payload);
    response.extend_from_slice(b"\r\n0\r\n\r\n");
    let mut sent = 0;

    // The engine runs all it can before each write, so a client that read on without credit would have emptied the
    // socket before the server is seen held back.
    waiting::until(BUDGET, || {
        harness.settle();
        let blocked = write_some(&mut peer, &response, &mut sent) == Written::Blocked;
        let parts = harness.state(follower).map_or(0, |state| state.held.len());
        let out_of_credit = parts == usize::try_from(WINDOW).unwrap_or(usize::MAX);

        blocked && out_of_credit
    });
    let held = harness.state(follower)?.held.len();
    assert_eq!(held, usize::try_from(WINDOW).unwrap_or(usize::MAX), "the reader took its window, and holds it");
    assert!(sent < response.len(), "the server is held back once the reader's credit is spent: it sent {sent} bytes");

    harness.tell(follower, &Order::Release)?;
    waiting::until(BUDGET, || {
        harness.step();
        write_some(&mut peer, &response, &mut sent);

        harness.state(follower).is_ok_and(|state| !state.finished.is_empty())
    });
    let follower = harness.state(follower)?;
    assert!(follower.bytes() == payload, "every byte of the body is read, in order, once credit comes");
    assert_eq!(follower.finished, [Finished::Whole]);
    let largest = follower.heard.iter().map(Heard::length).max().unwrap_or(0);
    assert!(largest <= most_part, "no part holds more than the client's window of chunks: one held {largest} bytes");

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
fn a_switched_connection_is_handed_over_raw_with_the_bytes_that_came_with_its_head_first() -> Result<(), Unheard> {
    // Catches bytes that arrived in the same read as the head lost, or parsed as HTTP; a request that asks to switch
    // without both the headers that ask it; a `200` whose body runs until the connection closes, as Docker answers an
    // attach it takes over without `Upgrade`, read as a body rather than handed over; writes through the handed
    // reference not reaching the server; and a switched stream that does not end when the server hangs up.
    let heads: [&[u8]; 2] = [
        b"HTTP/1.1 101 UPGRADED\r\nContent-Type: application/vnd.docker.raw-stream\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Type: application/vnd.docker.raw-stream\r\n\r\n",
    ];
    for (round, answer) in heads.into_iter().enumerate() {
        let sockets = Sockets::new(&format!("switch-{round}"));
        let (listener, at) = sockets.listen();
        let mut harness = host(HttpSettings::default())?;
        let client = connect(&mut harness, at)?;
        let mut peer = peer(&listener);
        let request = outgoing(&mut harness, Method::Post, "/containers/7/attach", b"")?;
        let switch = Switch::To { protocol: String::from("tcp") };
        let follower = harness.root::<Follower>();
        harness.tell(follower, &Order::Follow { client, follow: Follow { request, switch }, pace: Pace::Taking })?;
        let head = request_head(&mut harness, &mut peer);
        assert!(head.contains("\r\nconnection: upgrade\r\nupgrade: tcp\r\n"), "{head}");

        send(&mut peer, &[answer, b"\x01\0\0\0\0\0\0\x05hello"].concat());
        let follower_state = heard(&mut harness, |follower| follower.bytes().len() == 13)?;
        let Some(Heard::Switched { status, writing }) = follower_state.heard.first() else {
            panic!("the connection is handed over: {:?}", follower_state.heard);
        };
        let (status, writing) = (*status, *writing);
        assert_eq!(follower_state.bytes(), b"\x01\0\0\0\0\0\0\x05hello", "the raw bytes come whole, and first");

        let parts = vec![harness.put(b"stdin\n".to_vec())?];
        let wrote = harness.ask(writing, &Write { parts })?;
        assert_eq!(harness.answer(wrote)?, Answered::Yes(Wrote::Sent));
        let mut written = [0; 6];
        waiting::until(BUDGET, || peer.read_exact(&mut written).is_ok());
        assert_eq!(&written, b"stdin\n", "a write through the handed reference reaches the server");

        drop(peer);
        let follower_state = heard(&mut harness, |follower| !follower.finished.is_empty())?;
        assert_eq!(follower_state.finished, [Finished::Closed(HungUp::Peer)], "round {round}: status {status}");
    }

    Ok(())
}

#[test]
fn a_response_cut_off_mid_body_fails_and_its_client_closes() -> Result<(), Unheard> {
    // Catches a body cut short by a hang-up answered as whole, or never answered; and a client that stays to take
    // another request on a connection that is gone.
    let sockets = Sockets::new("cut");
    let (listener, at) = sockets.listen();
    let mut harness = host(HttpSettings::default())?;
    let client = connect(&mut harness, at)?;
    let mut peer = peer(&listener);
    let request = outgoing(&mut harness, Method::Get, "/version", b"")?;
    let fetching = harness.ask(client, &Fetch { request })?;
    request_head(&mut harness, &mut peer);

    send(&mut peer, b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nhalf");
    drop(peer);

    assert!(matches!(harness.answer(fetching)?, Answered::Yes(Fetched::Failed(Failure::HungUp(HungUp::Peer)))));
    let request = outgoing(&mut harness, Method::Get, "/version", b"")?;
    assert!(matches!(fetched(&mut harness, client, request)?, Answered::No(Unanswered::Ended)), "the client closed");

    Ok(())
}

#[test]
fn a_head_past_the_limit_fails_before_it_goes_further() -> Result<(), Unheard> {
    // Catches a response's head read past its limit and held without bound, and a client that stays on a connection
    // left mid-response.
    let settings = HttpSettings { most_reply_head_bytes: 256, ..HttpSettings::default() };
    let sockets = Sockets::new("long");
    let (listener, at) = sockets.listen();
    let mut harness = host(settings)?;
    let client = connect(&mut harness, at)?;
    let mut peer = peer(&listener);
    let request = outgoing(&mut harness, Method::Get, "/info", b"")?;
    let fetching = harness.ask(client, &Fetch { request })?;
    request_head(&mut harness, &mut peer);

    let long = format!("HTTP/1.1 200 OK\r\nX-Long: {}\r\nContent-Length: 0\r\n\r\n", "a".repeat(1024));
    send(&mut peer, long.as_bytes());

    let failed = harness.answer(fetching)?;
    assert!(matches!(failed, Answered::Yes(Fetched::Failed(Failure::TooLarge(Limit::Head)))), "{failed:?}");
    let request = outgoing(&mut harness, Method::Get, "/info", b"")?;
    assert!(matches!(fetched(&mut harness, client, request)?, Answered::No(Unanswered::Ended)), "the client closed");

    Ok(())
}
