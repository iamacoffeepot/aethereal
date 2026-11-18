//! A pipe: the engine's end of a program's stdin and stdout, read and written as a connection is (ADR-0040, ADR-0033).

use crate::{Exits, Processes};
use aethereal_actor::{
    Actor, Answer, Answered, Anyone, Beginning, ChildOf, Ended, Ending, Enough, Fault, Given, Handles, Later, More,
    Open, ProtocolRef, Receives, Reply, Turn,
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

impl ChildOf for Pipe {
    type Parent = Processes;
}

impl Actor for Pipe {
    type Config = PipeConfig;
    type Key = u64;
    type Handles = (Open<Reading>, More, Enough, Ended, Write, Finish, Shut, Answer<Readied, Interest>);
    type Depends = ();
    type Spawns = ();

    fn begin(config: PipeConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.watch(config.starter);
        let connection =
            ConnectionConfig { socket: config.socket, framing: FRAMING, most_unsent_bytes: MOST_UNSENT_BYTES };

        Ok(Self { connection: Connection::begin(connection, turn)?, starter: config.starter })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Open<Reading>> for Pipe {
    type From = Anyone;
    type Returns = Later<HungUp>;

    fn handle(&mut self, open: Open<Reading>, turn: &mut Turn<'_, Self>) -> Result<Later<HungUp>, Fault> {
        self.connection.open(turn, open)
    }
}

impl Handles<More> for Pipe {
    type From = Anyone;
    type Returns = ();

    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.more(turn, &more)
    }
}

impl Handles<Enough> for Pipe {
    type From = Anyone;
    type Returns = ();

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.enough(turn, &enough)
    }
}

impl Receives<Ended> for Pipe {
    /// The program's starter has ended, so no one wants its pipe, and it hangs up; or its reader has, which the
    /// connection meets.
    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if ended.actor().id() == self.starter.id() {
            return self.connection.shut(turn);
        }

        self.connection.ended(turn, &ended)
    }
}

impl Handles<Write> for Pipe {
    type From = Anyone;
    type Returns = Reply<Wrote>;

    fn handle(&mut self, write: Write, turn: &mut Turn<'_, Self>) -> Result<Reply<Wrote>, Fault> {
        self.connection.write(turn, write)
    }
}

impl Handles<Finish> for Pipe {
    type From = Anyone;
    type Returns = ();

    /// Ends the program's stdin, once what was written is: the program reads to its end.
    fn handle(&mut self, _: Finish, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.finish(turn)
    }
}

impl Handles<Shut> for Pipe {
    type From = Anyone;
    type Returns = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.shut(turn)
    }
}

impl Receives<Answer<Readied, Interest>> for Pipe {
    fn handle(&mut self, readied: (Answered<Readied>, Interest), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.readied(turn, readied)
    }
}
