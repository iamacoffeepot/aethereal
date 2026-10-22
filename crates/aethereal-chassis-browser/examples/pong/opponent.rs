//! The opponent: an actor beneath the game that plays the right paddle as a player would, from the court the game
//! sends it and nothing else, and steers its paddle by mail.
//!
//! It is beatable by how it sees: it looks at the court only every so often, so it reacts a little late; it reads
//! where the ball will reach its end, along its curve and off the walls, but misjudges it by a little that it chooses afresh each time the
//! ball comes its way, and by more the faster the ball goes; and it never moves its paddle at more than part of the
//! speed the game allows, but at the last moment before the ball reaches it, it flicks its paddle up or down to spin
//! the ball, as hard and which way as it chooses afresh each time the ball comes its way.

use crate::court::{
    BALL_HALF, Court, PADDLE_HALF_HEIGHT, PADDLE_HALF_WIDTH, PADDLE_SPEED, PADDLE_X, Play, SERVE_SPEED, Side, Steer,
};
use crate::dice::Dice;
use crate::flight;
use crate::game::Pong;
use aethereal_actor::{Actor, Beginning, ChildOf, Ending, ErasedActorRef, Fault, Handles, Level, ToType, Turn};
use aethereal_data::{Bound, Kind, KindName, Plain};

/// How long it goes between looks at the court, at the least, in nanoseconds: its reaction time.
const REACTION_NANOS: u64 = 110_000_000;

/// The most it waits beyond its reaction time before it looks again, in nanoseconds, chosen afresh at each look.
const HESITATION_NANOS: f32 = 50_000_000.0;

/// The part of the game's paddle speed it moves at, at the most.
const EFFORT: f32 = 0.7;

/// How near its aim its paddle is, in court units, before it slows down to settle there rather than overshoot.
const SETTLING: f32 = 0.06;

/// How far it misjudges the ball at the serve's speed, at the most, as a part of half its paddle.
const MISJUDGED: f32 = 0.5;

/// How much further it misjudges the ball for each serve's speed more it goes, as a part of half its paddle.
const MISJUDGED_FASTER: f32 = 1.2;

/// How long before the ball reaches its paddle it flicks the paddle, in seconds.
const FLICK_SECS: f32 = 0.1;

/// The softest flick it makes, as a part of the paddle's full speed: the hardest is the full speed.
const SOFTEST_FLICK: f32 = 0.4;

/// How far it follows the ball's height while the ball goes away from it, as a part of that height.
const FOLLOWING: f32 = 0.3;

/// The opponent's config: its own chance.
#[derive(Plain)]
pub struct Seeded {
    pub dice: Dice,
}

impl Kind for Seeded {
    const NAME: KindName = KindName::new("aether.example.pong.seeded");
}

/// What it made of the ball when it last looked.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
enum Sight {
    /// The ball is coming its way, it misjudges where by `error`, in court units up, and it flicks its paddle at
    /// `flick` of its full speed just before the ball reaches it, up above nought and down below.
    Coming { error: f32, flick: f32 },
    /// The ball is going away, or waits to be served.
    Going,
}

/// The opponent: its chance, what it made of the ball, the height it steers its paddle toward, and when it looks
/// at the court again, as the game's clock counts.
#[derive(Bound)]
pub struct Opponent {
    dice: Dice,
    sight: Sight,
    aim: f32,
    looks_at_nanos: u64,
}

impl Kind for Opponent {
    const NAME: KindName = KindName::new("aether.example.pong.opponent");
}

impl ChildOf for Opponent {
    type Parent = Pong;
}

impl Actor for Opponent {
    type Config = Seeded;
    type Key = ();
    type Handles = (Court,);
    type BeginSends = ();
    type EndSends = ();

    fn begin(seeded: Seeded, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self { dice: seeded.dice, sight: Sight::Going, aim: 0.0, looks_at_nanos: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Where the ball's centre is across when it meets the face of the right paddle.
const FACE: f32 = PADDLE_X - PADDLE_HALF_WIDTH - BALL_HALF;

impl Opponent {
    /// Looks at `court`: whether the ball comes its way, choosing how far it misjudges it when it has just turned
    /// toward it, where to aim the paddle, and when to look again.
    fn look(&mut self, court: &Court) {
        let in_play = court.play == Play::Rally;
        let coming = in_play && court.ball.heads_to(Side::Right);
        self.sight = match (coming, self.sight) {
            (true, Sight::Going) => {
                let faster = (court.ball.speed() / SERVE_SPEED - 1.0).max(0.0);
                let most = PADDLE_HALF_HEIGHT * MISJUDGED_FASTER.mul_add(faster, MISJUDGED);
                let error = self.dice.between(-most, most);
                let way = self.dice.side().sign();
                let flick = way * self.dice.between(SOFTEST_FLICK, 1.0);
                Sight::Coming { error, flick }
            }
            (true, sight @ Sight::Coming { .. }) => sight,
            (false, _) => Sight::Going,
        };
        self.aim = match self.sight {
            // Aimed short of the ball by as far as the flick carries the paddle, so the flick meets it.
            Sight::Coming { error, flick } => {
                let carried = flick * PADDLE_SPEED * FLICK_SECS;
                flight::reaching(court.ball, FACE) + error - carried
            }
            Sight::Going => court.ball.y * FOLLOWING,
        };
        let hesitation = self.dice.between(0.0, HESITATION_NANOS);
        self.looks_at_nanos = court.elapsed_nanos + REACTION_NANOS + nanos(hesitation);
    }

    /// The way to steer the paddle at `paddle` as `court` stands: its flick once the ball is about to reach it, and
    /// otherwise toward the aim, at its effort while it is far, and slower as it nears.
    fn steer(&self, paddle: f32, court: &Court) -> Steer {
        let toward_aim = ((self.aim - paddle) / SETTLING).clamp(-EFFORT, EFFORT);
        let Sight::Coming { flick, .. } = self.sight else {
            return Steer { direction: toward_aim };
        };
        let secs = (FACE - court.ball.x) / court.ball.across;
        let is_arriving = secs <= FLICK_SECS;
        let direction = if is_arriving {
            flick
        } else {
            toward_aim
        };

        Steer { direction }
    }
}

/// The whole nanoseconds of `nanos`, which is never below nought.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a hesitation is a small positive time")]
fn nanos(nanos: f32) -> u64 {
    nanos as u64
}

/// Looks at the court from the game when it is time to, and steers its paddle toward its aim.
impl Handles<Court> for Opponent {
    type Sends = (ToType<Pong, Steer>,);

    fn handle(&mut self, court: Court, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let game = turn.parent();
        let is_game = turn.sender() == ErasedActorRef::from(game);
        if !is_game {
            turn.log(Level::Warn, "a court from an actor other than the game was left: only the game's is played");
            return Ok(());
        }
        let is_time = court.elapsed_nanos >= self.looks_at_nanos;
        if is_time {
            self.look(&court);
        }
        turn.to(game).send(&self.steer(court.right, &court))?;

        Ok(())
    }
}
