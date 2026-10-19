//! Requests as a step's turns ask them, and answers as the engine hands them back with what the asker kept
//! (ADR-0017).

use super::super::*;
use super::{Answering, Waiting};
use aethereal_actor::{
    Answer, Answered, Asking, Asks, Carries, Ending, Fault, Handles, Protocol, Tie, To, ToSender, ToType,
    ToWithoutMail, Turn,
};
use aethereal_data::{Bound, EncodeError, Kind, KindName, Plain, TypedKey};
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
struct WhichTile {
    x: u64,
}

impl Kind for WhichTile {
    const NAME: KindName = KindName::new("aether.test.which_tile");
}

/// The tile a board answers with.
#[derive(Plain)]
struct TileIs {
    tile: u64,
}

impl Kind for TileIs {
    const NAME: KindName = KindName::new("aether.test.tile_is");
}

/// What an asker keeps until its question is answered: which of its questions it was.
#[derive(Plain)]
struct Looking {
    at: u64,
}

impl Kind for Looking {
    const NAME: KindName = KindName::new("aether.test.looking");
}

/// The protocol a board is asked through: which tile, answered with the tile.
struct Tiles;

impl Protocol for Tiles {
    type Kinds = (WhichTile,);
}

impl Carries<WhichTile> for Tiles {
    type Answers = (TileIs,);
}

/// A board: it answers which tile is at `x` with ten times `x`, and at `7` answers twice.
#[derive(Bound)]
struct Board;

impl Kind for Board {
    const NAME: KindName = KindName::new("aether.test.board");
}

impl Actor for Board {
    type Config = Self;
    type Key = u64;
    type Handles = (WhichTile,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(board: Self, _: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
        Ok(board)
    }

    fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<WhichTile> for Board {
    type Sends = (ToSender<TileIs>,);

    fn handle(&mut self, which: WhichTile, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&TileIs { tile: which.x * 10 })?;
        let twice = which.x == 7;
        if twice {
            let again = turn.reply(&TileIs { tile: 0 });
            say(format!("again {}", again == Err(EncodeError::Answered)));
        }

        Ok(())
    }
}

/// An order a pointer sends itself.
#[derive(Plain)]
struct Order {
    text: String,
}

impl Kind for Order {
    const NAME: KindName = KindName::new("aether.test.pointer_order");
}

/// A pointer's orders, which it sends itself as it begins.
#[derive(Plain)]
struct PointerConfig {
    orders: Vec<String>,
}

impl Kind for PointerConfig {
    const NAME: KindName = KindName::new("aether.test.pointer_config");
}

/// An actor that asks a board it spawns, as its orders say: `ask X…` asks which tile is at each `X`, keeping its place
/// among them, `tell X` sends the question as plain mail, and `both X Y` asks at `X` and then names the board to send
/// it the question at `Y` as plain mail. It says each answer with what it kept, and each tile it
/// was told.
#[derive(Bound)]
struct Pointer;

impl Kind for Pointer {
    const NAME: KindName = KindName::new("aether.test.pointer");
}

impl Actor for Pointer {
    type Config = PointerConfig;
    type Key = ();
    type Handles = (Order, Answer<TileIs, Looking>, TileIs);
    type BeginSends = (ToType<Self, Order>,);
    type EndSends = ();

    fn begin(config: PointerConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        for text in config.orders {
            let me = turn.me();
            turn.send(ToWithoutMail::Held(me), &Order { text })?;
        }

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Order> for Pointer {
    type Sends = (Asks<Tiles, WhichTile, Looking>, ToType<Board, WhichTile>);

    fn handle(&mut self, order: Order, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        let mut words = order.text.split(' ');
        let verb = words.next().unwrap_or_default();
        let xs: Vec<u64> = words.map(|word| word.parse().unwrap_or_default()).collect();
        let Ok(board) = turn.spawn::<Board>(&1, Board) else {
            return Ok(());
        };
        match verb {
            "ask" => {
                for (at, x) in (0..).zip(xs) {
                    turn.ask_to(board.protocol::<Tiles, _>(), &WhichTile { x }, &Looking { at })?;
                }
            }
            "tell" => {
                for x in xs {
                    turn.send_to(board, &WhichTile { x })?;
                }
            }
            "both" => {
                let (asked, told) = (xs.first().copied().unwrap_or_default(), xs.last().copied().unwrap_or_default());
                turn.ask_to(board.protocol::<Tiles, _>(), &WhichTile { x: asked }, &Looking { at: 0 })?;
                turn.send::<Board, _, _>(To::ChildInstanced(Board::NAME, 1_u64.to_name()), &WhichTile { x: told })?;
            }
            _ => {}
        }

        Ok(())
    }
}

impl Handles<Answer<TileIs, Looking>> for Pointer {
    type Sends = ();

    fn handle(&mut self, (answered, looking): (Answered<TileIs>, Looking), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        match answered {
            Answered::Yes(tile) => say(format!("{} for {}", tile.tile, looking.at)),
            Answered::No(why) => say(format!("{why:?} for {}", looking.at)),
        }

        Ok(())
    }
}

impl Handles<TileIs> for Pointer {
    type Sends = ();

    fn handle(&mut self, tile: TileIs, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        say(format!("told {}", tile.tile));

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

/// What a pointer that follows `orders` hears, in an engine in four parts whose parts each run at most `hops` fused
/// hops a step.
fn heard(hops: usize, orders: &[&str]) -> Vec<String> {
    let Some(parts) = NonZeroUsize::new(4) else {
        panic!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 1 << 20, 1 << 20, hops);
    let orders = orders.iter().map(|text| String::from(*text)).collect();
    let made = engine.add_root::<Pointer>(&(), PointerConfig { orders });
    assert!(made.is_ok(), "the pointer begins");
    said();
    for _ in 0..16 {
        engine.step(&Stopped);
    }

    said()
}

#[test]
fn each_answer_reaches_its_asker_with_what_it_kept_for_that_request() {
    // Catches answers matched to the wrong request where two asks of one turn are alike, as root and depth would
    // leave them; a context lost or handed back twice; and an answer fused back within the step that finds nothing kept.
    for hops in [0, 64] {
        let heard = heard(hops, &["ask 3 3 5"]);

        assert_eq!(heard, ["30 for 0", "30 for 1", "50 for 2"], "hops={hops}");
    }
}

#[test]
fn a_request_is_answered_once() {
    // Catches a second reply to a request taken as a second answer, or sent on as mail the asker cannot place.
    for hops in [0, 64] {
        let heard = heard(hops, &["ask 7"]);

        assert_eq!(heard, ["again true", "70 for 0"], "hops={hops}");
    }
}

#[test]
fn a_reply_to_plain_mail_stays_plain_mail() {
    // Catches a reply to mail that asked nothing tied as an answer, which its sender would drop as answering nothing
    // it asked.
    for hops in [0, 64] {
        let heard = heard(hops, &["tell 4"]);

        assert_eq!(heard, ["told 40"], "hops={hops}");
    }
}

#[test]
fn plain_mail_sent_straight_after_a_request_stays_apart_from_it() {
    // Catches plain mail of the request's kind to the same receiver joined into the request's run, which would make it
    // a second request tied to the first's count.
    for hops in [0, 64] {
        let heard = heard(hops, &["both 4 5"]);

        assert_eq!(heard, ["40 for 0", "told 50"], "hops={hops}");
    }
}

#[test]
fn a_context_is_let_go_of_once_its_answer_has_been_handled() {
    // Catches a context kept after its answer's turn, which would stay beside the asker for as long as it lives.
    let mut waiting = Waiting::default();
    let mut asked = Asked::new();
    waiting.file(iter::once(Asking { count: 3, context: vec![9] }));
    let answer = Tie::answers(3);
    let kept = matches!(waiting.answering(answer), Answering::Kept([9]));

    waiting.settle(answer, &mut asked, &Ok(()));

    assert!(kept);
    assert!(matches!(waiting.answering(answer), Answering::Stray));
}
