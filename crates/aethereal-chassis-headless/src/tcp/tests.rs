//! The TCP capability on the native host (ADR-0033).

use super::*;
use crate::{Exit, waiting};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, More, Number, On, Open, Part,
    Path, Streamed, Streams, Tick, ToAny, Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::Host;
use aethereal_tcp::{
    Accept, Accepted, Accepting, Connect, Connected, Framing, HungUp, Listen, Listening, Read, Reading, Unbound, Write,
    Writing, Wrote,
};
use std::io::{ErrorKind, Read as _, Write as _};
use std::net::TcpListener;
use std::num::NonZeroU32;
use std::sync::Mutex;

/// A dialer on `threads` threads, giving each address a second.
fn dialers(threads: usize) -> Dialers {
    let Some(threads) = NonZeroUsize::new(threads) else {
        panic!("the test dials on some threads");
    };

    Dialers::new(threads, Duration::from_secs(1))
}

/// A listener on a loopback port the host picks, and its port.
fn listening() -> (TcpListener, u16) {
    let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
        panic!("the test binds a loopback port");
    };
    let Ok(address) = listener.local_addr() else {
        panic!("a bound listener has an address");
    };

    (listener, address.port())
}

#[test]
fn a_dial_connects_to_a_listener_and_hands_back_a_stream_that_does_not_block() {
    // Catches a dial that never wakes the job that polls it, a stream handed back still blocking, so the first read
    // of an actor that polls it stalls its turn, and many dials at once on few threads lost or handed to the wrong
    // dial: each listener answers with its own byte.
    let dialer = dialers(2);
    let listeners: Vec<(TcpListener, u16)> = (0..4).map(|_| listening()).collect();
    let dials: Vec<Dialing<'_>> = listeners.iter().map(|(_, port)| dialer.dial("127.0.0.1", *port)).collect();
    let mut streams = Vec::new();
    for dialing in dials {
        let Ok(stream) = pollster::block_on(dialing) else {
            panic!("the dial connects");
        };
        streams.push(stream);
    }
    let mut byte = [0];
    let mut unsent = streams.iter_mut().map(|stream| stream.read(&mut byte).map_err(|error| error.kind()));
    assert!(unsent.all(|read| read == Err(ErrorKind::WouldBlock)), "a read with nothing sent returns at once");
    for (number, (listener, _)) in listeners.iter().enumerate() {
        let Ok((mut accepted, _)) = listener.accept() else {
            panic!("the listener accepts the dial");
        };
        let byte = u8::try_from(number).unwrap_or(u8::MAX);
        assert!(accepted.write_all(&[byte]).is_ok(), "the listener writes");
    }

    let mut read = Vec::new();
    for mut stream in streams {
        let mut byte = [0];
        waiting::until(waiting::BUDGET, || stream.read(&mut byte).is_ok_and(|count| count == 1));
        read.push(byte[0]);
    }
    assert_eq!(read, [0, 1, 2, 3]);
}

#[test]
fn a_dial_to_a_port_nothing_listens_on_is_refused() {
    // Catches a refused connect answered as a name that found no address, so a caller cannot tell a host that is down
    // from a host that does not exist.
    let dialer = dialers(1);
    let (listener, port) = listening();
    drop(listener);

    assert_eq!(pollster::block_on(dialer.dial("127.0.0.1", port)).err(), Some(Undialed::Refused));
}

/// What the echo test's actor said, in order.
static SAID: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Notes what the echo test's actor did.
fn say(line: String) {
    SAID.lock().map(|mut said| said.push(line)).ok();
}

/// What the echo actor keeps for an ask of the root.
#[derive(Plain)]
#[kind(name = "aether.test.tcp.asked")]
struct Asked;

/// What the echo actor keeps for a write: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.tcp.writing")]
struct Writ;

/// Which end of the round trip a read stream is.
#[derive(Bound)]
#[kind(name = "aether.test.tcp.side")]
enum Side {
    /// The connection the listener accepted: what it reads is echoed back.
    Server(ActorRef<Accepted>),
    /// The connection dialed: what it reads is the echo.
    Client,
}

/// A root that listens on a port the host picks, accepts, dials that port, writes a frame on the dialed connection,
/// echoes it back on the accepted one, and asks the program to stop once the echo has come back; or gives up after
/// two hundred ticks.
#[derive(Bound)]
#[kind(name = "aether.test.tcp.echo")]
struct Echo {
    accepting: Streams<Accepting, Asked>,
    reading: Streams<Reading, Side>,
    ticks: u64,
}

impl ChildOf for Echo {
    type Parent = Top;
}

/// One frame of at most a kilobyte, its length first.
const FRAMING: Framing = Framing::LengthPrefixed { most_bytes: 1024 };

impl Actor for Echo {
    type Config = Asked;
    type Key = ();
    type Handles = (
        On<Tick>,
        Answer<Listening, Asked>,
        Answer<Connected, Asked>,
        Part<Accepting>,
        Part<Reading>,
        Answer<Unbound, Number>,
        Answer<HungUp, Number>,
        Answer<Wrote, Writ>,
    );
    type BeginSends = (Asks<Path<Tcp>, Listen, Asked>,);
    type EndSends = ();

    fn begin(_: Asked, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();
        turn.to(path::<Tcp>(())).ask(&Listen { port: 0, framing: FRAMING }, &Asked)?;
        let Some(window) = NonZeroU32::new(4) else {
            return Err(Fault::new(&Asked)?);
        };

        Ok(Self { accepting: Streams::new(window), reading: Streams::new(window), ticks: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Echo {
    const SECTION: &'static str = "echo";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Asked)
    }
}

impl Handles<On<Tick>> for Echo {
    type Sends = ();

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.ticks += 1;
        let is_long = self.ticks == 200;
        if is_long {
            say(String::from("gave up"));
            turn.quit();
        }

        Ok(())
    }
}

impl Handles<Answer<Listening, Asked>> for Echo {
    type Sends = (
        Asks<Streamed<Accepting>, Open<Accepting>, Number>,
        ToAny<Streamed<Accepting>, More>,
        Asks<Path<Tcp>, Connect, Asked>,
    );

    fn handle(
        &mut self,
        (answered, _): (Answered<Listening>, Asked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Listening::Bound { listener, port }) = answered else {
            say(format!("listen {answered:?}"));
            turn.quit();
            return Ok(());
        };
        self.accepting.open(turn, listener.protocol(), Accept, Asked)?;
        let connect = Connect { host: String::from("127.0.0.1"), port, framing: FRAMING };
        turn.to(path::<Tcp>(())).ask(&connect, &Asked)?;

        Ok(())
    }
}

impl Handles<Part<Accepting>> for Echo {
    type Sends = (
        ToAny<Streamed<Accepting>, More>,
        Asks<Streamed<Reading>, Open<Reading>, Number>,
        ToAny<Streamed<Reading>, More>,
    );

    fn handle(&mut self, part: Part<Accepting>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(taken) = self.accepting.part(turn, part)? else {
            return Ok(());
        };
        let connection = taken.value.connection;
        say(String::from("accepted"));
        self.reading.open(turn, connection.protocol(), Read, Side::Server(connection))?;

        Ok(())
    }
}

impl Handles<Answer<Connected, Asked>> for Echo {
    type Sends =
        (Asks<Streamed<Reading>, Open<Reading>, Number>, ToAny<Streamed<Reading>, More>, Asks<Writing, Write, Writ>);

    fn handle(
        &mut self,
        (answered, _): (Answered<Connected>, Asked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Connected::Open { connection, .. }) = answered else {
            say(format!("connect {answered:?}"));
            turn.quit();
            return Ok(());
        };
        say(String::from("connected"));
        self.reading.open(turn, connection.protocol(), Read, Side::Client)?;
        turn.to(connection.protocol::<Writing, _>()).ask(&Write { bytes: b"ping".to_vec() }, &Writ)?;

        Ok(())
    }
}

impl Handles<Part<Reading>> for Echo {
    type Sends = (ToAny<Streamed<Reading>, More>, Asks<Writing, Write, Writ>);

    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(taken) = self.reading.part(turn, part)? else {
            return Ok(());
        };
        let bytes = taken.value.bytes;
        match taken.kept {
            Side::Server(connection) => {
                let connection = *connection;
                say(format!("server read {}", String::from_utf8_lossy(&bytes)));
                turn.to(connection.protocol::<Writing, _>()).ask(&Write { bytes }, &Writ)?;
            }
            Side::Client => {
                say(format!("client read {}", String::from_utf8_lossy(&bytes)));
                turn.quit();
            }
        }

        Ok(())
    }
}

impl Handles<Answer<Unbound, Number>> for Echo {
    type Sends = ();

    fn handle(&mut self, (_, number): (Answered<Unbound>, Number), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.accepting.answered(number);

        Ok(())
    }
}

impl Handles<Answer<HungUp, Number>> for Echo {
    type Sends = ();

    fn handle(&mut self, (_, number): (Answered<HungUp>, Number), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.reading.answered(number);

        Ok(())
    }
}

impl Handles<Answer<Wrote, Writ>> for Echo {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<Wrote>, Writ), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        say(format!("wrote {answered:?}"));

        Ok(())
    }
}

#[test]
fn a_frame_dialed_to_a_listener_is_accepted_read_and_echoed_back() {
    // Catches a host that serves its runners no dialer (every connect unavailable), a listener that never accepts or a
    // connection never read on `Tick`, a length prefix written or read wrong so the frame is cut or never whole, a
    // write never answered once sent, and a part handed to the wrong stream. The program lists the capability once.
    let built = Headless::program().with_cycle::<(Tick,)>().root::<Tcp>().root::<Echo>().build();
    let Ok(headless) = built else {
        panic!("the program builds");
    };

    let exit = headless.run(Duration::from_millis(2), Duration::from_secs(30));

    assert_eq!(exit, Exit::Stopped);
    let Ok(said) = SAID.lock() else {
        panic!("what was said is not poisoned");
    };
    let mut said = said.clone();
    said.sort();
    assert_eq!(
        said,
        ["accepted", "client read ping", "connected", "server read ping", "wrote Yes(Sent)", "wrote Yes(Sent)"]
    );
}
