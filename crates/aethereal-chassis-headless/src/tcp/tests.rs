//! The TCP capability on the native host (ADR-0033).

use super::*;
use crate::waiting;
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, More, Number, Open, Part,
    Streamed, Streams, Tick, ToAny, Top, Turn,
};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::Host;
use aethereal_substrate::harness::{Harness, Unheard};
use aethereal_tcp::{
    Accept, Accepted, Accepting, Connect, Connected, Dialed, Framing, HungUp, Listen, Listener, Listening, Read,
    Reading, Unbound, Write, Writing, Wrote,
};
use std::io::{ErrorKind, Read as _, Write as _};
use std::net::TcpListener;
use std::num::NonZeroU32;

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

/// One frame of at most a kilobyte, its length first.
const FRAMING: Framing = Framing::LengthPrefixed { most_bytes: 1024 };

/// How many parts a stream is kept ahead of its asker.
const WINDOW: u32 = 4;

/// What the streamer keeps for a stream opened: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.tcp.asked")]
struct Asked;

/// Which end of the round trip a read stream is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Side {
    /// The connection the listener accepted.
    Server,
    /// The connection dialed.
    Client,
}

/// A frame the streamer took, from the stream of the end it was opened on.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
struct Heard {
    side: Side,
    bytes: Vec<u8>,
}

/// What the test has the streamer open a stream on.
#[derive(Bound)]
#[kind(name = "aether.test.tcp.follow")]
enum Follow {
    /// The stream of what this listener accepts.
    Accepting(ActorRef<Listener>),
    /// The stream of what this dialed connection reads.
    Reading(ActorRef<Dialed>),
}

/// A root that opens the streams the test sends it, and keeps what they carry: a stream's request and its parts are
/// made and taken only by [`Streams`], which only an actor holds, so it stands where the test cannot. It writes
/// nothing, and the test asks everything else.
#[derive(Bound)]
#[kind(name = "aether.test.tcp.streamer")]
struct Streamer {
    accepting: Streams<Accepting, Asked>,
    reading: Streams<Reading, Side>,
    /// The connections the listener handed it, in order.
    accepted: Vec<ActorRef<Accepted>>,
    /// The frames it took, in order.
    heard: Vec<Heard>,
}

impl ChildOf for Streamer {
    type Parent = Top;
}

impl Actor for Streamer {
    type Config = Asked;
    type Key = ();
    type Handles = (Follow, Part<Accepting>, Part<Reading>, Answer<Unbound, Number>, Answer<HungUp, Number>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Asked, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        let Some(window) = NonZeroU32::new(WINDOW) else {
            return Err(Fault::new(&Asked)?);
        };

        Ok(Self {
            accepting: Streams::new(window),
            reading: Streams::new(window),
            accepted: Vec::new(),
            heard: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Bootstrap<Headless> for Streamer {
    const SECTION: &'static str = "streamer";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Asked)
    }
}

impl Handles<Follow> for Streamer {
    type Sends = (
        Asks<Streamed<Accepting>, Open<Accepting>, Number>,
        ToAny<Streamed<Accepting>, More>,
        Asks<Streamed<Reading>, Open<Reading>, Number>,
        ToAny<Streamed<Reading>, More>,
    );

    fn handle(&mut self, follow: Follow, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match follow {
            Follow::Accepting(listener) => self.accepting.open(turn, listener.protocol(), Accept, Asked)?,
            Follow::Reading(connection) => self.reading.open(turn, connection.protocol(), Read, Side::Client)?,
        };

        Ok(())
    }
}

impl Handles<Part<Accepting>> for Streamer {
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
        self.accepted.push(connection);
        self.reading.open(turn, connection.protocol(), Read, Side::Server)?;

        Ok(())
    }
}

impl Handles<Part<Reading>> for Streamer {
    type Sends = (ToAny<Streamed<Reading>, More>,);

    fn handle(&mut self, part: Part<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(taken) = self.reading.part(turn, part)? else {
            return Ok(());
        };
        let heard = Heard { side: *taken.kept, bytes: taken.value.bytes };
        self.heard.push(heard);

        Ok(())
    }
}

impl Handles<Answer<Unbound, Number>> for Streamer {
    type Sends = ();

    fn handle(&mut self, (_, number): (Answered<Unbound>, Number), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.accepting.answered(number);

        Ok(())
    }
}

impl Handles<Answer<HungUp, Number>> for Streamer {
    type Sends = ();

    fn handle(&mut self, (_, number): (Answered<HungUp>, Number), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.reading.answered(number);

        Ok(())
    }
}

/// The frames `streamer` has taken, the harness stepping the host until there are `count` of them or the wait is spent.
fn heard(harness: &mut Harness<Headless>, streamer: ActorRef<Streamer>, count: usize) -> Result<Vec<Heard>, Unheard> {
    waiting::until(waiting::BUDGET, || {
        harness.step();

        harness.state(streamer).is_ok_and(|state| state.heard.len() >= count)
    });

    Ok(harness.state(streamer)?.heard.clone())
}

/// The frame `bytes`, heard on `side`.
fn frame(side: Side, bytes: &[u8]) -> Heard {
    Heard { side, bytes: bytes.to_vec() }
}

#[test]
fn a_frame_dialed_to_a_listener_is_accepted_read_and_echoed_back() -> Result<(), Unheard> {
    // Catches a host that serves its runners no dialer (every connect unavailable), a listener that never accepts or a
    // connection never read on `Tick`, a length prefix written or read wrong so the frame is cut or never whole, a
    // write never answered once sent, and a part handed to the wrong stream. The program lists the capability once.
    let mut harness = Harness::of(Headless::program().with_cycle::<(Tick,)>().root::<Tcp>().root::<Streamer>())?;
    let (tcp, streamer) = (harness.root::<Tcp>(), harness.root::<Streamer>());

    let listening = harness.ask(tcp, &Listen { port: 0, framing: FRAMING })?;
    let Answered::Yes(Listening::Bound { listener, port }) = harness.answer(listening)? else {
        panic!("the port is bound");
    };
    harness.tell(streamer, &Follow::Accepting(listener))?;
    let connect = Connect { host: String::from("127.0.0.1"), port, framing: FRAMING };
    let connected = harness.ask(tcp, &connect)?;
    let Answered::Yes(Connected::Open { connection, .. }) = harness.answer(connected)? else {
        panic!("the dial connects");
    };
    harness.tell(streamer, &Follow::Reading(connection))?;

    let wrote = harness.ask(connection.protocol::<Writing, _>(), &Write { bytes: b"ping".to_vec() })?;
    assert_eq!(harness.answer(wrote)?, Answered::Yes(Wrote::Sent));
    assert_eq!(heard(&mut harness, streamer, 1)?, [frame(Side::Server, b"ping")]);

    let &[accepted] = harness.state(streamer)?.accepted.as_slice() else {
        panic!("the listener accepted the one dial");
    };
    let echoed = harness.ask(accepted.protocol::<Writing, _>(), &Write { bytes: b"ping".to_vec() })?;
    assert_eq!(harness.answer(echoed)?, Answered::Yes(Wrote::Sent));
    assert_eq!(heard(&mut harness, streamer, 2)?, [frame(Side::Server, b"ping"), frame(Side::Client, b"ping")]);

    Ok(())
}
