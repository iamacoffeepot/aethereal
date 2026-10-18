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

/// An actor whose `begin` panics, which traps, as a guest's panic does.
#[derive(Bound)]
pub struct Doomed;

impl Kind for Doomed {
    const NAME: KindName = KindName::new("aether.guest.doomed");
}

impl Actor for Doomed {
    type Config = EchoConfig;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        panic!("a doomed actor crashes as it begins");
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What a brood is made from: how many children it spawns as it begins.
#[derive(Plain)]
pub struct BroodConfig {
    pub children: u64,
}

impl Kind for BroodConfig {
    const NAME: KindName = KindName::new("aether.guest.brood_config");
}

/// A word to a brood to spawn one more child.
#[derive(Plain)]
pub struct Grow;

impl Kind for Grow {
    const NAME: KindName = KindName::new("aether.guest.grow");
}

/// An actor that spawns adders: some as it begins, and one more on each word to grow. The `k`th adds `10 k + 1`, and
/// the first spawns one of its own.
#[derive(Bound)]
pub struct Brood {
    next: u64,
}

impl Kind for Brood {
    const NAME: KindName = KindName::new("aether.guest.brood");
}

impl Actor for Brood {
    type Config = BroodConfig;
    type Key = ();
    type Handles = (Grow,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: BroodConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        for key in 0..config.children {
            let config = AddBy { by: 10 * key + 1, deeper: u64::from(key == 0) };
            if turn.spawn::<Adder>(&key, config).is_err() {
                turn.log(Level::Error, "an adder was refused");
            }
        }

        Ok(Self { next: config.children })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Grow> for Brood {
    type Sends = ();

    fn handle(&mut self, _: Grow, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let key = self.next;
        if turn.spawn::<Adder>(&key, AddBy { by: 10 * key + 1, deeper: 0 }).is_err() {
            turn.log(Level::Error, "an adder was refused");
        }
        self.next += 1;

        Ok(())
    }
}

/// What an adder is made from: what it adds, and how many generations of adders to spawn beneath it.
#[derive(Plain)]
pub struct AddBy {
    pub by: u64,
    pub deeper: u64,
}

impl Kind for AddBy {
    const NAME: KindName = KindName::new("aether.guest.add_by");
}

/// An actor that answers a ping with a pong of the ping's number and its own, so the answer says its state survived
/// wherever it was kept. Made deeper, it spawns an adder of its own that adds a thousand more.
#[derive(Bound)]
pub struct Adder {
    by: u64,
}

impl Kind for Adder {
    const NAME: KindName = KindName::new("aether.guest.adder");
}

impl Actor for Adder {
    type Config = AddBy;
    type Key = u64;
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: AddBy, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        if config.deeper > 0 {
            let below = AddBy { by: config.by + 1000, deeper: config.deeper - 1 };
            if turn.spawn::<Self>(&0, below).is_err() {
                turn.log(Level::Error, "an adder below was refused");
            }
        }

        Ok(Self { by: config.by })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Adder {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong { n: ping.n + self.by })?;

        Ok(())
    }
}

aethereal_actor::guest!(Echo, Mirror, Responder, Doomed, Brood, Adder);
