//! A connection a listener accepted (ADR-0033).

use super::{Connection, ConnectionConfig, Finish, Reading, Shut, Write};
use crate::Listener;
use crate::{Interest, Readied};
use aethereal_actor::{
    Actor, Answer, Answered, Anyone, Beginning, ChildOf, Ended, Ending, Enough, Fault, Handles, More, Open, Receives,
    Turn,
};
use aethereal_data::Bound;

/// A connection beneath the listener that accepted it, keyed by that listener's count: read through a [`Reading`] stream, written with [`Write`], finished with [`Finish`], hung up with [`Shut`] (ADR-0033).
/// Holding a reference to it is what lets an actor read, write, or hang it up.
#[derive(Bound)]
#[kind(name = "aether.tcp.accepted")]
pub struct Accepted {
    connection: Connection,
}

impl ChildOf for Accepted {
    type Parent = Listener;
}

impl Actor for Accepted {
    type Config = ConnectionConfig;
    type Key = u64;
    type Handles = (Open<Reading>, More, Enough, Ended, Write, Finish, Shut, Answer<Readied, Interest>);
    type Depends = ();

    fn begin(config: ConnectionConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        Ok(Self { connection: Connection::begin(config, turn)? })
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Open<Reading>> for Accepted {
    type From = Anyone;

    fn handle(&mut self, open: Open<Reading>, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.open(turn, open)
    }
}

impl Handles<More> for Accepted {
    type From = Anyone;

    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.more(turn, &more)
    }
}

impl Handles<Enough> for Accepted {
    type From = Anyone;

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.enough(turn, &enough)
    }
}

impl Receives<Ended> for Accepted {
    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.ended(turn, &ended)
    }
}

impl Handles<Write> for Accepted {
    type From = Anyone;

    fn handle(&mut self, write: Write, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.write(turn, write)
    }
}

impl Handles<Finish> for Accepted {
    type From = Anyone;

    fn handle(&mut self, _: Finish, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.finish(turn)
    }
}

impl Handles<Shut> for Accepted {
    type From = Anyone;

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.shut(turn)
    }
}

impl Receives<Answer<Readied, Interest>> for Accepted {
    fn handle(&mut self, readied: (Answered<Readied>, Interest), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.connection.readied(turn, readied)
    }
}
