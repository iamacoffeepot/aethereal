//! A guest that answers each ping it is sent with a pong, to its sender (ADR-0015).

use aethereal_actor::{
    Actor, Beginning, BlobId, BlobRef, Ended, Ending, ErasedActorRef, Fault, Handles, How, Level, Refused, To, ToAny,
    ToSender, ToType, ToWithoutMail, Turn, Watches,
};
use aethereal_data::{ActorPath, Bound, Kind, KindName, Plain};

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

impl Actor for Seeker {
    type Config = Seek;
    type Key = ();
    type Handles = ();
    type BeginSends = (ToAny<(Ping,), Ping>,);
    type EndSends = ();

    fn begin(seek: Seek, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        for (n, path) in (0..).zip(&seek.paths) {
            if let Some(found) = turn.resolve(path) {
                turn.to_any::<(Ping,)>(ToWithoutMail::Held(found)).send(&Ping { n })?;
            }
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
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

impl Actor for Watcher {
    type Config = Watching;
    type Key = ();
    type Handles = (Ended,);
    type BeginSends = (Watches,);
    type EndSends = ();

    fn begin(watching: Watching, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
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

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ended> for Watcher {
    type Sends = (ToAny<(Ping,), Ping>,);

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let place = (0..).zip(&self.watched).find_map(|(place, watched)| (*watched == ended.actor()).then_some(place));
        let how = match ended.how() {
            How::Closed => 1,
            How::Failed(_) => 2,
            How::Earlier => 3,
        };
        if let Some(place) = place {
            turn.to_any::<(Ping,)>(To::Held(self.tell)).send(&Ping { n: 10 * place + how })?;
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

impl Actor for Reporter {
    type Config = AddBy;
    type Key = u64;
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: AddBy, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { key: config.by })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Reporter {
    type Sends = (ToAny<(Pong,), Pong>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.to_any::<(Pong,)>(To::Parent).send(&Pong { n: ping.n + self.key })?;

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
    }
}

/// A native actor the guest's crate holds but its blob does not make: one a guest spawns is begun by its host. It
/// tells its parent, as it begins, a pong of a hundred and what it is made to add, and logs that it began.
#[derive(Bound)]
pub struct Chick;

impl Kind for Chick {
    const NAME: KindName = KindName::new("aether.guest.chick");
}

impl Actor for Chick {
    type Config = AddBy;
    type Key = u64;
    type Handles = ();
    type BeginSends = (ToAny<(Pong,), Pong>,);
    type EndSends = ();

    fn begin(config: AddBy, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to_any::<(Pong,)>(ToWithoutMail::Parent).send(&Pong { n: 100 + config.by })?;
        turn.log(Level::Info, "a chick began");

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
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

impl Actor for FakeChick {
    type Config = AddBy;
    type Key = u64;
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: AddBy, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for FakeChick {
    type Sends = ();

    fn handle(&mut self, _: Ping, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A native actor no program keeps for a guest to spawn.
#[derive(Bound)]
pub struct Stray;

impl Kind for Stray {
    const NAME: KindName = KindName::new("aether.guest.stray");
}

impl Actor for Stray {
    type Config = EchoConfig;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
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

impl Actor for Hatcher {
    type Config = Hatch;
    type Key = ();
    type Handles = (Pong, Grow);
    type BeginSends = (ToType<Reporter, Ping>,);
    type EndSends = ();

    fn begin(hatch: Hatch, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let reporter = turn.spawn_from::<Reporter>(&hatch.other, &7, &AddBy { by: 20, deeper: 0 });
        turn.to_actor(reporter.map_err(|why| unhatched(&why))?).send(&Ping { n: 0 })?;

        Ok(Self { other: hatch.other, home: hatch.home, grown: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Pong> for Hatcher {
    type Sends = (ToAny<(Pong,), Pong>,);

    fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.to_any::<(Pong,)>(To::Parent).send(&pong)?;

        Ok(())
    }
}

impl Handles<Grow> for Hatcher {
    type Sends = (ToType<Boomerang, Ping>, ToAny<(Pong,), Pong>);

    fn handle(&mut self, _: Grow, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let chick = AddBy { by: 10 * self.grown + 2, deeper: 0 };
        turn.spawn::<Chick>(&self.grown, chick).map_err(|why| unhatched(&why))?;
        if let Err(why) = turn.spawn::<Stray>(&(), EchoConfig) {
            turn.to_any::<(Pong,)>(To::Parent).send(&Pong { n: 500 + refused(&why) })?;
        }
        if let Err(why) = turn.spawn::<FakeChick>(&(100 + self.grown), AddBy { by: 0, deeper: 0 }) {
            turn.to_any::<(Pong,)>(To::Parent).send(&Pong { n: 600 + refused(&why) })?;
        }
        let is_first = self.grown == 0;
        if is_first {
            let home = Home { home: self.home.clone() };
            let boomerang = turn.spawn_from::<Boomerang>(&self.other, &(), &home).map_err(|why| unhatched(&why))?;
            turn.to_actor(boomerang).send(&Ping { n: 0 })?;
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

impl Actor for Boomerang {
    type Config = Home;
    type Key = ();
    type Handles = (Ping,);
    type BeginSends = (ToAny<(Pong,), Pong>,);
    type EndSends = ();

    fn begin(home: Home, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let spawned = turn.spawn_from::<Reporter>(&home.home, &0, &AddBy { by: 0, deeper: 0 });
        let n = spawned.err().map_or(300, |why| 300 + refused(&why));
        turn.to_any::<(Pong,)>(ToWithoutMail::Parent).send(&Pong { n })?;

        Ok(Self { home: home.home })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Boomerang {
    type Sends = (ToAny<(Pong,), Pong>,);

    fn handle(&mut self, _: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let spawned = turn.spawn_from::<Reporter>(&self.home, &1, &AddBy { by: 0, deeper: 0 });
        let n = spawned.err().map_or(400, |why| 400 + refused(&why));
        turn.to_any::<(Pong,)>(To::Parent).send(&Pong { n })?;

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

impl Actor for Librarian {
    type Config = Shelf;
    type Key = ();
    type Handles = (Ping, Book, Shut);
    type BeginSends = (ToAny<(Pong,), Pong>,);
    type EndSends = ();

    fn begin(shelf: Shelf, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let n = turn.read(&shelf.book).len() as u64;
        if let Some(tell) = turn.resolve(&shelf.tell) {
            turn.to_any::<(Pong,)>(ToWithoutMail::Held(tell)).send(&Pong { n })?;
        }

        Ok(Self { book: shelf.book })
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Librarian {
    type Sends = (ToSender<Book>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let len = usize::try_from(ping.n).unwrap_or(0);
        let blob = turn.put(vec![7; len]).map_err(|why| unhatched(&why))?;
        turn.reply(&Book { blob })?;

        Ok(())
    }
}

impl Handles<Book> for Librarian {
    type Sends = (ToSender<Pong>,);

    fn handle(&mut self, book: Book, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let n = 100 + turn.read(&book.blob).len() as u64;
        turn.reply(&Pong { n })?;

        Ok(())
    }
}

impl Handles<Shut> for Librarian {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
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

/// An actor that makes a reference to a blob out of an id it was told as numbers, and reads it as it begins: its host
/// refuses the read, as the guest was never given the blob.
#[derive(Bound)]
pub struct Forger;

impl Kind for Forger {
    const NAME: KindName = KindName::new("aether.guest.forger");
}

impl Actor for Forger {
    type Config = Forge;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(forge: Forge, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        let forged = BlobRef::pin(BlobId::new(forge.engine, forge.bits));
        let _ = turn.read(&forged).len();

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
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

impl Actor for Spendthrift {
    type Config = EchoConfig;
    type Key = ();
    type Handles = (Ping,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: EchoConfig, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Spendthrift {
    type Sends = (ToSender<Book>,);

    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let len = usize::try_from(ping.n).unwrap_or(0);
        let book = turn.put(vec![7; len]).map_err(|why| unhatched(&why))?;
        let tell = ActorPath::from_root(&aethereal_data::PathSegment::new(KindName::new("aether.guest.no_one"), &()));
        let engine = book.id().engine();
        turn.spawn::<Librarian>(&(), Shelf { book, tell }).map_err(|why| unhatched(&why))?;
        turn.reply(&Book { blob: BlobRef::pin(BlobId::new(engine, u64::MAX)) })?;

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
    Spendthrift
);
