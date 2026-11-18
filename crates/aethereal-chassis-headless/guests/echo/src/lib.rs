//! A guest that answers each ping it is sent with a pong, to its sender (ADR-0015).

mod moving;

pub use moving::{Breaker, Keep, Keeper, LetGo, Mover, MoverConfig, Probe, Report, Stubborn};

use aethereal_actor::{
    ActorRef, Answered, Anywhere, Beginning, BlobId, Bytes, Ended, Ending, ErasedActorRef, Fault, Given, Handle, Held,
    How, Job, JobId, Later, Level, Origin, Portability, ProtocolRef, Recursive, Refused, Resource, Runner, Running,
    Speaks, Start, Stop, Tick, Turn, Unanswered, Upgrade, Upgrades, Within, actor, parent, path, protocol,
};
use aethereal_data::{
    ActorPath, Bound, Decode, DecodeError, Encode, EncodeError, KindId, KindName, Plain, Schema, Shape, Writer,
};
use core::mem;

/// A ping, which an echo answers with a pong of the same number.
#[derive(Plain)]
#[kind(name = "aether.guest.ping")]
pub struct Ping {
    pub n: u64,
}

/// The answer to a ping.
#[derive(Plain)]
#[kind(name = "aether.guest.pong")]
pub struct Pong {
    pub n: u64,
}

/// Some text, which an echo answers with a pong of how many bytes it holds: past the engine's read limit, mail its
/// receiver cannot read (ADR-0017). It is text, not bytes, as bytes cross between actors only as a blob, which keeps
/// mail small (ADR-0042).
#[derive(Plain)]
#[kind(name = "aether.guest.parcel")]
pub struct Parcel {
    pub text: String,
}

/// One to count, which an echo counts and does not answer.
#[derive(Plain)]
#[kind(name = "aether.guest.increment")]
pub struct Increment;

/// A word to close, which an echo closes on.
#[derive(Plain)]
#[kind(name = "aether.guest.shut")]
pub struct Shut;

/// A word to crash, on which a mirror panics, which traps, as a guest's panic does.
#[derive(Plain)]
#[kind(name = "aether.guest.crash")]
pub struct Crash;

/// What an echo is made from: nothing.
#[derive(Plain)]
#[kind(name = "aether.guest.echo_config")]
pub struct EchoConfig;

/// An actor that answers pings, and logs as it begins.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.echo")]
pub struct Echo {
    answered: u64,
    counted: u64,
}

#[actor(parent = Anywhere)]
impl Echo {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.log(Level::Info, "echo began");

        Ok(Self { answered: 0, counted: 0 })
    }

    #[request]
    fn ping(&mut self, ping: Ping) -> Result<Pong, Fault> {
        self.answered += 1;

        Ok(Pong { n: ping.n })
    }

    #[request]
    fn parcel(&mut self, parcel: Parcel) -> Result<Pong, Fault> {
        self.answered += 1;

        Ok(Pong { n: u64::try_from(parcel.text.len()).unwrap_or(u64::MAX) })
    }

    #[tell]
    fn increment(&mut self, _: Increment) -> Result<(), Fault> {
        self.counted += 1;

        Ok(())
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// A second kind of actor the blob makes, so two of its actors can live in one instance: it answers pings, and
/// closes on the word to.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.mirror")]
pub struct Mirror;

#[actor]
impl Mirror {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[request]
    fn ping(&mut self, ping: Ping) -> Result<Pong, Fault> {
        Ok(Pong { n: ping.n })
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }

    #[tell]
    fn crash(&mut self, _: Crash) -> Result<(), Fault> {
        panic!("a mirror crashes when it is told to");
    }
}

/// An actor that subscribes to the lifecycle's `Start` and `Stop` as it begins, logs each it is sent, and asks the
/// program to stop on `Start` (ADR-0020).
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.starter")]
pub struct Starter;

#[actor]
impl Starter {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Stop>();

        Ok(Self)
    }

    #[on]
    fn start(&mut self, _: Start, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.log(Level::Info, "started");
        turn.quit();

        Ok(())
    }

    #[on]
    fn stop(&mut self, _: Stop, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.log(Level::Info, "stopping");

        Ok(())
    }
}

/// An actor that subscribes to `Tick` as it begins: one an engine whose cycle runs no `Tick` does not make (ADR-0020).
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.ticker")]
pub struct Ticker;

#[actor(parent = Anywhere)]
impl Ticker {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.subscribe::<Tick>();

        Ok(Self)
    }

    #[on]
    fn tick(&mut self, _: Tick) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor that spawns a ticker of its own blob as it begins, and logs whether its engine made it: refused for
/// `Tick`, by the stage's kind, when its cycle runs none (ADR-0020).
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.metronome")]
pub struct Metronome;

#[actor(spawns(Ticker))]
impl Metronome {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let spawned = turn.spawn::<Ticker>(&(), EchoConfig);
        let said = match spawned {
            Ok(_) => "ticker made",
            Err(Refused::Unstaged(stage)) if stage.same(KindId::of::<Tick>()) => "ticker refused for tick",
            Err(_) => "ticker refused",
        };
        turn.log(Level::Info, said);

        Ok(Self)
    }
}

/// A job a guest runs, which answers with a pong of one more than its number (ADR-0018).
#[derive(Plain)]
#[kind(name = "aether.guest.chore")]
pub struct Chore {
    pub n: u64,
}

impl Job for Chore {
    type Done = Pong;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Pong, Fault> {
        Ok(Pong { n: self.n + 1 })
    }
}

/// A job a guest runs, which answers with a pong of how many bytes the blob it refers to holds (ADR-0014, ADR-0018).
#[derive(Bound)]
#[kind(name = "aether.guest.measure")]
pub struct Measure {
    pub blob: Bytes,
}

impl Job for Measure {
    type Done = Pong;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<Pong, Fault> {
        Ok(Pong { n: job.read(&self.blob).len() as u64 })
    }
}

/// The word to a foreman to run chores of the number and of one more, back to back, between two pongs to its sender.
#[derive(Plain)]
#[kind(name = "aether.guest.go")]
pub struct Go {
    pub n: u64,
}

/// The word to a foreman to cancel every job it has run.
#[derive(Plain)]
#[kind(name = "aether.guest.call_off")]
pub struct CallOff;

/// What a foreman keeps for a job until it is answered: the number of the order that ran it, nought for its `begin`'s.
#[derive(Plain)]
#[kind(name = "aether.guest.shift")]
pub struct Shift {
    pub at: u64,
}

/// An actor that runs jobs: as it begins, one that measures a blob it puts and keeps nowhere else, and on each order to,
/// two chores between two pongs to the order's sender. It logs the count each job took among what it sends, cancels them
/// all on the word to, logs what each job it did not cancel was answered with, and closes on the word to (ADR-0018).
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.foreman")]
pub struct Foreman {
    ran: Vec<JobId>,
}

#[actor]
impl Foreman {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let blob = turn.put(vec![7; 3]).map_err(|why| unhatched(&why))?;
        let job = turn.run(&Measure { blob }, &Shift { at: 0 })?;
        turn.log(Level::Info, format!("ran {}", job.mail().count));

        Ok(Self { ran: vec![job] })
    }

    #[tell]
    fn go(&mut self, go: Go, turn: &mut Turn<'_, Self, Speaks<(Pong,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Pong { n: go.n })?;
        let first = turn.run(&Chore { n: go.n }, &Shift { at: go.n })?;
        let second = turn.run(&Chore { n: go.n + 1 }, &Shift { at: go.n + 1 })?;
        turn.to(*turn.from()).send(&Pong { n: go.n })?;
        turn.log(Level::Info, format!("ran {} and {}", first.mail().count, second.mail().count));
        self.ran.extend([first, second]);

        Ok(())
    }

    #[tell]
    fn call_off(&mut self, _: CallOff, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        for job in self.ran.drain(..) {
            turn.cancel(job);
        }
        turn.log(Level::Info, "called off");

        Ok(())
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }

    #[answer]
    fn pong(&mut self, (done, shift): (Answered<Pong>, Shift), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let said = match done {
            Answered::Yes(pong) => format!("{} answered {}", shift.at, pong.n),
            Answered::No(why) => format!("{} unanswered: {why:?}", shift.at),
        };
        turn.log(Level::Info, &said);

        Ok(())
    }
}

/// An actor whose `begin` stands up an apprentice, which its host begins apart from the guest's call (ADR-0015).
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.overseer")]
pub struct Overseer;

#[actor(spawns(Apprentice))]
impl Overseer {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.spawn::<Apprentice>(&(), Chore { n: 7 }).map_err(|why| unhatched(&why))?;

        Ok(Self)
    }
}

/// A native actor the guest's crate holds but its blob does not make, begun by its host apart from the guest's call:
/// its `begin` runs the chore it was made from, and it logs what the chore was answered with (ADR-0018).
#[derive(Bound)]
#[kind(name = "aether.guest.apprentice")]
pub struct Apprentice;

#[actor(parent = Overseer)]
impl Apprentice {
    fn begin(chore: Chore, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let at = chore.n;
        turn.run(&chore, &Shift { at })?;

        Ok(Self)
    }

    #[answer]
    fn pong(&mut self, (done, shift): (Answered<Pong>, Shift), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let said = match done {
            Answered::Yes(pong) => format!("{} answered {}", shift.at, pong.n),
            Answered::No(why) => format!("{} unanswered: {why:?}", shift.at),
        };
        turn.log(Level::Info, &said);

        Ok(())
    }
}

/// An actor that tells each actor that pings it the time its turn reads, in nanoseconds, as a pong (ADR-0018).
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.timekeeper")]
pub struct Timekeeper;

#[actor]
impl Timekeeper {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn ping(&mut self, _: Ping, turn: &mut Turn<'_, Self, Speaks<(Pong,)>>) -> Result<(), Fault> {
        let n = turn.now().nanos();
        turn.to(*turn.from()).send(&Pong { n })?;

        Ok(())
    }
}

/// An actor of which the blob makes many, each under its own key: it tells each actor that pings it a pong of the
/// ping's number, and counts them, so each keeps state of its own in the instance it lives in, and closes on the word
/// to.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.responder")]
pub struct Responder {
    answered: u64,
}

#[actor(key = u64)]
impl Responder {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self { answered: 0 })
    }

    #[tell]
    fn ping(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Speaks<(Pong,)>>) -> Result<(), Fault> {
        self.answered += 1;
        turn.to(*turn.from()).send(&Pong { n: ping.n })?;

        Ok(())
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// An actor whose `begin` panics, which traps, as a guest's panic does.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.doomed")]
pub struct Doomed;

#[actor]
impl Doomed {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        panic!("a doomed actor crashes as it begins");
    }
}

/// What a brood is made from: how many children it spawns as it begins.
#[derive(Plain)]
#[kind(name = "aether.guest.brood_config")]
pub struct BroodConfig {
    pub children: u64,
}

/// A word to a brood to spawn one more child.
#[derive(Plain)]
#[kind(name = "aether.guest.grow")]
pub struct Grow;

/// An actor that spawns adders: some as it begins, and one more on each word to grow. The `k`th adds `10 k + 1`, and
/// the first spawns one of its own.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.brood")]
pub struct Brood {
    next: u64,
}

#[actor(spawns(Adder))]
impl Brood {
    fn begin(config: BroodConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        for key in 0..config.children {
            let config = AddBy { by: 10 * key + 1, deeper: u64::from(key == 0) };
            if turn.spawn::<Adder>(&key, config).is_err() {
                turn.log(Level::Error, "an adder was refused");
            }
        }

        Ok(Self { next: config.children })
    }

    #[tell]
    fn grow(&mut self, _: Grow, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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
#[kind(name = "aether.guest.add_by")]
pub struct AddBy {
    pub by: u64,
    pub deeper: u64,
}

/// An actor that tells whoever pings it a pong of the ping's number and its own, so the pong says its state survived
/// wherever it was kept. Made deeper, it spawns an adder of its own that adds a thousand more.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.adder")]
pub struct Adder {
    by: u64,
}

#[actor(key = u64, parent = Anywhere, spawns(Self))]
impl Adder {
    fn begin(config: AddBy, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        if config.deeper > 0 {
            let below = AddBy { by: config.by + 1000, deeper: config.deeper - 1 };
            if turn.spawn::<Self>(&0, below).is_err() {
                turn.log(Level::Error, "an adder below was refused");
            }
        }

        Ok(Self { by: config.by })
    }

    #[tell]
    fn ping(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Speaks<(Pong,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Pong { n: ping.n + self.by })?;

        Ok(())
    }
}

/// What a seeker is made from: the paths it resolves as it begins.
#[derive(Plain)]
#[kind(name = "aether.guest.seek")]
pub struct Seek {
    pub paths: Vec<ActorPath>,
}

/// An actor that resolves each path it is made with as it begins, and pings each actor one names with its place in
/// the list, so what is pinged says which paths it found.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.seeker")]
pub struct Seeker;

#[actor]
impl Seeker {
    fn begin(seek: Seek, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        for (n, path) in (0..).zip(&seek.paths) {
            if let Some(found) = turn.resolve(path).and_then(|found| turn.cast::<(Ping,)>(found)) {
                turn.to(found).send(&Ping { n })?;
            }
        }

        Ok(Self)
    }
}

/// An actor that resolves each path it is made with as it begins, casts each actor one names to a protocol, and
/// pings each that casts with its place in the list: to `(Ping,)` as it is, and to `(Ping, Shut)` past fifty, so what
/// is pinged says which casts answered.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.caster")]
pub struct Caster;

#[actor]
impl Caster {
    fn begin(seek: Seek, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        for (n, path) in (0..).zip(&seek.paths) {
            let Some(found) = turn.resolve(path) else {
                continue;
            };
            if let Some(pinged) = turn.cast::<(Ping,)>(found) {
                turn.to(pinged).send(&Ping { n })?;
            }
            if let Some(shut) = turn.cast::<(Ping, Shut)>(found) {
                turn.to(shut).send(&Ping { n: n + 50 })?;
            }
        }

        Ok(Self)
    }
}

/// What a watcher is made from: the paths of the actors it watches, and of the one it tells of their endings.
#[derive(Plain)]
#[kind(name = "aether.guest.watching")]
pub struct Watching {
    pub paths: Vec<ActorPath>,
    pub tell: ActorPath,
}

/// Why a watcher did not begin: the actor it tells was not found.
#[derive(Plain)]
#[kind(name = "aether.guest.untold")]
pub struct Untold;

/// An actor that watches each actor at the paths it is made with, and tells the actor at the last of how each ended:
/// a ping of ten for its place in the list, and one for a close, two for a fault, or three for an ending before the
/// watch.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.watcher")]
pub struct Watcher {
    watched: Vec<ErasedActorRef>,
    tell: ProtocolRef<(Ping,)>,
}

#[actor]
impl Watcher {
    fn begin(watching: Watching, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let Some(tell) = turn.resolve(&watching.tell).and_then(|found| turn.cast(found)) else {
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

    #[ended]
    fn ended(&mut self, ended: Ended, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let place = (0..).zip(&self.watched).find_map(|(place, watched)| (*watched == ended.actor()).then_some(place));
        let how = match ended.how() {
            How::Closed => 1,
            How::Failed(_) => 2,
            How::Earlier => 3,
        };
        if let Some(place) = place {
            turn.to(self.tell).send(&Ping { n: 10 * place + how })?;
        }

        Ok(())
    }
}

/// An actor that answers each ping by telling its parent, with a pong of the ping's number and its own key, so a turn
/// long after its `begin` still reaches whatever spawned it.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.reporter")]
pub struct Reporter {
    key: u64,
}

#[actor(key = u64, parent = Anywhere)]
impl Reporter {
    fn begin(config: AddBy) -> Result<Self, Fault> {
        Ok(Self { key: config.by })
    }

    #[tell]
    fn ping(&mut self, ping: Ping, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.to(parent::<(Pong,)>()).send(&Pong { n: ping.n + self.key })?;

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
        Refused::NoHead => 9,
        Refused::NotPublisher => 10,
    }
}

/// A native actor the guest's crate holds but its blob does not make: one a guest spawns is begun by its host. It
/// tells its parent, as it begins, a pong of a hundred and what it is made to add, and logs that it began.
#[derive(Bound)]
#[kind(name = "aether.guest.chick")]
pub struct Chick;

#[actor(key = u64, parent = Hatcher)]
impl Chick {
    fn begin(config: AddBy, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.to(turn.parent()).send(&Pong { n: 100 + config.by })?;
        turn.log(Level::Info, "a chick began");

        Ok(Self)
    }
}

/// A native actor named as a chick is, which declares other handlers: a guest's spawn of it is refused, as what the
/// program keeps of the name is not it.
#[derive(Bound)]
#[kind(name = "aether.guest.chick")]
pub struct FakeChick;

#[actor(key = u64, parent = Hatcher)]
impl FakeChick {
    fn begin(_: AddBy) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn ping(&mut self, _: Ping) -> Result<(), Fault> {
        Ok(())
    }
}

/// A native actor no program keeps for a guest to spawn.
#[derive(Bound)]
#[kind(name = "aether.guest.stray")]
pub struct Stray;

#[actor(parent = Hatcher)]
impl Stray {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self)
    }
}

/// Why a hatcher did not begin: a spawn was refused, for the reason numbered so.
#[derive(Plain)]
#[kind(name = "aether.guest.unhatched")]
pub struct Unhatched {
    pub why: u64,
}

/// The fault of a hatcher whose spawn was refused for `why`.
fn unhatched(why: &Refused) -> Fault {
    Fault::new(&Unhatched { why: refused(why) }).unwrap_or_else(|fault| fault.into())
}

/// What a hatcher is made from: another blob of code, and its own.
#[derive(Bound)]
#[kind(name = "aether.guest.hatch")]
pub struct Hatch {
    pub other: Bytes,
    pub home: Bytes,
}

/// An actor that spawns children of other code and tells its parent each pong they tell it. As it begins it spawns a
/// reporter of the other blob, adding twenty, which it pings. On the `k`th word to grow it spawns a chick, a native
/// child, adding `10 k + 2`; a stray and a fake chick, whose refusals it tells its parent as five and six hundred and
/// why; and, on the first, a boomerang of the other blob, which it pings.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.hatcher")]
pub struct Hatcher {
    other: Bytes,
    home: Bytes,
    grown: u64,
}

#[actor(parent = Anywhere, spawns(Boomerang, Reporter, Chick, Stray, FakeChick))]
impl Hatcher {
    fn begin(hatch: Hatch, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let reporter = turn.spawn_from::<Reporter>(Origin::Blob(&hatch.other), &7, &AddBy { by: 20, deeper: 0 });
        turn.to(reporter.map_err(|why| unhatched(&why))?).send(&Ping { n: 0 })?;

        Ok(Self { other: hatch.other, home: hatch.home, grown: 0 })
    }

    #[tell]
    fn pong(&mut self, pong: Pong, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.to(parent::<(Pong,)>()).send(&pong)?;

        Ok(())
    }

    #[tell]
    fn grow(&mut self, _: Grow, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let chick = AddBy { by: 10 * self.grown + 2, deeper: 0 };
        let _: ActorRef<Chick> = turn.spawn(&self.grown, chick).map_err(|why| unhatched(&why))?;
        let stray: Result<ActorRef<Stray>, _> = turn.spawn(&(), EchoConfig);
        if let Err(why) = stray {
            turn.to(parent::<(Pong,)>()).send(&Pong { n: 500 + refused(&why) })?;
        }
        let fake: Result<ActorRef<FakeChick>, _> = turn.spawn(&(100 + self.grown), AddBy { by: 0, deeper: 0 });
        if let Err(why) = fake {
            turn.to(parent::<(Pong,)>()).send(&Pong { n: 600 + refused(&why) })?;
        }
        let is_first = self.grown == 0;
        if is_first {
            let home = Home { home: self.home.clone() };
            let boomerang =
                turn.spawn_from::<Boomerang>(Origin::Blob(&self.other), &(), &home).map_err(|why| unhatched(&why))?;
            turn.to(boomerang).send(&Ping { n: 0 })?;
        }
        self.grown += 1;

        Ok(())
    }
}

/// What a boomerang is made from: the blob of code that spawned it.
#[derive(Bound)]
#[kind(name = "aether.guest.home")]
pub struct Home {
    pub home: Bytes,
}

/// An actor that spawns a reporter of the blob it was spawned from: refused as it begins, as that blob's instance waits
/// on the call out that begins it, which it tells its parent as three hundred and why, and made on a ping, which it
/// tells as four hundred.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.boomerang")]
pub struct Boomerang {
    home: Bytes,
}

#[actor(parent = Hatcher, spawns(Reporter))]
impl Boomerang {
    fn begin(home: Home, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let spawned = turn.spawn_from::<Reporter>(Origin::Blob(&home.home), &0, &AddBy { by: 0, deeper: 0 });
        let n = spawned.err().map_or(300, |why| 300 + refused(&why));
        turn.to(turn.parent()).send(&Pong { n })?;

        Ok(Self { home: home.home })
    }

    #[tell]
    fn ping(&mut self, _: Ping, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let spawned = turn.spawn_from::<Reporter>(Origin::Blob(&self.home), &1, &AddBy { by: 0, deeper: 0 });
        let n = spawned.err().map_or(400, |why| 400 + refused(&why));
        turn.to(turn.parent()).send(&Pong { n })?;

        Ok(())
    }
}

/// An actor that, as it begins, spawns a reporter of key five, adding one, from the head of the reporter's kind: refused
/// while no code has been published for that kind, which fails its `begin` with why (ADR-0054).
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.heir")]
pub struct Heir;

#[actor(spawns(Reporter))]
impl Heir {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let spawned = turn.spawn_from(Origin::Head, &5, &AddBy { by: 1, deeper: 0 });
        let _: ActorRef<Reporter> = spawned.map_err(|why| unhatched(&why))?;

        Ok(Self)
    }
}

/// What a librarian is made from: the book it keeps, and the path of the actor it tells how long the book is.
#[derive(Bound)]
#[kind(name = "aether.guest.shelf")]
pub struct Shelf {
    pub book: Bytes,
    pub tell: ActorPath,
}

/// A reference to a blob, sent as mail.
#[derive(Bound)]
#[kind(name = "aether.guest.book")]
pub struct Book {
    pub blob: Bytes,
}

/// An actor that keeps a blob and reads it as it begins, telling another a pong of its length; tells whoever pings it
/// for `n` a reference to a blob of `n` sevens, which it puts; tells whoever sends it a book a pong of a hundred and the
/// book's length; and closes on a word to shut.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.librarian")]
pub struct Librarian {
    book: Bytes,
}

#[actor(parent = Anywhere)]
impl Librarian {
    fn begin(shelf: Shelf, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let n = turn.read(&shelf.book).len() as u64;
        if let Some(tell) = turn.resolve(&shelf.tell).and_then(|found| turn.cast::<(Pong,)>(found)) {
            turn.to(tell).send(&Pong { n })?;
        }

        Ok(Self { book: shelf.book })
    }

    #[tell]
    fn ping(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Speaks<(Book,)>>) -> Result<(), Fault> {
        let len = usize::try_from(ping.n).unwrap_or(0);
        let blob = turn.put(vec![7; len]).map_err(|why| unhatched(&why))?;
        turn.to(*turn.from()).send(&Book { blob })?;

        Ok(())
    }

    #[tell]
    fn book(&mut self, book: Book, turn: &mut Turn<'_, Self, Speaks<(Pong,)>>) -> Result<(), Fault> {
        let n = 100 + turn.read(&book.blob).len() as u64;
        turn.to(*turn.from()).send(&Pong { n })?;

        Ok(())
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// What a forger is made from: the engine's number and the bits of a blob's id, as plain numbers, not a reference.
#[derive(Plain)]
#[kind(name = "aether.guest.forge")]
pub struct Forge {
    pub engine: u32,
    pub bits: u64,
}

/// A reference to the blob `id` names, which no one gave: what code that keeps no rules makes, as a guest's code
/// written by hand may. Its pin is noted, as a clone's is, so its host sees the guest take a reference.
fn forged(id: BlobId) -> Bytes {
    // SAFETY: a `Bytes` is laid out as its id. The one made holds no pin, and is let go of without its drop,
    // which would note one dropped; its clone notes the pin the guest claims.
    let unpinned = unsafe { core::mem::transmute::<BlobId, Bytes>(id) };
    let forged = unpinned.clone();
    core::mem::forget(unpinned);

    forged
}

/// An actor that makes a reference to a blob out of an id it was told as numbers, and reads it as it begins: its host
/// refuses the read, as the guest was never given the blob.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.forger")]
pub struct Forger;

#[actor]
impl Forger {
    fn begin(forge: Forge, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let forged = forged(BlobId::new(forge.engine, forge.bits));
        let _ = turn.read(&forged).len();

        Ok(Self)
    }
}

/// An actor that makes a reference to a blob out of an id it was told as numbers, and keeps it, reading nothing: its
/// host finds, among what its `begin` changed of its references, one to a blob the guest was never given.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.hoarder")]
pub struct Hoarder {
    forged: Bytes,
}

#[actor]
impl Hoarder {
    fn begin(forge: Forge) -> Result<Self, Fault> {
        Ok(Self { forged: forged(BlobId::new(forge.engine, forge.bits)) })
    }
}

/// An actor that, pinged for `n`, puts a blob of `n` sevens, spawns a librarian to keep it that tells no one, and then
/// tells its pinger a reference to a blob it was never given: its host refuses that send, and the turn fails with the
/// librarian it stood up, which the engine never ends.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.spendthrift")]
pub struct Spendthrift;

#[actor(spawns(Librarian))]
impl Spendthrift {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn ping(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Speaks<(Book,)>>) -> Result<(), Fault> {
        let len = usize::try_from(ping.n).unwrap_or(0);
        let book = turn.put(vec![7; len]).map_err(|why| unhatched(&why))?;
        let tell = ActorPath::from_root(&aethereal_data::PathSegment::new(KindName::new("aether.guest.no_one"), &()));
        let engine = book.id().engine();
        turn.spawn::<Librarian>(&(), Shelf { book, tell }).map_err(|why| unhatched(&why))?;
        turn.to(*turn.from()).send(&Book { blob: forged(BlobId::new(engine, u64::MAX)) })?;

        Ok(())
    }
}

/// What a parting actor is made from: the path of the actor it tells as it ends.
#[derive(Bound)]
#[kind(name = "aether.guest.farewell")]
pub struct Farewell {
    pub tell: ActorPath,
}

/// An actor that counts the pings it is sent, and fails on a ping of zero; closes on a word to shut; and, in its
/// closing turn, tells the actor it was made to tell a pong of six hundred, ten for each ping it counted, and one when
/// it ends because a turn of its own failed.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.parting")]
pub struct Parting {
    tell: ProtocolRef<(Pong,)>,
    pings: u64,
}

#[actor(key = u64)]
impl Parting {
    fn begin(farewell: Farewell, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let Some(tell) = turn.resolve(&farewell.tell).and_then(|found| turn.cast(found)) else {
            return Err(Fault::new(&Untold)?);
        };

        Ok(Self { tell, pings: 0 })
    }

    fn end(self, turn: &mut Ending<'_, Self>) -> Result<(), Fault> {
        let is_faulted = turn.why() == aethereal_actor::Why::Faulted;
        let n = 600 + 10 * self.pings + u64::from(is_faulted);
        turn.to(self.tell).send(&Pong { n })?;

        Ok(())
    }

    #[tell]
    fn ping(&mut self, ping: Ping) -> Result<(), Fault> {
        if ping.n == 0 {
            return Err(Fault::new(&Untold)?);
        }
        self.pings += 1;

        Ok(())
    }

    #[tell]
    fn shut(&mut self, _: Shut, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

/// What an archive is made from: the book its readers keep, and how many readers it spawns.
#[derive(Bound)]
#[kind(name = "aether.guest.stacks")]
pub struct Stacks {
    pub book: Bytes,
    pub readers: u64,
}

/// An actor that spawns readers as it begins, each keeping a reference to the book it was made with, and keeps none
/// itself.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.archive")]
pub struct Archive;

#[actor(spawns(Reader))]
impl Archive {
    fn begin(stacks: Stacks, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        for key in 0..stacks.readers {
            if turn.spawn::<Reader>(&key, Book { blob: stacks.book.clone() }).is_err() {
                turn.log(Level::Error, "a reader was refused");
            }
        }

        Ok(Self)
    }
}

/// An actor that keeps a reference to a book, and tells whoever pings it a pong of the ping's number and the length
/// it reads the book to be, and closes.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.reader")]
pub struct Reader {
    book: Bytes,
}

#[actor(key = u64, parent = Archive)]
impl Reader {
    fn begin(book: Book) -> Result<Self, Fault> {
        Ok(Self { book: book.blob })
    }

    #[tell]
    fn ping(&mut self, ping: Ping, turn: &mut Turn<'_, Self, Speaks<(Pong,)>>) -> Result<(), Fault> {
        let n = ping.n + turn.read(&self.book).len() as u64;
        turn.to(*turn.from()).send(&Pong { n })?;
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
#[kind(name = "aether.guest.hand")]
pub struct Hand {
    pub lamps: Vec<Given<Lamp>>,
}

/// What a porter is made from: who it passes lamps on to, the lamps it begins with, and how it passes those on.
#[derive(Bound)]
#[kind(name = "aether.guest.porting")]
pub struct Porting {
    pub to: ActorPath,
    pub lamps: Vec<Given<Lamp>>,
    pub how: u64,
}

/// A guest that passes on every lamp it is given to the actor `to` names, which handles hands of lamps: what comes in
/// mail it sends on, and what its config holds as `how` says, sending it on for nought, handing it in its config to a
/// porter it spawns of its own code, which sends it on, for one, and to a bearer, of native code, which it pings to
/// send it on, for two.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.porter")]
pub struct Porter {
    to: ActorPath,
}

impl Porter {
    /// The actor it passes lamps on to, as one that handles hands of them.
    fn to(&self, turn: &impl Resolves) -> Result<ProtocolRef<(Hand,)>, Fault> {
        let to = turn.resolve_cast(&self.to);

        to.ok_or_else(|| Fault::new(&Shut).unwrap_or_else(|why| why.into()))
    }
}

/// What a porter's turns read to find who they pass lamps on to.
trait Resolves {
    /// The actor `path` names, when it is live and handles hands of lamps.
    fn resolve_cast(&self, path: &ActorPath) -> Option<ProtocolRef<(Hand,)>>;
}

impl<A> Resolves for Beginning<'_, A> {
    fn resolve_cast(&self, path: &ActorPath) -> Option<ProtocolRef<(Hand,)>> {
        let found = self.resolve(path)?;

        self.cast(found)
    }
}

impl<A> Resolves for Turn<'_, A> {
    fn resolve_cast(&self, path: &ActorPath) -> Option<ProtocolRef<(Hand,)>> {
        let found = self.resolve(path)?;

        self.cast(found)
    }
}

#[actor(key = u64, parent = Recursive, spawns(Self, Bearer))]
impl Porter {
    fn begin(config: Porting, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
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
                turn.to(to).send(&Hand { lamps })?;
            }
            1 => {
                let spawned: Result<ActorRef<Self>, _> = turn.spawn(&1, Porting { to, lamps, how: 0 });
                spawned.map_err(|why| unhatched(&why))?;
            }
            _ => {
                let spawned: Result<ActorRef<Bearer>, _> = turn.spawn(&0, Porting { to, lamps, how: 0 });
                let bearer = spawned.map_err(|why| unhatched(&why))?;
                turn.to(bearer).send(&Ping { n: 0 })?;
            }
        }

        Ok(porter)
    }

    #[tell]
    fn hand(&mut self, hand: Hand, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let held: Vec<Handle<Lamp>> = hand.lamps.into_iter().map(|lamp| turn.take(lamp)).collect::<Result<_, _>>()?;
        let lamps = held.into_iter().map(|lamp| turn.give(lamp)).collect::<Result<Vec<_>, _>>()?;
        let to = self.to(turn)?;
        turn.to(to).send(&Hand { lamps })?;

        Ok(())
    }
}

/// A native actor the guest's crate holds but its blob does not make, which a porter spawns: it takes the lamps its
/// config holds as its host begins it, and keeps them until it is pinged, when it sends them on to the actor `to`
/// names.
#[derive(Bound)]
#[kind(name = "aether.guest.bearer")]
pub struct Bearer {
    to: ActorPath,
    lamps: Vec<Handle<Lamp>>,
}

#[actor(key = u64, parent = Porter)]
impl Bearer {
    fn begin(config: Porting, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let Porting { to, lamps, .. } = config;
        let lamps = lamps.into_iter().map(|lamp| turn.take(lamp)).collect::<Result<_, _>>()?;

        Ok(Self { to, lamps })
    }

    #[tell]
    fn ping(&mut self, _: Ping, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let held = core::mem::take(&mut self.lamps);
        let lamps = held.into_iter().map(|lamp| turn.give(lamp)).collect::<Result<Vec<_>, _>>()?;
        let to = turn.resolve_cast(&self.to).ok_or_else(|| Fault::new(&Shut).unwrap_or_else(|why| why.into()))?;
        turn.to(to).send(&Hand { lamps })?;

        Ok(())
    }
}

/// The protocol an echo is asked pings through: each answered with a pong.
#[protocol(Ping(answers = Pong))]
pub struct Pings;

/// The protocol an echo is asked to count through, as if each were answered with a pong: an echo counts and does not
/// answer, so the engine says it went unanswered. An echo's type does not conform to it, as its handler of an
/// increment answers nothing: an asker reaches it through a cast, which reads only that the echo takes the kind.
#[protocol(Increment(answers = Pong))]
pub struct Counts;

/// A word to a querent to ask its echo `pings` pings and then to count once, each kept with which it was, counting
/// from `first`.
#[derive(Plain)]
#[kind(name = "aether.guest.query")]
pub struct Query {
    pub first: u64,
    pub pings: u64,
}

/// What a querent keeps until a request it asked is answered: which it was.
#[derive(Plain)]
#[kind(name = "aether.guest.which")]
pub struct Which {
    pub at: u64,
}

/// What became of a querent's request, as it tells its parent: the pong it was answered with, or why it was not, with
/// which request it was.
#[derive(Plain)]
#[kind(name = "aether.guest.heard")]
pub enum Heard {
    Pong { at: u64, n: u64 },
    No { at: u64, why: Unanswered },
}

/// An actor that asks an echo it spawns as it begins, and tells its parent what became of each request, with what it
/// kept for it.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.querent")]
pub struct Querent {
    echo: ActorRef<Echo>,
}

#[actor(parent = Anywhere, spawns(Echo))]
impl Querent {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let echo = turn.spawn(&(), EchoConfig).map_err(|_| Fault::new(&Shut).unwrap_or_else(Fault::from))?;

        Ok(Self { echo })
    }

    #[tell]
    fn query(&mut self, query: Query, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let last = query.first + query.pings;
        for at in query.first..last {
            turn.to(self.echo.protocol::<Pings>()).ask(&Ping { n: at * 10 }, &Which { at })?;
        }
        let Some(counts) = turn.cast::<Counts>(self.echo) else {
            return Err(Fault::new(&Shut)?);
        };
        turn.to(counts).ask(&Increment, &Which { at: last })?;

        Ok(())
    }

    #[answer]
    fn pong(&mut self, (pong, which): (Answered<Pong>, Which), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let heard = match pong {
            Answered::Yes(pong) => Heard::Pong { at: which.at, n: pong.n },
            Answered::No(why) => Heard::No { at: which.at, why },
        };
        turn.to(parent::<(Heard,)>()).send(&heard)?;

        Ok(())
    }
}

/// What an opener is made from: the ping it asks as it begins, and how many more openers to stand up beneath it.
#[derive(Plain)]
#[kind(name = "aether.guest.opening")]
pub struct Opening {
    pub n: u64,
    pub below: u64,
}

/// An actor whose `begin` spawns an echo and asks it a ping of `n`, and, while `below` is more than none, stands up an
/// opener of its own code beneath it that asks a ping of one more; the last stands up an inlet that does. It tells
/// its parent what became of its request, and passes on what each actor below tells it.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.opener")]
pub struct Opener;

#[actor(key = u64, parent = Anywhere, spawns(Echo, Self, Inlet))]
impl Opener {
    fn begin(opening: Opening, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let echo: ActorRef<Echo> = turn.spawn(&(), EchoConfig).map_err(|why| unhatched(&why))?;
        turn.to(echo.protocol::<Pings>()).ask(&Ping { n: opening.n }, &Which { at: opening.n })?;
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

    #[answer]
    fn pong(&mut self, (pong, which): (Answered<Pong>, Which), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let heard = match pong {
            Answered::Yes(pong) => Heard::Pong { at: which.at, n: pong.n },
            Answered::No(why) => Heard::No { at: which.at, why },
        };
        turn.to(parent::<(Heard,)>()).send(&heard)?;

        Ok(())
    }

    #[tell]
    fn heard(&mut self, heard: Heard, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.to(parent::<(Heard,)>()).send(&heard)?;

        Ok(())
    }
}

/// A native actor the guest's crate holds but its blob does not make, begun by its host apart from the guest's call:
/// its `begin` spawns an echo and asks it the ping it was made from, and it tells its parent what became of it.
#[derive(Bound)]
#[kind(name = "aether.guest.inlet")]
pub struct Inlet;

#[actor(parent = Opener, spawns(Echo))]
impl Inlet {
    fn begin(ping: Ping, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let echo = turn.spawn::<Echo>(&(), EchoConfig).map_err(|why| unhatched(&why))?;
        turn.to(echo.protocol::<Pings>()).ask(&Ping { n: ping.n }, &Which { at: ping.n })?;

        Ok(Self)
    }

    #[answer]
    fn pong(&mut self, (pong, which): (Answered<Pong>, Which), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let heard = match pong {
            Answered::Yes(pong) => Heard::Pong { at: which.at, n: pong.n },
            Answered::No(why) => Heard::No { at: which.at, why },
        };
        turn.to(turn.parent()).send(&heard)?;

        Ok(())
    }
}

/// An actor that, pinged for `n`, tells its parent a pong of `n`, and then two books, of `n` sevens and of one
/// more, sent back to back: a run of mail, after other mail, whose references its host must place.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.shelver")]
pub struct Shelver;

#[actor(parent = Anywhere)]
impl Shelver {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn ping(&mut self, ping: Ping, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let len = usize::try_from(ping.n).unwrap_or(0);
        turn.to(parent::<(Pong,)>()).send(&Pong { n: ping.n })?;
        for more in 0..2 {
            let blob = turn.put(vec![7; len + more]).map_err(|why| unhatched(&why))?;
            turn.to(parent::<(Book,)>()).send(&Book { blob })?;
        }

        Ok(())
    }
}

/// A request to weigh `n`, which a steward holds.
#[derive(Plain)]
#[kind(name = "aether.guest.weigh")]
pub struct Weigh {
    pub n: u64,
}

/// What something weighs: the answer to a [`Weigh`].
#[derive(Plain)]
#[kind(name = "aether.guest.weight")]
pub struct Weight {
    pub n: u64,
}

/// The protocol a steward is asked to weigh through.
#[protocol(Weigh(answers = Weight))]
pub struct Weighs;

/// What a steward does with the pings it holds: `0` answers each, `1` lets go of each, `2` closes on them, and `3`
/// keeps each in what it keeps for a request of its own, and answers it as what became of that request is handed back.
#[derive(Plain)]
#[kind(name = "aether.guest.release")]
pub struct Release {
    pub how: u64,
}

/// An actor that holds each ping it is asked and, in the turn that holds it, asks a steward it spawns as it begins the
/// same ping, stating no deadline, so its ask inherits the ping's: it tells its parent what became of its own
/// (ADR-0041).
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.envoy")]
pub struct Envoy {
    steward: ActorRef<Steward>,
    held: Vec<Held<Pong>>,
}

#[actor(parent = Anywhere, spawns(Steward))]
impl Envoy {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let steward = turn.spawn(&(), EchoConfig).map_err(|_| Fault::new(&Shut).unwrap_or_else(Fault::from))?;

        Ok(Self { steward, held: Vec::new() })
    }

    #[request]
    fn ping(&mut self, ping: Ping, turn: &mut Turn<'_, Self>) -> Result<Later<Pong>, Fault> {
        let (held, later) = turn.hold();
        self.held.push(held);
        turn.to(self.steward.protocol::<Pings>()).ask(&Ping { n: ping.n }, &Which { at: ping.n })?;

        Ok(later)
    }

    #[answer]
    fn pong(&mut self, (pong, which): (Answered<Pong>, Which), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let heard = match pong {
            Answered::Yes(pong) => Heard::Pong { at: which.at, n: pong.n },
            Answered::No(why) => Heard::No { at: which.at, why },
        };
        turn.to(parent::<(Heard,)>()).send(&heard)?;

        Ok(())
    }
}

/// What a steward keeps for a request of its own: a ping it holds, and the ping's number.
#[derive(Bound)]
#[kind(name = "aether.guest.kept")]
pub struct Kept {
    n: u64,
    held: Held<Pong>,
}

/// An actor that holds each ping it is asked, to answer it in a later turn as a [`Release`] says: with a pong of a
/// hundred and its number, or, through a request of its own to an echo it spawns as it begins, of two hundred and it.
/// It holds each weighing too, answered with a weight of a hundred and its number, or of three hundred and it, with
/// no request of its own. Closed on a release past three, its closing turn answers each with four hundred and it.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.steward")]
pub struct Steward {
    held: Vec<(u64, Held<Pong>)>,
    weighing: Vec<(u64, Held<Weight>)>,
    echo: ActorRef<Echo>,
    closed_on: u64,
}

#[actor(parent = Anywhere, spawns(Echo))]
impl Steward {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let echo = turn.spawn(&(), EchoConfig).map_err(|_| Fault::new(&Shut).unwrap_or_else(Fault::from))?;

        Ok(Self { held: Vec::new(), weighing: Vec::new(), echo, closed_on: 0 })
    }

    fn end(self, turn: &mut Ending<'_, Self>) -> Result<(), Fault> {
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

    #[request]
    fn ping(&mut self, ping: Ping, turn: &mut Turn<'_, Self>) -> Result<Later<Pong>, Fault> {
        let (held, later) = turn.hold();
        self.held.push((ping.n, held));

        Ok(later)
    }

    #[request]
    fn weigh(&mut self, weigh: Weigh, turn: &mut Turn<'_, Self>) -> Result<Later<Weight>, Fault> {
        let (held, later) = turn.hold();
        self.weighing.push((weigh.n, held));

        Ok(later)
    }

    #[tell]
    fn release(&mut self, release: Release, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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
                let Some(counts) = turn.cast::<Counts>(self.echo) else {
                    return Err(Fault::new(&Shut)?);
                };
                for (n, held) in held {
                    turn.to(counts).ask(&Increment, &Kept { n, held })?;
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

    #[answer]
    fn pong(&mut self, (_, kept): (Answered<Pong>, Kept), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.answer(kept.held, &Pong { n: 200 + kept.n })?;

        Ok(())
    }
}

/// A word to a stasher to put a blob of `len` bytes and ask its echo a ping, keeping the blob until it is answered.
#[derive(Plain)]
#[kind(name = "aether.guest.stash")]
pub struct Stash {
    pub len: u64,
}

/// What a stasher keeps until its request is answered: a blob it refers to nowhere else.
#[derive(Bound)]
#[kind(name = "aether.guest.stashed")]
pub struct Stashed {
    pub blob: Bytes,
}

/// An actor that asks an echo it spawns as it begins, keeping only in its context a blob it puts, and tells its parent
/// how many bytes the blob holds once it is answered, as a pong heard at that many.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.stasher")]
pub struct Stasher {
    echo: ActorRef<Echo>,
}

#[actor(parent = Anywhere, spawns(Echo))]
impl Stasher {
    fn begin(_: EchoConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let echo = turn.spawn(&(), EchoConfig).map_err(|_| Fault::new(&Shut).unwrap_or_else(Fault::from))?;

        Ok(Self { echo })
    }

    #[tell]
    fn stash(&mut self, stash: Stash, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let len = usize::try_from(stash.len).unwrap_or_default();
        let blob = turn.put(vec![7; len]).map_err(|why| unhatched(&why))?;
        turn.to(self.echo.protocol::<Pings>()).ask(&Ping { n: 0 }, &Stashed { blob })?;

        Ok(())
    }

    #[answer]
    fn pong(&mut self, (pong, stashed): (Answered<Pong>, Stashed), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let at = turn.read(&stashed.blob).len() as u64;
        let heard = match pong {
            Answered::Yes(pong) => Heard::Pong { at, n: pong.n },
            Answered::No(why) => Heard::No { at, why },
        };
        turn.to(parent::<(Heard,)>()).send(&heard)?;

        Ok(())
    }
}

/// A native actor the guest's crate holds but its blob does not make, begun by its host from a config that refers to a
/// blob: it tells its parent, as it begins, a pong of two hundred and the blob's length.
#[derive(Bound)]
#[kind(name = "aether.guest.nestling")]
pub struct Nestling;

#[actor(parent = Nester)]
impl Nestling {
    fn begin(book: Book, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let n = 200 + turn.read(&book.blob).len() as u64;
        turn.to(turn.parent()).send(&Pong { n })?;

        Ok(Self)
    }
}

/// An actor that answers a book it is mailed by spawning a nestling made from it, keeping none of it, and tells its
/// parent each pong it is told.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.nester")]
pub struct Nester;

#[actor(parent = Anywhere, spawns(Nestling))]
impl Nester {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn book(&mut self, book: Book, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.spawn::<Nestling>(&(), book).map_err(|why| unhatched(&why))?;

        Ok(())
    }

    #[tell]
    fn pong(&mut self, pong: Pong, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.to(parent::<(Pong,)>()).send(&pong)?;

        Ok(())
    }
}

/// What a ringer keeps while it waits on the bell.
#[derive(Plain)]
#[kind(name = "aether.guest.ringing")]
pub struct Ringing;

/// What a ringer is made from: the path of the actor it tells what it heard.
#[derive(Plain)]
#[kind(name = "aether.guest.ring")]
pub struct Ring {
    pub keeper: ActorPath,
}

/// A root that answers each ping with a pong of one more, which an actor reaches by its type alone.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.bell")]
pub struct Bell;

#[actor(key = u64)]
impl Bell {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[request]
    fn ping(&mut self, ping: Ping) -> Result<Pong, Fault> {
        Ok(Pong { n: ping.n + 1 })
    }
}

/// A root that, as it begins, asks the bell of key zero at its typed path, which its spawn does not check, a ping of four, and pings the actor at its keeper's
/// path with what it hears: the bell's pong, or ninety when the ask went unanswered.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.ringer")]
pub struct Ringer {
    keeper: ActorPath,
}

#[actor(depends(Bell))]
impl Ringer {
    fn begin(ring: Ring, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.to(path::<Bell>(0)).ask(&Ping { n: 4 }, &Ringing)?;

        Ok(Self { keeper: ring.keeper })
    }

    #[answer]
    fn pong(&mut self, (answered, _): (Answered<Pong>, Ringing), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let n = match answered {
            Answered::Yes(pong) => pong.n,
            Answered::No(_) => 90,
        };
        if let Some(keeper) = turn.resolve(&self.keeper).and_then(|found| turn.cast::<(Ping,)>(found)) {
            turn.to(keeper).send(&Ping { n })?;
        }

        Ok(())
    }
}

/// A root, a singleton, that a sexton reaches at its typed path.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.steeple")]
pub struct Steeple;

#[actor]
impl Steeple {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn ping(&mut self, _: Ping) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor, a root or beneath any other, that declares a send at the steeple's typed path as it begins, and sends nothing: its spawn is checked by
/// what its section says of the path.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.sexton")]
pub struct Sexton;

#[actor(parent = Anywhere, depends(Steeple))]
impl Sexton {
    fn begin(_: EchoConfig) -> Result<Self, Fault> {
        Ok(Self)
    }
}

/// A root that subscribes to `Start` as it begins, and pings the actor at its keeper's path with seven when it is
/// sent `Start`: a stage's handler that sends, which its host checks against what that handler declares.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.chimer")]
pub struct Chimer {
    keeper: ActorPath,
}

#[actor]
impl Chimer {
    fn begin(ring: Ring, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();

        Ok(Self { keeper: ring.keeper })
    }

    #[on]
    fn start(&mut self, _: Start, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if let Some(keeper) = turn.resolve(&self.keeper).and_then(|found| turn.cast::<(Ping,)>(found)) {
            turn.to(keeper).send(&Ping { n: 7 })?;
        }

        Ok(())
    }
}

/// A root that spawns an echo beneath itself as it begins and looks for it there in the same turn, and on `Start`
/// looks for it again, now that its host's table holds it, and for one beneath its keeper, which has none. It pings
/// the actor at its keeper's path with what it found: a hundred for the first, ten for the second, and one for the
/// third, each when it was found.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.usher")]
pub struct Usher {
    keeper: ActorPath,
    found_as_it_began: bool,
}

#[actor(spawns(Echo))]
impl Usher {
    fn begin(ring: Ring, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        let _ = turn.spawn::<Echo>(&(), EchoConfig);
        let me = turn.me();
        let found_as_it_began = turn.child::<Echo>(me, &()).is_some();

        Ok(Self { keeper: ring.keeper, found_as_it_began })
    }

    #[on]
    fn start(&mut self, _: Start, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(keeper) = turn.resolve(&self.keeper) else {
            return Ok(());
        };
        let me = turn.me();
        let found_live = turn.child::<Echo>(me, &()).is_some();
        let found_beneath_keeper = turn.child::<Echo>(keeper, &()).is_some();
        let n = 100 * u64::from(self.found_as_it_began) + 10 * u64::from(found_live) + u64::from(found_beneath_keeper);
        if let Some(keeper) = turn.cast::<(Ping,)>(keeper) {
            turn.to(keeper).send(&Ping { n })?;
        }

        Ok(())
    }
}

/// What a litter is made from: how many homebodies it spawns as it begins.
#[derive(Plain)]
#[kind(name = "aether.guest.litter_config")]
pub struct LitterConfig {
    pub children: u64,
}

/// A root that spawns homebodies as it begins, keyed from nought: past its part's room they are seated in other
/// parts, where its guest must take each out, which a homebody's state refuses.
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.litter")]
pub struct Litter;

#[actor(spawns(Homebody))]
impl Litter {
    fn begin(config: LitterConfig, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        for key in 0..config.children {
            if turn.spawn::<Homebody>(&key, HomebodyConfig).is_err() {
                turn.log(Level::Error, "a homebody was refused");
            }
        }

        Ok(Self)
    }
}

/// What a homebody is made from: nothing.
#[derive(Plain)]
#[kind(name = "aether.guest.homebody_config")]
pub struct HomebodyConfig;

/// A value whose codec is broken: it refuses to be written at `Within`, as a state is to be taken out to another
/// part's instance, which a sound codec never does.
pub struct Tether;

impl Schema for Tether {
    const SHAPE: Shape = <u32 as Schema>::SHAPE;
}

impl Encode<Within> for Tether {
    fn encode(&self, _: &mut Writer<'_, Within>) -> Result<(), EncodeError> {
        Err(EncodeError::NotCarried { name: "Tether" })
    }
}

impl Decode<Within> for Tether {
    fn decode(input: &mut aethereal_data::Reader<'_, Within>) -> Result<Self, DecodeError> {
        u32::decode(input)?;

        Ok(Self)
    }
}

/// An actor whose state holds a tether, so its guest cannot take it out to another part: a bug in its type, which
/// traps the call that stood it up there (ADR-0015).
#[derive(Bound, Portability)]
#[kind(name = "aether.guest.homebody")]
pub struct Homebody {
    tether: Tether,
}

#[actor(key = u64, parent = Litter)]
impl Homebody {
    fn begin(_: HomebodyConfig) -> Result<Self, Fault> {
        Ok(Self { tether: Tether })
    }
}

/// Says of each type that it moves onto its kind's head as soon as it is asked, and writes no `take_over`: the tests
/// publish no new code for it (ADR-0054).
macro_rules! moves_when_asked {
    ($($actor:ty),+ $(,)?) => {
        $(
            impl Upgrades for $actor {
                fn upgrade(&self) -> Upgrade {
                    Upgrade::Now
                }
            }
        )+
    };
}

moves_when_asked!(
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
    Heir,
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
    Envoy,
    Stasher,
    Nester,
    Opener,
    Timekeeper,
    Starter,
    Ticker,
    Metronome,
    Foreman,
    Overseer,
    Bell,
    Ringer,
    Steeple,
    Sexton,
    Chimer,
    Usher,
    Litter,
    Homebody,
);

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
    Heir,
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
    Envoy,
    Stasher,
    Nester,
    Opener,
    Timekeeper,
    Starter,
    Ticker,
    Metronome,
    Foreman,
    Overseer,
    Bell,
    Ringer,
    Steeple,
    Sexton,
    Chimer,
    Usher,
    Mover,
    Keeper,
    Stubborn,
    Breaker,
    Litter,
    Homebody
);
