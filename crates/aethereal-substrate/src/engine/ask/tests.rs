//! Requests as a step's turns ask them, and answers as the engine hands them back with what the asker kept
//! (ADR-0017).

use super::super::*;
use super::{Answering, Waiting, owed};
use aethereal_actor::Time;
use aethereal_actor::Written;
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asking, Asks, BlobRef, Carries, ChildOf, Ending, Fault, Handles, Held,
    Protocol, Tie, ToSender, ToType, Top, Turn, Unanswered, child,
};
use aethereal_data::{Bound, EncodeError, Plain};
use std::cell::RefCell;
use std::iter;
use std::mem;

thread_local! {
    /// What this thread's actors have said, in order.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what an actor did.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// What has been said since this was last asked.
fn said() -> Vec<String> {
    SAID.with_borrow_mut(mem::take)
}

/// Which tile is at `x`: what a board is asked.
#[derive(Plain)]
#[kind(name = "aether.test.which_tile")]
struct WhichTile {
    x: u64,
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

/// A board: it answers which tile is at `x` with ten times `x`, but at `0` fails, at `1` answers nothing, at `2`
/// answers and closes, at `6` holds the request, at `7` answers twice, and from `100` replies `x - 100` times with `x`
/// itself. What it holds it answers with `50`, lets go of, or closes on, as a [`Release`] says, and its closing turn
/// does with what it still holds what the release it closed on says.
#[derive(Bound)]
#[kind(name = "aether.test.board")]
struct Board {
    held: Vec<Held<TileIs>>,
    closed_on: u64,
}

/// What a board is made from: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.blank")]
struct Blank;

/// What a board does with what it holds: `0` answers it, `1` lets go of it, `2` closes on it, and `3` answers it
/// twice, through a copy read back from bytes it was written into. From `4` it closes on it, and its closing turn
/// answers it with `60`: and at `5` answers it again through a copy, and at `6` fails after answering.
#[derive(Plain)]
#[kind(name = "aether.test.release")]
struct Release {
    how: u64,
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
        Ok(Self { held: Vec::new(), closed_on: 0 })
    }

    fn end(self, turn: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        let answers = self.closed_on >= 4;
        if !answers {
            return Ok(());
        }
        for held in self.held {
            let copy = Written::of(&held)?.read(64)?;
            turn.answer(held, &TileIs { tile: 60 })?;
            match self.closed_on {
                5 => turn.answer(copy, &TileIs { tile: 61 })?,
                6 => return Err(Fault::new(&Release { how: 6 })?),
                _ => drop(copy),
            }
        }

        Ok(())
    }
}

impl Handles<WhichTile> for Board {
    type Sends = (ToSender<TileIs>,);

    fn handle(&mut self, which: WhichTile, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        match which.x {
            0 => return Err(Fault::new(&which)?),
            1 => return Ok(()),
            2 => turn.close(),
            6 => {
                self.held.push(turn.hold()?);
                let again = turn.reply(&TileIs { tile: 0 });
                say(format!("held {}", again == Err(EncodeError::Answered)));
                return Ok(());
            }
            100.. => {
                for _ in 100..which.x {
                    turn.reply(&TileIs { tile: which.x })?;
                }
                return Ok(());
            }
            _ => {}
        }
        turn.reply(&TileIs { tile: which.x * 10 })?;
        let twice = which.x == 7;
        if twice {
            let again = turn.reply(&TileIs { tile: 0 });
            say(format!("again {}", again == Err(EncodeError::Answered)));
        }

        Ok(())
    }
}

impl Handles<Release> for Board {
    type Sends = ();

    fn handle(&mut self, release: Release, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let held = mem::take(&mut self.held);
        match release.how {
            0 => {
                for held in held {
                    turn.answer(held, &TileIs { tile: 50 })?;
                }
            }
            1 => drop(held),
            3 => {
                for held in held {
                    let copy = Written::of(&held)?.read(64)?;
                    turn.answer(held, &TileIs { tile: 50 })?;
                    turn.answer(copy, &TileIs { tile: 51 })?;
                }
            }
            how => {
                self.held = held;
                self.closed_on = how;
                turn.close();
            }
        }

        Ok(())
    }
}

/// An order a pointer sends itself.
#[derive(Plain)]
#[kind(name = "aether.test.pointer_order")]
struct Order {
    text: String,
}

/// A pointer's orders, which it sends itself as it begins.
#[derive(Plain)]
#[kind(name = "aether.test.pointer_config")]
struct PointerConfig {
    orders: Vec<String>,
}

/// An actor that asks a board it spawns as it begins, so its requests may be fused to it, as its orders say: `ask X…`
/// asks which tile is at each `X`, keeping its place among them, `tell X…` sends the question as plain mail, `both X
/// Y` asks at `X` and then names the board to send it the question at `Y` as plain mail, `again X` asks at `X`
/// keeping `9`, `seek X` and `fail X` spawn a seeker that asks at `X`, and `later ORDER` sends itself `ORDER`. Its
/// `begin` asks at `X` itself for `first X`, keeping `100`, and spawns the seeker for `begin seek X`. It says each
/// answer with what it kept, each tile it was told, and each seeker refused.
#[derive(Bound)]
#[kind(name = "aether.test.pointer")]
struct Pointer {
    board: ActorRef<Board>,
}

impl ChildOf for Pointer {
    type Parent = Top;
}

impl Actor for Pointer {
    type Config = PointerConfig;
    type Key = ();
    type Handles = (Order, Answer<TileIs, Looking>, TileIs);
    type BeginSends = (ToType<Self, Order>, Asks<Tiles, WhichTile, Looking>);
    type EndSends = ();

    fn begin(config: PointerConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(board) = turn.spawn(&1, Blank) else {
            return Err(Fault::new(&Order { text: String::from("no board") })?);
        };
        for text in config.orders {
            if let Some(x) = text.strip_prefix("first ") {
                let x = x.parse().unwrap_or_default();
                turn.to(board.protocol()).ask(&WhichTile { x }, &Looking { at: 100 })?;
            } else if let Some(x) = text.strip_prefix("begin seek ") {
                let x = x.parse().unwrap_or_default();
                let spawned: Result<ActorRef<Seeker>, _> = turn.spawn(&x, Seeking { x, fails: 0 });
                say(format!("seeker {}", spawned.is_ok()));
            } else {
                turn.to(turn.me()).send(&Order { text })?;
            }
        }

        Ok(Self { board })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Order> for Pointer {
    type Sends =
        (Asks<Tiles, WhichTile, Looking>, ToType<Board, WhichTile>, ToType<Board, Release>, ToType<Self, Order>);

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let (verb, rest) = order.text.split_once(' ').unwrap_or((&order.text, ""));
        let xs: Vec<u64> = rest.split(' ').map(|word| word.parse().unwrap_or_default()).collect();
        let board = self.board;
        match verb {
            "again" => {
                let x = xs.first().copied().unwrap_or_default();
                turn.to(board.protocol()).ask(&WhichTile { x }, &Looking { at: 9 })?;
            }
            "release" => {
                let how = xs.first().copied().unwrap_or_default();
                turn.to(board).send(&Release { how })?;
            }
            "later" => {
                turn.to(turn.me()).send(&Order { text: String::from(rest) })?;
            }
            "ask" => {
                for (at, x) in (0..).zip(xs) {
                    turn.to(board.protocol()).ask(&WhichTile { x }, &Looking { at })?;
                }
            }
            "tell" => {
                for x in xs {
                    turn.to(board).send(&WhichTile { x })?;
                }
            }
            "seek" | "fail" => {
                let x = xs.first().copied().unwrap_or_default();
                let fails = u64::from(verb == "fail");
                let spawned = turn.spawn::<Seeker>(&x, Seeking { x, fails });
                say(format!("seeker {}", spawned.is_ok()));
            }
            "both" => {
                let (asked, told) = (xs.first().copied().unwrap_or_default(), xs.last().copied().unwrap_or_default());
                turn.to(board.protocol()).ask(&WhichTile { x: asked }, &Looking { at: 0 })?;
                turn.to(child::<Board>(1)).send(&WhichTile { x: told })?;
            }
            _ => {}
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
        match answered {
            Answered::Yes(tile) => say(format!("{} for {}", tile.tile, looking.at)),
            Answered::No(why) => say(format!("{why:?} for {}", looking.at)),
        }

        Ok(())
    }
}

impl Handles<TileIs> for Pointer {
    type Sends = ();

    fn handle(&mut self, tile: TileIs, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        say(format!("told {}", tile.tile));

        Ok(())
    }
}

/// Where a seeker asks, and whether its `begin` fails once it has asked.
#[derive(Plain)]
#[kind(name = "aether.test.seeking")]
struct Seeking {
    x: u64,
    fails: u64,
}

/// A child whose `begin` spawns a board of its own and asks it which tile is at `x`, keeping `200`, and then fails if
/// it was told to: it says each answer with what it kept.
#[derive(Bound)]
#[kind(name = "aether.test.seeker")]
struct Seeker;

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
        turn.to(board.protocol()).ask(&WhichTile { x: seeking.x }, &Looking { at: 200 })?;
        let fails = seeking.fails != 0;
        if fails {
            return Err(Fault::new(&seeking)?);
        }

        Ok(Self)
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
        match answered {
            Answered::Yes(tile) => say(format!("seeker heard {} for {}", tile.tile, looking.at)),
            Answered::No(why) => say(format!("seeker heard {why:?} for {}", looking.at)),
        }

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
/// else, and says how many bytes the blob holds once it is answered.
#[derive(Bound)]
#[kind(name = "aether.test.keeper")]
struct Keeper {
    board: ActorRef<Board>,
}

impl ChildOf for Keeper {
    type Parent = Top;
}

impl Actor for Keeper {
    type Config = Blank;
    type Key = ();
    type Handles = (Order, Answer<TileIs, Kept>);
    type BeginSends = (ToType<Self, Order>,);
    type EndSends = ();

    fn begin(_: Blank, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let Ok(board) = turn.spawn(&1, Blank) else {
            return Err(Fault::new(&Order { text: String::from("no board") })?);
        };
        turn.to(turn.me()).send(&Order { text: String::from("ask") })?;

        Ok(Self { board })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Order> for Keeper {
    type Sends = (Asks<Tiles, WhichTile, Kept>,);

    fn handle(&mut self, _: Order, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Ok(blob) = turn.put(vec![9; 3]) else {
            return Err(Fault::new(&Order { text: String::from("no room") })?);
        };
        let board = self.board;
        turn.to(board.protocol()).ask(&WhichTile { x: 3 }, &Kept { blob })?;

        Ok(())
    }
}

impl Handles<Answer<TileIs, Kept>> for Keeper {
    type Sends = ();

    fn handle(&mut self, (_, kept): (Answered<TileIs>, Kept), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        say(format!("kept {}", turn.read(&kept.blob).len()));

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

/// What a pointer that follows `orders` hears, in an engine in `parts` parts that each run at most `hops` fused hops a
/// step.
fn heard((parts, hops): (usize, usize), orders: &[&str]) -> Vec<String> {
    let Some(parts) = NonZeroUsize::new(parts) else {
        panic!("an engine has parts");
    };
    let mut engine = Engine::new(parts, 1024, 1 << 20, 1 << 20, hops).ready();
    let orders = orders.iter().map(|text| String::from(*text)).collect();
    let made = engine.add_root::<Pointer>(&(), PointerConfig { orders });
    assert!(made.is_ok(), "the pointer begins");
    said();
    for _ in 0..16 {
        engine.step(Time::ZERO, &Stopped);
    }

    said()
}

#[test]
fn each_answer_reaches_its_asker_with_what_it_kept_for_that_request() {
    // Catches answers matched to the wrong request where two asks of one turn are alike, as root and depth would
    // leave them; a context lost or handed back twice; and an answer fused back within the step that finds nothing kept.
    for engine in ENGINES {
        let heard = heard(engine, &["ask 3 3 5"]);

        assert_eq!(heard, ["30 for 0", "30 for 1", "50 for 2"], "{engine:?}");
    }
}

#[test]
fn a_request_is_answered_once() {
    // Catches a second reply to a request taken as a second answer, or sent on as mail the asker cannot place.
    for engine in ENGINES {
        let heard = heard(engine, &["ask 7"]);

        assert_eq!(heard, ["again true", "70 for 0"], "{engine:?}");
    }
}

#[test]
fn a_reply_to_plain_mail_stays_plain_mail() {
    // Catches a reply to mail that asked nothing tied as an answer, which its sender would drop as answering nothing
    // it asked.
    for engine in ENGINES {
        let heard = heard(engine, &["tell 4"]);

        assert_eq!(heard, ["told 40"], "{engine:?}");
    }
}

#[test]
fn plain_mail_sent_straight_after_a_request_stays_apart_from_it() {
    // Catches plain mail of the request's kind to the same receiver joined into the request's run, which would make it
    // a second request tied to the first's count.
    for engine in ENGINES {
        let heard = heard(engine, &["both 4 5"]);

        assert_eq!(heard, ["40 for 0", "told 50"], "{engine:?}");
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
        let heard = heard(engine, &["ask 0"]);

        assert_eq!(heard, [format!("{:?} for 0", Unanswered::Failed)], "{engine:?}");
    }
}

#[test]
fn a_request_whose_turn_does_not_reply_is_answered_that_it_was_not() {
    // As a hop as well, where the pointer and its board fuse.
    for engine in ENGINES {
        let failed = heard(engine, &["ask 4", "later again 0"]);
        assert_eq!(failed, ["40 for 0".to_owned(), format!("{:?} for 9", Unanswered::Failed)], "{engine:?}");
        let unreplied = heard(engine, &["ask 4", "later again 1"]);
        assert_eq!(unreplied, ["40 for 0".to_owned(), format!("{:?} for 9", Unanswered::Unreplied)], "{engine:?}");
    }
    // Catches a turn that replies nothing leaving its request unanswered; and the engine's answer counted out of
    // order with what the receiver sends next, which would hand the asker the later answer first.
    for engine in ENGINES {
        let heard = heard(engine, &["ask 1 4"]);

        assert_eq!(heard, [format!("{:?} for 0", Unanswered::Unreplied), String::from("40 for 1")], "{engine:?}");
    }
}

#[test]
fn a_request_to_an_actor_that_ends_before_it_is_handed_is_answered_that_it_ended() {
    // Catches a request dropped unanswered where it was not handed to its receiver: behind the turn that ended it in
    // the same step, as a hop or mail to an actor that is ending, or as mail to one that has gone.
    let ended = format!("{:?}", Unanswered::Ended);
    for engine in ENGINES {
        assert_eq!(heard(engine, &["ask 2 3"]), ["20 for 0".to_owned(), format!("{ended} for 1")], "{engine:?}");
        let failed = heard(engine, &["ask 0 3"]);
        assert_eq!(failed, [format!("{:?} for 0", Unanswered::Failed), format!("{ended} for 1")], "{engine:?}");
        let hopped = heard(engine, &["ask 2", "again 3"]);
        assert_eq!(hopped, ["20 for 0".to_owned(), format!("{ended} for 9")], "{engine:?}");
        let ending = heard(engine, &["ask 2", "later again 3"]);
        assert_eq!(ending, ["20 for 0".to_owned(), format!("{ended} for 9")], "{engine:?}");
        let gone = heard(engine, &["ask 2", "later later later again 3"]);
        assert_eq!(gone, ["20 for 0".to_owned(), format!("{ended} for 9")], "{engine:?}");
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
    let told = String::from("told 110");
    for engine in ENGINES {
        let heard = heard(engine, &["ask 4", "later tell 110", "later again 2", "later again 3"]);

        let mut expected = vec!["40 for 0".to_owned()];
        expected.extend(vec![told.clone(); 10]);
        expected.extend(["20 for 9".to_owned(), format!("{:?} for 9", Unanswered::Ended)]);
        assert_eq!(heard, expected, "{engine:?}");
    }
}

#[test]
fn a_request_asked_in_a_begin_is_answered_to_the_actor_that_asked_it() {
    // Catches a request a `begin` asked filed beside no one, or beside the actor whose turn stood the asker up, which
    // would hand the answer to an actor that never asked or to none: for a root's own `begin`, a child its `begin`
    // stood up, and a child a later turn stood up. What the root's `begin` says is said before the pointer listens.
    for engine in ENGINES {
        let mut heard = heard(engine, &["first 3", "begin seek 4", "seek 5"]);
        heard.sort();

        let expected = ["30 for 100", "seeker heard 40 for 200", "seeker heard 50 for 200", "seeker true"];
        assert_eq!(heard, expected, "{engine:?}");
    }
}

#[test]
fn a_request_asked_in_a_begin_that_fails_is_never_asked() {
    // Catches the request of a `begin` that failed kept once its mail is dropped: filed beside the actor whose turn
    // stood it up, or left over for the next turn, which the engine checks it never is.
    for engine in ENGINES {
        let heard = heard(engine, &["fail 4", "later ask 3"]);

        assert_eq!(heard, ["seeker false", "30 for 0"], "{engine:?}");
    }
}

#[test]
fn a_held_request_is_answered_in_a_later_turn() {
    // Catches a held request answered as if it were not, or not at all: the reply in the turn that held it refused,
    // and the answer from a later turn handed to the asker with what it kept.
    for engine in ENGINES {
        let heard = heard(engine, &["ask 6", "later release 0"]);

        assert_eq!(heard, ["held true", "50 for 0"], "{engine:?}");
    }
}

#[test]
fn a_held_request_let_go_of_is_answered_that_it_was_not() {
    // Catches a dropped held request left unanswered until its holder ends, which would keep its asker waiting for as
    // long as the holder lives.
    for engine in ENGINES {
        let heard = heard(engine, &["ask 6", "later release 1"]);

        assert_eq!(heard, ["held true".to_owned(), format!("{:?} for 0", Unanswered::Unreplied)], "{engine:?}");
    }
}

#[test]
fn a_held_request_whose_holder_ends_is_answered_that_it_ended() {
    // Catches a request its holder still held as it ended left unanswered, or answered twice: once that it ended and
    // once more as its held request is dropped with the holder.
    for engine in ENGINES {
        let heard = heard(engine, &["ask 6", "later release 2"]);

        assert_eq!(heard, ["held true".to_owned(), format!("{:?} for 0", Unanswered::Ended)], "{engine:?}");
    }
}

#[test]
fn a_held_request_is_answered_in_its_holder_s_closing_turn() {
    // Catches a closing turn's answer dropped or never filed: the asker would be told the holder ended owing it, after
    // or instead of the answer it was given.
    for engine in ENGINES {
        let heard = heard(engine, &["ask 6", "later release 4"]);

        assert_eq!(heard, ["held true", "60 for 0"], "{engine:?}");
    }
}

#[test]
fn a_closing_turn_that_answers_what_it_does_not_hold_or_fails_after_answering_is_answered_that_it_ended() {
    // Catches a closing turn's answer to a debt not held sent as if it were owed, and the answers of a closing turn
    // that failed sent or filed: the asker is told only that its holder ended owing it.
    for engine in ENGINES {
        for release in ["later release 5", "later release 6"] {
            let heard = heard(engine, &["ask 6", release]);

            assert_eq!(
                heard,
                ["held true".to_owned(), format!("{:?} for 0", Unanswered::Ended)],
                "{engine:?} {release}"
            );
        }
    }
}

#[test]
fn a_turn_that_answers_a_request_it_does_not_hold_fails() {
    // Catches a second answer, through a copy of a held request read back from bytes, sent as if it were owed: the
    // turn fails instead, what it sent is dropped, and the asker is told the holder ended still owing it.
    for engine in ENGINES {
        let heard = heard(engine, &["ask 6", "later release 3"]);

        assert_eq!(heard, ["held true".to_owned(), format!("{:?} for 0", Unanswered::Ended)], "{engine:?}");
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
        let made = engine.add_root::<Keeper>(&(), Blank);
        assert!(made.is_ok(), "the keeper begins");
        said();
        for _ in 0..16 {
            engine.step(Time::ZERO, &Stopped);
        }

        assert_eq!(said(), ["kept 3"], "{:?}", (parts, hops));
    }
}
