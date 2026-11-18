//! A connection: its socket, owned by an actor that reads it as a stream paced by its reader's credit, and writes
//! what it is asked, each call made in the handler that makes it possible, and waiting for the socket through the
//! host's poller when a call would wait (ADR-0033). Its write half ends apart from its read half: when its holder
//! finishes writing, or a write fails, it reads on until the read half ends. An accepted connection and a dialed one
//! stand in two places, so they are two types, which share what they do.

mod accepted;
mod dialed;
#[cfg(test)]
mod tests;

pub use accepted::Accepted;
pub use dialed::Dialed;

use crate::framing::Framed;
use crate::ready::{Ready, Wait};
use crate::{Framing, Interest, Readied, Socket};
use aethereal_actor::{
    Answer, Answered, Beginning, Bytes, Ended, Enough, Fault, Given, Handle, Held, Later, More, Open, Receives, Reply,
    Serving, Stream, StreamKey, Turn, Unsent, protocol,
};
use aethereal_data::{Bound, Plain};
use std::io::{self, ErrorKind, IoSlice, Read as _};
use std::{iter, mem};

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

/// One frame a connection read, cut by its framing, in a buffer the connection kept (ADR-0042).
#[derive(Debug, Bound)]
#[kind(name = "aether.tcp.frame")]
pub struct Frame {
    pub bytes: Bytes,
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
    /// The engine had no room to hold a frame read (ADR-0014).
    NoRoom,
    /// The connection already has a reader.
    Busy,
}

/// Bytes to write to a connection's peer, its parts in order, written with one gathered write: under
/// `LengthPrefixed`, one frame of them all (ADR-0042).
#[derive(Debug, Bound)]
#[kind(name = "aether.tcp.write")]
pub struct Write {
    pub parts: Vec<Bytes>,
}

/// What became of a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.wrote")]
pub enum Wrote {
    /// Every byte was handed to the kernel.
    Sent,
    /// The connection holds too many bytes unsent to take these.
    Full,
    /// The connection hung up, or its socket failed a write, before they were sent.
    HungUp,
    /// The connection was asked to [`Finish`] before these came.
    Finished,
}

/// Asks a connection to hang up, or a listener to stop listening.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.shut")]
pub struct Shut;

/// Asks a connection to finish writing: once every write it holds is written, it shuts its socket's write half, so
/// the peer reads to its end, and the connection reads on until its read half ends.
#[derive(Debug, Plain)]
#[kind(name = "aether.tcp.finish")]
pub struct Finish;

/// What a connection is written, finished, and hung up through, whichever place it stands in.
#[protocol(Write(answers = Wrote), Finish, Shut)]
pub struct Writing;

/// Who reads a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Reader {
    /// No one yet: the connection does not read its socket.
    Waiting,
    /// The stream `key` names.
    Reading(StreamKey),
}

/// Where a connection's writing stands. Its reading goes on whichever case it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum WriteHalf {
    /// It takes writes.
    Open,
    /// It was asked to finish with writes unsent: it writes them, then shuts its write half. A write is answered
    /// `Finished`.
    Finishing,
    /// Its write half is shut, so the peer reads to its end. A write is answered `Finished`.
    Finished,
    /// Its socket failed a write, or the shut of its write half: what was unsent was answered `HungUp`, as is every
    /// later write.
    Failed,
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
    /// What the framing writes before the parts: under `LengthPrefixed`, their length.
    prefix: Vec<u8>,
    parts: Vec<Bytes>,
    /// How many bytes the prefix and the parts hold together.
    length: u64,
    held: Held<Wrote>,
}

impl Pending {
    /// Its prefix and the bytes of each of its parts, in the order they are written.
    fn pieces<'p, A>(&'p self, turn: &'p Turn<'_, A>) -> impl Iterator<Item = &'p [u8]> {
        let parts = self.parts.iter().map(|part| turn.read(part));

        iter::once(&self.prefix[..]).chain(parts)
    }
}

/// How a gathered write of one pending write stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gathered {
    /// Every byte was handed to the kernel.
    Whole,
    /// The socket would wait.
    Blocked,
}

/// What a connection does, whichever place it stands in: its socket, what it has read and not framed, what it has to
/// write and where its writing stands, its reader, and what it waits on its socket for. It reads and writes either
/// kind of [`Socket`], so an actor of another crate that holds one can keep a `Connection` to serve it.
#[derive(Bound)]
pub struct Connection {
    socket: Handle<Socket>,
    framing: Framing,
    read: Vec<u8>,
    unsent: Vec<Pending>,
    /// How many bytes of the first unsent write are written.
    written: u64,
    most_unsent_bytes: u64,
    /// The bytes it last told the engine it holds unsent, so it tells again only when they change (ADR-0053).
    told_unsent: u64,
    write_half: WriteHalf,
    reading: Serving<Reading>,
    reader: Reader,
    readable: Wait,
    writable: Wait,
}

impl Connection {
    /// The connection `config` gives, which makes no call on its socket until it has a reader with credit or a write.
    pub fn begin<A>(config: ConnectionConfig, turn: &mut Beginning<'_, A>) -> Result<Self, Fault> {
        let socket = turn.take(config.socket)?;

        Ok(Self {
            socket,
            framing: config.framing,
            read: Vec::new(),
            unsent: Vec::new(),
            written: 0,
            most_unsent_bytes: u64::from(config.most_unsent_bytes),
            told_unsent: 0,
            write_half: WriteHalf::Open,
            reading: Serving::new(),
            reader: Reader::Waiting,
            readable: Wait::Not,
            writable: Wait::Not,
        })
    }

    /// Opens the stream `open` asks for: the connection's reader, or a second, finished at once as `Busy`. The reader's
    /// credit follows the request, and its `More` is what reads.
    pub fn open<A: Receives<Ended>>(
        &mut self,
        turn: &mut Turn<'_, A>,
        open: Open<Reading>,
    ) -> Result<Later<HungUp>, Fault> {
        let (key, Read, later) = self.reading.open(turn, open)?;
        if let Reader::Reading(_) = self.reader {
            if let Some(second) = self.reading.take(key) {
                second.finish(turn, &HungUp::Busy)?;
            }
            return Ok(later);
        }
        self.reader = Reader::Reading(key);

        Ok(later)
    }

    /// Adds the credit `more` grants to the reader's stream, and reads with it.
    pub fn more<A: Receives<Answer<Readied, Interest>>>(
        &mut self,
        turn: &mut Turn<'_, A>,
        more: &More,
    ) -> Result<(), Fault> {
        self.reading.more(turn, more);

        self.read(turn)
    }

    /// The reader stops reading: the connection hangs up.
    pub fn enough<A>(&mut self, turn: &mut Turn<'_, A>, enough: &Enough) -> Result<(), Fault> {
        let Some(stopped) = self.reading.stop(turn, enough) else {
            return Ok(());
        };
        self.reader = Reader::Waiting;
        stopped.finish(turn, &HungUp::Shut)?;

        self.hang_up(turn, HungUp::Shut)
    }

    /// An actor watched has ended: when it was the reader, no one is left to read, and the connection hangs up.
    pub fn ended<A>(&mut self, turn: &mut Turn<'_, A>, ended: &Ended) -> Result<(), Fault> {
        let gone = self.reading.ended(ended);
        if gone.is_empty() {
            return Ok(());
        }
        self.reader = Reader::Waiting;

        self.hang_up(turn, HungUp::Shut)
    }

    /// Takes `write` to send, and writes what the socket takes of it now; or answers `Full` when it would put the
    /// connection past what it holds unsent, `Finished` once it was asked to finish, and `HungUp` once its socket
    /// failed a write.
    pub fn write<A: Receives<Answer<Readied, Interest>>>(
        &mut self,
        turn: &mut Turn<'_, A>,
        write: Write,
    ) -> Result<Reply<Wrote>, Fault> {
        match self.write_half {
            WriteHalf::Open => {}
            WriteHalf::Finishing | WriteHalf::Finished => return Ok(Reply::Now(Wrote::Finished)),
            WriteHalf::Failed => return Ok(Reply::Now(Wrote::HungUp)),
        }
        let parts_length: usize = write.parts.iter().map(|part| turn.read(part).len()).sum();
        let prefix = self.framing.prefix(parts_length);
        let length = (prefix.len() + parts_length) as u64;
        let fits = self.waiting() + length <= self.most_unsent_bytes;
        if !fits {
            return Ok(Reply::Now(Wrote::Full));
        }
        let (held, later) = turn.hold();
        self.unsent.push(Pending { prefix, parts: write.parts, length, held });
        self.send(turn)?;

        Ok(Reply::Later(later))
    }

    /// Asked to finish writing: it shuts its write half once what it holds unsent is written, and at once when nothing
    /// is. A second ask, or one after a write failed, changes nothing.
    pub fn finish<A: Receives<Answer<Readied, Interest>>>(&mut self, turn: &mut Turn<'_, A>) -> Result<(), Fault> {
        if self.write_half != WriteHalf::Open {
            return Ok(());
        }
        self.write_half = WriteHalf::Finishing;

        self.send(turn)
    }

    /// What a wait for the socket came to: it is ready for `interest`, or may be, so the calls that waited on it are
    /// made again; or the host has no poller, and the socket can never be waited on, so the connection hangs up.
    pub fn readied<A: Receives<Answer<Readied, Interest>>>(
        &mut self,
        turn: &mut Turn<'_, A>,
        (readied, interest): (Answered<Readied>, Interest),
    ) -> Result<(), Fault> {
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
    pub fn shut<A>(&mut self, turn: &mut Turn<'_, A>) -> Result<(), Fault> {
        self.hang_up(turn, HungUp::Shut)
    }

    /// The bytes of its writes the socket has not taken yet.
    fn waiting(&self) -> u64 {
        self.unsent.iter().map(|unsent| unsent.length).sum::<u64>() - self.written
    }

    /// Writes what it can of what is unsent, answering each write once all its bytes are handed to the kernel: those
    /// sent before the socket failed are answered `Sent` all the same, and a failure fails the write half alone. What
    /// the socket would not take waits for it to be writable. Once all is written, a connection finishing shuts its
    /// write half. What it then holds unsent, it tells the engine when that has changed, for the engine's backlog
    /// (ADR-0053).
    fn send<A: Receives<Answer<Readied, Interest>>>(&mut self, turn: &mut Turn<'_, A>) -> Result<(), Fault> {
        let mut sent = Vec::new();
        let written = self.write_unsent(turn, &mut sent);
        for held in sent {
            turn.answer(held, &Wrote::Sent)?;
        }
        if written.is_err() {
            return self.fail_writing(turn);
        }
        self.tell_unsent(turn);

        if !self.unsent.is_empty() {
            return self.wait(turn, Interest::Writable);
        }
        if self.write_half == WriteHalf::Finishing {
            return self.shut_writing(turn);
        }

        Ok(())
    }

    /// Shuts its socket's write half, everything asked of it written: the peer reads to its end. A shut the socket
    /// refuses fails the write half.
    fn shut_writing<A>(&mut self, turn: &mut Turn<'_, A>) -> Result<(), Fault> {
        let Ok(socket) = turn.resource(&self.socket) else {
            return self.fail_writing(turn);
        };
        if socket.shut_writing().is_err() {
            return self.fail_writing(turn);
        }
        self.write_half = WriteHalf::Finished;

        Ok(())
    }

    /// Its socket failed a write: it answers every write unsent `HungUp`, stops its wait for the socket to be
    /// writable, and takes no more writes, but reads on, so what the peer sent before it stopped reading still
    /// arrives, and the connection ends when its read half does.
    fn fail_writing<A>(&mut self, turn: &mut Turn<'_, A>) -> Result<(), Fault> {
        self.write_half = WriteHalf::Failed;
        for unsent in self.unsent.drain(..) {
            turn.answer(unsent.held, &Wrote::HungUp)?;
        }
        self.written = 0;
        if let Wait::On(job) = self.writable {
            turn.cancel(job);
        }
        self.writable = Wait::Not;
        self.tell_unsent(turn);

        Ok(())
    }

    /// Tells the engine the bytes it holds unsent, when they have changed since it last told it (ADR-0053).
    fn tell_unsent<A>(&mut self, turn: &mut Turn<'_, A>) {
        let waiting = self.waiting();
        if waiting != self.told_unsent {
            turn.holds_unsent(waiting, self.most_unsent_bytes);
            self.told_unsent = waiting;
        }
    }

    /// Reads, frames, and sends its reader as many frames as its credit allows. A read that would wait waits for the
    /// socket to be readable; one stopped for want of credit waits for the reader's `More`.
    fn read<A: Receives<Answer<Readied, Interest>>>(&mut self, turn: &mut Turn<'_, A>) -> Result<(), Fault> {
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
                        let cut = self.cut(turn, skip, bytes);
                        let Ok(frame) = turn.put(cut) else {
                            return self.hang_up(turn, HungUp::NoRoom);
                        };
                        let Some(stream) = self.reading.get_mut(key) else {
                            return Ok(());
                        };
                        if let Err(Unsent::Unwritable(error)) = stream.send(turn, Frame { bytes: frame }) {
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
    fn wait<A: Receives<Answer<Readied, Interest>>>(
        &mut self,
        turn: &mut Turn<'_, A>,
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

    /// Writes unsent bytes until the socket would wait, each write's prefix and parts with gathered writes, noting in
    /// `sent` each write whose bytes all went. The socket and the parts' bytes are both shared borrows of the turn, so
    /// they are held together: the kernel writes a socket through `&` (ADR-0042).
    fn write_unsent<A>(&mut self, turn: &Turn<'_, A>, sent: &mut Vec<Held<Wrote>>) -> Result<(), HungUp> {
        let Ok(socket) = turn.resource(&self.socket) else {
            return Err(HungUp::Reset);
        };
        // `Write` is implemented for `&Socket`, and its methods take `&mut self`: the reference is the `self`.
        let mut socket: &Socket = socket;
        while let Some(first) = self.unsent.first() {
            let gathered = write_gathered(&mut socket, first.pieces(turn), &mut self.written)?;
            if gathered == Gathered::Blocked {
                return Ok(());
            }
            sent.push(self.unsent.remove(0).held);
            self.written = 0;
        }

        Ok(())
    }

    /// Reads what its socket holds into what it has read and not framed, up to what its framing keeps, and says how
    /// it stopped. The bytes are read into that buffer itself, which the connection keeps from read to read.
    fn fill<A>(&mut self, turn: &mut Turn<'_, A>) -> Result<Filled, HungUp> {
        let most = self.framing.most();
        let Ok(socket) = turn.open(&mut self.socket) else {
            return Err(HungUp::Reset);
        };
        // `Read` is implemented for `&Socket`, as `Write` is.
        let mut socket: &Socket = socket;
        while self.read.len() < most {
            let start = self.read.len();
            let room = (most - start).min(CHUNK);
            self.read.resize(start + room, 0);
            let read = socket.read(&mut self.read[start..]);
            let count = read.as_ref().map_or(0, |count| *count);
            self.read.truncate(start + count);
            match read {
                Ok(0) => return Ok(Filled::Closed),
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(Filled::Blocked),
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => return Err(HungUp::Reset),
            }
        }

        Ok(Filled::Full)
    }

    /// Cuts the frame at the front of what it has read, `bytes` long after `skip` bytes, into a buffer to put: the
    /// read buffer itself when the frame is all of it and fills at least half of it, by the rule a kept buffer is
    /// handed out by (ADR-0042), so a large frame is put where it was read; else a copy, in a buffer the turn hands
    /// out.
    fn cut<A>(&mut self, turn: &mut Turn<'_, A>, skip: usize, bytes: usize) -> Vec<u8> {
        let end = skip + bytes;
        let is_all = skip == 0 && end == self.read.len();
        let fills = bytes.saturating_mul(2) >= self.read.capacity();
        let hands_over = is_all && fills;
        if hands_over {
            let room = self.framing.most().min(CHUNK);
            return mem::replace(&mut self.read, Vec::from(turn.buffer(room)));
        }
        let mut frame = turn.buffer(bytes);
        frame.extend_from_slice(&self.read[skip..end]);
        self.read.drain(..end);

        Vec::from(frame)
    }

    /// Ends the reader's stream with `why`, answers every write not sent, stops its waits for the socket, and closes
    /// the connection, whose socket the engine drops after its closing turn (ADR-0010).
    fn hang_up<A>(&mut self, turn: &mut Turn<'_, A>, why: HungUp) -> Result<(), Fault> {
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

/// Writes `pieces`, from `written` bytes into them on, to `out` with gathered writes until every byte is handed to it
/// or it would wait, counting in `written` what it takes, so a write the socket takes in part goes on from there,
/// across pieces, when it is writable again.
fn write_gathered<'p>(
    out: &mut impl io::Write,
    pieces: impl IntoIterator<Item = &'p [u8]>,
    written: &mut u64,
) -> Result<Gathered, HungUp> {
    let mut slices: Vec<IoSlice<'_>> = pieces.into_iter().map(IoSlice::new).collect();
    let mut left = &mut slices[..];
    IoSlice::advance_slices(&mut left, usize::try_from(*written).unwrap_or(usize::MAX));
    while !left.is_empty() {
        match out.write_vectored(left) {
            Ok(0) => return Err(HungUp::Reset),
            Ok(count) => {
                *written += count as u64;
                IoSlice::advance_slices(&mut left, count);
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => return Ok(Gathered::Blocked),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err(HungUp::Reset),
        }
    }

    Ok(Gathered::Whole)
}
