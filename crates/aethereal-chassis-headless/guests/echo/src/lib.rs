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
}

impl Kind for Echo {
    const NAME: KindName = KindName::new("aether.guest.echo");
}

impl Actor for Echo {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        turn.log(Level::Info, "echo began");

        Ok(Self { answered: 0 })
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

aethereal_actor::guest!(Echo);
