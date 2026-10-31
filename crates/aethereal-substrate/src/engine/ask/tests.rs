//! Requests as a step's turns ask them, and answers as the engine hands them back with what the asker kept
//! (ADR-0017).

use super::super::*;
use super::{Answering, Waiting, owed};
use aethereal_actor::Time;
use aethereal_actor::Written;
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asking, Asks, BlobRef, Carries, ChildOf, Ending, Fault, Handles, Held,
    Protocol, Tie, ToAny, ToSender, ToType, Top, Turn, Unanswered, child,
};
use aethereal_data::{Bound, EncodeError, Kind, Plain};
use std::iter;
use std::mem;

/// What became of a request, as the actor that asked it keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Heard {
    /// It was answered with `tile`, and what was kept for it is `at`.
    Tile { tile: u64, at: u64 },
    /// It went unanswered for `why`, and what was kept for it is `at`.
    No { why: Unanswered, at: u64 },
}

/// An answer of `tile` to the request that kept `at`.
const fn tile(tile: u64, at: u64) -> Heard {
    Heard::Tile { tile, at }
}

/// The engine's word that the request that kept `at` went unanswered for `why`.
const fn no(why: Unanswered, at: u64) -> Heard {
    Heard::No { why, at }
}

/// What a request's or a reply's receiver answered with, kept as a [`Heard`].
fn heard_of(answered: Answered<TileIs>, looking: &Looking) -> Heard {
    match answered {
        Answered::Yes(answer) => tile(answer.tile, looking.at),
        Answered::No(why) => no(why, looking.at),
    }
}

/// Whether a reply in the turn that held a request was refused as answered, told to the asker: a board that closes on
/// what it holds may go within the step, as a hop, so a read of its state between steps would miss it.
#[derive(Plain)]
#[kind(name = "aether.test.held_reply")]
struct HeldReply {
    refused: bool,
}

/// Where a board is asked about, which says what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Spot {
    /// A plain place: it answers with ten times this.
    At(u64),
    /// It fails its turn.
    Fails,
    /// It answers nothing.
    StaysSilent,
    /// It closes, and answers with [`CLOSING_TILE`].
    Closes,
    /// It holds the request, and tries to answer it in the same turn.
    HoldsThenReplies,
    /// It answers with [`TWICE_TILE`], and tries to answer again.
    AnswersTwice,
    /// It replies `times` times with `tile`.
    Repeats { times: u64, tile: u64 },
}

/// What a board answers at [`Spot::Closes`].
const CLOSING_TILE: u64 = 20;

/// What a board answers at [`Spot::AnswersTwice`].
const TWICE_TILE: u64 = 70;

/// A plain place of a board.
const fn at(x: u64) -> Spot {
    Spot::At(x)
}

/// Which tile is at `spot`: what a board is asked.
#[derive(Plain)]
#[kind(name = "aether.test.which_tile")]
struct WhichTile {
    spot: Spot,
}

/// The tile a board answers with.
#[derive(Plain)]
#[kind(name = "aether.test.tile_is")]
struct TileIs {
    tile: u64,
}

/// What an asker keeps until its question is answered: which of its questions it was.
#[derive(Plain)]
#[kind(name = "aether.test.looking")]
struct Looking {
    at: u64,
}

/// The protocol a board is asked through: which tile, answered with the tile.
struct Tiles;

impl Protocol for Tiles {
    type Kinds = (WhichTile,);
}

impl Carries<WhichTile> for Tiles {
    type Answers = (TileIs,);
}

/// A board: it answers which tile is at a [`Spot`] as the spot says. What it holds it answers with `50`, lets go of, or
/// closes on, as a [`Release`] says, and its closing turn does with what it still holds what the release it closed on
/// says.
#[derive(Bound)]
#[kind(name = "aether.test.board")]
struct Board {
    held: Vec<Held<TileIs>>,
    closing: Closing,
    /// Whether each second reply to a request it had answered was refused, in order.
    again: Vec<bool>,
}

/// What a board is made from: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.blank")]
struct Blank;

/// What a board's closing turn does with what it still holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Closing {
    /// Nothing.
    Silently,
    /// It answers it with `60`.
    Answering,
    /// It answers it with `60`, and again through a copy read back from bytes it was written into.
    AnsweringTwice,
    /// It answers it with `60`, and then fails.
    AnsweringThenFailing,
}

/// What a board does with what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.release")]
enum Release {
    /// It answers it.
    Answers,
    /// It lets go of it.
    LetsGo,
    /// It answers it twice, through a copy read back from bytes it was written into.
    AnswersTwiceThroughACopy,
    /// It closes on it, and its closing turn does as this says.
    ClosesOnIt(Closing),
}

impl ChildOf for Board {
    type Parent = Anywhere;
}

impl Actor for Board {
    type Config = Blank;
    type Key = u64;
    type Handles = (WhichTile, Release);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Blank, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { held: Vec::new(), closing: Closing::Silently, again: Vec::new() })
    }

    fn end(self, turn: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        let answers = self.closing != Closing::Silently;
        if !answers {
            return Ok(());
        }
        for held in self.held {
            let copy = Written::of(&held)?.read(64)?;
            turn.answer(held, &TileIs { tile: 60 })?;
            match self.closing {
                Closing::AnsweringTwice => turn.answer(copy, &TileIs { tile: 61 })?,
                Closing::AnsweringThenFailing => return Err(Fault::new(&Release::ClosesOnIt(self.closing))?),
                Closing::Silently | Closing::Answering => drop(copy),
            }
        }

        Ok(())
    }
}

impl Handles<WhichTile> for Board {
    type Sends = (ToSender<TileIs>, ToAny<(HeldReply,), HeldReply>);

    fn handle(&mut self, which: WhichTile, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let tile = match which.spot {
            Spot::At(x) => x * 10,
            Spot::Fails => return Err(Fault::new(&which)?),
            Spot::StaysSilent => return Ok(()),
            Spot::Closes => {
                turn.close();

                CLOSING_TILE
            }
            Spot::HoldsThenReplies => {
                self.held.push(turn.hold()?);
                let again = turn.reply(&TileIs { tile: 0 });
                let refused = again == Err(EncodeError::Answered);
                if let Some(asker) = turn.cast(turn.sender()) {
                    turn.to(asker).send(&HeldReply { refused })?;
                }
                return Ok(());
            }
            Spot::AnswersTwice => {
                turn.reply(&TileIs { tile: TWICE_TILE })?;
                let again = turn.reply(&TileIs { tile: 0 });
                self.again.push(again == Err(EncodeError::Answered));
                return Ok(());
            }
            Spot::Repeats { times, tile } => {
                for _ in 0..times {
                    turn.reply(&TileIs { tile })?;
                }
                return Ok(());
            }
        };
        turn.reply(&TileIs { tile })?;

        Ok(())
    }
}

impl Handles<Release> for Board {
    type Sends = ();

    fn handle(&mut self, release: Release, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let held = mem::take(&mut self.held);
        match release {
            Release::Answers => {
                for held in held {
                    turn.answer(held, &TileIs { tile: 50 })?;
                }
            }
            Release::LetsGo => drop(held),
            Release::AnswersTwiceThroughACopy => {
                for held in held {
                    let copy = Written::of(&held)?.read(64)?;
                    turn.answer(held, &TileIs { tile: 50 })?;
                    turn.answer(copy, &TileIs { tile: 51 })?;
                }
            }
            Release::ClosesOnIt(closing) => {
                self.held = held;
                self.closing = closing;
                turn.close();
            }
        }

        Ok(())
    }
}

/// What one order of [`Orders`] has a pointer do. The orders after the one acted on are its rest: a type that holds
/// itself does not encode, so a pointer's orders are a list.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Order {
    /// Asks which tile is at each of these, keeping its place among them.
    Ask(Vec<Spot>),
    /// Sends the question at each of these as plain mail.
    Tell(Vec<Spot>),
    /// Asks at `asked`, keeping its place, and then names the board to send it the question at `told` as plain mail.
    Both { asked: Spot, told: Spot },
    /// Asks at this, keeping `9`.
    Again(Spot),
    /// Sends its board this release.
    Release(Release),
    /// Sends itself the rest, for the next step.
    Later,
    /// Spawns a seeker that asks at this.
    Seek(u64),
    /// Spawns a seeker that asks at this and fails in its `begin`.
    SeekAndFail(u64),
    /// In its `begin`: asks at this itself, keeping `100`.
    FirstAsk(Spot),
    /// In its `begin`: spawns a seeker that asks at this.
    FirstSeek(u64),
}

/// What a pointer sends itself: orders, the first of which it acts on.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.pointer_order")]
struct Orders {
    orders: Vec<Order>,
}

impl Orders {
    /// The orders of `order` alone.
    fn of(order: Order) -> Self {
        Self { orders: vec![order] }
    }

    /// The order to act on and the rest: none where it has no orders.
    fn parted(&self) -> Option<(&Order, Self)> {
        let (first, rest) = self.orders.split_first()?;

        Some((first, Self { orders: rest.to_vec() }))
    }
}

/// See [`Order::Ask`].
fn ask(spots: &[Spot]) -> Orders {
    Orders::of(Order::Ask(spots.to_vec()))
}

/// See [`Order::Tell`].
fn tell(spots: &[Spot]) -> Orders {
    Orders::of(Order::Tell(spots.to_vec()))
}

/// See [`Order::Both`].
fn both(asked: Spot, told: Spot) -> Orders {
    Orders::of(Order::Both { asked, told })
}

/// See [`Order::Again`].
fn again(spot: Spot) -> Orders {
    Orders::of(Order::Again(spot))
}

/// See [`Order::Release`].
fn release(how: Release) -> Orders {
    Orders::of(Order::Release(how))
}

/// See [`Order::Later`].
fn later(rest: Orders) -> Orders {
    let mut orders = vec![Order::Later];
    orders.extend(rest.orders);

    Orders { orders }
}

/// See [`Order::Seek`].
fn seek(x: u64) -> Orders {
    Orders::of(Order::Seek(x))
}

/// See [`Order::SeekAndFail`].
fn seek_and_fail(x: u64) -> Orders {
    Orders::of(Order::SeekAndFail(x))
}

/// See [`Order::FirstAsk`].
fn first_ask(spot: Spot) -> Orders {
    Orders::of(Order::FirstAsk(spot))
}

/// See [`Order::FirstSeek`].
fn first_seek(x: u64) -> Orders {
    Orders::of(Order::FirstSeek(x))
}

/// Why an actor of these tests could not begin or go on.
#[derive(Plain)]
#[kind(name = "aether.test.ask_trouble")]
enum Trouble {
    NoBoard,
    NoRoom,
}

/// A pointer's orders, which it sends itself as it begins.
#[derive(Plain)]
#[kind(name = "aether.test.pointer_config")]
struct PointerConfig {
    orders: Vec<Orders>,
}

/// An actor that asks a board it spawns as it begins, so its requests may be fused to it, as its [`Order`]s say. It
/// keeps each answer with what it kept, each tile it was told, and whether each seeker stood up.
#[derive(Bound)]
#[kind(name = "aether.test.pointer")]
struct Pointer {
    board: ActorRef<Board>,
    /// What became of each of its requests, in the order it heard.
    heard: Vec<Heard>,
    /// Each tile it was told as plain mail, in order.
    told: Vec<u64>,
    /// Whether each seeker it spawned stood up, in order.
    seekers: Vec<bool>,
    /// Whether each reply in the turn that held its request was refused, in order.
    held: Vec<bool>,
}

impl ChildOf for Pointer {
    type Parent = Top;
}

impl Actor for Pointer {
    type Config = PointerConfig;
    type Key = ();
    type Handles = (Orders, Answer<TileIs, Looking>, TileIs, HeldReply);
    type BeginSends = (ToType<Self, Orders>, Asks<Tiles, WhichTile, Looking>);
    type EndSends = ();

    fn begin(config: PointerConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(board) = turn.spawn(&1, Blank) else {
            return Err(Fault::new(&Trouble::NoBoard)?);
        };
        let mut seekers = Vec::new();
        for orders in config.orders {
            match orders.orders[..] {
                [Order::FirstAsk(spot)] => {
                    turn.to(board.protocol()).ask(&WhichTile { spot }, &Looking { at: 100 })?;
                }
                [Order::FirstSeek(x)] => {
                    let spawned: Result<ActorRef<Seeker>, _> = turn.spawn(&x, Seeking { x, fails: false });
                    seekers.push(spawned.is_ok());
                }
                _ => {
                    turn.to(turn.me()).send(&orders)?;
                }
            }
        }

        Ok(Self { board, heard: Vec::new(), told: Vec::new(), seekers, held: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Orders> for Pointer {
    type Sends =
        (Asks<Tiles, WhichTile, Looking>, ToType<Board, WhichTile>, ToType<Board, Release>, ToType<Self, Orders>);

    fn handle(&mut self, orders: Orders, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Some((order, rest)) = orders.parted() else {
            return Ok(());
        };
        let board = self.board;
        match order {
            Order::Again(spot) => {
                turn.to(board.protocol()).ask(&WhichTile { spot: *spot }, &Looking { at: 9 })?;
            }
            Order::Release(how) => {
                turn.to(board).send(how)?;
            }
            Order::Later => {
                turn.to(turn.me()).send(&rest)?;
            }
            Order::Ask(spots) => {
                for (at, spot) in (0..).zip(spots) {
                    turn.to(board.protocol()).ask(&WhichTile { spot: *spot }, &Looking { at })?;
                }
            }
            Order::Tell(spots) => {
                for spot in spots {
                    turn.to(board).send(&WhichTile { spot: *spot })?;
                }
            }
            Order::Seek(x) => {
                let spawned = turn.spawn::<Seeker>(x, Seeking { x: *x, fails: false });
                self.seekers.push(spawned.is_ok());
            }
            Order::SeekAndFail(x) => {
                let spawned = turn.spawn::<Seeker>(x, Seeking { x: *x, fails: true });
                self.seekers.push(spawned.is_ok());
            }
            Order::Both { asked, told } => {
                turn.to(board.protocol()).ask(&WhichTile { spot: *asked }, &Looking { at: 0 })?;
                turn.to(child::<Board>(1)).send(&WhichTile { spot: *told })?;
            }
            Order::FirstAsk(_) | Order::FirstSeek(_) => {}
        }

        Ok(())
    }
}

impl Handles<Answer<TileIs, Looking>> for Pointer {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, looking): (Answered<TileIs>, Looking),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.heard.push(heard_of(answered, &looking));

        Ok(())
    }
}

impl Handles<TileIs> for Pointer {
    type Sends = ();

    fn handle(&mut self, tile: TileIs, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.told.push(tile.tile);

        Ok(())
    }
}

impl Handles<HeldReply> for Pointer {
    type Sends = ();

    fn handle(&mut self, reply: HeldReply, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.held.push(reply.refused);

        Ok(())
    }
}

/// Where a seeker asks, and whether its `begin` fails once it has asked.
#[derive(Plain)]
#[kind(name = "aether.test.seeking")]
struct Seeking {
    x: u64,
    fails: bool,
}

/// A child whose `begin` spawns a board of its own and asks it which tile is at `x`, keeping `200`, and then fails if
/// it was told to: it keeps each answer with what it kept.
#[derive(Bound)]
#[kind(name = "aether.test.seeker")]
struct Seeker {
    /// What became of its request.
    heard: Vec<Heard>,
}

impl ChildOf for Seeker {
    type Parent = Pointer;
}

impl Actor for Seeker {
    type Config = Seeking;
    type Key = u64;
    type Handles = (Answer<TileIs, Looking>,);
    type BeginSends = (Asks<Tiles, WhichTile, Looking>,);
    type EndSends = ();

    fn begin(seeking: Seeking, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(board) = turn.spawn::<Board>(&2, Blank) else {
            return Err(Fault::new(&seeking)?);
        };
        turn.to(board.protocol()).ask(&WhichTile { spot: at(seeking.x) }, &Looking { at: 200 })?;
        if seeking.fails {
            return Err(Fault::new(&seeking)?);
        }

        Ok(Self { heard: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Answer<TileIs, Looking>> for Seeker {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, looking): (Answered<TileIs>, Looking),
        _: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        self.heard.push(heard_of(answered, &looking));

        Ok(())
    }
}

/// What a keeper keeps until its question is answered: a blob nothing else refers to.
#[derive(Bound)]
#[kind(name = "aether.test.kept")]
struct Kept {
    blob: BlobRef,
}

/// An actor that asks a board it spawns as it begins, keeping a blob of three bytes it puts and refers to nowhere
/// else, and keeps how many bytes the blob holds once it is answered.
#[derive(Bound)]
#[kind(name = "aether.test.keeper")]
struct Keeper {
    board: ActorRef<Board>,
    /// How many bytes the blob held, for each answer.
    kept: Vec<u64>,
}

impl ChildOf for Keeper {
    type Parent = Top;
}

impl Actor for Keeper {
    type Config = Blank;
    type Key = ();
    type Handles = (Orders, Answer<TileIs, Kept>);
    type BeginSends = (ToType<Self, Orders>,);
    type EndSends = ();

    fn begin(_: Blank, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(board) = turn.spawn(&1, Blank) else {
            return Err(Fault::new(&Trouble::NoBoard)?);
        };
        turn.to(turn.me()).send(&ask(&[at(3)]))?;

        Ok(Self { board, kept: Vec::new() })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Orders> for Keeper {
    type Sends = (Asks<Tiles, WhichTile, Kept>,);

    fn handle(&mut self, _: Orders, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Ok(blob) = turn.put(vec![9; 3]) else {
            return Err(Fault::new(&Trouble::NoRoom)?);
        };
        let board = self.board;
        turn.to(board.protocol()).ask(&WhichTile { spot: at(3) }, &Kept { blob })?;

        Ok(())
    }
}

impl Handles<Answer<TileIs, Kept>> for Keeper {
    type Sends = ();

    fn handle(&mut self, (_, kept): (Answered<TileIs>, Kept), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.kept.push(u64::try_from(turn.read(&kept.blob).len()).unwrap_or(u64::MAX));

        Ok(())
    }
}

/// A clock that never moves.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// Each engine the tests run in: how many parts, and how many fused hops each part runs at most in a step. In one part
/// the pointer and its board fuse.
const ENGINES: [(usize, usize); 4] = [(1, 0), (1, 64), (4, 0), (4, 64)];

/// What one run of a pointer came to.
struct Run {
    /// What the pointer heard of its requests, in order.
    heard: Vec<Heard>,
    /// The tiles it was told as plain mail, in order.
    told: Vec<u64>,
    /// Whether each seeker stood up, in order.
    seekers: Vec<bool>,
    /// What each seeker it stood up heard, by the place it asked at.
    sought: Vec<(u64, Vec<Heard>)>,
    /// Whether each reply in the turn that held its request was refused, in order.
    held: Vec<bool>,
    /// Whether each second reply its board tried was refused, for a board live at the end.
    again: Vec<bool>,
}

/// The state of the live actor `id` stands for in `engine`, when one of type `T` sits there.
fn live<T: 'static>(engine: &Engine, id: ActorId) -> Option<&T> {
    let seat = engine.table().seat(id)?;

    engine.state(Seated { id, seat })
}

/// What came of a pointer that follows `orders`, in an engine in `parts` parts that each run at most `hops` fused hops a
/// step, with seekers at each of `seeking`.
fn run((parts, hops): (usize, usize), orders: &[Orders], seeking: &[u64]) -> Run {
    let Some(parts) = NonZeroUsize::new(parts) else {
        panic!("an engine has parts");
    };
    let mut engine = Engine::new(parts, 1024, 1 << 20, 1 << 20, hops).ready();
    let Ok(pointer) = engine.add_root::<Pointer>(&(), PointerConfig { orders: orders.to_vec() }) else {
        panic!("the pointer begins");
    };
    let board = crate::child_id(pointer, &PathSegment::new(Board::NAME, &1u64));

    for _ in 0..16 {
        engine.step(Time::ZERO, &Stopped);
    }

    let sought = |x: &u64| {
        let id = crate::child_id(pointer, &PathSegment::new(Seeker::NAME, x));
        let Some(seeker) = live::<Seeker>(&engine, id) else {
            panic!("the seeker is live");
        };

        (*x, seeker.heard.clone())
    };
    let sought = seeking.iter().map(sought).collect();
    let again = live::<Board>(&engine, board).map_or_else(Vec::new, |board| board.again.clone());
    let Some(pointer) = live::<Pointer>(&engine, pointer) else {
        panic!("the pointer is live");
    };
    let Pointer { heard, told, seekers, held, .. } = pointer;

    Run { heard: heard.clone(), told: told.clone(), seekers: seekers.clone(), sought, held: held.clone(), again }
}

/// What a pointer that follows `orders` hears of its requests.
fn heard(engine: (usize, usize), orders: &[Orders]) -> Vec<Heard> {
    run(engine, orders, &[]).heard
}

#[test]
fn each_answer_reaches_its_asker_with_what_it_kept_for_that_request() {
    // Catches answers matched to the wrong request where two asks of one turn are alike, as root and depth would
    // leave them; a context lost or handed back twice; and an answer fused back within the step that finds nothing kept.
    for engine in ENGINES {
        assert_eq!(
            heard(engine, &[ask(&[at(3), at(3), at(5)])]),
            [tile(30, 0), tile(30, 1), tile(50, 2)],
            "{engine:?}"
        );
    }
}

#[test]
fn a_request_is_answered_once() {
    // Catches a second reply to a request taken as a second answer, or sent on as mail the asker cannot place.
    for engine in ENGINES {
        let run = run(engine, &[ask(&[Spot::AnswersTwice])], &[]);

        assert_eq!(run.again, [true], "{engine:?}");
        assert_eq!(run.heard, [tile(TWICE_TILE, 0)], "{engine:?}");
    }
}

#[test]
fn a_reply_to_plain_mail_stays_plain_mail() {
    // Catches a reply to mail that asked nothing tied as an answer, which its sender would drop as answering nothing
    // it asked.
    for engine in ENGINES {
        let run = run(engine, &[tell(&[at(4)])], &[]);

        assert_eq!(run.told, [40], "{engine:?}");
        assert_eq!(run.heard, [], "{engine:?}");
    }
}

#[test]
fn plain_mail_sent_straight_after_a_request_stays_apart_from_it() {
    // Catches plain mail of the request's kind to the same receiver joined into the request's run, which would make it
    // a second request tied to the first's count.
    for engine in ENGINES {
        let run = run(engine, &[both(at(4), at(5))], &[]);

        assert_eq!(run.heard, [tile(40, 0)], "{engine:?}");
        assert_eq!(run.told, [50], "{engine:?}");
    }
}

/// A context of one byte, `9`.
fn nine() -> Written {
    Written::new(vec![9], Vec::new())
}

#[test]
fn a_context_is_let_go_of_once_its_answer_has_been_handled() {
    // Catches a context kept after its answer's turn, which would stay beside the asker for as long as it lives.
    let mut waiting = Waiting::default();
    let mut asked = Asked::new();
    for answer in [Tie::answers(3), Tie::unanswered(3)] {
        waiting.file(iter::once(Asking {
            count: 3,
            answer: KindId::of::<TileIs>(),
            context: nine(),
            lent: Vec::new(),
        }));
        let answering = waiting.answering(answer);
        let kept =
            matches!(answering, Answering::Kept(asking) | Answering::Unanswered(asking) if asking.context == nine());

        let me = Seated { id: ActorId::new(1), seat: SeatNumber::new(0) };
        waiting.settle(me, answer, &mut asked, &Ok(()));

        assert!(kept, "{answer:?}");
        assert!(matches!(waiting.answering(answer), Answering::Stray), "{answer:?}");
    }
}

#[test]
fn what_the_engine_says_of_an_unanswered_request_is_handed_to_the_handler_of_its_answer() {
    // Catches an unanswered request handed to no handler, or to one that is not its answer's: the engine's mail is of
    // its own kind, and the asker kept which kind it waits for.
    let mut waiting = Waiting::default();
    waiting.file(iter::once(Asking { count: 3, answer: KindId::of::<TileIs>(), context: nine(), lent: Vec::new() }));

    let answering = waiting.answering(Tie::unanswered(3));

    let Answering::Unanswered(asking) = answering else {
        panic!("what the engine says is handed with what the asker kept");
    };
    assert_eq!(asking.answer, KindId::of::<TileIs>());
}

#[test]
fn a_request_whose_turn_fails_is_answered_that_it_failed() {
    // Catches a failed turn's request left unanswered, its asker's context kept for as long as it lives; and the
    // engine's answer dropped with the failed turn's mail.
    for engine in ENGINES {
        assert_eq!(heard(engine, &[ask(&[Spot::Fails])]), [no(Unanswered::Failed, 0)], "{engine:?}");
    }
}

#[test]
fn a_request_whose_turn_does_not_reply_is_answered_that_it_was_not() {
    // As a hop as well, where the pointer and its board fuse.
    for engine in ENGINES {
        let failed = heard(engine, &[ask(&[at(4)]), later(again(Spot::Fails))]);
        assert_eq!(failed, [tile(40, 0), no(Unanswered::Failed, 9)], "{engine:?}");
        let unreplied = heard(engine, &[ask(&[at(4)]), later(again(Spot::StaysSilent))]);
        assert_eq!(unreplied, [tile(40, 0), no(Unanswered::Unreplied, 9)], "{engine:?}");
    }
    // Catches a turn that replies nothing leaving its request unanswered; and the engine's answer counted out of
    // order with what the receiver sends next, which would hand the asker the later answer first.
    for engine in ENGINES {
        assert_eq!(
            heard(engine, &[ask(&[Spot::StaysSilent, at(4)])]),
            [no(Unanswered::Unreplied, 0), tile(40, 1)],
            "{engine:?}"
        );
    }
}

#[test]
fn a_request_to_an_actor_that_ends_before_it_is_handed_is_answered_that_it_ended() {
    // Catches a request dropped unanswered where it was not handed to its receiver: behind the turn that ended it in
    // the same step, as a hop or mail to an actor that is ending, or as mail to one that has gone.
    let ended = Unanswered::Ended;
    for engine in ENGINES {
        assert_eq!(heard(engine, &[ask(&[Spot::Closes, at(3)])]), [tile(CLOSING_TILE, 0), no(ended, 1)], "{engine:?}");
        assert_eq!(
            heard(engine, &[ask(&[Spot::Fails, at(3)])]),
            [no(Unanswered::Failed, 0), no(ended, 1)],
            "{engine:?}"
        );
        assert_eq!(
            heard(engine, &[ask(&[Spot::Closes]), again(at(3))]),
            [tile(CLOSING_TILE, 0), no(ended, 9)],
            "{engine:?}"
        );
        assert_eq!(
            heard(engine, &[ask(&[Spot::Closes]), later(again(at(3)))]),
            [tile(CLOSING_TILE, 0), no(ended, 9)],
            "{engine:?}"
        );
        let gone = heard(engine, &[ask(&[Spot::Closes]), later(later(later(again(at(3)))))]);
        assert_eq!(gone, [tile(CLOSING_TILE, 0), no(ended, 9)], "{engine:?}");
    }
}

#[test]
fn the_engine_answers_after_what_the_turn_sent_and_only_a_request_the_turn_left_unanswered() {
    // Catches a request the turn answered answered again, which its asker would drop unseen as answering nothing it
    // waits on; and the engine's answer counted as one the turn already sent, which would order the two by chance.
    let (me, pointer) = (
        Seated { id: ActorId::new(1), seat: SeatNumber::new(0) },
        Seated { id: ActorId::new(2), seat: SeatNumber::new(1) },
    );
    let mut asked = Asked::new();
    let mut told = asked.unanswered(me, pointer, 5, 3, Unanswered::Unreplied);
    told.tie = Tie::TELL;
    asked.buffers_mut().mailed.push(told);

    owed(me, pointer, Tie::asks(3), &Ok(()), 2, &mut asked);

    let mailed = asked.mailed();
    assert_eq!(mailed.len(), 2, "the engine says the request was not answered");
    assert_eq!((mailed[1].count, mailed[1].tie), (6, Tie::unanswered(3)), "after what the turn sent");

    asked.buffers_mut().mailed[1].tie = Tie::answers(3);
    owed(me, pointer, Tie::asks(3), &Ok(()), 2, &mut asked);

    assert_eq!(asked.mailed().len(), 2, "a request the turn answered is not answered again");
}

#[test]
fn what_the_engine_says_of_a_request_to_an_ended_actor_comes_after_all_that_actor_sent() {
    // Catches the engine's answer ordered by the asker's count among the receiver's own mail, which would hand it
    // before what the receiver sent first, or drop that mail as the same item where the two counts meet.
    for engine in ENGINES {
        let many = Spot::Repeats { times: 10, tile: 110 };
        let run =
            run(engine, &[ask(&[at(4)]), later(tell(&[many])), later(again(Spot::Closes)), later(again(at(3)))], &[]);

        assert_eq!(run.heard, [tile(40, 0), tile(CLOSING_TILE, 9), no(Unanswered::Ended, 9)], "{engine:?}");
        assert_eq!(run.told, [110; 10], "{engine:?}");
    }
}

#[test]
fn a_request_asked_in_a_begin_is_answered_to_the_actor_that_asked_it() {
    // Catches a request a `begin` asked filed beside no one, or beside the actor whose turn stood the asker up, which
    // would hand the answer to an actor that never asked or to none: for a root's own `begin`, a child its `begin`
    // stood up, and a child a later turn stood up.
    for engine in ENGINES {
        let run = run(engine, &[first_ask(at(3)), first_seek(4), seek(5)], &[4, 5]);

        assert_eq!(run.heard, [tile(30, 100)], "{engine:?}");
        assert_eq!(run.seekers, [true, true], "{engine:?}");
        assert_eq!(run.sought, [(4, vec![tile(40, 200)]), (5, vec![tile(50, 200)])], "{engine:?}");
    }
}

#[test]
fn a_request_asked_in_a_begin_that_fails_is_never_asked() {
    // Catches the request of a `begin` that failed kept once its mail is dropped: filed beside the actor whose turn
    // stood it up, or left over for the next turn, which the engine checks it never is.
    for engine in ENGINES {
        let run = run(engine, &[seek_and_fail(4), later(ask(&[at(3)]))], &[]);

        assert_eq!(run.seekers, [false], "{engine:?}");
        assert_eq!(run.heard, [tile(30, 0)], "{engine:?}");
    }
}

#[test]
fn a_held_request_is_answered_in_a_later_turn() {
    // Catches a held request answered as if it were not, or not at all: the reply in the turn that held it refused,
    // and the answer from a later turn handed to the asker with what it kept.
    for engine in ENGINES {
        let run = run(engine, &[ask(&[Spot::HoldsThenReplies]), later(release(Release::Answers))], &[]);

        assert_eq!(run.held, [true], "{engine:?}");
        assert_eq!(run.heard, [tile(50, 0)], "{engine:?}");
    }
}

#[test]
fn a_held_request_let_go_of_is_answered_that_it_was_not() {
    // Catches a dropped held request left unanswered until its holder ends, which would keep its asker waiting for as
    // long as the holder lives.
    for engine in ENGINES {
        let run = run(engine, &[ask(&[Spot::HoldsThenReplies]), later(release(Release::LetsGo))], &[]);

        assert_eq!(run.held, [true], "{engine:?}");
        assert_eq!(run.heard, [no(Unanswered::Unreplied, 0)], "{engine:?}");
    }
}

#[test]
fn a_held_request_whose_holder_ends_is_answered_that_it_ended() {
    // Catches a request its holder still held as it ended left unanswered, or answered twice: once that it ended and
    // once more as its held request is dropped with the holder.
    for engine in ENGINES {
        let run =
            run(engine, &[ask(&[Spot::HoldsThenReplies]), later(release(Release::ClosesOnIt(Closing::Silently)))], &[]);

        assert_eq!(run.held, [true], "{engine:?}");
        assert_eq!(run.heard, [no(Unanswered::Ended, 0)], "{engine:?}");
    }
}

#[test]
fn a_held_request_is_answered_in_its_holder_s_closing_turn() {
    // Catches a closing turn's answer dropped or never filed: the asker would be told the holder ended owing it, after
    // or instead of the answer it was given.
    for engine in ENGINES {
        let run = run(
            engine,
            &[ask(&[Spot::HoldsThenReplies]), later(release(Release::ClosesOnIt(Closing::Answering)))],
            &[],
        );

        assert_eq!(run.held, [true], "{engine:?}");
        assert_eq!(run.heard, [tile(60, 0)], "{engine:?}");
    }
}

#[test]
fn a_closing_turn_that_answers_what_it_does_not_hold_or_fails_after_answering_is_answered_that_it_ended() {
    // Catches a closing turn's answer to a debt not held sent as if it were owed, and the answers of a closing turn
    // that failed sent or filed: the asker is told only that its holder ended owing it.
    for engine in ENGINES {
        for closing in [Closing::AnsweringTwice, Closing::AnsweringThenFailing] {
            let closes = later(release(Release::ClosesOnIt(closing)));
            let run = run(engine, &[ask(&[Spot::HoldsThenReplies]), closes], &[]);

            assert_eq!(run.held, [true], "{engine:?} {closing:?}");
            assert_eq!(run.heard, [no(Unanswered::Ended, 0)], "{engine:?} {closing:?}");
        }
    }
}

#[test]
fn a_turn_that_answers_a_request_it_does_not_hold_fails() {
    // Catches a second answer, through a copy of a held request read back from bytes, sent as if it were owed: the
    // turn fails instead, what it sent is dropped, and the asker is told the holder ended still owing it.
    for engine in ENGINES {
        let run =
            run(engine, &[ask(&[Spot::HoldsThenReplies]), later(release(Release::AnswersTwiceThroughACopy))], &[]);

        assert_eq!(run.held, [true], "{engine:?}");
        assert_eq!(run.heard, [no(Unanswered::Ended, 0)], "{engine:?}");
    }
}

#[test]
fn a_blob_a_context_refers_to_is_kept_until_its_answer_is_handled() {
    // Catches a context that holds no pin of its own, whose blob is freed once the asker lets go of its reference, so
    // the answer's handler reads a blob no longer kept.
    for (parts, hops) in ENGINES {
        let Some(parts) = NonZeroUsize::new(parts) else {
            panic!("an engine has parts");
        };
        let mut engine = Engine::new(parts, 1024, 1 << 20, 1 << 20, hops).ready();
        let Ok(keeper) = engine.add_root::<Keeper>(&(), Blank) else {
            panic!("the keeper begins");
        };
        for _ in 0..16 {
            engine.step(Time::ZERO, &Stopped);
        }

        let Some(keeper) = live::<Keeper>(&engine, keeper) else {
            panic!("the keeper is live");
        };
        assert_eq!(keeper.kept, [3], "{:?}", (parts, hops));
    }
}
