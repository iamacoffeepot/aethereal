//! A guest that answers each ping it is sent with a pong, to its sender (ADR-0015).

use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, BlobId, BlobRef, Carries, ChildOf, Ended, Ending,
    ErasedActorRef, Fault, Given, Handle, Handles, Held, How, Job, Level, On, Path, Protocol, Recursive, Refused,
    Resource, Runner, Running, Start, Stop, Tells, Tick, To, ToAny, ToSender, ToType, ToWithoutMail, Top, Turn,
    Unanswered, Unrun, path,
};
use aethereal_data::{ActorPath, Bound, Kind, KindId, KindName, Plain};
use core::mem;

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

impl ChildOf for Echo {
    type Parent = Anywhere;
}

impl Actor for Echo {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping, Increment, Shut);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.log(Level::Info, "echo began");

        Ok(Self { answered: 0, counted: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Echo {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.answered += 1;
        turn.reply(&Pong { n: ping.n })?;

        Ok(())
    }
}

impl Handles<Increment> for Echo {
    type Sends = ();

    fn handle(&mut self, _: Increment, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.counted += 1;

        Ok(())
    }
}

impl Handles<Shut> for Echo {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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

impl ChildOf for Mirror {
    type Parent = Top;
}

impl Actor for Mirror {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping, Shut, Crash);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Mirror {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong { n: ping.n })?;

        Ok(())
    }
}

impl Handles<Shut> for Mirror {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

impl Handles<Crash> for Mirror {
    type Sends = ();

    fn handle(&mut self, _: Crash, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        panic!("a mirror crashes when it is told to");
    }
}

/// An actor that subscribes to the lifecycle's `Start` and `Stop` as it begins, logs each it is sent, and asks the
/// program to stop on `Start` (ADR-0020).
#[derive(Bound)]
pub struct Starter;

impl Kind for Starter {
    const NAME: KindName = KindName::new("aether.guest.starter");
}

impl ChildOf for Starter {
    type Parent = Top;
}

impl Actor for Starter {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (On<Start>, On<Stop>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Stop>();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Starter {
    type Sends = ();

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.log(Level::Info, "started");
        turn.quit();

        Ok(())
    }
}

impl Handles<On<Stop>> for Starter {
    type Sends = ();

    fn handle(&mut self, _: Stop, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.log(Level::Info, "stopping");

        Ok(())
    }
}

/// An actor that subscribes to `Tick` as it begins: one an engine whose cycle runs no `Tick` does not make (ADR-0020).
#[derive(Bound)]
pub struct Ticker;

impl Kind for Ticker {
    const NAME: KindName = KindName::new("aether.guest.ticker");
}

impl ChildOf for Ticker {
    type Parent = Anywhere;
}

impl Actor for Ticker {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (On<Tick>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Tick>> for Ticker {
    type Sends = ();

    fn handle(&mut self, _: Tick, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that spawns a ticker of its own blob as it begins, and logs whether its engine made it: refused for
/// `Tick`, by the stage's kind, when its cycle runs none (ADR-0020).
#[derive(Bound)]
pub struct Metronome;

impl Kind for Metronome {
    const NAME: KindName = KindName::new("aether.guest.metronome");
}

impl ChildOf for Metronome {
    type Parent = Top;
}

impl Actor for Metronome {
    type Config = EchoConfig;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let spawned = turn.spawn::<Ticker>(&(), EchoConfig);
        let said = match spawned {
            Ok(_) => "ticker made",
            Err(Refused::Unstaged(stage)) if stage.same(KindId::of::<Tick>()) => "ticker refused for tick",
            Err(_) => "ticker refused",
        };
        turn.log(Level::Info, said);

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A job a guest cannot run yet: its host reads no job (ADR-0018).
#[derive(Plain)]
pub struct Chore;

impl Kind for Chore {
    const NAME: KindName = KindName::new("aether.guest.chore");
}

impl Job for Chore {
    type Done = Pong;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Pong, Fault> {
        Ok(Pong { n: 0 })
    }
}

/// An actor that runs a chore as it begins, and logs that its turn refused it, as a guest's does.
#[derive(Bound)]
pub struct Errand;

impl Kind for Errand {
    const NAME: KindName = KindName::new("aether.guest.errand");
}

impl ChildOf for Errand {
    type Parent = Top;
}

impl Actor for Errand {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Answer<Pong, EchoConfig>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let ran = turn.run(&Chore, &EchoConfig);
        let said = match ran {
            Ok(_) => "chore run",
            Err(Unrun::InGuest) => "chore refused in a guest",
            Err(Unrun::Unwritten(_)) => "chore unwritten",
        };
        turn.log(Level::Info, said);

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Pong, EchoConfig>> for Errand {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Pong>, EchoConfig), _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that answers each ping with the time its turn reads, in nanoseconds (ADR-0018).
#[derive(Bound)]
pub struct Timekeeper;

impl Kind for Timekeeper {
    const NAME: KindName = KindName::new("aether.guest.timekeeper");
}

impl ChildOf for Timekeeper {
    type Parent = Top;
}

impl Actor for Timekeeper {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Timekeeper {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, _: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong { n: turn.now().nanos() })?;

        Ok(())
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

impl ChildOf for Responder {
    type Parent = Top;
}

impl Actor for Responder {
    type Config = EchoConfig;
    type Key = u64;
    type Handles = (Ping, Shut);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { answered: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Responder {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.answered += 1;
        turn.reply(&Pong { n: ping.n })?;

        Ok(())
    }
}

impl Handles<Shut> for Responder {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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

impl ChildOf for Doomed {
    type Parent = Top;
}

impl Actor for Doomed {
    type Config = EchoConfig;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        panic!("a doomed actor crashes as it begins");
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
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

impl ChildOf for Brood {
    type Parent = Top;
}

impl Actor for Brood {
    type Config = BroodConfig;
    type Key = ();
    type Handles = (Grow,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: BroodConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for key in 0..config.children {
            let config = AddBy { by: 10 * key + 1, deeper: u64::from(key == 0) };
            if turn.spawn::<Adder>(&key, config).is_err() {
                turn.log(Level::Error, "an adder was refused");
            }
        }

        Ok(Self { next: config.children })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Grow> for Brood {
    type Sends = ();

    fn handle(&mut self, _: Grow, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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

impl ChildOf for Adder {
    type Parent = Anywhere;
}

impl Actor for Adder {
    type Config = AddBy;
    type Key = u64;
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: AddBy, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        if config.deeper > 0 {
            let below = AddBy { by: config.by + 1000, deeper: config.deeper - 1 };
            if turn.spawn::<Self>(&0, below).is_err() {
                turn.log(Level::Error, "an adder below was refused");
            }
        }

        Ok(Self { by: config.by })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Adder {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong { n: ping.n + self.by })?;

        Ok(())
    }
}

/// What a seeker is made from: the paths it resolves as it begins.
#[derive(Plain)]
pub struct Seek {
    pub paths: Vec<ActorPath>,
}

impl Kind for Seek {
    const NAME: KindName = KindName::new("aether.guest.seek");
}

/// An actor that resolves each path it is made with as it begins, and pings each actor one names with its place in
/// the list, so what is pinged says which paths it found.
#[derive(Bound)]
pub struct Seeker;

impl Kind for Seeker {
    const NAME: KindName = KindName::new("aether.guest.seeker");
}

impl ChildOf for Seeker {
    type Parent = Top;
}

impl Actor for Seeker {
    type Config = Seek;
    type Key = ();
    type Handles = ();
    type BeginSends = (ToAny<(Ping,), Ping>,);
    type EndSends = ();

    fn begin(seek: Seek, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for (n, path) in (0..).zip(&seek.paths) {
            if let Some(found) = turn.resolve(path) {
                turn.send_to_any(ToWithoutMail::Held(found), &Ping { n })?;
            }
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that resolves each path it is made with as it begins, casts each actor one names to a protocol, and
/// pings each that casts with its place in the list: to `(Ping,)` as it is, and to `(Ping, Shut)` past fifty, so what
/// is pinged says which casts answered.
#[derive(Bound)]
pub struct Caster;

impl Kind for Caster {
    const NAME: KindName = KindName::new("aether.guest.caster");
}

impl ChildOf for Caster {
    type Parent = Top;
}

impl Actor for Caster {
    type Config = Seek;
    type Key = ();
    type Handles = ();
    type BeginSends = (ToAny<(Ping,), Ping>,);
    type EndSends = ();

    fn begin(seek: Seek, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for (n, path) in (0..).zip(&seek.paths) {
            let Some(found) = turn.resolve(path) else {
                continue;
            };
            if let Some(pinged) = turn.cast::<(Ping,)>(found) {
                turn.send_to_any(ToWithoutMail::Held(pinged.erase()), &Ping { n })?;
            }
            if let Some(shut) = turn.cast::<(Ping, Shut)>(found) {
                turn.send_to_any(ToWithoutMail::Held(shut.erase()), &Ping { n: n + 50 })?;
            }
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What a watcher is made from: the paths of the actors it watches, and of the one it tells of their endings.
#[derive(Plain)]
pub struct Watching {
    pub paths: Vec<ActorPath>,
    pub tell: ActorPath,
}

impl Kind for Watching {
    const NAME: KindName = KindName::new("aether.guest.watching");
}

/// Why a watcher did not begin: the actor it tells was not found.
#[derive(Plain)]
pub struct Untold;

impl Kind for Untold {
    const NAME: KindName = KindName::new("aether.guest.untold");
}

/// An actor that watches each actor at the paths it is made with, and tells the actor at the last of how each ended:
/// a ping of ten for its place in the list, and one for a close, two for a fault, or three for an ending before the
/// watch.
#[derive(Bound)]
pub struct Watcher {
    watched: Vec<ErasedActorRef>,
    tell: ErasedActorRef,
}

impl Kind for Watcher {
    const NAME: KindName = KindName::new("aether.guest.watcher");
}

impl ChildOf for Watcher {
    type Parent = Top;
}

impl Actor for Watcher {
    type Config = Watching;
    type Key = ();
    type Handles = (Ended,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(watching: Watching, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Some(tell) = turn.resolve(&watching.tell) else {
            return Err(Fault::new(&Untold)?);
        };
        let mut watched = Vec::new();
        for path in &watching.paths {
            if let Some(found) = turn.resolve(path) {
                turn.watch(found);
                watched.push(found);
            }
        }

        Ok(Self { watched, tell })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ended> for Watcher {
    type Sends = (ToAny<(Ping,), Ping>,);

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let place = (0..).zip(&self.watched).find_map(|(place, watched)| (*watched == ended.actor()).then_some(place));
        let how = match ended.how() {
            How::Closed => 1,
            How::Failed(_) => 2,
            How::Earlier => 3,
        };
        if let Some(place) = place {
            turn.send_to_any(To::Held(self.tell), &Ping { n: 10 * place + how })?;
        }

        Ok(())
    }
}

/// An actor that answers each ping by telling its parent, with a pong of the ping's number and its own key, so a turn
/// long after its `begin` still reaches whatever spawned it.
#[derive(Bound)]
pub struct Reporter {
    key: u64,
}

impl Kind for Reporter {
    const NAME: KindName = KindName::new("aether.guest.reporter");
}

impl ChildOf for Reporter {
    type Parent = Anywhere;
}

impl Actor for Reporter {
    type Config = AddBy;
    type Key = u64;
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: AddBy, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { key: config.by })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Reporter {
    type Sends = (ToAny<(Pong,), Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.send_to_any(To::Parent, &Pong { n: ping.n + self.key })?;

        Ok(())
    }
}

/// A number for why a spawn was refused, as a hatchling tells it.
const fn refused(refused: &Refused) -> u64 {
    match refused {
        Refused::KeyTaken => 1,
        Refused::TooDeep => 2,
        Refused::Began(_) => 3,
        Refused::NoRoom => 4,
        Refused::NotMade => 5,
        Refused::Waiting => 6,
        Refused::Unstaged(_) => 7,
        Refused::Missing(_) => 8,
    }
}

/// A native actor the guest's crate holds but its blob does not make: one a guest spawns is begun by its host. It
/// tells its parent, as it begins, a pong of a hundred and what it is made to add, and logs that it began.
#[derive(Bound)]
pub struct Chick;

impl Kind for Chick {
    const NAME: KindName = KindName::new("aether.guest.chick");
}

impl ChildOf for Chick {
    type Parent = Hatcher;
}

impl Actor for Chick {
    type Config = AddBy;
    type Key = u64;
    type Handles = ();
    type BeginSends = (ToAny<(Pong,), Pong>,);
    type EndSends = ();

    fn begin(config: AddBy, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.send_to_any(ToWithoutMail::Parent, &Pong { n: 100 + config.by })?;
        turn.log(Level::Info, "a chick began");

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A native actor named as a chick is, which declares other handlers: a guest's spawn of it is refused, as what the
/// program keeps of the name is not it.
#[derive(Bound)]
pub struct FakeChick;

impl Kind for FakeChick {
    const NAME: KindName = KindName::new("aether.guest.chick");
}

impl ChildOf for FakeChick {
    type Parent = Hatcher;
}

impl Actor for FakeChick {
    type Config = AddBy;
    type Key = u64;
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: AddBy, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for FakeChick {
    type Sends = ();

    fn handle(&mut self, _: Ping, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A native actor no program keeps for a guest to spawn.
#[derive(Bound)]
pub struct Stray;

impl Kind for Stray {
    const NAME: KindName = KindName::new("aether.guest.stray");
}

impl ChildOf for Stray {
    type Parent = Hatcher;
}

impl Actor for Stray {
    type Config = EchoConfig;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Why a hatcher did not begin: a spawn was refused, for the reason numbered so.
#[derive(Plain)]
pub struct Unhatched {
    pub why: u64,
}

impl Kind for Unhatched {
    const NAME: KindName = KindName::new("aether.guest.unhatched");
}

/// The fault of a hatcher whose spawn was refused for `why`.
fn unhatched(why: &Refused) -> Fault {
    Fault::new(&Unhatched { why: refused(why) }).unwrap_or_else(|fault| fault.into())
}

/// What a hatcher is made from: another blob of code, and its own.
#[derive(Bound)]
pub struct Hatch {
    pub other: BlobRef,
    pub home: BlobRef,
}

impl Kind for Hatch {
    const NAME: KindName = KindName::new("aether.guest.hatch");
}

/// An actor that spawns children of other code and tells its parent each pong they tell it. As it begins it spawns a
/// reporter of the other blob, adding twenty, which it pings. On the `k`th word to grow it spawns a chick, a native
/// child, adding `10 k + 2`; a stray and a fake chick, whose refusals it tells its parent as five and six hundred and
/// why; and, on the first, a boomerang of the other blob, which it pings.
#[derive(Bound)]
pub struct Hatcher {
    other: BlobRef,
    home: BlobRef,
    grown: u64,
}

impl Kind for Hatcher {
    const NAME: KindName = KindName::new("aether.guest.hatcher");
}

impl ChildOf for Hatcher {
    type Parent = Anywhere;
}

impl Actor for Hatcher {
    type Config = Hatch;
    type Key = ();
    type Handles = (Pong, Grow);
    type BeginSends = (ToType<Reporter, Ping>,);
    type EndSends = ();

    fn begin(hatch: Hatch, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let reporter = turn.spawn_from(&hatch.other, &7, &AddBy { by: 20, deeper: 0 });
        turn.send_to(reporter.map_err(|why| unhatched(&why))?, &Ping { n: 0 })?;

        Ok(Self { other: hatch.other, home: hatch.home, grown: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Pong> for Hatcher {
    type Sends = (ToAny<(Pong,), Pong>,);

    fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.send_to_any(To::Parent, &pong)?;

        Ok(())
    }
}

impl Handles<Grow> for Hatcher {
    type Sends = (ToType<Boomerang, Ping>, ToAny<(Pong,), Pong>);

    fn handle(&mut self, _: Grow, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let chick = AddBy { by: 10 * self.grown + 2, deeper: 0 };
        let _: ActorRef<Chick> = turn.spawn(&self.grown, chick).map_err(|why| unhatched(&why))?;
        let stray: Result<ActorRef<Stray>, _> = turn.spawn(&(), EchoConfig);
        if let Err(why) = stray {
            turn.send_to_any(To::Parent, &Pong { n: 500 + refused(&why) })?;
        }
        let fake: Result<ActorRef<FakeChick>, _> = turn.spawn(&(100 + self.grown), AddBy { by: 0, deeper: 0 });
        if let Err(why) = fake {
            turn.send_to_any(To::Parent, &Pong { n: 600 + refused(&why) })?;
        }
        let is_first = self.grown == 0;
        if is_first {
            let home = Home { home: self.home.clone() };
            let boomerang = turn.spawn_from(&self.other, &(), &home).map_err(|why| unhatched(&why))?;
            turn.send_to(boomerang, &Ping { n: 0 })?;
        }
        self.grown += 1;

        Ok(())
    }
}

/// What a boomerang is made from: the blob of code that spawned it.
#[derive(Bound)]
pub struct Home {
    pub home: BlobRef,
}

impl Kind for Home {
    const NAME: KindName = KindName::new("aether.guest.home");
}

/// An actor that spawns a reporter of the blob it was spawned from: refused as it begins, as that blob's instance waits
/// on the call out that begins it, which it tells its parent as three hundred and why, and made on a ping, which it
/// tells as four hundred.
#[derive(Bound)]
pub struct Boomerang {
    home: BlobRef,
}

impl Kind for Boomerang {
    const NAME: KindName = KindName::new("aether.guest.boomerang");
}

impl ChildOf for Boomerang {
    type Parent = Hatcher;
}

impl Actor for Boomerang {
    type Config = Home;
    type Key = ();
    type Handles = (Ping,);
    type BeginSends = (ToAny<(Pong,), Pong>,);
    type EndSends = ();

    fn begin(home: Home, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let spawned = turn.spawn_from::<Reporter>(&home.home, &0, &AddBy { by: 0, deeper: 0 });
        let n = spawned.err().map_or(300, |why| 300 + refused(&why));
        turn.send_to_any(ToWithoutMail::Parent, &Pong { n })?;

        Ok(Self { home: home.home })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Boomerang {
    type Sends = (ToAny<(Pong,), Pong>,);

    fn handle(&mut self, _: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let spawned = turn.spawn_from::<Reporter>(&self.home, &1, &AddBy { by: 0, deeper: 0 });
        let n = spawned.err().map_or(400, |why| 400 + refused(&why));
        turn.send_to_any(To::Parent, &Pong { n })?;

        Ok(())
    }
}

/// What a librarian is made from: the book it keeps, and the path of the actor it tells how long the book is.
#[derive(Bound)]
pub struct Shelf {
    pub book: BlobRef,
    pub tell: ActorPath,
}

impl Kind for Shelf {
    const NAME: KindName = KindName::new("aether.guest.shelf");
}

/// A reference to a blob, sent as mail.
#[derive(Bound)]
pub struct Book {
    pub blob: BlobRef,
}

impl Kind for Book {
    const NAME: KindName = KindName::new("aether.guest.book");
}

/// An actor that keeps a blob and reads it as it begins, telling another a pong of its length; answers a ping of `n`
/// with a reference to a blob of `n` sevens, which it puts; answers a book it is sent with a pong of a hundred and its
/// length; and closes on a word to shut.
#[derive(Bound)]
pub struct Librarian {
    book: BlobRef,
}

impl Kind for Librarian {
    const NAME: KindName = KindName::new("aether.guest.librarian");
}

impl ChildOf for Librarian {
    type Parent = Anywhere;
}

impl Actor for Librarian {
    type Config = Shelf;
    type Key = ();
    type Handles = (Ping, Book, Shut);
    type BeginSends = (ToAny<(Pong,), Pong>,);
    type EndSends = ();

    fn begin(shelf: Shelf, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let n = turn.read(&shelf.book).len() as u64;
        if let Some(tell) = turn.resolve(&shelf.tell) {
            turn.send_to_any(ToWithoutMail::Held(tell), &Pong { n })?;
        }

        Ok(Self { book: shelf.book })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Librarian {
    type Sends = (ToSender<Book>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let len = usize::try_from(ping.n).unwrap_or(0);
        let blob = turn.put(vec![7; len]).map_err(|why| unhatched(&why))?;
        turn.reply(&Book { blob })?;

        Ok(())
    }
}

impl Handles<Book> for Librarian {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, book: Book, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let n = 100 + turn.read(&book.blob).len() as u64;
        turn.reply(&Pong { n })?;

        Ok(())
    }
}

impl Handles<Shut> for Librarian {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// What a forger is made from: the engine's number and the bits of a blob's id, as plain numbers, not a reference.
#[derive(Plain)]
pub struct Forge {
    pub engine: u32,
    pub bits: u64,
}

impl Kind for Forge {
    const NAME: KindName = KindName::new("aether.guest.forge");
}

/// A reference to the blob `id` names, which no one gave: what code that keeps no rules makes, as a guest's code
/// written by hand may. Its pin is noted, as a clone's is, so its host sees the guest take a reference.
fn forged(id: BlobId) -> BlobRef {
    // SAFETY: a `BlobRef` is laid out as its id. The one made holds no pin, and is let go of without its drop,
    // which would note one dropped; its clone notes the pin the guest claims.
    let unpinned = unsafe { core::mem::transmute::<BlobId, BlobRef>(id) };
    let forged = unpinned.clone();
    core::mem::forget(unpinned);

    forged
}

/// An actor that makes a reference to a blob out of an id it was told as numbers, and reads it as it begins: its host
/// refuses the read, as the guest was never given the blob.
#[derive(Bound)]
pub struct Forger;

impl Kind for Forger {
    const NAME: KindName = KindName::new("aether.guest.forger");
}

impl ChildOf for Forger {
    type Parent = Top;
}

impl Actor for Forger {
    type Config = Forge;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(forge: Forge, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let forged = forged(BlobId::new(forge.engine, forge.bits));
        let _ = turn.read(&forged).len();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that makes a reference to a blob out of an id it was told as numbers, and keeps it, reading nothing: its
/// host finds, among what its `begin` changed of its references, one to a blob the guest was never given.
#[derive(Bound)]
pub struct Hoarder {
    forged: BlobRef,
}

impl Kind for Hoarder {
    const NAME: KindName = KindName::new("aether.guest.hoarder");
}

impl ChildOf for Hoarder {
    type Parent = Top;
}

impl Actor for Hoarder {
    type Config = Forge;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(forge: Forge, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { forged: forged(BlobId::new(forge.engine, forge.bits)) })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that, pinged for `n`, puts a blob of `n` sevens, spawns a librarian to keep it that tells no one, and then
/// answers with a reference to a blob it was never given: its host refuses that send, and the turn fails with the librarian it stood
/// up, which the engine never ends.
#[derive(Bound)]
pub struct Spendthrift;

impl Kind for Spendthrift {
    const NAME: KindName = KindName::new("aether.guest.spendthrift");
}

impl ChildOf for Spendthrift {
    type Parent = Top;
}

impl Actor for Spendthrift {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Spendthrift {
    type Sends = (ToSender<Book>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let len = usize::try_from(ping.n).unwrap_or(0);
        let book = turn.put(vec![7; len]).map_err(|why| unhatched(&why))?;
        let tell = ActorPath::from_root(&aethereal_data::PathSegment::new(KindName::new("aether.guest.no_one"), &()));
        let engine = book.id().engine();
        turn.spawn::<Librarian>(&(), Shelf { book, tell }).map_err(|why| unhatched(&why))?;
        turn.reply(&Book { blob: forged(BlobId::new(engine, u64::MAX)) })?;

        Ok(())
    }
}

/// What a parting actor is made from: the path of the actor it tells as it ends.
#[derive(Bound)]
pub struct Farewell {
    pub tell: ActorPath,
}

impl Kind for Farewell {
    const NAME: KindName = KindName::new("aether.guest.farewell");
}

/// An actor that counts the pings it is sent, and fails on a ping of zero; closes on a word to shut; and, in its
/// closing turn, tells the actor it was made to tell a pong of six hundred, ten for each ping it counted, and one when
/// it ends because a turn of its own failed.
#[derive(Bound)]
pub struct Parting {
    tell: ErasedActorRef,
    pings: u64,
}

impl Kind for Parting {
    const NAME: KindName = KindName::new("aether.guest.parting");
}

impl ChildOf for Parting {
    type Parent = Top;
}

impl Actor for Parting {
    type Config = Farewell;
    type Key = u64;
    type Handles = (Ping, Shut);
    type BeginSends = ();
    type EndSends = (ToAny<(Pong,), Pong>,);

    fn begin(farewell: Farewell, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Some(tell) = turn.resolve(&farewell.tell) else {
            return Err(Fault::new(&Untold)?);
        };

        Ok(Self { tell, pings: 0 })
    }

    fn end(self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        let is_faulted = turn.why() == aethereal_actor::Why::Faulted;
        let n = 600 + 10 * self.pings + u64::from(is_faulted);
        turn.send_to_any(ToWithoutMail::Held(self.tell), &Pong { n })?;

        Ok(())
    }
}

impl Handles<Ping> for Parting {
    type Sends = ();

    fn handle(&mut self, ping: Ping, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if ping.n == 0 {
            return Err(Fault::new(&Untold)?);
        }
        self.pings += 1;

        Ok(())
    }
}

impl Handles<Shut> for Parting {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// What an archive is made from: the book its readers keep, and how many readers it spawns.
#[derive(Bound)]
pub struct Stacks {
    pub book: BlobRef,
    pub readers: u64,
}

impl Kind for Stacks {
    const NAME: KindName = KindName::new("aether.guest.stacks");
}

/// An actor that spawns readers as it begins, each keeping a reference to the book it was made with, and keeps none
/// itself.
#[derive(Bound)]
pub struct Archive;

impl Kind for Archive {
    const NAME: KindName = KindName::new("aether.guest.archive");
}

impl ChildOf for Archive {
    type Parent = Top;
}

impl Actor for Archive {
    type Config = Stacks;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(stacks: Stacks, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for key in 0..stacks.readers {
            if turn.spawn::<Reader>(&key, Book { blob: stacks.book.clone() }).is_err() {
                turn.log(Level::Error, "a reader was refused");
            }
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that keeps a reference to a book, and answers a ping with a pong of the ping's number and the length it
/// reads the book to be, and closes.
#[derive(Bound)]
pub struct Reader {
    book: BlobRef,
}

impl Kind for Reader {
    const NAME: KindName = KindName::new("aether.guest.reader");
}

impl ChildOf for Reader {
    type Parent = Archive;
}

impl Actor for Reader {
    type Config = Book;
    type Key = u64;
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(book: Book, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { book: book.blob })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Reader {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let n = ping.n + turn.read(&self.book).len() as u64;
        turn.reply(&Pong { n })?;
        turn.close();

        Ok(())
    }
}

/// A resource its host makes and a guest passes on: a guest holds only its ticket, and native code reads its light
/// (ADR-0010).
pub struct Lamp {
    pub light: u64,
}

impl Resource for Lamp {}

/// Lamps handed from one actor to another in mail.
#[derive(Bound)]
pub struct Hand {
    pub lamps: Vec<Given<Lamp>>,
}

impl Kind for Hand {
    const NAME: KindName = KindName::new("aether.guest.hand");
}

/// What a porter is made from: who it passes lamps on to, the lamps it begins with, and how it passes those on.
#[derive(Bound)]
pub struct Porting {
    pub to: ActorPath,
    pub lamps: Vec<Given<Lamp>>,
    pub how: u64,
}

impl Kind for Porting {
    const NAME: KindName = KindName::new("aether.guest.porting");
}

/// A guest that passes on every lamp it is given to the actor `to` names, which handles hands of lamps: what comes in
/// mail it sends on, and what its config holds as `how` says, sending it on for nought, handing it in its config to a
/// porter it spawns of its own code, which sends it on, for one, and to a bearer, of native code, which it pings to
/// send it on, for two.
#[derive(Bound)]
pub struct Porter {
    to: ActorPath,
}

impl Kind for Porter {
    const NAME: KindName = KindName::new("aether.guest.porter");
}

impl Porter {
    /// The actor it passes lamps on to, as one that handles hands of them.
    fn to(&self, turn: &impl Resolves) -> Result<ErasedActorRef, Fault> {
        let to = turn.resolve_cast(&self.to);

        to.ok_or_else(|| Fault::new(&Shut).unwrap_or_else(|why| why.into()))
    }
}

/// What a porter's turns read to find who they pass lamps on to.
trait Resolves {
    /// The actor `path` names, when it is live and handles hands of lamps.
    fn resolve_cast(&self, path: &ActorPath) -> Option<ErasedActorRef>;
}

impl<A, S> Resolves for Beginning<'_, A, S> {
    fn resolve_cast(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        let found = self.resolve(path)?;

        self.cast::<(Hand,)>(found).map(|to| to.erase())
    }
}

impl<A, S> Resolves for Turn<'_, A, S> {
    fn resolve_cast(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        let found = self.resolve(path)?;

        self.cast::<(Hand,)>(found).map(|to| to.erase())
    }
}

impl ChildOf for Porter {
    type Parent = Recursive;
}

impl Actor for Porter {
    type Config = Porting;
    type Key = u64;
    type Handles = (Hand,);
    type BeginSends = (ToAny<(Hand,), Hand>, ToType<Bearer, Ping>);
    type EndSends = ();

    fn begin(config: Porting, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Porting { to, lamps, how } = config;
        let held: Vec<Handle<Lamp>> = lamps.into_iter().map(|lamp| turn.take(lamp)).collect::<Result<_, _>>()?;
        let porter = Self { to: to.clone() };
        let has_lamps = !held.is_empty();
        if !has_lamps {
            return Ok(porter);
        }
        let lamps = held.into_iter().map(|lamp| turn.give(lamp)).collect::<Result<Vec<_>, _>>()?;
        match how {
            0 => {
                let to = porter.to(turn)?;
                turn.send_to_any(ToWithoutMail::Held(to), &Hand { lamps })?;
            }
            1 => {
                let spawned: Result<ActorRef<Self>, _> = turn.spawn(&1, Porting { to, lamps, how: 0 });
                spawned.map_err(|why| unhatched(&why))?;
            }
            _ => {
                let spawned: Result<ActorRef<Bearer>, _> = turn.spawn(&0, Porting { to, lamps, how: 0 });
                let bearer = spawned.map_err(|why| unhatched(&why))?;
                turn.send_to(bearer, &Ping { n: 0 })?;
            }
        }

        Ok(porter)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Hand> for Porter {
    type Sends = (ToAny<(Hand,), Hand>,);

    fn handle(&mut self, hand: Hand, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held: Vec<Handle<Lamp>> = hand.lamps.into_iter().map(|lamp| turn.take(lamp)).collect::<Result<_, _>>()?;
        let lamps = held.into_iter().map(|lamp| turn.give(lamp)).collect::<Result<Vec<_>, _>>()?;
        let to = self.to(turn)?;
        turn.send_to_any(To::Held(to), &Hand { lamps })?;

        Ok(())
    }
}

/// A native actor the guest's crate holds but its blob does not make, which a porter spawns: it takes the lamps its
/// config holds as its host begins it, and keeps them until it is pinged, when it sends them on to the actor `to`
/// names.
#[derive(Bound)]
pub struct Bearer {
    to: ActorPath,
    lamps: Vec<Handle<Lamp>>,
}

impl Kind for Bearer {
    const NAME: KindName = KindName::new("aether.guest.bearer");
}

impl ChildOf for Bearer {
    type Parent = Porter;
}

impl Actor for Bearer {
    type Config = Porting;
    type Key = u64;
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: Porting, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Porting { to, lamps, .. } = config;
        let lamps = lamps.into_iter().map(|lamp| turn.take(lamp)).collect::<Result<_, _>>()?;

        Ok(Self { to, lamps })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Bearer {
    type Sends = (ToAny<(Hand,), Hand>,);

    fn handle(&mut self, _: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = core::mem::take(&mut self.lamps);
        let lamps = held.into_iter().map(|lamp| turn.give(lamp)).collect::<Result<Vec<_>, _>>()?;
        let to = turn.resolve_cast(&self.to).ok_or_else(|| Fault::new(&Shut).unwrap_or_else(|why| why.into()))?;
        turn.send_to_any(To::Held(to), &Hand { lamps })?;

        Ok(())
    }
}

/// The protocol an echo is asked pings through: each answered with a pong.
pub struct Pings;

impl Protocol for Pings {
    type Kinds = (Ping,);
}

impl Carries<Ping> for Pings {
    type Answers = (Pong,);
}

/// The protocol an echo is asked to count through, as if each were answered with a pong: an echo counts and does not
/// answer, so the engine says it went unanswered.
pub struct Counts;

impl Protocol for Counts {
    type Kinds = (Increment,);
}

impl Carries<Increment> for Counts {
    type Answers = (Pong,);
}

/// A word to a querent to ask its echo `pings` pings and then to count once, each kept with which it was, counting
/// from `first`.
#[derive(Plain)]
pub struct Query {
    pub first: u64,
    pub pings: u64,
}

impl Kind for Query {
    const NAME: KindName = KindName::new("aether.guest.query");
}

/// What a querent keeps until a request it asked is answered: which it was.
#[derive(Plain)]
pub struct Which {
    pub at: u64,
}

impl Kind for Which {
    const NAME: KindName = KindName::new("aether.guest.which");
}

/// What became of a querent's request, as it tells its parent: the pong it was answered with, or why it was not, with
/// which request it was.
#[derive(Plain)]
pub enum Heard {
    Pong { at: u64, n: u64 },
    No { at: u64, why: Unanswered },
}

impl Kind for Heard {
    const NAME: KindName = KindName::new("aether.guest.heard");
}

/// An actor that asks an echo it spawns as it begins, and tells its parent what became of each request, with what it
/// kept for it.
#[derive(Bound)]
pub struct Querent {
    echo: ActorRef<Echo>,
}

impl Kind for Querent {
    const NAME: KindName = KindName::new("aether.guest.querent");
}

impl ChildOf for Querent {
    type Parent = Anywhere;
}

impl Actor for Querent {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Query, Answer<Pong, Which>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let echo = turn.spawn(&(), EchoConfig).map_err(|_| Fault::new(&Shut).unwrap_or_else(Fault::from))?;

        Ok(Self { echo })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Query> for Querent {
    type Sends = (Asks<Pings, Ping, Which>, Asks<Counts, Increment, Which>);

    fn handle(&mut self, query: Query, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let last = query.first + query.pings;
        for at in query.first..last {
            turn.ask_to(self.echo.protocol(), &Ping { n: at * 10 }, &Which { at })?;
        }
        turn.ask_to(self.echo.protocol(), &Increment, &Which { at: last })?;

        Ok(())
    }
}

impl Handles<Answer<Pong, Which>> for Querent {
    type Sends = (ToAny<(Heard,), Heard>,);

    fn handle(
        &mut self,
        (pong, which): (Answered<Pong>, Which),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let heard = match pong {
            Answered::Yes(pong) => Heard::Pong { at: which.at, n: pong.n },
            Answered::No(why) => Heard::No { at: which.at, why },
        };
        turn.send_to_any(To::Parent, &heard)?;

        Ok(())
    }
}

/// What an opener is made from: the ping it asks as it begins, and how many more openers to stand up beneath it.
#[derive(Plain)]
pub struct Opening {
    pub n: u64,
    pub below: u64,
}

impl Kind for Opening {
    const NAME: KindName = KindName::new("aether.guest.opening");
}

/// An actor whose `begin` spawns an echo and asks it a ping of `n`, and, while `below` is more than none, stands up an
/// opener of its own code beneath it that asks a ping of one more; the last stands up an inlet that does. It tells
/// its parent what became of its request, and passes on what each actor below tells it.
#[derive(Bound)]
pub struct Opener;

impl Kind for Opener {
    const NAME: KindName = KindName::new("aether.guest.opener");
}

impl ChildOf for Opener {
    type Parent = Anywhere;
}

impl Actor for Opener {
    type Config = Opening;
    type Key = u64;
    type Handles = (Answer<Pong, Which>, Heard);
    type BeginSends = (Asks<Pings, Ping, Which>,);
    type EndSends = ();

    fn begin(opening: Opening, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let echo: ActorRef<Echo> = turn.spawn(&(), EchoConfig).map_err(|why| unhatched(&why))?;
        turn.ask_to(echo.protocol(), &Ping { n: opening.n }, &Which { at: opening.n })?;
        let n = opening.n + 1;
        let is_deeper = opening.below > 0;
        if is_deeper {
            let _: ActorRef<Self> =
                turn.spawn(&n, Opening { n, below: opening.below - 1 }).map_err(|why| unhatched(&why))?;
        } else {
            let _: ActorRef<Inlet> = turn.spawn(&(), Ping { n }).map_err(|why| unhatched(&why))?;
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Pong, Which>> for Opener {
    type Sends = (ToAny<(Heard,), Heard>,);

    fn handle(
        &mut self,
        (pong, which): (Answered<Pong>, Which),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let heard = match pong {
            Answered::Yes(pong) => Heard::Pong { at: which.at, n: pong.n },
            Answered::No(why) => Heard::No { at: which.at, why },
        };
        turn.send_to_any(To::Parent, &heard)?;

        Ok(())
    }
}

impl Handles<Heard> for Opener {
    type Sends = (ToAny<(Heard,), Heard>,);

    fn handle(&mut self, heard: Heard, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.send_to_any(To::Parent, &heard)?;

        Ok(())
    }
}

/// A native actor the guest's crate holds but its blob does not make, begun by its host apart from the guest's call:
/// its `begin` spawns an echo and asks it the ping it was made from, and it tells its parent what became of it.
#[derive(Bound)]
pub struct Inlet;

impl Kind for Inlet {
    const NAME: KindName = KindName::new("aether.guest.inlet");
}

impl ChildOf for Inlet {
    type Parent = Opener;
}

impl Actor for Inlet {
    type Config = Ping;
    type Key = ();
    type Handles = (Answer<Pong, Which>,);
    type BeginSends = (Asks<Pings, Ping, Which>,);
    type EndSends = ();

    fn begin(ping: Ping, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let echo = turn.spawn::<Echo>(&(), EchoConfig).map_err(|why| unhatched(&why))?;
        turn.ask_to(echo.protocol(), &Ping { n: ping.n }, &Which { at: ping.n })?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Pong, Which>> for Inlet {
    type Sends = (ToAny<(Heard,), Heard>,);

    fn handle(
        &mut self,
        (pong, which): (Answered<Pong>, Which),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let heard = match pong {
            Answered::Yes(pong) => Heard::Pong { at: which.at, n: pong.n },
            Answered::No(why) => Heard::No { at: which.at, why },
        };
        turn.send_to_any(To::Parent, &heard)?;

        Ok(())
    }
}

/// An actor that answers a ping of `n` by telling its parent a pong of `n`, and then two books, of `n` sevens and of one
/// more, sent back to back: a run of mail, after other mail, whose references its host must place.
#[derive(Bound)]
pub struct Shelver;

impl Kind for Shelver {
    const NAME: KindName = KindName::new("aether.guest.shelver");
}

impl ChildOf for Shelver {
    type Parent = Anywhere;
}

impl Actor for Shelver {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Shelver {
    type Sends = (ToAny<(Pong,), Pong>, ToAny<(Book,), Book>);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let len = usize::try_from(ping.n).unwrap_or(0);
        turn.send_to_any(To::Parent, &Pong { n: ping.n })?;
        for more in 0..2 {
            let blob = turn.put(vec![7; len + more]).map_err(|why| unhatched(&why))?;
            turn.send_to_any(To::Parent, &Book { blob })?;
        }

        Ok(())
    }
}

/// A request to weigh `n`, which a steward holds.
#[derive(Plain)]
pub struct Weigh {
    pub n: u64,
}

impl Kind for Weigh {
    const NAME: KindName = KindName::new("aether.guest.weigh");
}

/// What something weighs: the answer to a [`Weigh`].
#[derive(Plain)]
pub struct Weight {
    pub n: u64,
}

impl Kind for Weight {
    const NAME: KindName = KindName::new("aether.guest.weight");
}

/// The protocol a steward is asked to weigh through.
pub struct Weighs;

impl Protocol for Weighs {
    type Kinds = (Weigh,);
}

impl Carries<Weigh> for Weighs {
    type Answers = (Weight,);
}

/// What a steward does with the pings it holds: `0` answers each, `1` lets go of each, `2` closes on them, and `3`
/// keeps each in what it keeps for a request of its own, and answers it as what became of that request is handed back.
#[derive(Plain)]
pub struct Release {
    pub how: u64,
}

impl Kind for Release {
    const NAME: KindName = KindName::new("aether.guest.release");
}

/// What a steward keeps for a request of its own: a ping it holds, and the ping's number.
#[derive(Bound)]
pub struct Kept {
    n: u64,
    held: Held<Pong>,
}

impl Kind for Kept {
    const NAME: KindName = KindName::new("aether.guest.kept");
}

/// An actor that holds each ping it is asked, to answer it in a later turn as a [`Release`] says: with a pong of a
/// hundred and its number, or, through a request of its own to an echo it spawns as it begins, of two hundred and it.
/// It holds each weighing too, answered with a weight of a hundred and its number, or of three hundred and it, with
/// no request of its own. Closed on a release past three, its closing turn answers each with four hundred and it.
#[derive(Bound)]
pub struct Steward {
    held: Vec<(u64, Held<Pong>)>,
    weighing: Vec<(u64, Held<Weight>)>,
    echo: ActorRef<Echo>,
    closed_on: u64,
}

impl Kind for Steward {
    const NAME: KindName = KindName::new("aether.guest.steward");
}

impl ChildOf for Steward {
    type Parent = Anywhere;
}

impl Actor for Steward {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping, Weigh, Release, Answer<Pong, Kept>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let echo = turn.spawn(&(), EchoConfig).map_err(|_| Fault::new(&Shut).unwrap_or_else(Fault::from))?;

        Ok(Self { held: Vec::new(), weighing: Vec::new(), echo, closed_on: 0 })
    }

    fn end(self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        let answers = self.closed_on > 3;
        if !answers {
            return Ok(());
        }
        for (n, held) in self.held {
            turn.answer(held, &Pong { n: 400 + n })?;
        }
        for (n, held) in self.weighing {
            turn.answer(held, &Weight { n: 400 + n })?;
        }

        Ok(())
    }
}

impl Handles<Ping> for Steward {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.held.push((ping.n, turn.hold()?));

        Ok(())
    }
}

impl Handles<Weigh> for Steward {
    type Sends = (ToSender<Weight>,);

    fn handle(&mut self, weigh: Weigh, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.weighing.push((weigh.n, turn.hold()?));

        Ok(())
    }
}

impl Handles<Release> for Steward {
    type Sends = (Asks<Counts, Increment, Kept>,);

    fn handle(&mut self, release: Release, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = mem::take(&mut self.held);
        let weighing = mem::take(&mut self.weighing);
        match release.how {
            0 => {
                for (n, held) in held {
                    turn.answer(held, &Pong { n: 100 + n })?;
                }
                for (n, held) in weighing {
                    turn.answer(held, &Weight { n: 100 + n })?;
                }
            }
            1 => drop((held, weighing)),
            3 => {
                for (n, held) in held {
                    turn.ask_to(self.echo.protocol(), &Increment, &Kept { n, held })?;
                }
                for (n, held) in weighing {
                    turn.answer(held, &Weight { n: 300 + n })?;
                }
            }
            how => {
                self.held = held;
                self.weighing = weighing;
                self.closed_on = how;
                turn.close();
            }
        }

        Ok(())
    }
}

impl Handles<Answer<Pong, Kept>> for Steward {
    type Sends = ();

    fn handle(&mut self, (_, kept): (Answered<Pong>, Kept), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.answer(kept.held, &Pong { n: 200 + kept.n })?;

        Ok(())
    }
}

/// A word to a stasher to put a blob of `len` bytes and ask its echo a ping, keeping the blob until it is answered.
#[derive(Plain)]
pub struct Stash {
    pub len: u64,
}

impl Kind for Stash {
    const NAME: KindName = KindName::new("aether.guest.stash");
}

/// What a stasher keeps until its request is answered: a blob it refers to nowhere else.
#[derive(Bound)]
pub struct Stashed {
    pub blob: BlobRef,
}

impl Kind for Stashed {
    const NAME: KindName = KindName::new("aether.guest.stashed");
}

/// An actor that asks an echo it spawns as it begins, keeping only in its context a blob it puts, and tells its parent
/// how many bytes the blob holds once it is answered, as a pong heard at that many.
#[derive(Bound)]
pub struct Stasher {
    echo: ActorRef<Echo>,
}

impl Kind for Stasher {
    const NAME: KindName = KindName::new("aether.guest.stasher");
}

impl ChildOf for Stasher {
    type Parent = Anywhere;
}

impl Actor for Stasher {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Stash, Answer<Pong, Stashed>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let echo = turn.spawn(&(), EchoConfig).map_err(|_| Fault::new(&Shut).unwrap_or_else(Fault::from))?;

        Ok(Self { echo })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Stash> for Stasher {
    type Sends = (Asks<Pings, Ping, Stashed>,);

    fn handle(&mut self, stash: Stash, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let len = usize::try_from(stash.len).unwrap_or_default();
        let blob = turn.put(vec![7; len]).map_err(|why| unhatched(&why))?;
        turn.ask_to(self.echo.protocol(), &Ping { n: 0 }, &Stashed { blob })?;

        Ok(())
    }
}

impl Handles<Answer<Pong, Stashed>> for Stasher {
    type Sends = (ToAny<(Heard,), Heard>,);

    fn handle(
        &mut self,
        (pong, stashed): (Answered<Pong>, Stashed),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let at = turn.read(&stashed.blob).len() as u64;
        let heard = match pong {
            Answered::Yes(pong) => Heard::Pong { at, n: pong.n },
            Answered::No(why) => Heard::No { at, why },
        };
        turn.send_to_any(To::Parent, &heard)?;

        Ok(())
    }
}

/// A native actor the guest's crate holds but its blob does not make, begun by its host from a config that refers to a
/// blob: it tells its parent, as it begins, a pong of two hundred and the blob's length.
#[derive(Bound)]
pub struct Nestling;

impl Kind for Nestling {
    const NAME: KindName = KindName::new("aether.guest.nestling");
}

impl ChildOf for Nestling {
    type Parent = Nester;
}

impl Actor for Nestling {
    type Config = Book;
    type Key = ();
    type Handles = ();
    type BeginSends = (ToAny<(Pong,), Pong>,);
    type EndSends = ();

    fn begin(book: Book, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let n = 200 + turn.read(&book.blob).len() as u64;
        turn.send_to_any(ToWithoutMail::Parent, &Pong { n })?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that answers a book it is mailed by spawning a nestling made from it, keeping none of it, and tells its
/// parent each pong it is told.
#[derive(Bound)]
pub struct Nester;

impl Kind for Nester {
    const NAME: KindName = KindName::new("aether.guest.nester");
}

impl ChildOf for Nester {
    type Parent = Anywhere;
}

impl Actor for Nester {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Book, Pong);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Book> for Nester {
    type Sends = ();

    fn handle(&mut self, book: Book, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.spawn::<Nestling>(&(), book).map_err(|why| unhatched(&why))?;

        Ok(())
    }
}

impl Handles<Pong> for Nester {
    type Sends = (ToAny<(Pong,), Pong>,);

    fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.send_to_any(To::Parent, &pong)?;

        Ok(())
    }
}

/// What a ringer keeps while it waits on the bell.
#[derive(Plain)]
pub struct Ringing;

impl Kind for Ringing {
    const NAME: KindName = KindName::new("aether.guest.ringing");
}

/// What a ringer is made from: the path of the actor it tells what it heard.
#[derive(Plain)]
pub struct Ring {
    pub keeper: ActorPath,
}

impl Kind for Ring {
    const NAME: KindName = KindName::new("aether.guest.ring");
}

/// A root that answers each ping with a pong of one more, which an actor reaches by its type alone.
#[derive(Bound)]
pub struct Bell;

impl Kind for Bell {
    const NAME: KindName = KindName::new("aether.guest.bell");
}

impl ChildOf for Bell {
    type Parent = Top;
}

impl Actor for Bell {
    type Config = EchoConfig;
    type Key = u64;
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Carries<Ping> for Bell {
    type Answers = (Pong,);
}

impl Handles<Ping> for Bell {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(turn.reply(&Pong { n: ping.n + 1 })?)
    }
}

/// A root that, as it begins, asks the bell of key zero at its typed path, which its spawn does not check, a ping of four, and pings the actor at its keeper's
/// path with what it hears: the bell's pong, or ninety when the ask went unanswered.
#[derive(Bound)]
pub struct Ringer {
    keeper: ActorPath,
}

impl Kind for Ringer {
    const NAME: KindName = KindName::new("aether.guest.ringer");
}

impl ChildOf for Ringer {
    type Parent = Top;
}

impl Actor for Ringer {
    type Config = Ring;
    type Key = ();
    type Handles = (Answer<Pong, Ringing>,);
    type BeginSends = (Asks<Path<Bell>, Ping, Ringing>,);
    type EndSends = ();

    fn begin(ring: Ring, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.ask_to(path::<Bell>(0), &Ping { n: 4 }, &Ringing)?;

        Ok(Self { keeper: ring.keeper })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Pong, Ringing>> for Ringer {
    type Sends = (ToAny<(Ping,), Ping>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Pong>, Ringing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let n = match answered {
            Answered::Yes(pong) => pong.n,
            Answered::No(_) => 90,
        };
        if let Some(keeper) = turn.resolve(&self.keeper) {
            turn.send_to_any(To::Held(keeper), &Ping { n })?;
        }

        Ok(())
    }
}

/// A root, a singleton, that a sexton reaches at its typed path.
#[derive(Bound)]
pub struct Steeple;

impl Kind for Steeple {
    const NAME: KindName = KindName::new("aether.guest.steeple");
}

impl ChildOf for Steeple {
    type Parent = Top;
}

impl Actor for Steeple {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Steeple {
    type Sends = ();

    fn handle(&mut self, _: Ping, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor, a root or beneath any other, that declares a send at the steeple's typed path as it begins, and sends nothing: its spawn is checked by
/// what its section says of the path.
#[derive(Bound)]
pub struct Sexton;

impl Kind for Sexton {
    const NAME: KindName = KindName::new("aether.guest.sexton");
}

impl ChildOf for Sexton {
    type Parent = Anywhere;
}

impl Actor for Sexton {
    type Config = EchoConfig;
    type Key = ();
    type Handles = ();
    type BeginSends = (Tells<Path<Steeple>, Ping>,);
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A root that subscribes to `Start` as it begins, and pings the actor at its keeper's path with seven when it is
/// sent `Start`: a stage's handler that sends, which its host checks against what that handler declares.
#[derive(Bound)]
pub struct Chimer {
    keeper: ActorPath,
}

impl Kind for Chimer {
    const NAME: KindName = KindName::new("aether.guest.chimer");
}

impl ChildOf for Chimer {
    type Parent = Top;
}

impl Actor for Chimer {
    type Config = Ring;
    type Key = ();
    type Handles = (On<Start>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(ring: Ring, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();

        Ok(Self { keeper: ring.keeper })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Chimer {
    type Sends = (ToAny<(Ping,), Ping>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if let Some(keeper) = turn.resolve(&self.keeper) {
            turn.send_to_any(To::Held(keeper), &Ping { n: 7 })?;
        }

        Ok(())
    }
}

/// A root that spawns an echo beneath itself as it begins and looks for it there in the same turn, and on `Start`
/// looks for it again, now that its host's table holds it, and for one beneath its keeper, which has none. It pings
/// the actor at its keeper's path with what it found: a hundred for the first, ten for the second, and one for the
/// third, each when it was found.
#[derive(Bound)]
pub struct Usher {
    keeper: ActorPath,
    found_as_it_began: bool,
}

impl Kind for Usher {
    const NAME: KindName = KindName::new("aether.guest.usher");
}

impl ChildOf for Usher {
    type Parent = Top;
}

impl Actor for Usher {
    type Config = Ring;
    type Key = ();
    type Handles = (On<Start>,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(ring: Ring, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        let _ = turn.spawn::<Echo>(&(), EchoConfig);
        let me = turn.me();
        let found_as_it_began = turn.child::<Echo>(me, &()).is_some();

        Ok(Self { keeper: ring.keeper, found_as_it_began })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Start>> for Usher {
    type Sends = (ToAny<(Ping,), Ping>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some(keeper) = turn.resolve(&self.keeper) else {
            return Ok(());
        };
        let me = turn.me();
        let found_live = turn.child::<Echo>(me, &()).is_some();
        let found_beneath_keeper = turn.child::<Echo>(keeper, &()).is_some();
        let n = 100 * u64::from(self.found_as_it_began) + 10 * u64::from(found_live) + u64::from(found_beneath_keeper);
        turn.send_to_any(To::Held(keeper), &Ping { n })?;

        Ok(())
    }
}

aethereal_actor::guest!(
    Echo,
    Mirror,
    Responder,
    Doomed,
    Brood,
    Adder,
    Seeker,
    Watcher,
    Reporter,
    Hatcher,
    Boomerang,
    Librarian,
    Forger,
    Hoarder,
    Spendthrift,
    Parting,
    Archive,
    Reader,
    Caster,
    Porter,
    Querent,
    Shelver,
    Steward,
    Stasher,
    Nester,
    Opener,
    Timekeeper,
    Starter,
    Ticker,
    Metronome,
    Errand,
    Bell,
    Ringer,
    Steeple,
    Sexton,
    Chimer,
    Usher
);
