//! The engine's tests: what a step runs, and what is there for the next one.

use super::outside::{Handed, Turned};
use super::*;
use crate::watch::Notice;
use crate::{ByActor, Entry, child_id, path_id};
use aethereal_actor::Time;
use aethereal_actor::{
    Actor, ActorRef, Allows, Answer, Answered, Anywhere, Asks, Carries, ChildOf, Ended, Ending, Fault, Handles, How,
    Level, Path, Recursive, Table, Tells, ToAny, ToSender, ToType, Top, Turn, Unanswered, Why, child, path,
};
use aethereal_data::{Bound, Kind, KindName, Plain};
use std::sync::atomic::{AtomicU64, Ordering};

/// What a test reads of one turn from the engine's record (ADR-0036). A number is the last key of the actor's path,
/// and an actor with no key reads as nought.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Seen {
    /// The actor was handed this note.
    Got(u64, Note),
    /// The note it was just handed, a `where`, lies this deep in its chain.
    At(u64, u32),
    /// The actor ran its closing turn, for this reason.
    Ended(u64, Why),
    /// The actor was shown that one it watches ended in this way.
    Saw(u64, How),
    /// A seer was answered with this.
    Told(Reply),
    /// A seer was told why its question went unanswered.
    Unanswered(Unanswered),
    /// A clocked actor's parent was told what its child read.
    Finished(Finished),
}

impl Seen {
    /// What `turned` shows: nothing for a turn over mail no test reads.
    fn of(turned: &Turned) -> Vec<Self> {
        let number = numbered(turned);
        match &turned.handed {
            Handed::Closing(why) => vec![Self::Ended(number, *why)],
            Handed::Ended(ended) => vec![Self::Saw(number, ended.how().clone())],
            Handed::Mail(_) => Self::of_mail(turned, number),
        }
    }

    /// What `turned`, a turn of the actor `number` over mail, shows.
    fn of_mail(turned: &Turned, number: u64) -> Vec<Self> {
        if let Ok(note) = turned.read::<Note>() {
            let asks_depth = note.first() == Some(&Order::Depth);
            let got = Self::Got(number, note);
            if asks_depth {
                return vec![got, Self::At(number, turned.depth)];
            }

            return vec![got];
        }
        if let Ok(reply) = turned.read() {
            return vec![Self::Told(reply)];
        }
        if let Ok(why) = turned.read() {
            return vec![Self::Unanswered(why)];
        }
        if let Ok(finished) = turned.read() {
            return vec![Self::Finished(finished)];
        }

        Vec::new()
    }

    /// The number of the actor it is of, and nought for what a seer heard.
    const fn number(&self) -> u64 {
        match self {
            Self::Got(number, _) | Self::At(number, _) | Self::Ended(number, _) | Self::Saw(number, _) => *number,
            Self::Told(_) | Self::Unanswered(_) => 0,
            Self::Finished(finished) => finished.number,
        }
    }

    /// The note the actor `number` was handed, when this is one.
    fn note_of(&self, number: u64) -> Option<&Note> {
        match self {
            Self::Got(of, note) if *of == number => Some(note),
            _ => None,
        }
    }
}

/// The last key of the path of the actor that ran `turned`, read as a number, and nought for an actor with no key.
fn numbered(turned: &Turned) -> u64 {
    let last = turned.path.last();

    last.key().and_then(|key| key.to_string().parse().ok()).unwrap_or_default()
}

/// The actor `number` was handed `note`.
const fn got(number: u64, note: Note) -> Seen {
    Seen::Got(number, note)
}

/// The note the actor `number` was handed lies `depth` deep in its chain.
const fn at(number: u64, depth: u32) -> Seen {
    Seen::At(number, depth)
}

/// The actor `number` ran its closing turn because it asked to close.
const fn closed(number: u64) -> Seen {
    Seen::Ended(number, Why::Closed)
}

/// The actor `number` ran its closing turn because a turn of its own failed.
const fn faulted(number: u64) -> Seen {
    Seen::Ended(number, Why::Faulted)
}

/// The actor `number` was shown that one it watches ended as `how` says.
const fn saw(number: u64, how: How) -> Seen {
    Seen::Saw(number, how)
}

/// A seer was answered with `reply`.
const fn told(reply: Reply) -> Seen {
    Seen::Told(reply)
}

/// An ending by a failed turn, over `note`.
fn failed_over(note: &Note) -> How {
    let Ok(fault) = Fault::new(note) else {
        panic!("a short note encodes");
    };

    How::Failed(fault)
}

/// What the turns recorded since this was last asked show.
fn seen(engine: &mut Engine) -> Vec<Seen> {
    engine.take_turned().iter().flat_map(Seen::of).collect()
}

/// Why a spawn or a put was refused, as an actor's state can hold it: [`Refused`] does not encode.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Refusal {
    /// A blob had no room.
    NoRoom,
    /// A typed path of singletons held no live actor of the type of this name.
    Missing(String),
    /// Anything else, which these tests never expect.
    Other,
}

impl From<Refused> for Refusal {
    fn from(refused: Refused) -> Self {
        match refused {
            Refused::NoRoom => Self::NoRoom,
            Refused::Missing(name) => Self::Missing(name.to_string()),
            _ => Self::Other,
        }
    }
}

/// A word an actor says, which only marks a note as itself: acting on it does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Word {
    Hello,
    Again,
    First,
    Second,
    /// A tick of nothing in particular, which a clocked actor reads the time on.
    Tick,
    /// What a node says to answer `Order::Reply`.
    Answer,
    /// What a node that is told `bye` says to the child, which has ended by then.
    Late,
    /// What a turn sends before it fails, which is dropped with it.
    Never,
    /// What a closing turn that fails faults with.
    Unlucky,
    /// A blob had no room for what an actor put as it began.
    NoRoom,
    /// What a barracks tells its squad.
    Report,
    /// A squad's own word, which goes before what it was told.
    Squad,
    /// A scout's own word.
    Scout,
    /// A child did not begin.
    Unborn,
    /// What a holder sends itself between two blobs.
    Between,
}

/// How a node leaves, which its closing turn does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Leaving {
    /// It says `Order::Bye` to its parent.
    Quietly,
    /// It says `Order::Bye` to its parent and fails its closing turn, so what it sent there is dropped.
    Failing,
    /// It says `Order::ByeWatchMe` to its parent.
    AskingToBeWatched,
}

/// What one order of a [`Note`] has an actor do. The orders of a note after the one acted on are its rest, which most
/// orders hand on as a note of their own: a type that holds itself does not encode, so a note is a list.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Order {
    /// Closes the actor.
    Close,
    /// Fails its turn.
    Fail,
    /// Logs this line, sends its parent `Word::Never`, and fails its turn.
    FailLogging(String),
    /// Answers the sender `Word::Answer`.
    Reply,
    /// Answers the sender `Bounce(2)`, then `Last`.
    Pair,
    /// Answers the sender one fewer until this is zero.
    Bounce(u64),
    /// Ends what `Pair` began; acting on it does nothing.
    Last,
    /// Sends its parent the rest, with `From` its number.
    Up,
    /// Spawns this node beneath it, and sends that child the rest.
    Spawn { number: u64, leaving: Leaving },
    /// As `Spawn`, and watches the child as well.
    Watch { number: u64, leaving: Leaving },
    /// Spawns the node `number`, which spawns the node `beneath` beneath itself as it begins.
    Nest { number: u64, beneath: u64 },
    /// Sends the rest to this node beneath it.
    Tell(u64),
    /// Sends its parent `WatchMe` and itself `Close`.
    Both,
    /// Watches the sender.
    WatchMe,
    /// Sends itself the rest, for the next step.
    Later,
    /// Sends the sender `Word::Late` through the reference to it, as an actor of its own type.
    Keep,
    /// Sends itself `Mark(I)` for each `I` below this: notes of one length, back to back.
    Many(u64),
    /// Spawns each of these nodes beneath it, and sends them all the rest in one send.
    Fan(Vec<u64>),
    /// Spawns this node, and keeps whether it finds it beneath itself in the same turn.
    Birth(u64),
    /// Keeps whether it finds this node beneath its parent.
    Sibling(u64),
    /// What a child says as it ends, `Leaving::Quietly`: its parent sends that child `Word::Late`.
    Bye(u64),
    /// As `Bye`, and its parent watches the sender as well.
    ByeWatchMe(u64),
    /// Does nothing, and the record says how deep in its chain the note is.
    Depth,
    /// Says this word: does nothing.
    Say(Word),
    /// Stands for the note numbered this, among many: does nothing.
    Mark(u64),
    /// The note came from this node, by `Up`: does nothing.
    From(u64),
    /// Adds the bytes of one order to its note, so it is not of one length with the notes beside it: does nothing.
    Pad,
    /// Has a clocked actor spawn the actors `2` and `3` beneath it, which each send themselves `Close`.
    Twins,
    /// Has a holder put a blob of this many bytes and keep it, or keep why it was refused.
    Put(u64),
    /// Has a holder keep how many bytes each blob it keeps holds.
    Read,
    /// Has a holder keep the hash of each blob it keeps.
    Hash,
    /// Has a holder let go of the blob it kept last.
    LetGo,
    /// Has a holder mail itself the blob it kept last, keeping none of it.
    Send,
    /// As `Send`, and closes it as well.
    SendAndClose,
}

/// What one actor sends another: orders, the first of which it acts on.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.note")]
struct Note {
    orders: Vec<Order>,
}

impl Note {
    /// The note of `order` alone.
    fn of(order: Order) -> Self {
        Self { orders: vec![order] }
    }

    /// `order`, with `rest` to hand on.
    fn then(order: Order, rest: Self) -> Self {
        let mut orders = vec![order];
        orders.extend(rest.orders);

        Self { orders }
    }

    /// The note, followed by `rest`.
    fn and(mut self, rest: Self) -> Self {
        self.orders.extend(rest.orders);

        self
    }

    /// The note, as it reaches a parent from the node `number`.
    fn from(self, number: u64) -> Self {
        self.and(Self::of(Order::From(number)))
    }

    /// The note, an order longer.
    fn padded(self) -> Self {
        self.and(Self::of(Order::Pad))
    }

    /// The order to act on.
    fn first(&self) -> Option<&Order> {
        self.orders.first()
    }

    /// The order to act on and the rest, as a note: none for a note with no orders.
    fn parted(&self) -> Option<(&Order, Self)> {
        let (first, rest) = self.orders.split_first()?;

        Some((first, Self { orders: rest.to_vec() }))
    }

    /// What it has its receiver send up, when it says to.
    fn sent_up(&self) -> Option<Self> {
        let (first, rest) = self.parted()?;

        (*first == Order::Up).then_some(rest)
    }

    /// What the node `number` sent up, when this is that.
    fn up_from(&self, number: u64) -> Option<Self> {
        let (last, rest) = self.orders.split_last()?;

        (*last == Order::From(number)).then(|| Self { orders: rest.to_vec() })
    }

    /// Whether it tells a child something.
    fn tells(&self) -> bool {
        matches!(self.first(), Some(Order::Tell(_)))
    }
}

/// The note that says `word`.
fn said(word: Word) -> Note {
    Note::of(Order::Say(word))
}

/// A note that says hello.
fn hello() -> Note {
    said(Word::Hello)
}

/// A note that says it again.
fn again() -> Note {
    said(Word::Again)
}

/// The first of two notes alike.
fn first() -> Note {
    said(Word::First)
}

/// The second of two notes alike.
fn second() -> Note {
    said(Word::Second)
}

/// The note that stands for `number`.
fn mark(number: u64) -> Note {
    Note::of(Order::Mark(number))
}

/// See [`Order::Close`].
fn close() -> Note {
    Note::of(Order::Close)
}

/// See [`Order::Fail`].
fn fail() -> Note {
    Note::of(Order::Fail)
}

/// See [`Order::FailLogging`].
fn fail_logging(line: &str) -> Note {
    Note::of(Order::FailLogging(String::from(line)))
}

/// See [`Order::Reply`].
fn reply() -> Note {
    Note::of(Order::Reply)
}

/// See [`Word::Answer`].
fn answer() -> Note {
    said(Word::Answer)
}

/// See [`Order::Pair`].
fn pair() -> Note {
    Note::of(Order::Pair)
}

/// See [`Order::Bounce`].
fn bounce(left: u64) -> Note {
    Note::of(Order::Bounce(left))
}

/// See [`Order::Last`].
fn last() -> Note {
    Note::of(Order::Last)
}

/// See [`Order::Up`].
fn up(rest: Note) -> Note {
    Note::then(Order::Up, rest)
}

/// See [`Order::Spawn`]: a child that leaves quietly.
fn spawn(number: u64, rest: Note) -> Note {
    spawn_leaving(number, Leaving::Quietly, rest)
}

/// See [`Order::Spawn`].
fn spawn_leaving(number: u64, leaving: Leaving, rest: Note) -> Note {
    Note::then(Order::Spawn { number, leaving }, rest)
}

/// See [`Order::Watch`]: a child that leaves quietly.
fn watch(number: u64, rest: Note) -> Note {
    watch_leaving(number, Leaving::Quietly, rest)
}

/// See [`Order::Watch`].
fn watch_leaving(number: u64, leaving: Leaving, rest: Note) -> Note {
    Note::then(Order::Watch { number, leaving }, rest)
}

/// See [`Order::Nest`].
fn nest(number: u64, beneath: u64) -> Note {
    Note::of(Order::Nest { number, beneath })
}

/// See [`Order::Tell`].
fn tell(number: u64, rest: Note) -> Note {
    Note::then(Order::Tell(number), rest)
}

/// See [`Order::Both`].
fn both() -> Note {
    Note::of(Order::Both)
}

/// See [`Order::WatchMe`].
fn watch_me() -> Note {
    Note::of(Order::WatchMe)
}

/// See [`Order::Later`].
fn later(rest: Note) -> Note {
    Note::then(Order::Later, rest)
}

/// See [`Order::Keep`].
fn keep() -> Note {
    Note::of(Order::Keep)
}

/// See [`Order::Many`].
fn many(count: u64) -> Note {
    Note::of(Order::Many(count))
}

/// See [`Order::Fan`].
fn fan(numbers: &[u64], rest: Note) -> Note {
    Note::then(Order::Fan(numbers.to_vec()), rest)
}

/// See [`Order::Birth`].
fn birth(number: u64) -> Note {
    Note::of(Order::Birth(number))
}

/// See [`Order::Sibling`].
fn sibling(number: u64) -> Note {
    Note::of(Order::Sibling(number))
}

/// See [`Order::Bye`].
fn bye(number: u64) -> Note {
    Note::of(Order::Bye(number))
}

/// See [`Order::ByeWatchMe`].
fn bye_watch_me(number: u64) -> Note {
    Note::of(Order::ByeWatchMe(number))
}

/// See [`Order::Depth`].
fn depth() -> Note {
    Note::of(Order::Depth)
}

/// See [`Order::Twins`].
fn twins() -> Note {
    Note::of(Order::Twins)
}

/// See [`Order::Put`].
fn put(bytes: u64) -> Note {
    Note::of(Order::Put(bytes))
}

/// See [`Order::Read`].
fn read() -> Note {
    Note::of(Order::Read)
}

/// See [`Order::Hash`].
fn hash() -> Note {
    Note::of(Order::Hash)
}

/// See [`Order::LetGo`].
fn let_go() -> Note {
    Note::of(Order::LetGo)
}

/// See [`Order::Send`].
fn send() -> Note {
    Note::of(Order::Send)
}

/// See [`Order::SendAndClose`].
fn send_and_close() -> Note {
    Note::of(Order::SendAndClose)
}

/// A node's number, the notes it sends itself as it begins, the children it spawns as it begins, each of which closes
/// on its first note, and how it leaves.
#[derive(Plain)]
#[kind(name = "aether.test.node_config")]
struct NodeConfig {
    number: u64,
    to_self: Vec<Note>,
    spawns: Vec<u64>,
    leaving: Leaving,
}

impl NodeConfig {
    /// The node `number`, which does nothing as it begins and leaves as `leaving` says.
    const fn leaving(number: u64, leaving: Leaving) -> Self {
        Self { number, to_self: Vec::new(), spawns: Vec::new(), leaving }
    }

    /// The node `number`, which does nothing as it begins and leaves quietly.
    const fn quiet(number: u64) -> Self {
        Self::leaving(number, Leaving::Quietly)
    }
}

/// An actor that acts on the first order of each [`Note`] it is sent, as [`Order`] says.
#[derive(Plain)]
#[kind(name = "aether.test.node")]
struct Node {
    number: u64,
    leaving: Leaving,
    /// Each node it looked for, and whether it found it, in order.
    found: Vec<(u64, bool)>,
}

impl Node {
    /// Keeps whether it finds the node `number` beneath itself, once it has spawned it in this turn.
    fn look_after_birth<S>(&mut self, turn: &mut Turn<'_, Self, S>, number: u64) {
        let _ = turn.spawn::<Self>(&number, NodeConfig::quiet(number));
        let found = turn.child::<Self>(turn.me(), &number);
        self.found.push((number, found.is_some()));
    }

    /// Keeps whether it finds the node `number` beneath its parent.
    fn look_beneath_parent<S>(&mut self, turn: &mut Turn<'_, Self, S>, number: u64) {
        let found = turn.parent().and_then(|parent| turn.child::<Self>(parent, &number));
        self.found.push((number, found.is_some()));
    }

    /// Spawns the node `number`, leaving as `leaving` says, watching it when `watches` says to, and sends it `rest`.
    fn spawn_child<S, I>(
        turn: &mut Turn<'_, Self, S>,
        number: u64,
        leaving: Leaving,
        watches: Watches,
        rest: &Note,
    ) -> Result<(), Fault>
    where
        S: Allows<ToType<Self, Note>, I>,
    {
        if let Ok(child) = turn.spawn::<Self>(&number, NodeConfig::leaving(number, leaving)) {
            let is_watched = watches == Watches::Child;
            if is_watched {
                turn.watch(child);
            }
            turn.to(child).send(rest)?;
        }

        Ok(())
    }
}

/// Whether a spawn watches the child it spawns.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Watches {
    Nothing,
    Child,
}

impl ChildOf for Node {
    type Parent = Recursive;
}

impl Actor for Node {
    type Config = NodeConfig;
    type Key = u64;
    type Handles = (Note, Ended);
    type BeginSends = (ToType<Self, Note>,);
    type EndSends = (ToType<Self, Note>,);

    fn begin(config: NodeConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for number in config.spawns {
            let child = NodeConfig { to_self: vec![close()], ..NodeConfig::quiet(number) };
            let _ = turn.spawn::<Self>(&number, child);
        }
        for note in config.to_self {
            turn.to(turn.me()).send(&note)?;
        }

        Ok(Self { number: config.number, leaving: config.leaving, found: Vec::new() })
    }

    fn end(self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        if let Some(parent) = turn.parent() {
            let number = self.number;
            let goodbye = match self.leaving {
                Leaving::Quietly | Leaving::Failing => bye(number),
                Leaving::AskingToBeWatched => bye_watch_me(number),
            };
            turn.to(parent).send(&goodbye)?;
        }
        let fails = self.leaving == Leaving::Failing;
        if fails {
            return Err(Fault::new(&said(Word::Unlucky))?);
        }

        Ok(())
    }
}

impl Handles<Note> for Node {
    type Sends = (ToType<Self, Note>, ToSender<Note>, ToAny<(Note,), Note>);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some((order, rest)) = note.parted() else {
            return Ok(());
        };
        match order {
            Order::Close => turn.close(),
            Order::Fail => return Err(Fault::new(&note)?),
            Order::FailLogging(line) => {
                turn.log(Level::Warn, line);
                if let Some(parent) = turn.parent() {
                    turn.to(parent).send(&said(Word::Never))?;
                }

                return Err(Fault::new(&note)?);
            }
            Order::Reply => turn.reply(&answer())?,
            Order::Pair => {
                turn.reply(&bounce(2))?;
                turn.reply(&last())?;
            }
            Order::Bounce(left) => {
                if *left > 0 {
                    turn.reply(&bounce(left - 1))?;
                }
            }
            Order::Up => {
                if let Some(parent) = turn.parent() {
                    turn.to(parent).send(&rest.from(self.number))?;
                }
            }
            Order::Spawn { number, leaving } => Self::spawn_child(turn, *number, *leaving, Watches::Nothing, &rest)?,
            Order::Watch { number, leaving } => Self::spawn_child(turn, *number, *leaving, Watches::Child, &rest)?,
            Order::Nest { number, beneath } => {
                let child = NodeConfig { spawns: vec![*beneath], ..NodeConfig::quiet(*number) };
                let _ = turn.spawn::<Self>(number, child);
            }
            Order::Tell(number) => {
                turn.to(child::<Self>(*number)).send(&rest)?;
            }
            Order::Both => {
                if let Some(parent) = turn.parent() {
                    turn.to(parent).send(&watch_me())?;
                }
                turn.to(turn.me()).send(&close())?;
            }
            Order::WatchMe => {
                let sender = turn.sender();
                turn.watch(sender);
            }
            Order::Later => {
                turn.to(turn.me()).send(&rest)?;
            }
            Order::Keep => {
                if let Some(sender) = turn.cast(turn.sender()) {
                    turn.to(sender).send(&said(Word::Late))?;
                }
            }
            Order::Many(count) => {
                let me = turn.me();
                for index in 0..*count {
                    turn.to(me).send(&mark(index))?;
                }
            }
            Order::Fan(numbers) => {
                let children: Vec<ActorRef<Self>> =
                    numbers.iter().filter_map(|number| turn.spawn(number, NodeConfig::quiet(*number)).ok()).collect();
                turn.to(&children).send(&rest)?;
            }
            Order::Birth(number) => self.look_after_birth(turn, *number),
            Order::Sibling(number) => self.look_beneath_parent(turn, *number),
            Order::Bye(number) => {
                turn.to(child::<Self>(*number)).send(&said(Word::Late))?;
            }
            Order::ByeWatchMe(number) => {
                let sender = turn.sender();
                turn.watch(sender);
                turn.to(child::<Self>(*number)).send(&said(Word::Late))?;
            }
            _ => {}
        }

        Ok(())
    }
}

impl Handles<Ended> for Node {
    type Sends = ();

    fn handle(&mut self, _: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A clock that never moves: these tests are about what turns leave, not how long they take.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An engine with the root node `1`, which sends itself `to_self` as it begins, and fuses no mail.
fn engine(to_self: &[Note]) -> (Engine, ActorId) {
    fusing(0, to_self)
}

/// An engine with the root node `1`, which sends itself `to_self` as it begins, whose parts each run at most `hops`
/// fused hops a step.
fn fusing(hops: usize, to_self: &[Note]) -> (Engine, ActorId) {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, hops).ready();
    engine.record_turns();
    let config = NodeConfig { to_self: to_self.to_vec(), ..NodeConfig::quiet(1) };
    let Ok(root) = engine.add_root::<Node>(&1, config) else {
        panic!("the root begins");
    };

    (engine, root)
}

/// Runs one step and answers with what its turns show.
fn step(engine: &mut Engine) -> Vec<Seen> {
    engine.step(Time::ZERO, &Stopped);

    seen(engine)
}

/// The live actor `id` stands for, in its seat.
fn seated(engine: &Engine, id: ActorId) -> Seated {
    let Some(seat) = engine.table().seat(id) else {
        panic!("the actor is live");
    };

    Seated { id, seat }
}

/// The state of the live actor `id` stands for, which is a `T`.
fn state<T: 'static>(engine: &Engine, id: ActorId) -> &T {
    let Some(state) = engine.state(seated(engine, id)) else {
        panic!("the actor's state is here, and of the type asked for");
    };

    state
}

/// The id of the root of the singleton type `T`.
fn root_of<T: Kind>() -> ActorId {
    path_id(&ActorPath::from_root(&PathSegment::new(T::NAME, &())))
}

/// The id of the node `number` beneath `parent`.
fn node(parent: ActorId, number: u64) -> ActorId {
    child_id(parent, &PathSegment::new(Node::NAME, &number))
}

#[test]
fn mail_is_handled_in_a_step_after_it_is_sent() {
    // Catches mail handled as it is sent, inside the turn or the step that sent it, and a reply sent to anyone
    // but the sender.
    let (mut engine, _) = engine(&[reply()]);
    assert!(engine.take_turned().is_empty());
    assert!(!engine.is_idle());

    assert_eq!(step(&mut engine), [got(1, reply())]);
    assert_eq!(step(&mut engine), [got(1, answer())]);
    assert!(engine.is_idle());
    assert_eq!(engine.step_number(), 2);
}

#[test]
fn child_takes_mail_sent_in_the_step_that_spawned_it() {
    // Catches mail delivered before its step's spawns are folded: the child would not be there for it.
    let (mut engine, root) = engine(&[spawn(2, up(hello()))]);

    assert_eq!(step(&mut engine), [got(1, spawn(2, up(hello())))]);
    assert!(engine.table().is_live(node(root, 2)));
    assert_eq!(step(&mut engine), [got(2, up(hello()))]);
    assert_eq!(step(&mut engine), [got(1, hello().from(2))]);
}

#[test]
fn closed_actor_handles_no_more_mail_and_ends_in_the_next_step() {
    // Catches mail queued behind a close being handled, a closing turn run in the step that closed, and mail
    // delivered to an actor that has ended.
    let (mut engine, root) = engine(&[spawn(2, close()), spawn(3, up(hello()))]);
    assert_eq!(step(&mut engine), [got(1, spawn(2, close())), got(1, spawn(3, up(hello())))]);

    // The two children run in the order of their ids, which nothing here depends on.
    let second = sorted(step(&mut engine));
    assert_eq!(second, [got(2, close()), got(3, up(hello()))]);
    assert!(!engine.table().is_live(node(root, 2)));

    let third = sorted(step(&mut engine));
    assert_eq!(third, [got(1, hello().from(3)), closed(2)]);
    assert_eq!(step(&mut engine), [got(1, bye(2))]);
    assert!(step(&mut engine).is_empty());
    assert!(engine.is_idle());
}

#[test]
fn mail_queued_behind_a_close_is_dropped() {
    // Catches an ended actor handling what it was sent before it ended.
    let (mut engine, _) = engine(&[close(), reply()]);

    assert_eq!(step(&mut engine), [got(1, close())]);
    assert_eq!(step(&mut engine), [closed(1)]);
    assert!(step(&mut engine).is_empty());
    assert!(engine.is_idle());
}

#[test]
fn failed_turn_ends_its_actor_and_sends_nothing() {
    // Catches a failed turn's asks being applied, a failed actor left live, and a closing turn that is not told
    // its actor failed.
    let (mut engine, root) = engine(&[spawn(2, fail())]);
    step(&mut engine);

    assert_eq!(step(&mut engine), [got(2, fail())]);
    assert!(!engine.table().is_live(node(root, 2)));
    assert_eq!(step(&mut engine), [faulted(2)]);
    assert_eq!(step(&mut engine), [got(1, bye(2))]);
}

#[test]
fn send_through_a_reference_to_an_actor_that_has_ended_is_dropped() {
    // Catches a send checked against the type of an actor the table no longer holds, which would stop the engine
    // over mail that is only to be dropped, and mail handed to an actor that has ended.
    let (mut engine, _) = engine(&[spawn(2, up(keep())), tell(2, close())]);
    step(&mut engine);
    assert_eq!(step(&mut engine), [got(2, up(keep())), got(2, close())]);

    let third = sorted(step(&mut engine));
    assert_eq!(third, [got(1, keep().from(2)), closed(2)]);
    assert_eq!(step(&mut engine), [got(1, bye(2))]);
    assert!(step(&mut engine).is_empty());
}

#[test]
fn a_child_is_found_beneath_a_reference_held_while_it_is_live() {
    // Catches `turn.child` looking beneath the turn's own actor rather than the reference it is handed, finding a key
    // no child holds, finding a child that has ended, and missing one its own turn stood up beneath the reference it
    // holds to itself (ADR-0022). Node 2 looks for its siblings beneath its parent, and the root for the child it
    // just stood up.
    let (mut engine, root) = engine(&[
        spawn(3, hello()),
        spawn(2, sibling(3)),
        later(tell(2, sibling(5))),
        later(tell(3, close())),
        later(later(later(tell(2, sibling(3))))),
        birth(4),
    ]);
    for _ in 0..5 {
        step(&mut engine);
    }

    assert_eq!(state::<Node>(&engine, root).found, [(4, true)]);
    assert_eq!(state::<Node>(&engine, node(root, 2)).found, [(3, true), (5, false), (3, false)]);
}

#[test]
fn a_child_is_found_in_the_table_only_of_its_kind() {
    // Catches the table's lookup of a child beneath a reference answering an actor of another kind at the place, or a
    // place no child holds (ADR-0022).
    let (mut engine, root) = engine(&[spawn(2, hello())]);
    step(&mut engine);
    let two = PathSegment::new(Node::NAME, &2u64);
    let three = PathSegment::new(Node::NAME, &3u64);
    let child = seated(&engine, child_id(root, &two));

    assert_eq!(Table::reach_child(engine.table(), root, &two, KindId::of::<Node>()), Some(child));
    assert_eq!(Table::reach_child(engine.table(), root, &two, KindId::of::<Note>()), None);
    assert_eq!(Table::reach_child(engine.table(), root, &three, KindId::of::<Node>()), None);
}

#[test]
fn key_stays_taken_after_its_child_has_ended() {
    // Catches a parent's key freed once its child has ended and left the table, which would bring a closed path
    // back to life.
    let (mut engine, _) = engine(&[spawn(2, close()), later(later(spawn(2, up(again()))))]);
    step(&mut engine);
    step(&mut engine);

    let third = sorted(step(&mut engine));
    assert_eq!(third, [got(1, spawn(2, up(again()))), closed(2)]);
    assert_eq!(step(&mut engine), [got(1, bye(2))]);
    assert!(step(&mut engine).is_empty());
}

#[test]
fn key_a_child_used_as_it_began_stays_taken() {
    // Catches the children a child stands up in its `begin` going unnoted, as when a child is seated before its
    // parent has a seat to note it in.
    let (mut engine, _) = engine(&[nest(2, 3), later(later(tell(2, spawn(3, up(again())))))]);
    step(&mut engine);
    let second = sorted(step(&mut engine));
    assert_eq!(second, [got(1, later(tell(2, spawn(3, up(again()))))), got(3, close())]);

    let third = sorted(step(&mut engine));
    assert_eq!(third, [got(1, tell(2, spawn(3, up(again())))), closed(3)]);
    let fourth = sorted(step(&mut engine));
    assert_eq!(fourth, [got(2, bye(3)), got(2, spawn(3, up(again())))]);
    assert!(step(&mut engine).is_empty());
}

#[test]
fn root_key_stays_taken_after_its_root_has_ended() {
    // Catches a root's key freed once the table no longer holds its ended root, which would bring a closed path
    // back to life.
    let (mut engine, _) = engine(&[close()]);
    step(&mut engine);
    step(&mut engine);

    let again = engine.add_root::<Node>(&1, NodeConfig::quiet(1));
    assert!(matches!(again, Err(Refused::KeyTaken)));
}

#[test]
fn mail_from_two_senders_arrives_in_the_order_of_their_ids() {
    // Catches a queue left in the order its senders happened to run: the same turns must leave the same queue.
    let (mut engine, root) = engine(&[spawn(2, up(hello())), spawn(3, up(hello())), spawn(4, up(hello()))]);
    step(&mut engine);
    step(&mut engine);

    let mut numbers = [2, 3, 4];
    numbers.sort_by_key(|number| node(root, *number).bits());
    assert_eq!(step(&mut engine), numbers.map(|number| got(1, hello().from(number))));
}

#[test]
fn key_spawned_earlier_in_the_step_is_taken() {
    // Catches an actor's second turn of a step reading its own key as free, which would make two actors under
    // one id.
    let (mut engine, _) = engine(&[spawn(2, up(first())), spawn(2, up(second()))]);
    step(&mut engine);

    assert_eq!(step(&mut engine), [got(2, up(first()))]);
}

#[test]
fn watcher_reacts_to_an_ending_once_with_its_fault_after_its_last_mail() {
    // Catches a watcher never run for an ending, run without the fault, run before the ended actor's last mail
    // has reached it, and run for it again in a later step.
    let (mut engine, _) = engine(&[watch(2, fail())]);
    step(&mut engine);
    step(&mut engine);

    assert_eq!(step(&mut engine), [faulted(2)]);
    assert_eq!(step(&mut engine), [got(1, bye(2)), saw(1, failed_over(&fail()))]);
    assert!(step(&mut engine).is_empty());
    assert!(engine.is_idle());
}

#[test]
fn watcher_with_no_mail_is_run_for_an_ending() {
    // Catches an ending that waits for mail to arrive before its watcher runs.
    // The child fails its closing turn, so its parent is sent nothing.
    let (mut engine, _) = engine(&[watch_leaving(13, Leaving::Failing, close())]);
    step(&mut engine);
    step(&mut engine);
    assert_eq!(step(&mut engine), [closed(13)]);

    assert_eq!(step(&mut engine), [saw(1, How::Closed)]);
}

#[test]
fn watch_asked_for_in_the_step_its_actor_ends_is_shown() {
    // Catches a watch that only counts once folded: its actor's ending is in the same step's effects, and would
    // be missed.
    let (mut engine, _) = engine(&[spawn(2, both())]);
    step(&mut engine);
    step(&mut engine);

    let third = sorted(step(&mut engine));
    assert_eq!(third, [got(1, watch_me()), got(2, close())]);
    assert_eq!(step(&mut engine), [closed(2)]);
    assert_eq!(step(&mut engine), [got(1, bye(2)), saw(1, How::Closed)]);
}

#[test]
fn watch_asked_for_before_the_closing_turn_has_run_is_shown_how() {
    // Catches a watch that lands between an actor's close and its closing turn being shown only that the actor
    // ended earlier, when how it ended is still to come.
    let (mut engine, _) = engine(&[spawn(2, up(watch_me())), tell(2, close())]);
    step(&mut engine);
    assert_eq!(step(&mut engine), [got(2, up(watch_me())), got(2, close())]);

    let third = sorted(step(&mut engine));
    assert_eq!(third, [got(1, watch_me().from(2)), closed(2)]);
    assert_eq!(step(&mut engine), [got(1, bye(2)), saw(1, How::Closed)]);
}

#[test]
fn watch_of_an_actor_whose_life_is_over_is_shown() {
    // Catches a watch on an actor whose closing turn has run that waits for an ending which has already been.
    let (mut engine, _) = engine(&[spawn_leaving(5, Leaving::AskingToBeWatched, close())]);
    step(&mut engine);
    step(&mut engine);
    assert_eq!(step(&mut engine), [closed(5)]);

    assert_eq!(step(&mut engine), [got(1, bye_watch_me(5))]);
    assert_eq!(step(&mut engine), [saw(1, How::Earlier)]);
}

#[test]
fn failed_turn_keeps_its_log_lines_and_leaves_its_fault() {
    // Catches a failed turn's lines dropped with the rest of what it asked for, the rest kept with the lines,
    // and a failure that leaves nothing to read.
    let (mut engine, root) = engine(&[spawn(2, fail_logging("low"))]);
    step(&mut engine);
    let child = seated(&engine, node(root, 2));
    step(&mut engine);
    let Ok(fault) = Fault::new(&fail_logging("low")) else {
        panic!("a short note encodes");
    };

    let kept: Vec<&Entry> = engine.log(child).collect();
    assert_eq!(kept, [&Entry::Line { level: Level::Warn, text: String::from("low") }, &Entry::Failed(fault)]);
    assert_eq!(step(&mut engine), [faulted(2)]);
    assert_eq!(step(&mut engine), [got(1, bye(2))]);
}

#[test]
fn failed_closing_turn_leaves_its_fault_and_sends_nothing() {
    // Catches a closing turn whose failure leaves nothing to read, and one whose mail goes out all the same.
    let (mut engine, root) = engine(&[spawn_leaving(13, Leaving::Failing, close())]);
    step(&mut engine);
    let child = seated(&engine, node(root, 13));
    step(&mut engine);
    let Ok(fault) = Fault::new(&said(Word::Unlucky)) else {
        panic!("a short note encodes");
    };

    assert_eq!(step(&mut engine), [closed(13)]);
    assert_eq!(engine.log(child).collect::<Vec<_>>(), [&Entry::EndFailed(fault)]);
    assert!(step(&mut engine).is_empty());
}

#[test]
fn an_actor_counts_what_it_sends_across_its_begin_and_its_steps() {
    // Catches a count of mail sent that is lost between steps, or not kept from a root's `begin`, so the actor
    // starts counting again and gives a later piece of mail the id of an earlier one.
    let (mut engine, root) = engine(&[spawn(2, reply())]);
    assert_eq!(step(&mut engine), [got(1, spawn(2, reply()))]);
    assert_eq!(step(&mut engine), [got(2, reply())]);
    assert_eq!(step(&mut engine), [got(1, answer())]);

    let sent = |actor: ActorId| {
        let seat = engine.table.seat(actor).unwrap_or(SeatNumber::new(u64::MAX));
        let me = Seated { id: actor, seat };

        engine.lives.shard(seat.number()).life(me).map(lives::Life::sent)
    };
    assert_eq!(sent(root), Some(2), "one piece from its begin, one from its first turn");
    assert_eq!(sent(node(root, 2)), Some(1));
}

#[test]
fn depth_counts_each_send_down_a_chain() {
    // Catches a depth that is not carried through a spawn and a reply, and one that starts from the wrong end:
    // a root's first mail is at depth one.
    let (mut engine, _) = engine(&[depth(), spawn(2, up(depth()))]);

    assert_eq!(step(&mut engine), [got(1, depth()), at(1, 1), got(1, spawn(2, up(depth())))]);
    step(&mut engine);
    assert_eq!(step(&mut engine), [got(1, depth().from(2)), at(1, 3)]);
}

#[test]
fn item_placed_under_a_seat_reaches_only_the_actor_it_was_placed_for() {
    // Catches an item placed under a seat handed to whichever actor sits there, when the actor it was placed for
    // has left the seat or never sat in it.
    let (mut engine, root) = engine(&[]);
    let seat = engine.table.seat(root).map_or(u64::MAX, SeatNumber::number);
    let context = Context { root: MailId { sender: root, count: 0 }, depth: 1 };
    let place = |engine: &mut Engine, to: ActorId| {
        let ended = Ended::new(Seated { id: node(root, 2), seat: SeatNumber::new(7) }, How::Closed);
        let to = Seated { id: to, seat: SeatNumber::new(seat) };
        engine.streams.shard_mut(seat).end(Notice { to, ended, context });
        engine.waiting.push(engine.part(to.seat));
    };

    place(&mut engine, node(root, 3));
    assert!(step(&mut engine).is_empty());
    place(&mut engine, root);
    assert_eq!(step(&mut engine), [saw(1, How::Closed)]);
}

/// A clock that moves a microsecond each time it is read, so every turn is measured to cost something.
#[derive(Default)]
struct Ticking(AtomicU64);

impl Clock for Ticking {
    fn nanos(&self) -> u64 {
        self.0.fetch_add(1_000, Ordering::Relaxed)
    }
}

#[test]
fn children_spawned_on_two_workers_each_take_a_seat_of_their_own() {
    // Catches two workers giving one seat to two children, a seat a part numbered or took back left neither taken nor
    // free, and a child spawned in an earlier turn of the step that a send to it cannot find.
    let (mut engine, root) =
        engine(&[spawn(2, spawn(4, up(mark(1)))), spawn(3, spawn(5, up(mark(2)))), tell(3, spawn(6, up(mark(3))))]);
    let clock = Ticking::default();
    engine.step(Time::ZERO, &clock);
    let _ = engine.take_turned();

    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::ZERO).into_split() else {
        panic!("two workers share the step");
    };
    assert_eq!(shares.len(), 2);
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &clock)).collect();
    engine.apply(done);
    let _ = engine.take_turned();

    let third = sorted(step(&mut engine));
    assert_eq!(third, [got(4, up(mark(1))), got(5, up(mark(2))), got(6, up(mark(3)))]);
    let (two, three) = (node(root, 2), node(root, 3));
    let actors = [root, two, three, node(two, 4), node(three, 5), node(three, 6)];
    let taken: Vec<u64> = actors.iter().filter_map(|id| engine.table().seat(*id)).map(SeatNumber::number).collect();
    assert_eq!(taken.len(), actors.len());
    let free: Vec<u64> = engine.seats.iter().flat_map(|seats| seats.held().0.map(SeatNumber::number)).collect();
    let numbered = engine.seats.iter().zip(0..).flat_map(|(seats, first)| (first..seats.held().1).step_by(4));
    let each_once = numbered.clone().all(|seat| taken.iter().chain(&free).filter(|held| **held == seat).count() == 1);
    assert!(each_once);
    assert_eq!(taken.len() + free.len(), numbered.count());
}

#[test]
fn keys_used_on_two_workers_stay_taken_after_their_children_have_ended() {
    // Catches the children stood up in a step shared among workers going unnoted in their parents' seats when their
    // parts are folded, which would free a key once its child has ended and left the table.
    let (mut engine, _) = engine(&[
        spawn(2, spawn(20, close())),
        spawn(3, spawn(30, close())),
        later(later(later(tell(2, spawn(20, up(again())))))),
        later(later(later(tell(3, spawn(30, up(again())))))),
    ]);
    let clock = Ticking::default();
    engine.step(Time::ZERO, &clock);
    let _ = engine.take_turned();

    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::ZERO).into_split() else {
        panic!("two workers share the step");
    };
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &clock)).collect();
    engine.apply(done);
    let mut heard = seen(&mut engine);
    while !engine.is_idle() {
        heard.extend(step(&mut engine));
    }

    assert!(heard.contains(&got(2, spawn(20, up(again())))));
    assert!(heard.contains(&got(3, spawn(30, up(again())))));
    let respawned_second = heard.contains(&got(2, again().from(20)));
    let respawned_third = heard.contains(&got(3, again().from(30)));
    let respawned = respawned_second || respawned_third;
    assert!(!respawned, "{heard:?}");
}

#[test]
fn children_seated_in_a_part_another_worker_ran_are_kept() {
    // Catches a worker's children being seated in a part another worker still holds, and lost when that worker's
    // part is put back over them. Node 2 sits beside the root in part 0, which is full, so its children take the
    // seats set aside for part 0 in the other parts, and its fourth takes one in part 2, where node 3 runs on the
    // other worker.
    let hi = || up(hello());
    let (mut engine, root) = engine(&[
        spawn(2, spawn(20, hi())),
        tell(2, spawn(21, hi())),
        tell(2, spawn(22, hi())),
        tell(2, spawn(23, hi())),
        spawn(3, spawn(30, hi())),
    ]);
    let clock = Ticking::default();
    engine.step(Time::ZERO, &clock);
    let _ = engine.take_turned();

    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::ZERO).into_split() else {
        panic!("two workers share the step");
    };
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &clock)).collect();
    engine.apply(done);
    let _ = engine.take_turned();

    let third = sorted(step(&mut engine));
    assert_eq!(third, [got(20, hi()), got(21, hi()), got(22, hi()), got(23, hi()), got(30, hi())]);
    let (two, three) = (node(root, 2), node(root, 3));
    let grandchildren = [node(two, 20), node(two, 21), node(two, 22), node(two, 23), node(three, 30)];
    let kept = grandchildren.iter().all(|id| engine.table().seat(*id).is_some());
    assert!(kept);
}

#[test]
fn chain_between_actors_in_one_part_runs_within_the_step() {
    // Catches a chain not fused between actors in one part, a fused send also delivered as mail, and a child not
    // seated in its parent's part while that part has room.
    let (mut fused, _) = fusing(8, &[spawn(2, up(hello()))]);
    assert_eq!(step(&mut fused), [got(1, spawn(2, up(hello())))]);
    assert_eq!(step(&mut fused), [got(2, up(hello())), got(1, hello().from(2))]);
    assert!(fused.is_idle());

    let (mut unfused, _) = engine(&[spawn(2, up(hello()))]);
    assert_eq!(step(&mut unfused), [got(1, spawn(2, up(hello())))]);
    assert_eq!(step(&mut unfused), [got(2, up(hello()))]);
    assert_eq!(step(&mut unfused), [got(1, hello().from(2))]);
}

#[test]
fn chain_runs_at_most_the_budget_of_hops_in_a_step() {
    // Catches a part running more hops in a step than its budget, or stopping a chain short of it.
    let (mut engine, _) = fusing(3, &[spawn(2, bounce(0)), tell(2, bounce(7))]);
    step(&mut engine);

    let mut steps = Vec::new();
    while !engine.is_idle() {
        steps.push(step(&mut engine));
    }
    // Each step runs the items waiting, then three hops.
    let second = [got(2, bounce(0)), got(2, bounce(7)), got(1, bounce(6)), got(2, bounce(5)), got(1, bounce(4))];
    let third = [got(2, bounce(3)), got(1, bounce(2)), got(2, bounce(1)), got(1, bounce(0))];
    assert_eq!(steps, [second.to_vec(), third.to_vec()]);
}

#[test]
fn chains_begun_by_different_items_all_run_in_the_step_in_the_order_their_hops_were_made() {
    // Catches a part fusing only the first chain its items begin, and hops run newest first. Node 2's `Later` begins
    // a chain to itself and its `Reply` one to its parent; the parent's answer runs between node 2's two hops.
    let (mut engine, _) = fusing(8, &[spawn(2, later(later(depth()))), tell(2, reply())]);
    step(&mut engine);

    let second = [
        got(2, later(later(depth()))),
        got(2, reply()),
        got(2, later(depth())),
        got(1, answer()),
        got(2, depth()),
        at(2, 4),
    ];
    assert_eq!(step(&mut engine), second);
    assert!(engine.is_idle());
}

#[test]
fn each_run_sees_only_the_children_it_stood_up_and_a_part_begins_with_none() {
    // Catches two runs of one part sharing what they stood up, and a run kept for its room still holding a child
    // from the part a worker ran before.
    let (engine, root) = fusing(8, &[]);
    let Some(root_path) = engine.table().path(root) else {
        panic!("the root is in the table");
    };
    let segment = PathSegment::new(Node::NAME, &2u64);
    let Ok(child_path) = root_path.child(&segment) else {
        panic!("a child of the root fits");
    };
    let child = Seated { id: child_id(root, &segment), seat: SeatNumber::new(9) };

    let mut fusing = Fusing::default();
    fusing.begin(8);
    let first = fusing.begin_run();
    let second = fusing.begin_run();
    fusing.run_mut(first).stand([(child, child_path.clone())]);
    assert_eq!(fusing.run(first).resolve(&child_path), Some(child));
    assert_eq!(fusing.run(second).resolve(&child_path), None);

    fusing.begin(8);
    let again = fusing.begin_run();
    assert_eq!(fusing.run(again).resolve(&child_path), None);
}

#[test]
fn hop_never_overtakes_mail_its_sender_sent_as_mail() {
    // Catches a hop handed over in the step its sender mailed the same receiver as mail, ahead of that mail. The root
    // answers `Pair` with a fused `Bounce(2)` and `Last` as mail, so its `Bounce(0)` must wait behind `Last`.
    let (mut engine, _) = fusing(8, &[spawn(2, up(pair()))]);
    step(&mut engine);

    assert_eq!(step(&mut engine), [got(2, up(pair())), got(1, pair().from(2)), got(2, bounce(2)), got(1, bounce(1))]);
    assert_eq!(step(&mut engine), [got(2, last()), got(2, bounce(0))]);
}

#[test]
fn each_send_a_part_could_fuse_is_counted_once_by_what_became_of_it() {
    // Catches a send counted under the wrong rule, counted twice or not at all, and sends counted when the engine
    // fuses nothing. The root's `Spawn` mails an unseated child; the child's `Up` fuses to the root, whose `Pair`
    // fuses `Bounce(2)` and sends `Last` after it in its turn; the child's `Bounce(1)` fuses back, and the root's
    // `Bounce(0)` is held back by the `Last` it sent as mail.
    let (mut fused, _) = fusing(8, &[spawn(2, up(pair()))]);
    while !fused.is_idle() {
        step(&mut fused);
    }
    let counted = Hops { fused: 3, spilled: 1, unseated: 1, burst: 1, ..Hops::default() };
    assert_eq!(fused.hopped(), counted);

    let (mut unfused, _) = engine(&[spawn(2, up(pair()))]);
    while !unfused.is_idle() {
        step(&mut unfused);
    }
    assert_eq!(unfused.hopped(), Hops::default());
}

#[test]
fn run_sees_its_own_spawns_and_endings_and_no_other_turn_does() {
    // Catches a child a run stood up left unresolved to the run's later turns, an actor that ended in the run still
    // found by them, and either seen by a turn outside the run before the step is folded; by a resolve, and by a
    // typed path's reach, which also reads the child's kind.
    let (engine, root) = fusing(8, &[]);
    let Some(root_path) = engine.table().path(root) else {
        panic!("the root is in the table");
    };
    let segment = PathSegment::new(Node::NAME, &2u64);
    let Ok(child_path) = root_path.child(&segment) else {
        panic!("a child of the root fits");
    };
    let child = Seated { id: child_id(root, &segment), seat: SeatNumber::new(9) };
    let mut run = fuse::Run::default();
    run.stand([(child, child_path.clone())]);
    run.end(root);
    let mut stood = ByActor::default();
    stood.insert(child.id.bits(), (child_path.clone(), child.seat, Declared::of::<Node>()));
    let node = KindId::of::<Node>();
    let nowhere = |path: &ActorPath| Seated { id: path_id(path), seat: SeatNumber::NOWHERE };

    let in_run = Since {
        table: engine.table(),
        stood: &stood,
        sees: Sees::Run(&run),
        blobs: &engine.blobs,
        now: Time::ZERO,
        cycle: &[],
    };
    assert_eq!(Table::resolve(&in_run, &child_path), Some(child));
    assert!(Table::is_live(&in_run, child.id));
    assert_eq!(Table::resolve(&in_run, &root_path), None);
    assert!(!Table::is_live(&in_run, root));
    assert_eq!(Table::reach(&in_run, &child_path, node), child);
    assert_eq!(Table::reach(&in_run, &child_path, KindId::of::<Note>()), nowhere(&child_path));
    assert_eq!(Table::reach(&in_run, &root_path, node), nowhere(&root_path));
    assert_eq!(Table::reach_child(&in_run, root, &segment, node), Some(child));
    assert_eq!(Table::reach_child(&in_run, root, &segment, KindId::of::<Note>()), None);

    let outside = Since {
        table: engine.table(),
        stood: &stood,
        sees: Sees::Step,
        blobs: &engine.blobs,
        now: Time::ZERO,
        cycle: &[],
    };
    assert_eq!(Table::resolve(&outside, &child_path), None);
    assert!(!Table::is_live(&outside, child.id));
    assert!(Table::is_live(&outside, root));
    assert_eq!(Table::reach(&outside, &child_path, node), nowhere(&child_path));
    assert_eq!(Table::reach(&outside, &root_path, node), seated(&engine, root));
    assert_eq!(Table::reach_child(&outside, root, &segment, node), None);
}

#[test]
fn a_child_that_ended_in_a_run_is_not_found_by_its_later_turns() {
    // Catches `turn.child` in a run finding beneath a reference a child the table holds that ended in the run, and
    // a turn outside the run not finding it before the step is folded (ADR-0013, ADR-0022).
    let (mut engine, root) = fusing(8, &[spawn(2, hello())]);
    step(&mut engine);
    let segment = PathSegment::new(Node::NAME, &2u64);
    let child = seated(&engine, child_id(root, &segment));
    let mut run = fuse::Run::default();
    run.end(child.id);
    let stood = ByActor::default();
    let node = KindId::of::<Node>();
    let since =
        |sees| Since { table: engine.table(), stood: &stood, sees, blobs: &engine.blobs, now: Time::ZERO, cycle: &[] };

    assert_eq!(Table::reach_child(&since(Sees::Run(&run)), root, &segment, node), None);
    assert_eq!(Table::reach_child(&since(Sees::Step), root, &segment, node), Some(child));
}

#[test]
fn back_to_back_mail_to_one_actor_is_one_run_handed_out_piece_by_piece() {
    // Catches a run that loses or repeats a piece, hands its pieces out of another order than they were sent, is
    // kept as an item for each piece, or counts its sender's mail short, so a later piece of mail reuses an id.
    let (mut engine, root) = engine(&[many(3)]);
    assert_eq!(step(&mut engine), [got(1, many(3))]);

    let me = seated(&engine, root);
    let pieces: Vec<u32> = engine.streams.shard(me.seat.number()).arrived().map(|(_, _, pieces)| pieces).collect();
    assert_eq!(pieces, [3], "one item, of three pieces");
    let count = engine.lives.shard(me.seat.number()).life(me).map(lives::Life::sent);
    assert_eq!(count, Some(4), "one piece from its begin, three from its first turn");

    assert_eq!(step(&mut engine), [got(1, mark(0)), got(1, mark(1)), got(1, mark(2))]);
}

#[test]
fn run_fuses_its_first_piece_and_mails_the_rest() {
    // Catches a fused run whose rest is dropped, handed over twice, or run in the step ahead of the piece fused.
    let (mut engine, _) = fusing(8, &[many(3)]);

    assert_eq!(step(&mut engine), [got(1, many(3)), got(1, mark(0))]);
    assert_eq!(step(&mut engine), [got(1, mark(1)), got(1, mark(2))]);
    assert!(engine.is_idle());
}

/// Runs one step planned for `workers` workers on this thread, shared or whole as the plan says, timed by `clock`,
/// and answers with what its turns show.
fn step_on(engine: &mut Engine, workers: usize, clock: &Ticking) -> Vec<Seen> {
    let workers = Workers { active: workers, max: workers, wake_nanos: 0, split: SplitCost::NONE };
    match engine.plan(workers, Time::ZERO).into_split() {
        Split::Whole(whole) => {
            let left = engine.run_whole(whole, clock);
            engine.apply_whole(left);
        }
        Split::Shares(shares) => {
            let reading = engine.reading();
            let done = shares.into_iter().map(|share| reading.run(share, clock)).collect();
            engine.apply(done);
        }
    }

    seen(engine)
}

#[test]
fn an_actor_that_sends_past_its_slice_hands_on_the_rest_in_later_steps_in_order() {
    // Catches a slice that never cuts, items an actor was cut off from lost, handed twice, or out of order, among them
    // through a backlog moved to its front, and a hop to an actor cut off in its step run ahead of what it carries.
    // Notes of two lengths, one after the other, are not joined into runs and leave a backlog of many.
    sliced(|index| {
        if index.is_multiple_of(2) {
            mark(index)
        } else {
            mark(index).padded()
        }
    });
}

#[test]
fn an_actor_cut_off_inside_a_run_hands_on_the_rest_of_it_in_order() {
    // Catches a run cut at the slice that hands on pieces it already handed, skips some, or numbers what it carries
    // as the pieces it began with: notes of one length, all sent by the root's begin, are one run.
    sliced(mark);
}

/// Runs a root handed 3,000 notes, each `note` of its place, that each tell a child, on two workers, and checks the
/// root was cut off at its slice and the child was told, and answered, every note in the order sent, with and without
/// fusion.
fn sliced(note: impl Fn(u64) -> Note) {
    for hops in [0, 64] {
        let mut notes = vec![spawn(2, hello())];
        notes.extend((0..3000).map(|index| tell(2, up(note(index)))));
        let (mut engine, _) = fusing(hops, &notes);
        let clock = Ticking::default();
        let mut steps = Vec::new();
        while !engine.is_idle() {
            steps.push(step_on(&mut engine, 2, &clock));
        }

        let told: Vec<Note> = steps.iter().flatten().filter_map(|seen| seen.note_of(2)?.sent_up()).collect();
        let answered: Vec<Note> = steps.iter().flatten().filter_map(|seen| seen.note_of(1)?.up_from(2)).collect();
        let sent: Vec<Note> = (0..3000).map(&note).collect();
        assert_eq!(told, sent, "hops={hops}");
        assert_eq!(answered, sent, "hops={hops}");
        let tells = |seen: &Seen| seen.note_of(1).is_some_and(Note::tells);
        let telling = steps.iter().filter(|step| step.iter().any(tells)).count();
        assert!(telling > 1, "the root is cut off at its slice, hops={hops}");
    }
}

#[test]
fn a_child_stood_up_in_a_turn_counts_what_it_sent_from_its_begin() {
    // Catches the count of a child's `begin` routed ahead of the spawn that makes its life, and dropped, so its next
    // turn numbers its mail from nought and gives a later piece the id of one its `begin` sent.
    let (mut engine, root) = engine(&[nest(2, 3)]);
    assert_eq!(step(&mut engine), [got(1, nest(2, 3))]);
    let grandchild = node(node(root, 2), 3);
    let me = seated(&engine, grandchild);
    let count = engine.lives.shard(me.seat.number()).life(me).map(lives::Life::sent);
    assert_eq!(count, Some(1), "3 sent itself close from its begin");
}

/// The notes a holder sends itself as it begins, and how many times it mails itself a blob of three bytes there,
/// keeping none of it.
#[derive(Plain)]
#[kind(name = "aether.test.holder_config")]
struct HolderConfig {
    to_self: Vec<Note>,
    gives: u64,
}

/// A blob, given in mail, to be given on `left` more times.
#[derive(Bound)]
#[kind(name = "aether.test.given")]
struct Given {
    blob: BlobRef,
    left: u64,
}

/// An actor that puts, keeps, reads, and lets go of blobs as the orders of its notes say (ADR-0014), and otherwise as
/// a node would fail and send itself the rest, and keeps a blob it is given, with how many bytes it holds, but for one
/// it gave itself as it began, which it reads, gives itself on while it has more times to go, and lets go of.
#[derive(Bound)]
#[kind(name = "aether.test.holder")]
struct Holder {
    held: Vec<BlobRef>,
    /// Whether it gave itself a blob as it began.
    gave: bool,
    /// How many bytes each blob held, each time it read them, in order.
    sizes: Vec<u64>,
    /// The hash of each blob, each time it hashed them, in order.
    hashes: Vec<[u8; 32]>,
    /// Why each put it was refused was refused, in order.
    refused: Vec<Refusal>,
    /// How many bytes each blob it was given held, in order.
    given: Vec<u64>,
}

impl ChildOf for Holder {
    type Parent = Top;
}

impl Actor for Holder {
    type Config = HolderConfig;
    type Key = ();
    type Handles = (Note, Given);
    type BeginSends = (ToType<Self, Note>, ToType<Self, Given>);
    type EndSends = ();

    fn begin(config: HolderConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        for note in config.to_self {
            turn.to(turn.me()).send(&note)?;
        }
        // Each blob it gives is one only that mail refers to, so mail that loses its pin loses its blob.
        for _ in 0..config.gives {
            let Ok(blob) = turn.put(vec![9; 3]) else {
                return Err(Fault::new(&said(Word::NoRoom))?);
            };
            // A note between each two keeps them from joining into one run, so the part carries many items.
            let me = turn.me();
            turn.to(me).send(&Given { blob, left: 1 })?;
            turn.to(me).send(&said(Word::Between))?;
        }

        Ok(Self {
            held: Vec::new(),
            gave: config.gives > 0,
            sizes: Vec::new(),
            hashes: Vec::new(),
            refused: Vec::new(),
            given: Vec::new(),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Holder {
    type Sends = (ToType<Self, Note>, ToType<Self, Given>);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some((order, rest)) = note.parted() else {
            return Ok(());
        };
        match order {
            Order::Put(bytes) => {
                let bytes = usize::try_from(*bytes).unwrap_or_default();
                match turn.put(vec![7; bytes]) {
                    Ok(blob) => self.held.push(blob),
                    Err(refused) => self.refused.push(Refusal::from(refused)),
                }
            }
            Order::Read => {
                for blob in &self.held {
                    self.sizes.push(length(turn.read(blob)));
                }
            }
            Order::Hash => {
                for blob in &self.held {
                    self.hashes.push(hashed(turn.hash(blob)));
                }
            }
            Order::LetGo => drop(self.held.pop()),
            Order::Fail => return Err(Fault::new(&note)?),
            Order::Send | Order::SendAndClose => {
                if let Some(blob) = self.held.pop() {
                    turn.to(turn.me()).send(&Given { blob, left: 0 })?;
                }
                let closes = *order == Order::SendAndClose;
                if closes {
                    turn.close();
                }
            }
            Order::Later => {
                turn.to(turn.me()).send(&rest)?;
            }
            _ => {}
        }

        Ok(())
    }
}

impl Handles<Given> for Holder {
    type Sends = (ToType<Self, Given>,);

    fn handle(&mut self, given: Given, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = length(turn.read(&given.blob));
        if self.gave {
            let Given { blob, left } = given;
            if left > 0 {
                turn.to(turn.me()).send(&Given { blob, left: left - 1 })?;
            }
        } else {
            self.given.push(held);
            self.held.push(given.blob);
        }

        Ok(())
    }
}

/// An engine whose blobs hold at most `blob_bytes`, with a holder that sends itself `to_self` as it begins. It records
/// no turns: a recorded piece of mail keeps its pins, which would hold the blobs these tests watch go.
fn holding(blob_bytes: usize, to_self: &[Note]) -> Engine {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, blob_bytes, 0).ready();
    let Ok(_) = engine.add_root::<Holder>(&(), HolderConfig { to_self: to_self.to_vec(), gives: 0 }) else {
        panic!("the holder begins");
    };

    engine
}

/// How many bytes `blob` holds.
fn length(blob: &[u8]) -> u64 {
    u64::try_from(blob.len()).unwrap_or(u64::MAX)
}

/// What a holder keeps of a blob with the hash `hash`.
fn hashed(hash: BlobHash) -> [u8; 32] {
    hash.bytes()
}

/// The state of the holder of `engine`.
fn holder(engine: &Engine) -> &Holder {
    state(engine, root_of::<Holder>())
}

#[test]
fn a_hash_asked_for_in_a_turn_is_kept_at_the_fold() {
    // Catches a hash a turn computed never kept, one kept before the blobs put in its step are taken in, which loses
    // the hash of a blob put and hashed in one step, and blobs hashed as they are put, which a blob nothing asks the
    // hash of should never pay for.
    let three = BlobHash::of(&[7; 3]);
    let mut engine = holding(1 << 20, &[put(3), hash(), put(4), later(hash())]);
    step(&mut engine);
    assert_eq!(holder(&engine).hashes, [hashed(three)]);
    assert_eq!(engine.blobs.kept_hashes(), [three], "only the blob asked for is hashed");

    let four = BlobHash::of(&[7; 4]);
    step(&mut engine);
    assert_eq!(holder(&engine).hashes, [hashed(three), hashed(three), hashed(four)]);
    let mut kept = engine.blobs.kept_hashes();
    kept.sort();
    let mut both = [three, four];
    both.sort();
    assert_eq!(kept, both);
}

#[test]
fn the_engine_keeps_a_hash_it_is_asked_for_between_steps() {
    // Catches the chassis's ask for a hash answered and not kept, so a turn that asks for it later computes it again.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let Ok(blob) = engine.put_blob(vec![1, 2, 3]) else {
        panic!("three bytes fit");
    };
    assert_eq!(engine.blobs.kept_hashes(), []);

    assert_eq!(engine.hash(&blob), Some(BlobHash::of(&[1, 2, 3])));
    assert_eq!(engine.blobs.kept_hashes(), [BlobHash::of(&[1, 2, 3])]);
}

#[test]
fn a_blob_mailed_and_kept_by_no_one_else_reaches_its_receiver_as_a_hop_or_as_it_waited() {
    // Catches mail read with no pins where it was fused into a hop, whose pins lie in the arena its receiver's turn
    // writes into, or where it waited: its reference is refused, and its turn fails.
    for (parts, hops) in [(1, 0), (1, 64), (4, 0), (4, 64)] {
        let Some(parts) = NonZeroUsize::new(parts) else {
            panic!("an engine has parts");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, hops).ready();
        let to_self = vec![put(3), send()];
        let Ok(_) = engine.add_root::<Holder>(&(), HolderConfig { to_self, gives: 0 }) else {
            panic!("the holder begins");
        };
        while !engine.is_idle() {
            step(&mut engine);
        }

        assert_eq!(holder(&engine).given, [3], "{parts} parts, {hops} hops");
    }
}

#[test]
fn a_blob_put_in_a_turn_is_read_in_that_step_and_the_ones_after() {
    // Catches a read that looks only in the store and misses what its worker put in the step, and a store that never
    // takes in what was put.
    let mut engine = holding(1 << 20, &[put(3), read(), later(read())]);
    step(&mut engine);
    assert_eq!(holder(&engine).sizes, [3]);
    assert_eq!(engine.blobs_held(), (1, 3));

    step(&mut engine);
    assert_eq!(holder(&engine).sizes, [3, 3]);
}

#[test]
fn a_blob_is_freed_at_the_fold_after_its_last_reference_goes() {
    // Catches a blob kept once nothing refers to it, and the bytes it held kept against the budget.
    let mut engine = holding(1 << 20, &[put(4), put(5), later(let_go())]);
    step(&mut engine);
    assert_eq!(engine.blobs_held(), (2, 9));

    step(&mut engine);
    assert_eq!(engine.blobs_held(), (1, 4));
}

#[test]
fn a_put_past_the_budget_is_refused_until_a_blob_is_freed() {
    // Catches a put that goes past the budget, a refusal the actor is not told of, and freed bytes never given back.
    let mut engine = holding(8, &[put(6), put(6), later(let_go()), later(later(put(6)))]);
    step(&mut engine);
    assert_eq!(holder(&engine).refused, [Refusal::NoRoom]);
    assert_eq!(engine.blobs_held(), (1, 6));

    step(&mut engine);
    assert_eq!(engine.blobs_held(), (0, 0));
    step(&mut engine);
    assert_eq!(holder(&engine).refused, [Refusal::NoRoom], "the put after the blob is freed is not refused");
    assert_eq!(engine.blobs_held(), (1, 6));
}

#[test]
fn an_actor_that_fails_lets_go_of_what_it_held_when_it_ends() {
    // Catches the pins an actor's state held lost when the actor is dropped, which leaves its blobs held for good.
    let mut engine = holding(1 << 20, &[put(4), later(fail())]);
    step(&mut engine);
    assert_eq!(engine.blobs_held(), (1, 4));

    step(&mut engine);
    step(&mut engine);
    assert_eq!(engine.blobs_held(), (0, 0));
}

#[test]
fn references_held_or_dropped_between_steps_are_counted() {
    // Catches the changes made outside a step's turns never summed: a blob the chassis let go of kept for good, and
    // one it holds freed for want of its pin.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let Ok(kept) = engine.put_blob(vec![1, 2, 3]) else {
        panic!("three bytes fit");
    };
    let Ok(dropped) = engine.put_blob(vec![4; 5]) else {
        panic!("five bytes fit");
    };
    drop(dropped);
    engine.step(Time::ZERO, &Stopped);

    assert_eq!(engine.blobs_held(), (1, 3));
    assert_eq!(engine.blob(&kept), Some(&[1, 2, 3][..]));
}

#[test]
fn one_engine_sums_only_the_changes_to_its_own_blobs() {
    // Catches the changes made outside a step shared by every engine of the process, so one engine's dropped
    // reference takes a pin from another's blob that has the same number, and frees it while it is held.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut kept = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let mut dropping = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let Ok(blob) = kept.put_blob(vec![1, 2, 3]) else {
        panic!("three bytes fit");
    };
    let Ok(other) = dropping.put_blob(vec![4, 5, 6]) else {
        panic!("three bytes fit");
    };
    assert_eq!(blob.id().bits(), other.id().bits(), "the two engines number their first blobs alike");
    drop(other);
    kept.step(Time::ZERO, &Stopped);
    dropping.step(Time::ZERO, &Stopped);

    assert_eq!(kept.blobs_held(), (1, 3));
    assert_eq!(dropping.blobs_held(), (0, 0));
}

#[test]
fn a_blob_sent_in_mail_lives_until_its_receiver_holds_it() {
    // Catches mail that holds no pin, so a blob nobody else refers to is freed before it is delivered, and a receiver
    // that reads a reference back with no pin of its own, so the blob goes once the mail's arena is emptied.
    let mut engine = holding(1 << 20, &[put(3), later(send())]);
    step(&mut engine);
    step(&mut engine);
    assert!(holder(&engine).given.is_empty());
    assert_eq!(engine.blobs_held(), (1, 3), "only the mail refers to it");

    step(&mut engine);
    assert_eq!(holder(&engine).given, [3]);
    for _ in 0..3 {
        step(&mut engine);
    }
    assert_eq!(engine.blobs_held(), (1, 3), "the receiver keeps it once the mail is gone");
}

#[test]
fn mail_that_is_never_delivered_lets_go_of_its_blob() {
    // Catches the pin of mail dropped undelivered kept for good, which holds its blob past everything that could
    // read it.
    let mut engine = holding(1 << 20, &[put(3), later(send_and_close())]);
    for _ in 0..6 {
        step(&mut engine);
    }

    assert_eq!(engine.blobs_held(), (0, 0));
}

#[test]
fn mail_sent_to_all_reaches_each_receiver_once_wherever_it_is_seated() {
    // Catches a send to all that leaves a receiver out, hands one of them the piece twice, or reads the shared bytes
    // wrong for a receiver in another part than its sender's, fused or not.
    for hops in [0, 64] {
        let (mut engine, root) = fusing(hops, &[fan(&[2, 3, 4, 5, 6, 7, 8, 9], hello())]);
        let mut heard = Vec::new();
        while !engine.is_idle() {
            heard.extend(step(&mut engine));
        }
        let part = |id: ActorId| seated(&engine, id).seat.index() % 4;
        let elsewhere = (2..=9).filter(|number| part(node(root, *number)) != part(root)).count();
        assert!(elsewhere > 0, "some receiver is seated in another part than its sender, hops={hops}");

        let expected: Vec<Seen> = (2..=9).map(|number| got(number, hello())).collect();
        let hello: Vec<Seen> =
            sorted(heard).into_iter().filter(|seen| matches!(seen, Seen::Got(_, note) if *note == hello())).collect();
        assert_eq!(hello, expected, "hops={hops}");
    }
}

#[test]
fn mail_carried_past_its_receivers_slice_keeps_its_blob_until_it_is_handed() {
    // Catches the bytes of mail carried over into a part's backlog without the pins of what they refer to, so the
    // blob is freed once the arena it was sent in is emptied while the backlog still holds mail that refers to it:
    // mail carried as it arrived, mail moved to the front of a backlog, and a hop to an actor cut off in its step.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    for hops in [0, 64] {
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, hops).ready();
        let Ok(_) = engine.add_root::<Holder>(&(), HolderConfig { to_self: Vec::new(), gives: 3000 }) else {
            panic!("the holder begins");
        };
        let clock = Ticking::default();
        let mut steps = 0;
        while !engine.is_idle() {
            step_on(&mut engine, 2, &clock);
            steps += 1;
        }
        assert!(steps > 10, "the holder is cut off at its slice and carries what it was given, hops={hops}");

        // A part idle for long enough gives back what it carried, and the pins with it.
        for _ in 0..=stream::IDLE_STEPS {
            step_on(&mut engine, 2, &clock);
        }
        assert_eq!(engine.blobs_held(), (0, 0), "hops={hops}");
    }
}

/// What a clocked actor is told as it begins: its number, and the note it sends itself.
#[derive(Plain)]
#[kind(name = "aether.test.clocked_config")]
struct ClockedConfig {
    number: u64,
    to_self: Note,
}

/// What a clocked actor's closing turn tells its parent, since its state ends with it: the times it read, and how many
/// bytes the blob it put as it began holds.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.finished")]
struct Finished {
    number: u64,
    began: u64,
    turns: Vec<u64>,
    ended: u64,
    bytes: u64,
}

/// An actor that keeps the time each of its turns reads, and puts a blob of three bytes as it begins, which its closing
/// turn reads (ADR-0014, ADR-0018): it acts on `Order::Twins` and `Order::Close`.
#[derive(Bound)]
#[kind(name = "aether.test.clocked")]
struct Clocked {
    number: u64,
    blob: BlobRef,
    /// The time its `begin` read, in nanoseconds.
    began: u64,
    /// The time each turn over a note read, in nanoseconds, in order.
    turns: Vec<u64>,
}

impl ChildOf for Clocked {
    type Parent = Recursive;
}

impl Actor for Clocked {
    type Config = ClockedConfig;
    type Key = u64;
    type Handles = (Note, Finished);
    type BeginSends = (ToType<Self, Note>,);
    type EndSends = (ToType<Self, Finished>,);

    fn begin(config: ClockedConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let ClockedConfig { number, to_self } = config;
        let began = turn.now().nanos();
        let Ok(blob) = turn.put(vec![7; 3]) else {
            return Err(Fault::new(&said(Word::NoRoom))?);
        };
        turn.to(turn.me()).send(&to_self)?;

        Ok(Self { number, blob, began, turns: Vec::new() })
    }

    fn end(self, turn: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        let bytes = length(turn.read(&self.blob));
        let Self { number, began, turns, .. } = self;
        let finished = Finished { number, began, turns, ended: turn.now().nanos(), bytes };
        if let Some(parent) = turn.parent() {
            turn.to(parent).send(&finished)?;
        }

        Ok(())
    }
}

impl Handles<Note> for Clocked {
    type Sends = ();

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        self.turns.push(turn.now().nanos());
        match note.first() {
            Some(Order::Twins) => {
                for number in [2, 3] {
                    let _ = turn.spawn::<Self>(&number, ClockedConfig { number, to_self: close() });
                }
            }
            Some(Order::Close) => turn.close(),
            _ => {}
        }

        Ok(())
    }
}

impl Handles<Finished> for Clocked {
    type Sends = ();

    fn handle(&mut self, _: Finished, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What the clocked actor `number` tells its parent, having begun at 5, turned at 9, and closed at 12.
fn finished(number: u64) -> Seen {
    Seen::Finished(Finished { number, began: 5, turns: vec![9], ended: 12, bytes: 3 })
}

#[test]
fn every_turn_reads_the_time_its_step_was_given_and_a_closing_turn_reads_blobs() {
    // Catches a turn, a `begin` run inside one, or a closing turn reading a time other than its step's: one kept from
    // an earlier step, or none, as the bare table a closing turn was handed holds, which also held no blob for it to
    // read. A root made before any step reads the host's first moment.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    engine.record_turns();
    let config = ClockedConfig { number: 1, to_self: twins() };
    let Ok(root) = engine.add_root::<Clocked>(&1, config) else {
        panic!("the root begins");
    };
    assert_eq!(state::<Clocked>(&engine, root).began, 0);

    engine.step(Time::from_nanos(5), &Stopped);
    assert_eq!(seen(&mut engine), [got(1, twins())]);
    assert_eq!(state::<Clocked>(&engine, root).turns, [5]);
    engine.step(Time::from_nanos(9), &Stopped);
    assert_eq!(sorted(seen(&mut engine)), [got(2, close()), got(3, close())]);
    engine.step(Time::from_nanos(12), &Stopped);
    assert_eq!(sorted(seen(&mut engine)), [closed(2), closed(3)]);
    engine.step(Time::from_nanos(15), &Stopped);
    assert_eq!(sorted(seen(&mut engine)), [finished(2), finished(3)]);
}

#[test]
fn the_workers_of_a_shared_step_read_the_time_it_was_planned_with() {
    // Catches the workers of a step shared among them reading another time than the one the host planned it with.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    // Each root sits in the part with fewest actors, so the two are in parts of their own.
    let roots = [1, 2].map(|number| {
        let config = ClockedConfig { number, to_self: said(Word::Tick) };
        let Ok(root) = engine.add_root::<Clocked>(&number, config) else {
            panic!("the root begins");
        };

        root
    });

    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::from_nanos(7)).into_split() else {
        panic!("two workers share the step");
    };
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &Stopped)).collect();
    engine.apply(done);
    for root in roots {
        assert_eq!(state::<Clocked>(&engine, root).turns, [7]);
    }
}

/// `seen`, in order of the actors' numbers: for turns of one step, which run in no order the tests depend on.
fn sorted(mut seen: Vec<Seen>) -> Vec<Seen> {
    seen.sort_by_key(|seen| (seen.number(), format!("{seen:?}")));

    seen
}

/// The chain the `begin` of the root `root` set off (ADR-0012).
const fn begun(root: ActorId) -> MailId {
    MailId { sender: root, count: 0 }
}

#[test]
fn a_root_s_chain_finishes_once_nothing_it_set_off_is_left_to_run() {
    // Catches a chain taken for finished while mail of it waits, while an actor it ended is still to run its closing
    // turn, or while one watching that actor is still to be shown the ending, and a chain taken for open once all of
    // it has run. Everything here follows from the root's `begin`, so its chain is open exactly while a step would
    // run a turn. A node leaving failing fails its closing turn, so what it sends is dropped and only the turn itself
    // is left to run.
    let chains = [
        vec![spawn(2, up(hello()))],
        vec![watch(2, close())],
        vec![spawn_leaving(13, Leaving::Failing, close())],
        vec![fan(&[2, 3], up(hello()))],
        vec![bounce(3), pair()],
    ];
    for notes in chains {
        let (mut engine, root) = engine(&notes);
        let mut steps = 0;
        loop {
            let finished = engine.has_finished(begun(root));
            assert_eq!(finished, engine.is_idle(), "{notes:?} after {steps} steps");
            if finished {
                break;
            }
            step(&mut engine);
            steps += 1;
            assert!(steps < 50, "{notes:?} settles");
        }
        assert!(steps > 1, "{notes:?} runs past its first step");
    }
}

#[test]
fn a_chain_carried_past_its_receivers_slice_has_not_finished() {
    // Catches the items a part carries over into its next step left out of whether their chain has finished: an
    // actor cut off at its slice, with mail as it arrived and hops, still has turns of the chain to run.
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    for hops in [0, 64] {
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, hops).ready();
        let Ok(holder) = engine.add_root::<Holder>(&(), HolderConfig { to_self: Vec::new(), gives: 3000 }) else {
            panic!("the holder begins");
        };
        let clock = Ticking::default();
        let mut steps = 0;
        while !engine.is_idle() {
            assert!(!engine.has_finished(begun(holder)), "hops={hops} after {steps} steps");
            step_on(&mut engine, 2, &clock);
            steps += 1;
        }
        assert!(steps > 10, "the holder is cut off at its slice and carries what it was given, hops={hops}");
        assert!(engine.has_finished(begun(holder)), "hops={hops}");
    }
}

#[test]
fn one_chain_finishes_while_another_runs_on() {
    // Catches whether a chain has finished answered for every chain at once: the second root's chain ends with its
    // closing turn while the first's mail is still going.
    let (mut engine, first) = engine(&[spawn(2, spawn(3, up(hello())))]);
    let config = NodeConfig { to_self: vec![close()], ..NodeConfig::quiet(2) };
    let Ok(second) = engine.add_root::<Node>(&2, config) else {
        panic!("the second root begins");
    };
    step(&mut engine);
    assert!(!engine.has_finished(begun(second)), "the second root is still to run its closing turn");
    step(&mut engine);
    assert!(engine.has_finished(begun(second)));
    assert!(!engine.has_finished(begun(first)), "3 has yet to say hello");
}

/// A root that takes what it is told, at the listener's name.
#[derive(Plain)]
struct Listener;

/// A root at the listener's name of another layout, so another kind.
#[derive(Plain)]
struct Impostor {
    at: u64,
}

/// What a root at the listener's name is: one that takes what it is told.
macro_rules! listening {
    ($name:ident) => {
        impl Kind for $name {
            const NAME: KindName = KindName::new("aether.test.listener");
        }

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = u64;
            type Handles = (Note,);
            type BeginSends = ();
            type EndSends = ();

            fn begin(listener: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
                Ok(listener)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Handles<Note> for $name {
            type Sends = ();

            fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }
    };
}

listening!(Listener);
listening!(Impostor);

/// A root that, as it begins, tells the listener of key zero at its typed path what it is made from: a keyed path,
/// which its spawn does not check, so it may hold anything as the caller sends.
#[derive(Plain)]
#[kind(name = "aether.test.caller")]
struct Caller {
    note: Note,
}

impl ChildOf for Caller {
    type Parent = Top;
}

impl Actor for Caller {
    type Config = Self;
    type Key = ();
    type Handles = ();
    type BeginSends = (Tells<Path<Listener>, Note>,);
    type EndSends = ();

    fn begin(caller: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Listener>(0)).send(&caller.note)?;

        Ok(caller)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An engine with whichever root `listening` makes at the listener's name, and a caller that tells it `note`.
fn calling(listening: impl FnOnce(&mut Engine), note: Note) -> Vec<Seen> {
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).ready();
    engine.record_turns();
    listening(&mut engine);
    let Ok(_) = engine.add_root::<Caller>(&(), Caller { note }) else {
        panic!("the caller begins");
    };

    step(&mut engine)
}

#[test]
fn a_send_at_a_typed_path_reaches_only_a_live_actor_of_the_paths_type() {
    // Catches a send at a path that does not find the actor live there, one handed to an actor of another kind that
    // shares its name and so its id, and one at a place that holds no actor kept rather than dropped (ADR-0022).
    let listener = calling(
        |engine| {
            let Ok(_) = engine.add_root::<Listener>(&0, Listener) else {
                panic!("the listener begins");
            };
        },
        hello(),
    );
    let impostor = calling(
        |engine| {
            let Ok(_) = engine.add_root::<Impostor>(&0, Impostor { at: 0 }) else {
                panic!("the impostor begins");
            };
        },
        hello(),
    );
    let nobody = calling(|_| {}, hello());

    assert_eq!(listener, [got(0, hello())]);
    assert!(impostor.is_empty(), "an actor of another kind at the path is not reached");
    assert!(nobody.is_empty());
}

/// A question an oracle answers.
#[derive(Plain)]
#[kind(name = "aether.test.question")]
struct Question;

/// An oracle's answer to a question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.reply")]
enum Reply {
    /// What the oracle answers.
    Truth,
    /// What a root of another kind at its name would answer.
    Lie,
}

/// What a seer keeps while its question is out.
#[derive(Plain)]
#[kind(name = "aether.test.wondering")]
struct Wondering;

/// A root that answers each question, at the oracle's name.
#[derive(Plain)]
struct Oracle;

/// A root at the oracle's name of another layout, so another kind.
#[derive(Plain)]
struct FalseOracle {
    at: u64,
}

/// What a root at the oracle's name is: one that answers each question with `$answers`.
macro_rules! answering {
    ($name:ident, $answers:expr) => {
        impl Kind for $name {
            const NAME: KindName = KindName::new("aether.test.oracle");
        }

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = u64;
            type Handles = (Question,);
            type BeginSends = ();
            type EndSends = ();

            fn begin(oracle: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
                Ok(oracle)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Carries<Question> for $name {
            type Answers = (Reply,);
        }

        impl Handles<Question> for $name {
            type Sends = (ToSender<Reply>,);

            fn handle(&mut self, _: Question, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
                Ok(turn.reply(&$answers)?)
            }
        }
    };
}

answering!(Oracle, Reply::Truth);
answering!(FalseOracle, Reply::Lie);

/// A root that, as it begins, asks the oracle of key zero at its typed path a question and watches it, and takes what
/// it hears: a keyed path, which its spawn does not check.
#[derive(Plain)]
#[kind(name = "aether.test.seer")]
struct Seer;

impl ChildOf for Seer {
    type Parent = Top;
}

impl Actor for Seer {
    type Config = Self;
    type Key = ();
    type Handles = (Answer<Reply, Wondering>, Ended);
    type BeginSends = (Asks<Path<Oracle>, Question, Wondering>,);
    type EndSends = ();

    fn begin(seer: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Oracle>(0)).ask(&Question, &Wondering)?;
        turn.watch(path::<Oracle>(0));

        Ok(seer)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<Reply, Wondering>> for Seer {
    type Sends = ();

    fn handle(&mut self, _: (Answered<Reply>, Wondering), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ended> for Seer {
    type Sends = ();

    fn handle(&mut self, _: Ended, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What a seer hears over a few steps, beside whichever root `oracle` makes at the oracle's name, in order.
fn seeing(oracle: impl FnOnce(&mut Engine)) -> Vec<Seen> {
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).ready();
    engine.record_turns();
    oracle(&mut engine);
    let Ok(_) = engine.add_root::<Seer>(&(), Seer) else {
        panic!("the seer begins");
    };
    for _ in 0..3 {
        engine.step(Time::ZERO, &Stopped);
    }

    // The record notes an item before it is decoded, so a turn that heard it is one that did not fail.
    let turns = engine.take_turned();
    let all_ran = turns.iter().all(|turn| !turn.failed());
    assert!(all_ran);

    sorted(turns.iter().flat_map(Seen::of).collect())
}

#[test]
fn an_ask_and_a_watch_at_a_typed_path_reach_only_a_live_actor_of_the_paths_type() {
    // Catches an ask at a path whose answer never comes, one handed to an actor of another kind that shares its name
    // and so its id, and one at a place that holds no actor that goes unanswered forever, rather than `Ended`; and a
    // watch at such a place that waits for an ending that will never come (ADR-0022).
    let oracle = seeing(|engine| {
        let Ok(_) = engine.add_root::<Oracle>(&0, Oracle) else {
            panic!("the oracle begins");
        };
    });
    let false_oracle = seeing(|engine| {
        let Ok(_) = engine.add_root::<FalseOracle>(&0, FalseOracle { at: 0 }) else {
            panic!("the false oracle begins");
        };
    });
    let nobody = seeing(|_| {});

    let gone = [saw(0, How::Earlier), Seen::Unanswered(Unanswered::Ended)];
    assert_eq!(oracle, [told(Reply::Truth)], "a live oracle is not shown as ended");
    assert_eq!(false_oracle, gone, "an actor of another kind at the path is not reached");
    assert_eq!(nobody, gone);
}

/// A root, a singleton, that other types reach at its typed path.
#[derive(Plain)]
struct Lighthouse;

/// A root at the lighthouse's name of another layout, so another kind.
#[derive(Plain)]
struct FalseLighthouse {
    at: u64,
}

/// What a root at the lighthouse's name is: one that takes notes and does nothing with them.
macro_rules! lighthouse {
    ($name:ident) => {
        impl Kind for $name {
            const NAME: KindName = KindName::new("aether.test.lighthouse");
        }

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = ();
            type Handles = (Note,);
            type BeginSends = ();
            type EndSends = ();

            fn begin(lighthouse: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
                Ok(lighthouse)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Handles<Note> for $name {
            type Sends = ();

            fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }
    };
}

lighthouse!(Lighthouse);
lighthouse!(FalseLighthouse);

/// A root that declares a send at the lighthouse's typed path in its `begin`, in its handler, or as it ends, whichever
/// of `$begins`, `$handler`, and `$ends` names it, and sends nothing: a declaration is what its spawn is checked by.
macro_rules! reaching {
    ($name:ident, $named:literal, $begins:ty, $handler:ty, $ends:ty) => {
        #[derive(Plain, Default)]
        #[kind(name = $named)]
        struct $name;

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = ();
            type Handles = (Note,);
            type BeginSends = $begins;
            type EndSends = $ends;

            fn begin(reaching: Self, _: &mut Beginning<'_, Self, $begins>) -> Result<Self, Fault> {
                Ok(reaching)
            }

            fn end(self, _: &mut Ending<'_, Self, $ends>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl Handles<Note> for $name {
            type Sends = $handler;

            fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, $handler>) -> Result<(), Fault> {
                Ok(())
            }
        }
    };
}

/// A send at the lighthouse's typed path.
type ToLighthouse = (Tells<Path<Lighthouse>, Note>,);

reaching!(Sailor, "aether.test.sailor", ToLighthouse, (), ());
reaching!(Lookout, "aether.test.lookout", (), ToLighthouse, ());
reaching!(Wrecker, "aether.test.wrecker", (), (), ToLighthouse);

/// Whether a root of type `R` is made beside whichever root `lighthouse` makes at the lighthouse's name.
fn reaches<R: Root<Config = R, Key = ()> + Default>(lighthouse: impl FnOnce(&mut Engine)) -> Result<(), Refused> {
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).ready();
    lighthouse(&mut engine);

    engine.add_root::<R>(&(), R::default()).map(|_| ())
}

#[test]
fn a_root_that_reaches_a_typed_path_of_singletons_is_refused_while_no_actor_of_its_type_is_live_there() {
    // Catches a check that reads only the `begin`'s sends and not a handler's or the closing turn's, one that counts
    // an actor of another kind of the same name as live there, and one that refuses a root whose path is live, or
    // names something other than the path's type (ADR-0022).
    let missing = Err(Refused::Missing(Lighthouse::NAME));
    let made = |engine: &mut Engine| {
        let Ok(_) = engine.add_root::<Lighthouse>(&(), Lighthouse) else {
            panic!("the lighthouse begins");
        };
    };
    let impostor = |engine: &mut Engine| {
        let Ok(_) = engine.add_root::<FalseLighthouse>(&(), FalseLighthouse { at: 0 }) else {
            panic!("the false lighthouse begins");
        };
    };

    assert_eq!(reaches::<Sailor>(|_| {}), missing, "a send its begin declares");
    assert_eq!(reaches::<Lookout>(|_| {}), missing, "a send a handler declares");
    assert_eq!(reaches::<Wrecker>(|_| {}), missing, "a send its closing turn declares");
    assert_eq!(reaches::<Sailor>(impostor), missing, "an actor of another kind at the place");
    assert_eq!(reaches::<Sailor>(made), Ok(()));
    assert_eq!(reaches::<Lookout>(made), Ok(()));
    assert_eq!(reaches::<Wrecker>(made), Ok(()));
}

/// A root that, as it begins, spawns a pilot, which reaches it back at its typed path, and a smuggler, which reaches
/// the lighthouse, and keeps what became of each.
#[derive(Plain)]
#[kind(name = "aether.test.harbor")]
struct Harbor {
    /// What became of its pilot: as a config, nothing yet.
    pilot: Result<(), Refusal>,
    /// What became of its smuggler: as a config, nothing yet.
    smuggler: Result<(), Refusal>,
}

impl ChildOf for Harbor {
    type Parent = Top;
}

impl Actor for Harbor {
    type Config = Self;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let pilot: Result<ActorRef<Pilot>, _> = turn.spawn(&(), Pilot);
        let smuggler: Result<ActorRef<Smuggler>, _> = turn.spawn(&(), Smuggler);

        Ok(Self {
            pilot: pilot.map(|_| ()).map_err(Refusal::from),
            smuggler: smuggler.map(|_| ()).map_err(Refusal::from),
        })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Harbor {
    type Sends = ();

    fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What reaches the harbor that spawns it, at its typed path.
#[derive(Plain)]
struct Pilot;

/// What reaches the lighthouse, at its typed path.
#[derive(Plain)]
struct Smuggler;

/// What a child of the harbor is: one that declares a send at a typed path as it begins.
macro_rules! harbored {
    ($name:ident, $named:literal, $reaches:ty) => {
        impl Kind for $name {
            const NAME: KindName = KindName::new($named);
        }

        impl ChildOf for $name {
            type Parent = Harbor;
        }

        impl Actor for $name {
            type Config = Self;
            type Key = ();
            type Handles = ();
            type BeginSends = (Tells<Path<$reaches>, Note>,);
            type EndSends = ();

            fn begin(child: Self, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
                Ok(child)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }
    };
}

harbored!(Pilot, "aether.test.pilot", Harbor);
harbored!(Smuggler, "aether.test.smuggler", Lighthouse);

#[test]
fn a_child_that_reaches_a_typed_path_of_singletons_is_refused_while_no_actor_is_live_there_but_its_spawner() {
    // Catches a turn's spawn that does not check the paths its child reaches, and one that refuses a child reaching
    // back the actor whose `begin` spawns it, which is not yet in the table (ADR-0022).
    let mut engine = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).ready();
    let Ok(root) = engine.add_root::<Harbor>(&(), Harbor { pilot: Ok(()), smuggler: Ok(()) }) else {
        panic!("the harbor begins");
    };

    let harbor = state::<Harbor>(&engine, root);
    assert_eq!(harbor.pilot, Ok(()));
    assert_eq!(harbor.smuggler, Err(Refusal::Missing(String::from("aether.test.lighthouse"))));
}

#[test]
fn a_setup_makes_each_root_of_an_instanced_type_it_is_handed_at_once() {
    // Catches a setup that makes only the first of the roots it is handed together, or none.
    let made =
        Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).add_roots::<Listener>([(0, Listener), (1, Listener)]);
    let Ok(setup) = made else {
        panic!("the listeners begin");
    };
    let mut engine = setup.ready();

    assert_eq!(engine.add_root::<Listener>(&0, Listener), Err(Refused::KeyTaken));
    assert_eq!(engine.add_root::<Listener>(&1, Listener), Err(Refused::KeyTaken));
}

/// A root that, as it begins, spawns a squad beneath it, which tells it of its parent, and a scout and a cell, which
/// may stand anywhere and beneath their own type, and takes what each tells it.
#[derive(Plain)]
#[kind(name = "aether.test.barracks")]
struct Barracks;

impl ChildOf for Barracks {
    type Parent = Top;
}

impl Actor for Barracks {
    type Config = Self;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = (ToType<Squad, Note>,);
    type EndSends = ();

    fn begin(barracks: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let squad: Result<ActorRef<Squad>, _> = turn.spawn(&(), Squad);
        let scout: Result<ActorRef<Scout>, _> = turn.spawn(&(), Scout { is_root: false });
        let (Ok(squad), Ok(_)) = (squad, scout) else {
            return Err(Fault::new(&said(Word::Unborn))?);
        };
        turn.to(squad).send(&said(Word::Report))?;

        Ok(barracks)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Barracks {
    type Sends = ();

    fn handle(&mut self, _: Note, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A child of the barracks that, as it begins and when it is told to report, tells its parent it stands beneath it.
#[derive(Plain)]
#[kind(name = "aether.test.squad")]
struct Squad;

impl ChildOf for Squad {
    type Parent = Barracks;
}

impl Actor for Squad {
    type Config = Self;
    type Key = ();
    type Handles = (Note,);
    type BeginSends = (ToType<Barracks, Note>,);
    type EndSends = ();

    fn begin(squad: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(turn.parent()).send(&said(Word::Squad))?;

        Ok(squad)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Note> for Squad {
    type Sends = (ToType<Barracks, Note>,);

    fn handle(&mut self, note: Note, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(turn.parent()).send(&said(Word::Squad).and(note))?;

        Ok(())
    }
}

/// An actor that may stand anywhere: as it begins, it tells its parent, when it has one, that it stands beneath it, and
/// keeps whether it is a root.
#[derive(Plain)]
#[kind(name = "aether.test.scout")]
struct Scout {
    /// Whether it began with no parent: as a config, false.
    is_root: bool,
}

impl ChildOf for Scout {
    type Parent = Anywhere;
}

impl Actor for Scout {
    type Config = Self;
    type Key = ();
    type Handles = ();
    type BeginSends = (ToAny<(Note,), Note>,);
    type EndSends = ();

    fn begin(_: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let parent = turn.parent().and_then(|parent| turn.cast::<(Note,)>(parent));
        let is_root = parent.is_none();
        if let Some(parent) = parent {
            turn.to(parent).send(&said(Word::Scout))?;
        }

        Ok(Self { is_root })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// An actor of a tree of its own type: one of depth zero, a root, spawns one beneath it, and each keeps whether it has
/// a parent of its own type.
#[derive(Plain)]
#[kind(name = "aether.test.cell")]
struct Cell {
    depth: u64,
    /// Whether it began with a parent: as a config, false.
    has_parent: bool,
}

impl ChildOf for Cell {
    type Parent = Recursive;
}

impl Actor for Cell {
    type Config = Self;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(cell: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let parent: Option<ActorRef<Self>> = turn.parent();
        if cell.depth == 0 {
            let child: Result<ActorRef<Self>, _> = turn.spawn(&(), Self { depth: 1, has_parent: false });
            if child.is_err() {
                return Err(Fault::new(&said(Word::Unborn))?);
            }
        }

        Ok(Self { has_parent: parent.is_some(), ..cell })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

#[test]
fn a_turn_is_handed_its_parent_typed_by_where_its_type_stands() {
    // Catches a parent that is not the actor a child was spawned beneath, a root of a type that may stand anywhere or
    // beneath its own handed a parent it does not have, and one beneath its own type handed none (ADR-0022).
    let made = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0)
        .add_root::<Barracks>(&(), Barracks)
        .and_then(|setup| setup.add_root::<Scout>(&(), Scout { is_root: false }))
        .and_then(|setup| setup.add_root::<Cell>(&(), Cell { depth: 0, has_parent: false }));
    let Ok(setup) = made else {
        panic!("the roots begin");
    };
    let mut engine = setup.ready();
    engine.record_turns();
    for _ in 0..2 {
        engine.step(Time::ZERO, &Stopped);
    }

    // Every actor here has no key, so only the path says a note reached the barracks and not the squad itself.
    let turns = engine.take_turned();
    let at_barracks = |turn: &&Turned| turn.path.last().kind() == &Barracks::NAME;
    let heard: Vec<Seen> = turns.iter().filter(at_barracks).flat_map(Seen::of).collect();
    let squad = said(Word::Squad);
    let reported = [got(0, said(Word::Scout)), got(0, squad.clone().and(said(Word::Report))), got(0, squad)];
    assert_eq!(sorted(heard), reported);
    assert!(state::<Scout>(&engine, root_of::<Scout>()).is_root);
    let scout = child_id(root_of::<Barracks>(), &PathSegment::new(Scout::NAME, &()));
    assert!(!state::<Scout>(&engine, scout).is_root);
    assert!(!state::<Cell>(&engine, root_of::<Cell>()).has_parent);
    let cell = child_id(root_of::<Cell>(), &PathSegment::new(Cell::NAME, &()));
    assert!(state::<Cell>(&engine, cell).has_parent);
}
