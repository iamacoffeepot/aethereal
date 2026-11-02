//! A connection: its socket, owned by an actor that reads it as a stream paced by its reader's credit, and writes
//! what it is asked, each call made in the handler that makes it possible, and waiting for the socket through the
//! host's poller when a call would wait (ADR-0033). An accepted connection and a dialed one stand in two places, so
//! they are two types, which share what they do.

mod accepted;
mod dialed;

pub use accepted::Accepted;
pub use dialed::Dialed;

use crate::framing::Framed;
use crate::ready::{Ready, Wait};
use crate::{Framing, Interest, Readied, Socket};
use aethereal_actor::{
    Allows, Answer, Answered, Beginning, Carries, Ended, Enough, Fault, Given, Handle, Handles, Held, More, Open, Part,
    Protocol, Serving, Stream, StreamKey, ToAny, ToSender, Turn, Unsent,
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

/// How a read of the socket stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filled {
    /// The socket would wait: nothing more has arrived.
    Blocked,
    /// The buffer holds as much as the framing keeps.
    Full,
    /// The peer closed.
    Closed,
}

/// Bytes waiting to be written, and the request they answer once all are.
#[derive(Bound)]
struct Pending {
    bytes: Vec<u8>,
    held: Held<Wrote>,
}

/// What a connection does, whichever place it stands in: its socket, what it has read and not framed, what it has to
/// write, its reader, and what it waits on its socket for.
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
    readable: Wait,
    writable: Wait,
}

/// What a connection's actor sends as it reads: the parts of its reader's stream.
type Parts = ToAny<(Part<Reading>,), Part<Reading>>;

impl Connection {
    /// The connection `config` gives, which makes no call on its socket until it has a reader with credit or a write.
    pub fn begin<A, S>(config: ConnectionConfig, turn: &mut Beginning<'_, A, S>) -> Result<Self, Fault> {
        let socket = turn.take(config.socket)?;

        Ok(Self {
            socket,
            framing: config.framing,
            read: Vec::new(),
            unsent: Vec::new(),
            written: 0,
            most_unsent_bytes: u64::from(config.most_unsent_bytes),
            reading: Serving::new(),
            reader: Reader::Waiting,
            readable: Wait::Not,
            writable: Wait::Not,
        })
    }

    /// Opens the stream `open` asks for: the connection's reader, or a second, finished at once as `Busy`. The reader's
    /// credit follows the request, and its `More` is what reads.
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

    /// Adds the credit `more` grants to the reader's stream, and reads with it.
    pub fn more<A: Handles<Answer<Readied, Interest>>, S, I>(
        &mut self,
        turn: &mut Turn<'_, A, S>,
        more: &More,
    ) -> Result<(), Fault>
    where
        S: Allows<Parts, I>,
    {
        self.reading.more(turn, more);

        self.read(turn)
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

    /// Takes `write` to send, and writes what the socket takes of it now, or answers `Full` when it would put the
    /// connection past what it holds unsent.
    pub fn write<A: Handles<Answer<Readied, Interest>>, S, I>(
        &mut self,
        turn: &mut Turn<'_, A, S>,
        write: Write,
    ) -> Result<(), Fault>
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

        self.send(turn)
    }

    /// What a wait for the socket came to: it is ready for `interest`, or may be, so the calls that waited on it are
    /// made again; or the host has no poller, and the socket can never be waited on, so the connection hangs up.
    pub fn readied<A: Handles<Answer<Readied, Interest>>, S, I>(
        &mut self,
        turn: &mut Turn<'_, A, S>,
        (readied, interest): (Answered<Readied>, Interest),
    ) -> Result<(), Fault>
    where
        S: Allows<Parts, I>,
    {
        match interest {
            Interest::Readable => self.readable = Wait::Not,
            Interest::Writable => self.writable = Wait::Not,
        }
        let is_ready = readied == Answered::Yes(Readied::Ready);
        if !is_ready {
            return self.hang_up(turn, HungUp::Reset);
        }

        match interest {
            Interest::Readable => self.read(turn),
            Interest::Writable => self.send(turn),
        }
    }

    /// Asked to hang up: it does.
    pub fn shut<A, S>(&mut self, turn: &mut Turn<'_, A, S>) -> Result<(), Fault> {
        self.hang_up(turn, HungUp::Shut)
    }

    /// Writes what it can of what is unsent, answering each write once all its bytes are handed to the kernel: those
    /// sent before the socket failed are answered `Sent` all the same. What the socket would not take waits for it to
    /// be writable.
    fn send<A: Handles<Answer<Readied, Interest>>, S>(&mut self, turn: &mut Turn<'_, A, S>) -> Result<(), Fault> {
        let mut sent = Vec::new();
        let written = self.write_unsent(turn, &mut sent);
        for held in sent {
            turn.answer(held, &Wrote::Sent)?;
        }
        if let Err(why) = written {
            return self.hang_up(turn, why);
        }
        let blocked = !self.unsent.is_empty();
        if blocked {
            self.wait(turn, Interest::Writable)?;
        }

        Ok(())
    }

    /// Reads, frames, and sends its reader as many frames as its credit allows. A read that would wait waits for the
    /// socket to be readable; one stopped for want of credit waits for the reader's `More`.
    fn read<A: Handles<Answer<Readied, Interest>>, S, I>(&mut self, turn: &mut Turn<'_, A, S>) -> Result<(), Fault>
    where
        S: Allows<Parts, I>,
    {
        let Reader::Reading(key) = self.reader else {
            return Ok(());
        };
        loop {
            let has_credit = self.reading.get_mut(key).is_some_and(|stream| stream.credit() > 0);
            if !has_credit {
                return Ok(());
            }
            let filled = match self.fill(turn) {
                Ok(filled) => filled,
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
                    Framed::More => break,
                    Framed::TooLarge => return self.hang_up(turn, HungUp::TooLarge),
                }
            }
            // A buffer that was full has room again once its frames are sent: a whole frame always fits it.
            match filled {
                Filled::Full => {}
                Filled::Blocked => return self.wait(turn, Interest::Readable),
                Filled::Closed => return self.hang_up(turn, HungUp::Peer),
            }
        }
    }

    /// Runs a [`Ready`] for `interest`, unless one is in flight already.
    fn wait<A: Handles<Answer<Readied, Interest>>, S>(
        &mut self,
        turn: &mut Turn<'_, A, S>,
        interest: Interest,
    ) -> Result<(), Fault> {
        let waits = match interest {
            Interest::Readable => self.readable,
            Interest::Writable => self.writable,
        };
        if let Wait::On(_) = waits {
            return Ok(());
        }
        let descriptor = turn.open(&mut self.socket)?.descriptor();
        let job = turn.run(&Ready::new(descriptor, interest), &interest)?;
        match interest {
            Interest::Readable => self.readable = Wait::On(job),
            Interest::Writable => self.writable = Wait::On(job),
        }

        Ok(())
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

    /// Reads what its socket holds, up to what its framing keeps, and says how it stopped.
    fn fill<A, S>(&mut self, turn: &mut Turn<'_, A, S>) -> Result<Filled, HungUp> {
        let most = self.framing.most();
        let Ok(Socket(socket)) = turn.open(&mut self.socket) else {
            return Err(HungUp::Reset);
        };
        let mut chunk = vec![0; CHUNK];
        while self.read.len() < most {
            let room = (most - self.read.len()).min(CHUNK);
            match socket.read(&mut chunk[..room]) {
                Ok(0) => return Ok(Filled::Closed),
                Ok(count) => self.read.extend_from_slice(&chunk[..count]),
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(Filled::Blocked),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Err(HungUp::Reset),
            }
        }

        Ok(Filled::Full)
    }

    /// Ends the reader's stream with `why`, answers every write not sent, stops its waits for the socket, and closes
    /// the connection, whose socket the engine drops after its closing turn (ADR-0010).
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
        for wait in [self.readable, self.writable] {
            if let Wait::On(job) = wait {
                turn.cancel(job);
            }
        }
        self.readable = Wait::Not;
        self.writable = Wait::Not;
        turn.close();

        Ok(())
    }
}
