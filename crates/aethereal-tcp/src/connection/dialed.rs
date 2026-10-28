//! A connection dialed out (ADR-0033).

use super::{Connection, ConnectionConfig, HungUp, Reading, Shut, Write, Wrote};
use crate::Tcp;
use aethereal_actor::{
    Actor, Beginning, ChildOf, Ended, Ending, Enough, Fault, Handles, More, On, Open, Part, Tick, ToAny, ToSender, Turn,
};
use aethereal_data::Bound;

/// A connection beneath the root, keyed by the root's count: read through a [`Reading`] stream, written with [`Write`], hung up with [`Shut`] (ADR-0033).
/// Holding a reference to it is what lets an actor read, write, or hang it up.
#[derive(Bound)]
#[kind(name = "aether.tcp.dialed")]
pub struct Dialed {
    connection: Connection,
}

impl ChildOf for Dialed {
    type Parent = Tcp;
}

impl Actor for Dialed {
    type Config = ConnectionConfig;
    type Key = u64;
    type Handles = (Open<Reading>, More, Enough, Ended, Write, Shut, On<Tick>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: ConnectionConfig, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { connection: Connection::begin(config, turn)? })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Open<Reading>> for Dialed {
    type Sends = (ToSender<HungUp>,);

    fn handle(&mut self, open: Open<Reading>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.connection.open(turn, open)
    }
}

impl Handles<More> for Dialed {
    type Sends = ();

    fn handle(&mut self, more: More, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.connection.more(turn, &more);

        Ok(())
    }
}

impl Handles<Enough> for Dialed {
    type Sends = ();

    fn handle(&mut self, enough: Enough, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.connection.enough(turn, &enough)
    }
}

impl Handles<Ended> for Dialed {
    type Sends = ();

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.connection.ended(turn, &ended)
    }
}

impl Handles<Write> for Dialed {
    type Sends = (ToSender<Wrote>,);

    fn handle(&mut self, write: Write, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.connection.write(turn, write)
    }
}

impl Handles<Shut> for Dialed {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.connection.shut(turn)
    }
}

impl Handles<On<Tick>> for Dialed {
    type Sends = (ToAny<(Part<Reading>,), Part<Reading>>,);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.connection.tick(turn)
    }
}
