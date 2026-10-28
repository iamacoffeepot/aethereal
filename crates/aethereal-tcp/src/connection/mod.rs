//! A connection: its socket, owned by an actor that reads it as a stream paced by its reader's credit, and writes
//! what it is asked, polling on every `Tick` (ADR-0033). An accepted connection and a dialed one stand in two places,
//! so they are two types, which share what they do.

mod accepted;
mod dialed;

pub use accepted::Accepted;
pub use dialed::Dialed;

use crate::framing::Framed;
use crate::{Framing, Socket};
use aethereal_actor::{
    Allows, Beginning, Carries, Ended, Enough, Fault, Given, Handle, Handles, Held, More, Open, Part, Protocol,
    Serving, Stream, StreamKey, Tick, ToAny, ToSender, Turn, Unsent,
};
use aethereal_data::{Bound, Plain};
use std::io::{ErrorKind, Read as _, Write as _};

/// The most bytes a connection reads from its socket in one call.
const CHUNK: usize = 64 * 1024;

/// What a connection is made from: its socket, how it frames what it reads, and the most it holds unsent.
#[derive(Bound)]
#[kind(name = "aether.tcp.connection_config")]
pub struct ConnectionConfig {
    pub socket: Given<Socket>,
    pub framing: Framing,
    pub most_unsent_bytes: u32,
}

/// A connection's stream of frames: opened with [`Read`], its parts each a [`Frame`], ended with how the connection
/// hung up (ADR-0017). Its reader's credit is how many frames the connection may send before it stops reading its
/// socket, so a reader that falls behind slows the peer.
pub struct Reading;

impl Stream for Reading {
    type Open = Read;
    type Part = Frame;
    type Done = HungUp;
}

/// What a reader opens a connection's stream with.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.read")]
pub struct Read;

/// One frame a connection read, cut by its framing.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.frame")]
pub struct Frame {
    pub bytes: Vec<u8>,
}

/// How a connection hung up: the answer that ends its stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.hung_up")]
pub enum HungUp {
    /// The other side closed.
    Peer,
    /// This side asked.
    Shut,
    /// The socket failed.
    Reset,
    /// A frame went past its framing's limit.
    TooLarge,
    /// The connection already has a reader.
    Busy,
}

/// Bytes to write to a connection's peer: under `LengthPrefixed`, one frame.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.write")]
pub struct Write {
    pub bytes: Vec<u8>,
}

/// What became of a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.wrote")]
pub enum Wrote {
    /// Every byte was handed to the kernel.
    Sent,
    /// The connection holds too many bytes unsent to take these.
    Full,
    /// The connection hung up before they were sent.
    HungUp,
}

/// Asks a connection to hang up, or a listener to stop listening.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.shut")]
pub struct Shut;

/// What a connection is written and hung up through, whichever place it stands in.
pub struct Writing;

impl Protocol for Writing {
    type Kinds = (Write, Shut);
}

impl Carries<Write> for Writing {
    type Answers = (Wrote,);
}

impl Carries<Shut> for Writing {
    type Answers = ();
}

/// Who reads a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Reader {
    /// No one yet: the connection does not read its socket.
    Waiting,
    /// The stream `key` names.
    Reading(StreamKey),
}

/// Bytes waiting to be written, and the request they answer once all are.
#[derive(Bound)]
struct Pending {
    bytes: Vec<u8>,
    held: Held<Wrote>,
}

/// What a connection does, whichever place it stands in: its socket, what it has read and not framed, what it has to
/// write, and its reader.
#[derive(Bound)]
pub struct Connection {
    socket: Handle<Socket>,
    framing: Framing,
    read: Vec<u8>,
    unsent: Vec<Pending>,
    /// How many bytes of the first unsent write are written.
    written: u64,
    most_unsent_bytes: u64,
    reading: Serving<Reading>,
    reader: Reader,
}

impl Connection {
    /// The connection `config` gives, polling its socket on every `Tick`.
    pub fn begin<A: Handles<aethereal_actor::On<Tick>>, S>(
        config: ConnectionConfig,
        turn: &mut Beginning<'_, A, S>,
    ) -> Result<Self, Fault> {
        let socket = turn.take(config.socket)?;
        turn.subscribe::<Tick>();

        Ok(Self {
            socket,
            framing: config.framing,
            read: Vec::new(),
            unsent: Vec::new(),
            written: 0,
            most_unsent_bytes: u64::from(config.most_unsent_bytes),
            reading: Serving::new(),
            reader: Reader::Waiting,
        })
    }

    /// Opens the stream `open` asks for: the connection's reader, or a second, finished at once as `Busy`.
    pub fn open<A: Handles<Ended>, S, I>(&mut self, turn: &mut Turn<'_, A, S>, open: Open<Reading>) -> Result<(), Fault>
    where
        S: Allows<ToSender<HungUp>, I>,
    {
        let (key, Read) = self.reading.open::<_, _, I>(turn, open)?;
        if let Reader::Reading(_) = self.reader {
            if let Some(second) = self.reading.take(key) {
                second.finish(turn, &HungUp::Busy)?;
            }
            return Ok(());
        }
        self.reader = Reader::Reading(key);

        Ok(())
    }

    /// Adds the credit `more` grants to the reader's stream.
    pub fn more<A, S>(&mut self, turn: &Turn<'_, A, S>, more: &More) {
        self.reading.more(turn, more);
    }

    /// The reader stops reading: the connection hangs up.
    pub fn enough<A, S>(&mut self, turn: &mut Turn<'_, A, S>, enough: &Enough) -> Result<(), Fault> {
        let Some(stopped) = self.reading.stop(turn, enough) else {
            return Ok(());
        };
        self.reader = Reader::Waiting;
        stopped.finish(turn, &HungUp::Shut)?;

        self.hang_up(turn, HungUp::Shut)
    }

    /// An actor watched has ended: when it was the reader, no one is left to read, and the connection hangs up.
    pub fn ended<A, S>(&mut self, turn: &mut Turn<'_, A, S>, ended: &Ended) -> Result<(), Fault> {
        let gone = self.reading.ended(ended);
        if gone.is_empty() {
            return Ok(());
        }
        self.reader = Reader::Waiting;

        self.hang_up(turn, HungUp::Shut)
    }

    /// Takes `write` to send, or answers `Full` when it would put the connection past what it holds unsent.
    pub fn write<A, S, I>(&mut self, turn: &mut Turn<'_, A, S>, write: Write) -> Result<(), Fault>
    where
        S: Allows<ToSender<Wrote>, I>,
    {
        let bytes = self.framing.written(write.bytes);
        let waiting: u64 = self.unsent.iter().map(|unsent| unsent.bytes.len() as u64).sum::<u64>() - self.written;
        let fits = waiting + bytes.len() as u64 <= self.most_unsent_bytes;
        if !fits {
            turn.reply::<_, I>(&Wrote::Full)?;
            return Ok(());
        }
        let held = turn.hold::<Wrote, I>()?;
        self.unsent.push(Pending { bytes, held });

        Ok(())
    }

    /// Writes what it can of what is unsent, then reads, frames, and sends its reader as many frames as its credit
    /// allows. A call to the socket that would wait returns at once, and the rest waits for the next `Tick`.
    pub fn tick<A, S, I>(&mut self, turn: &mut Turn<'_, A, S>) -> Result<(), Fault>
    where
        S: Allows<ToAny<(Part<Reading>,), Part<Reading>>, I>,
    {
        if let Err(why) = self.send(turn) {
            return self.hang_up(turn, why);
        }
        let Reader::Reading(key) = self.reader else {
            return Ok(());
        };
        let credit = self.reading.get_mut(key).map_or(0, |stream| stream.credit());
        if credit == 0 {
            return Ok(());
        }
        let peer_closed = match self.fill(turn) {
            Ok(closed) => closed,
            Err(why) => return self.hang_up(turn, why),
        };
        loop {
            let Some(stream) = self.reading.get_mut(key) else {
                return Ok(());
            };
            if stream.credit() == 0 {
                return Ok(());
            }
            match self.framing.frame(&self.read) {
                Framed::Frame { skip, bytes } => {
                    let frame: Vec<u8> = self.read.drain(..skip + bytes).skip(skip).collect();
                    let Some(stream) = self.reading.get_mut(key) else {
                        return Ok(());
                    };
                    if let Err(Unsent::Unwritable(error)) = stream.send::<_, _, I>(turn, Frame { bytes: frame }) {
                        return Err(error.into());
                    }
                }
                Framed::More if peer_closed => return self.hang_up(turn, HungUp::Peer),
                Framed::More => return Ok(()),
                Framed::TooLarge => return self.hang_up(turn, HungUp::TooLarge),
            }
        }
    }

    /// Asked to hang up: it does.
    pub fn shut<A, S>(&mut self, turn: &mut Turn<'_, A, S>) -> Result<(), Fault> {
        self.hang_up(turn, HungUp::Shut)
    }

    /// Writes what it can of what is unsent, answering each write once all its bytes are handed to the kernel: those
    /// sent before the socket failed are answered `Sent` all the same.
    fn send<A, S>(&mut self, turn: &mut Turn<'_, A, S>) -> Result<(), HungUp> {
        let mut sent = Vec::new();
        let written = self.write_unsent(turn, &mut sent);
        for held in sent {
            turn.answer(held, &Wrote::Sent).map_err(|_| HungUp::Reset)?;
        }

        written
    }

    /// Writes unsent bytes until the socket would wait, noting in `sent` each write whose bytes all went.
    fn write_unsent<A, S>(&mut self, turn: &mut Turn<'_, A, S>, sent: &mut Vec<Held<Wrote>>) -> Result<(), HungUp> {
        let Ok(Socket(socket)) = turn.open(&mut self.socket) else {
            return Err(HungUp::Reset);
        };
        while let Some(first) = self.unsent.first() {
            let from = usize::try_from(self.written).unwrap_or(usize::MAX);
            match socket.write(&first.bytes[from..]) {
                Ok(0) => return Err(HungUp::Reset),
                Ok(count) => self.written += count as u64,
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return Err(HungUp::Reset),
            }
            let is_whole = self.written == first.bytes.len() as u64;
            if is_whole {
                sent.push(self.unsent.remove(0).held);
                self.written = 0;
            }
        }

        Ok(())
    }

    /// Reads what its socket holds, up to what its framing keeps: whether the peer has closed.
    fn fill<A, S>(&mut self, turn: &mut Turn<'_, A, S>) -> Result<bool, HungUp> {
        let most = self.framing.most();
        let Ok(Socket(socket)) = turn.open(&mut self.socket) else {
            return Err(HungUp::Reset);
        };
        let mut chunk = vec![0; CHUNK];
        while self.read.len() < most {
            let room = (most - self.read.len()).min(CHUNK);
            match socket.read(&mut chunk[..room]) {
                Ok(0) => return Ok(true),
                Ok(count) => self.read.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(false),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Err(HungUp::Reset),
            }
        }

        Ok(false)
    }

    /// Ends the reader's stream with `why`, answers every write not sent, and closes the connection, whose socket the
    /// engine drops after its closing turn (ADR-0010).
    fn hang_up<A, S>(&mut self, turn: &mut Turn<'_, A, S>, why: HungUp) -> Result<(), Fault> {
        if let Reader::Reading(key) = self.reader
            && let Some(stream) = self.reading.take(key)
        {
            stream.finish(turn, &why)?;
        }
        self.reader = Reader::Waiting;
        for unsent in self.unsent.drain(..) {
            turn.answer(unsent.held, &Wrote::HungUp)?;
        }
        turn.close();

        Ok(())
    }
}
