//! Connections dialed to Unix sockets on the native host (ADR-0033): each test against a real socket in a directory of
//! its own under the system's temporary directory.

use super::*;
use aethereal_tcp::{ConnectUnix, ConnectedUnix, SocketPath};
use std::fs;
use std::io::{Read as _, Write as _};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::{env, process};

/// A directory for a test's sockets, named for the test, removed when dropped.
struct Sockets(PathBuf);

impl Sockets {
    fn new(test: &str) -> Self {
        let dir = env::temp_dir().join(format!("aethereal-unix-{}-{test}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert!(fs::create_dir_all(&dir).is_ok(), "the test makes its directory");

        Self(dir)
    }

    /// Where the socket `name` is in the directory.
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// The socket `name` in the directory, as the root is asked to dial it.
    fn path(&self, name: &str) -> SocketPath {
        let Ok(path) = SocketPath::new(self.file(name).display().to_string()) else {
            panic!("the test's socket path is absolute and fits a socket's address");
        };

        path
    }

    /// A listener on the socket `name`, which blocks.
    fn listen(&self, name: &str) -> UnixListener {
        let Ok(listener) = UnixListener::bind(self.file(name)) else {
            panic!("the test binds its socket");
        };

        listener
    }
}

impl Drop for Sockets {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// What the root answers a connect to `path`, framed by `framing`.
fn connect(
    harness: &mut Harness<Headless>,
    tcp: ActorRef<Tcp>,
    path: SocketPath,
    framing: Framing,
) -> Result<Answered<ConnectedUnix>, Unheard> {
    let connected = harness.ask(tcp, &ConnectUnix { path, framing })?;

    harness.answer(connected)
}

/// The connection `answered` opened.
fn opened(answered: &Answered<ConnectedUnix>) -> ActorRef<Dialed> {
    let Answered::Yes(ConnectedUnix::Open { connection }) = answered else {
        panic!("the dial connects: {answered:?}");
    };

    *connection
}

/// The peer a dial reached, taken from the test's listener: the dial has connected, so it waits in the backlog.
fn peer(listener: &UnixListener) -> UnixStream {
    let Ok((peer, _)) = listener.accept() else {
        panic!("the listener accepts the dial");
    };

    peer
}

#[test]
fn a_dial_tells_a_path_with_no_socket_from_a_socket_nothing_listens_on() -> Result<(), Unheard> {
    // Catches a refusal answered as a missing socket or the other way round, so a caller cannot tell a service that is
    // not there from one that has stopped and left its socket behind.
    let sockets = Sockets::new("refused");
    drop(sockets.listen("stopped.sock"));
    let mut harness = Harness::of(Headless::program().root::<Tcp>())?;
    let tcp = harness.root::<Tcp>();

    let missing = connect(&mut harness, tcp, sockets.path("missing.sock"), FRAMING)?;
    let stopped = connect(&mut harness, tcp, sockets.path("stopped.sock"), FRAMING)?;

    assert!(matches!(missing, Answered::Yes(ConnectedUnix::Missing)), "no socket is at the path: {missing:?}");
    assert!(matches!(stopped, Answered::Yes(ConnectedUnix::Refused)), "nothing listens on it: {stopped:?}");

    Ok(())
}

#[test]
fn a_frame_written_to_a_unix_socket_is_read_back_and_the_connection_hangs_up() -> Result<(), Unheard> {
    // Catches a host whose dialer never connects a Unix socket, or hands it back blocking, so the connection's first
    // read stalls its turn; a connect the root never answers, or answers before the connection stands; bytes written to
    // or read from a Unix connection's socket wrong; and a connection shut that leaves its socket open, so the peer
    // never sees it hang up.
    let sockets = Sockets::new("echo");
    let listener = sockets.listen("echo.sock");
    let mut harness = Harness::of(Headless::program().root::<Tcp>().root::<Streamer>())?;
    let (tcp, streamer) = (harness.root::<Tcp>(), harness.root::<Streamer>());

    let connection = opened(&connect(&mut harness, tcp, sockets.path("echo.sock"), FRAMING)?);
    let mut peer = peer(&listener);
    harness.tell(streamer, &Follow::Reading(connection))?;
    let parts = vec![harness.put(b"pi".to_vec())?, harness.put(b"ng".to_vec())?];
    let wrote = harness.ask(connection.protocol::<Writing, _>(), &Write { parts })?;
    assert_eq!(harness.answer(wrote)?, Answered::Yes(Wrote::Sent));
    let mut sent = [0; 8];
    assert!(peer.read_exact(&mut sent).is_ok(), "the peer reads the frame");
    assert_eq!(&sent, b"\0\0\0\x04ping", "one frame of both parts, its length first");

    assert!(peer.write_all(b"\0\0\0\x04pong").is_ok(), "the peer replies");
    assert_eq!(heard(&mut harness, streamer, 1)?, [frame(Side::Client, b"pong")]);

    harness.tell(connection.protocol::<Writing, _>(), &Shut)?;
    assert!(peer.set_nonblocking(true).is_ok(), "the peer's reads return at once");
    let mut byte = [0];
    let mut read = Err(ErrorKind::WouldBlock);
    waiting::until(waiting::BUDGET, || {
        harness.step();
        read = peer.read(&mut byte).map_err(|error| error.kind());

        read != Err(ErrorKind::WouldBlock)
    });
    assert_eq!(read, Ok(0), "the peer sees the connection hang up");

    Ok(())
}

/// How a holder takes the parts of the stream it reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Pace {
    /// It holds back the credit of each part it takes.
    Holding,
    /// It grants the credit of each part as it takes it.
    Taking,
}

/// What the test tells a holder.
#[derive(Bound)]
#[kind(name = "aether.test.tcp.hold")]
enum Hold {
    /// Read this connection, holding back the credit of each part taken.
    Read(ActorRef<Dialed>),
    /// Grant the credit held back, and take every later part as it comes.
    Release,
}

/// A root that reads one connection and keeps its bytes, holding back its credit until the test releases it: a reader
/// that has fallen behind.
#[derive(Bound)]
#[kind(name = "aether.test.tcp.holder")]
struct Holder {
    reading: Streams<Reading, ()>,
    pace: Pace,
    /// The stream of each part whose credit it holds back.
    held: Vec<Number>,
    /// The bytes it took, in order.
    bytes: Vec<u8>,
}

impl ChildOf for Holder {
    type Parent = Top;
}

impl Actor for Holder {
    type Config = Asked;
    type Key = ();
    type Handles = (Hold, Part<Reading>, Answer<HungUp, Number>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Asked, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let Some(window) = NonZeroU32::new(WINDOW) else {
            return Err(Fault::new(&Asked)?);
        };

        Ok(Self { reading: Streams::new(window), pace: Pace::Holding, held: Vec::new(), bytes: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Holder {
    const SECTION: &'static str = "holder";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Asked)
    }
}

impl Handles<Hold> for Holder {
    type Sends = (Asks<Streamed<Reading>, Open<Reading>, Number>, ToAny<Streamed<Reading>, More>);

    fn handle(&mut self, hold: Hold, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match hold {
            Hold::Read(connection) => {
                self.reading.open(turn, connection.protocol(), Read, ())?;
            }
            Hold::Release => {
                self.pace = Pace::Taking;
                for number in self.held.drain(..) {
                    self.reading.grant(turn, number, NonZeroU32::MIN)?;
                }
            }
        }

        Ok(())
    }
}

impl Handles<Part<Reading>> for Holder {
    type Sends = (ToAny<Streamed<Reading>, More>,);

    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let taken = match self.pace {
            Pace::Holding => self.reading.part_held(turn, part),
            Pace::Taking => self.reading.part(turn, part)?,
        };
        let Some(taken) = taken else {
            return Ok(());
        };
        if self.pace == Pace::Holding {
            self.held.push(taken.number);
        }
        self.bytes.extend_from_slice(turn.read(&taken.value.bytes));

        Ok(())
    }
}

impl Handles<Answer<HungUp, Number>> for Holder {
    type Sends = ();

    fn handle(&mut self, (_, number): (Answered<HungUp>, Number), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.reading.answered(number);

        Ok(())
    }
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
fn a_unix_connection_whose_reader_holds_its_credit_holds_its_peer_back_until_it_grants_more() -> Result<(), Unheard> {
    // Catches a connection that keeps reading its socket, holding what it reads, while its reader has no credit, so a
    // reader that falls behind does not slow the peer; one that, stopped for want of credit, never reads again once
    // credit comes, as no wait for the socket is in flight to wake it; and bytes lost or reordered across the frames
    // read between, a frame sent past its reader's credit among them.
    let sockets = Sockets::new("credit");
    let listener = sockets.listen("credit.sock");
    let mut harness = Harness::of(Headless::program().root::<Tcp>().root::<Holder>())?;
    let (tcp, holder) = (harness.root::<Tcp>(), harness.root::<Holder>());
    let framing = Framing::Chunks { most_bytes: 64 * 1024 };
    let connection = opened(&connect(&mut harness, tcp, sockets.path("credit.sock"), framing)?);
    let mut peer = peer(&listener);
    assert!(peer.set_nonblocking(true).is_ok(), "the peer's writes return at once");
    harness.tell(holder, &Hold::Read(connection))?;
    // Far more than the kernel holds of a socket and the connection keeps for its reader together, in a pattern no
    // frame's length divides, so a frame lost or read twice shows.
    let payload: Vec<u8> = (0..4 * 1024 * 1024_u32).map(|index| u8::try_from(index % 251).unwrap_or(0)).collect();
    let mut sent = 0;

    waiting::until(waiting::BUDGET, || {
        harness.step();
        let written = write_some(&mut peer, &payload, &mut sent);
        let blocked = written == Written::Blocked;
        let parts = harness.state(holder).map_or(0, |state| state.held.len());
        let out_of_credit = parts == usize::try_from(WINDOW).unwrap_or(usize::MAX);

        blocked && out_of_credit
    });
    let held = harness.state(holder)?.held.len();
    assert_eq!(held, usize::try_from(WINDOW).unwrap_or(usize::MAX), "the reader took its window, and holds it");
    assert!(sent < payload.len(), "the peer is held back once the reader's credit is spent: it sent {sent} bytes");

    harness.tell(holder, &Hold::Release)?;
    waiting::until(waiting::BUDGET, || {
        harness.step();
        write_some(&mut peer, &payload, &mut sent);

        harness.state(holder).is_ok_and(|state| state.bytes.len() >= payload.len())
    });
    let bytes = &harness.state(holder)?.bytes;
    assert_eq!(bytes.len(), payload.len(), "every byte the peer sent is read once credit comes");
    assert!(*bytes == payload, "the bytes are read in the order they were sent");

    Ok(())
}
