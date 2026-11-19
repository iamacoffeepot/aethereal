//! A pipe: the engine's end of a program's stdin and stdout, read and written as a connection is (ADR-0040, ADR-0033).

use crate::{Exits, Processes};
use aethereal_actor::{
    Answered, Beginning, Ended, Enough, Fault, Given, Later, More, Open, Part, ProtocolRef, Reply, Speaks, Turn, actor,
};
use aethereal_data::Bound;
use aethereal_tcp::{
    Connection, ConnectionConfig, Finish, Framing, HungUp, Interest, Readied, Reading, Shut, Socket, Write, Wrote,
};

/// How a pipe cuts what the program writes: whatever arrived, in chunks of at most 64 KiB, as a connection reads an
/// HTTP peer. The program's output is bytes whose framing, if any, is its reader's to parse.
const FRAMING: Framing = Framing::Chunks { most_bytes: 64 * 1024 };

/// The most bytes a pipe holds unsent to the program's stdin before a write is answered `Full`: 1 MiB, as a connection
/// holds by default (ADR-0033).
const MOST_UNSENT_BYTES: u32 = 1 << 20;

/// What a pipe is made from: the engine's end of the socket pair the program was started with, and the actor the
/// program was started for.
#[derive(Bound)]
#[kind(name = "aether.process.pipe_config")]
pub struct PipeConfig {
    pub socket: Given<Socket>,
    pub starter: ProtocolRef<Exits>,
}

/// The engine's end of a program's pipe, beneath the root, keyed by the number of the program's process (ADR-0040).
/// One socket pair carries both of the program's ends that its start connected: the program's stdout is read through
/// a [`Reading`] stream, its stdin written with [`Write`] and ended with [`Finish`], and [`Shut`] hangs both up. It
/// reads and writes through the TCP capability's [`Connection`], whatever kind of socket it holds (ADR-0033).
///
/// It ends apart from the program's process: once the program has exited and its reader has read to the end, or once
/// its reader stops, it is shut, or the program's starter ends. So what the program wrote before it exited is read
/// whole, however late its reader is.
#[derive(Bound)]
#[kind(name = "aether.process.pipe")]
pub struct Pipe {
    connection: Connection,
    starter: ProtocolRef<Exits>,
}

#[actor(key = u64, parent = Processes)]
impl Pipe {
    fn begin(config: PipeConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.watch(config.starter);
        let connection =
            ConnectionConfig { socket: config.socket, framing: FRAMING, most_unsent_bytes: MOST_UNSENT_BYTES };

        // The program closes its output as it exits, so the pipe ends once that end is read, whatever its stdin holds.
        Ok(Self { connection: Connection::begin_ending_with_peer(connection, turn)?, starter: config.starter })
    }

    #[request]
    fn open(
        &mut self,
        open: Open<Reading>,
        turn: &mut Turn<'_, Self, Speaks<(Part<Reading>,)>>,
    ) -> Result<Later<HungUp>, Fault> {
        self.connection.open(turn, open)
    }

    #[tell]
    fn more(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.more(turn, &more)
    }

    #[tell]
    fn enough(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.enough(turn, &enough)
    }

    /// The program's starter has ended, so no one wants its pipe, and it hangs up; or its reader has, which the
    /// connection meets.
    #[ended]
    fn ended(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if ended.actor().id() == self.starter.id() {
            return self.connection.shut(turn);
        }

        self.connection.ended(turn, &ended)
    }

    #[request]
    fn write(&mut self, write: Write, turn: &mut Turn<'_, Self>) -> Result<Reply<Wrote>, Fault> {
        self.connection.write(turn, write)
    }

    /// Ends the program's stdin, once what was written is: the program reads to its end.
    #[tell]
    fn finish(&mut self, _: Finish, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.finish(turn)
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.shut(turn)
    }

    #[answer]
    fn readied(&mut self, readied: (Answered<Readied>, Interest), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.readied(turn, readied)
    }
}
