//! A connection dialed out (ADR-0033).

use super::{Connection, ConnectionConfig, Finish, HungUp, Reading, Shut, Write, Wrote};
use crate::Tcp;
use crate::{Interest, Readied};
use aethereal_actor::{Answered, Beginning, Ended, Enough, Fault, Later, More, Open, Part, Reply, Speaks, Turn, actor};
use aethereal_data::Bound;

/// A connection beneath the root, keyed by the root's count: read through a [`Reading`] stream, written with [`Write`], finished with [`Finish`], hung up with [`Shut`] (ADR-0033).
/// Holding a reference to it is what lets an actor read, write, or hang it up.
#[derive(Bound)]
#[kind(name = "aether.tcp.dialed")]
pub struct Dialed {
    connection: Connection,
}

#[actor(key = u64, parent = Tcp)]
impl Dialed {
    fn begin(config: ConnectionConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { connection: Connection::begin(config, turn)? })
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

    #[ended]
    fn ended(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.ended(turn, &ended)
    }

    #[request]
    fn write(&mut self, write: Write, turn: &mut Turn<'_, Self>) -> Result<Reply<Wrote>, Fault> {
        self.connection.write(turn, write)
    }

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
