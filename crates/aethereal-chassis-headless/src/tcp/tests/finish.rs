//! A connection's halves ending apart (ADR-0033): asked to finish writing, its write half shut once what it holds is
//! written, its read half open until the peer ends it; and its peer finished writing, its read half ended while it
//! writes on until it is finished.

use super::*;
use aethereal_tcp::Finish;
use std::net::Shutdown;

/// Whatever arrived, in parts of at most 64 KiB: what the peer sends comes back as it was sent.
const CHUNKS: Framing = Framing::Chunks { most_bytes: 64 * 1024 };

/// A connection the root dialed to a loopback listener of the test's, read by the streamer, and the peer the listener
/// accepted, whose reads return at once.
struct Pair {
    harness: Harness<Headless>,
    streamer: ActorRef<Streamer>,
    connection: ActorRef<Dialed>,
    peer: TcpStream,
}

impl Pair {
    /// The pair, the root running on `settings`.
    fn dial(settings: TcpSettings) -> Result<Self, Unheard> {
        let mut harness = Harness::of(Headless::program().root_with::<Tcp>(settings).root::<Streamer>())?;
        let (tcp, streamer) = (harness.root::<Tcp>(), harness.root::<Streamer>());
        let (listener, port) = listening();

        let connected = harness.ask(tcp, &Connect { host: String::from("127.0.0.1"), port, framing: CHUNKS })?;
        let Answered::Yes(Connected::Open { connection, .. }) = harness.answer(connected)? else {
            panic!("the dial connects");
        };
        let Ok((peer, _)) = listener.accept() else {
            panic!("the listener accepts the dial");
        };
        assert!(peer.set_nonblocking(true).is_ok(), "the peer's reads return at once");
        harness.tell(streamer, &Follow::Reading(connection))?;

        Ok(Self { harness, streamer, connection, peer })
    }

    /// Steps the host, the peer reading what has come into `read`, until the peer reads the end or the wait is spent;
    /// and answers whether it did.
    fn read_to_end(&mut self, read: &mut Vec<u8>) -> bool {
        let mut ended = false;
        waiting::until(waiting::BUDGET, || {
            self.harness.step();
            ended = read_some(&mut self.peer, read) == Peered::Ended;

            ended
        });

        ended
    }
}

/// What one read by the peer came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Peered {
    /// It read some bytes.
    Read,
    /// Nothing has come.
    Waiting,
    /// The connection shut its write half: nothing more will come.
    Ended,
}

/// Reads what has come to `peer` into `read`.
fn read_some(peer: &mut TcpStream, read: &mut Vec<u8>) -> Peered {
    let mut buffer = [0; 16 * 1024];
    match peer.read(&mut buffer) {
        Ok(0) => Peered::Ended,
        Ok(count) => {
            read.extend_from_slice(&buffer[..count]);
            Peered::Read
        }
        Err(error) if error.kind() == ErrorKind::WouldBlock => Peered::Waiting,
        Err(error) => panic!("the peer reads: {error}"),
    }
}

#[test]
fn a_connection_finished_writes_all_it_holds_before_its_peer_reads_the_end_and_reads_on() -> Result<(), Unheard> {
    // Catches a write half shut while writes are unsent, so the peer reads the end before every byte; one never shut
    // once they are written, so the peer waits for an end that never comes; and a finish that hangs the whole
    // connection up, so what the peer sends once it has read the end is never read.
    let settings = TcpSettings { most_unsent_bytes: 64 << 20, ..TcpSettings::default() };
    let mut pair = Pair::dial(settings)?;
    // Far more than the kernel holds of a socket whose peer is not reading, so the finish comes with bytes unsent, in
    // a pattern no part's length divides, so a part lost or written twice shows.
    let payload: Vec<u8> = (0..16 << 20_u32).map(|index| u8::try_from(index % 251).unwrap_or(0)).collect();
    let mut wrote = Vec::new();
    for chunk in payload.chunks(1 << 20) {
        let parts = vec![pair.harness.put(chunk.to_vec())?];
        wrote.push(pair.harness.ask(pair.connection.protocol::<Writing>(), &Write { parts })?);
    }
    pair.harness.tell(pair.connection.protocol::<Writing>(), &Finish)?;

    let mut read = Vec::new();
    assert!(pair.read_to_end(&mut read), "the peer reads the end once every byte is written");
    assert_eq!(read.len(), payload.len(), "every byte held is written before the end");
    assert!(read == payload, "the bytes are written in the order they were asked");
    let answers = pair.harness.answers(wrote)?;
    assert!(
        answers.iter().all(|answer| *answer == Answered::Yes(Wrote::Sent)),
        "each write is answered sent: {answers:?}"
    );

    assert!(pair.peer.write_all(b"reply").is_ok(), "the peer replies once it has read the end");
    assert_eq!(heard(&mut pair.harness, pair.streamer, 1)?, [frame(Side::Client, b"reply")]);

    Ok(())
}

#[test]
fn a_write_after_a_finish_is_answered_finished_and_never_reaches_the_peer() -> Result<(), Unheard> {
    // Catches a write taken once the connection was asked to finish: queued behind the shut and made on a socket that
    // refuses it, so it is answered `HungUp` as if the connection had gone, or sent before the end so the peer reads
    // bytes its writer had said were the last.
    let mut pair = Pair::dial(TcpSettings::default())?;

    pair.harness.tell(pair.connection.protocol::<Writing>(), &Finish)?;
    let parts = vec![pair.harness.put(b"late".to_vec())?];
    let late = pair.harness.ask(pair.connection.protocol::<Writing>(), &Write { parts })?;

    assert_eq!(pair.harness.answer(late)?, Answered::Yes(Wrote::Finished));
    let mut read = Vec::new();
    assert!(pair.read_to_end(&mut read), "the peer reads the end");
    assert!(read.is_empty(), "nothing is written after the finish: {read:?}");

    Ok(())
}

/// How each stream `streamer` read has ended, the harness stepping the host until one has or the wait is spent.
fn ended(harness: &mut Harness<Headless>, streamer: ActorRef<Streamer>) -> Result<Vec<HungUp>, Unheard> {
    waiting::until(waiting::BUDGET, || {
        harness.step();

        harness.state(streamer).is_ok_and(|state| !state.ended.is_empty())
    });

    Ok(harness.state(streamer)?.ended.clone())
}

#[test]
fn a_connection_whose_peer_finished_writing_still_writes_to_it_and_closes_once_finished() -> Result<(), Unheard> {
    // Catches the peer's end taken for its hang-up, so a write after it is never sent and a peer that shut only its
    // write half, as `nc -N` and some HTTP clients do, never reads its answer; a read stream left open past the peer's
    // end, so its reader waits for more for good; and a connection that never closes once both halves have ended.
    let mut pair = Pair::dial(TcpSettings::default())?;
    assert!(pair.peer.write_all(b"asked").is_ok(), "the peer writes");
    assert!(pair.peer.shutdown(Shutdown::Write).is_ok(), "the peer shuts its write half");
    assert_eq!(heard(&mut pair.harness, pair.streamer, 1)?, [frame(Side::Client, b"asked")]);
    assert_eq!(ended(&mut pair.harness, pair.streamer)?, [HungUp::Peer]);

    let parts = vec![pair.harness.put(b"answer".to_vec())?];
    let wrote = pair.harness.ask(pair.connection.protocol::<Writing>(), &Write { parts })?;
    assert_eq!(pair.harness.answer(wrote)?, Answered::Yes(Wrote::Sent));
    pair.harness.tell(pair.connection.protocol::<Writing>(), &Finish)?;
    let mut read = Vec::new();
    assert!(pair.read_to_end(&mut read), "the peer reads the end once the connection finishes");
    pair.harness.settle();

    assert_eq!(read, b"answer");
    assert!(pair.harness.state(pair.connection).is_err(), "both halves ended, the connection has closed");

    Ok(())
}
