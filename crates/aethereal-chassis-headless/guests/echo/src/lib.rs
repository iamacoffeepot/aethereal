//! A guest that answers each ping it is sent with a pong, to its sender (ADR-0015).

use aethereal_actor::{Actor, Beginning, Ending, Fault, Handles, Level, ToSender, Turn};
use aethereal_data::{Bound, Kind, KindName, Plain};

/// A ping, which an echo answers with a pong of the same number.
#[derive(Plain)]
pub struct Ping {
    pub n: u64,
}

impl Kind for Ping {
    const NAME: KindName = KindName::new("aether.guest.ping");
}

/// The answer to a ping.
#[derive(Plain)]
pub struct Pong {
    pub n: u64,
}

impl Kind for Pong {
    const NAME: KindName = KindName::new("aether.guest.pong");
}

/// One to count, which an echo counts and does not answer.
#[derive(Plain)]
pub struct Increment;

impl Kind for Increment {
    const NAME: KindName = KindName::new("aether.guest.increment");
}

/// A word to close, which an echo closes on.
#[derive(Plain)]
pub struct Shut;

impl Kind for Shut {
    const NAME: KindName = KindName::new("aether.guest.shut");
}

/// A word to crash, on which a mirror panics, which traps, as a guest's panic does.
#[derive(Plain)]
pub struct Crash;

impl Kind for Crash {
    const NAME: KindName = KindName::new("aether.guest.crash");
}

/// What an echo is made from: nothing.
#[derive(Plain)]
pub struct EchoConfig;

impl Kind for EchoConfig {
    const NAME: KindName = KindName::new("aether.guest.echo_config");
}

/// An actor that answers pings, and logs as it begins.
#[derive(Bound)]
pub struct Echo {
    answered: u64,
    counted: u64,
}

impl Kind for Echo {
    const NAME: KindName = KindName::new("aether.guest.echo");
}

impl Actor for Echo {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping, Increment, Shut);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        turn.log(Level::Info, "echo began");

        Ok(Self { answered: 0, counted: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Echo {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        self.answered += 1;
        turn.reply(&Pong { n: ping.n })?;

        Ok(())
    }
}

impl Handles<Increment> for Echo {
    type Sends = ();

    fn handle(&mut self, _: Increment, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        self.counted += 1;

        Ok(())
    }
}

impl Handles<Shut> for Echo {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// A second kind of actor the blob makes, so two of its actors can live in one instance: it answers pings, and
/// closes on the word to.
#[derive(Bound)]
pub struct Mirror;

impl Kind for Mirror {
    const NAME: KindName = KindName::new("aether.guest.mirror");
}

impl Actor for Mirror {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping, Shut, Crash);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Mirror {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong { n: ping.n })?;

        Ok(())
    }
}

impl Handles<Shut> for Mirror {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

impl Handles<Crash> for Mirror {
    type Sends = ();

    fn handle(&mut self, _: Crash, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        panic!("a mirror crashes when it is told to");
    }
}

/// An actor of which the blob makes many, each under its own key: it answers pings, and counts them, so each keeps
/// state of its own in the instance it lives in, and closes on the word to.
#[derive(Bound)]
pub struct Responder {
    answered: u64,
}

impl Kind for Responder {
    const NAME: KindName = KindName::new("aether.guest.responder");
}

impl Actor for Responder {
    type Config = EchoConfig;
    type Key = u64;
    type Handles = (Ping, Shut);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { answered: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Responder {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        self.answered += 1;
        turn.reply(&Pong { n: ping.n })?;

        Ok(())
    }
}

impl Handles<Shut> for Responder {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

aethereal_actor::guest!(Echo, Mirror, Responder);
