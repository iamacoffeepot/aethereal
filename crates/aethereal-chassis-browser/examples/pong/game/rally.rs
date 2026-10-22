//! How the ball and the paddles move over a stretch of time: the paddles at a capped speed, the ball off the walls and
//! the paddles, faster on every hit and spun by a paddle struck moving, until it passes a paddle and a point is scored.

use crate::court::{
    BALL_HALF, Ball, FULL_SPIN, HALF_HEIGHT, HALF_WIDTH, PADDLE_HALF_HEIGHT, PADDLE_HALF_WIDTH, PADDLE_SPEED, PADDLE_X,
    SERVE_SPEED, Side,
};
use crate::dice::Dice;
use crate::flight;

/// How much faster the ball goes after each hit.
const SPEED_UP: f32 = 1.07;

/// The fastest the ball goes, however many hits.
const TOP_SPEED: f32 = 2.1;

/// The steepest a serve leaves the middle, in radians from straight across.
const SERVE_ANGLE: f32 = 0.45;

/// The steepest the ball leaves a paddle, struck at its very end, in radians from straight across.
const HIT_ANGLE: f32 = 0.9;

/// The longest stretch the ball moves in one go, in nanoseconds: short enough that it never passes through a paddle in
/// a stretch, however long the frame.
const STRETCH_NANOS: u64 = 2_000_000;

/// The highest a paddle's centre goes, and the lowest below the middle: where its end meets the wall.
const PADDLE_REACH: f32 = HALF_HEIGHT - PADDLE_HALF_HEIGHT;

/// Where the paddle at `paddle` is once it has moved `direction` of its full speed for `secs`, kept between the walls.
#[must_use]
pub fn moved(paddle: f32, direction: f32, secs: f32) -> f32 {
    let speed = direction.clamp(-1.0, 1.0) * PADDLE_SPEED;

    speed.mul_add(secs, paddle).clamp(-PADDLE_REACH, PADDLE_REACH)
}

/// A paddle as the ball meets it: its centre's height, and how fast it rises, in court units a second, down below
/// nought.
#[derive(Clone, Copy)]
pub struct Paddle {
    at: f32,
    rising: f32,
}

impl Paddle {
    /// The paddle that moved from `before` to `now` over `secs`: still, when no time passed.
    #[must_use]
    pub fn between(before: f32, now: f32, secs: f32) -> Self {
        let rising = if secs > 0.0 {
            (now - before) / secs
        } else {
            0.0
        };

        Self { at: now, rising }
    }
}

/// The ball served from the middle toward `toward`, at the serve's speed and an angle the dice choose.
#[must_use]
pub fn serve(toward: Side, dice: &mut Dice) -> Ball {
    let angle = dice.between(-SERVE_ANGLE, SERVE_ANGLE);

    Ball { x: 0.0, y: 0.0, across: toward.sign() * SERVE_SPEED * angle.cos(), up: SERVE_SPEED * angle.sin(), spin: 0.0 }
}

/// What became of the ball over a stretch of play.
pub enum Rolled {
    /// It is still in play, here.
    On(Ball),
    /// It passed a paddle, and the side across from it scored.
    Scored(Side),
}

/// The seconds `nanos` nanoseconds are.
#[expect(clippy::cast_precision_loss, reason = "a tick's time is far shorter than a float loses a nanosecond in")]
pub fn seconds(nanos: u64) -> f32 {
    nanos as f32 / 1_000_000_000.0
}

/// Where `ball` is after `nanos` of play against the paddles at `left` and `right`, moved a short stretch at a time.
#[must_use]
pub fn roll(ball: Ball, left: Paddle, right: Paddle, nanos: u64) -> Rolled {
    let stretches = u16::try_from(nanos.div_ceil(STRETCH_NANOS)).unwrap_or(u16::MAX);
    let stretch = seconds(nanos) / f32::from(stretches.max(1));
    let mut ball = ball;
    for _ in 0..stretches {
        let before = ball;
        let moved = flight::off_walls(flight::ahead(before, stretch));
        ball = off_paddle(off_paddle(moved, before, Side::Left, left), before, Side::Right, right);
        let passed_right = ball.x - BALL_HALF > HALF_WIDTH;
        let passed_left = ball.x + BALL_HALF < -HALF_WIDTH;
        if passed_right {
            return Rolled::Scored(Side::Left);
        }
        if passed_left {
            return Rolled::Scored(Side::Right);
        }
    }

    Rolled::On(ball)
}

/// The ball `now`, which was at `before`, struck back by the paddle of `side` when it crossed that paddle's face
/// heading for it: faster, steeper the further from the paddle's centre it struck, and spun against the paddle's way
/// by how fast the paddle moved and how fast the ball leaves.
fn off_paddle(now: Ball, before: Ball, side: Side, paddle: Paddle) -> Ball {
    let face = side.sign() * (PADDLE_X - PADDLE_HALF_WIDTH);
    let reach = PADDLE_HALF_HEIGHT + BALL_HALF;
    let edge_before = side.sign().mul_add(BALL_HALF, before.x);
    let edge_now = side.sign().mul_add(BALL_HALF, now.x);
    let heading = now.heads_to(side);
    let was_short = (face - edge_before) * side.sign() >= 0.0;
    let is_past = (edge_now - face) * side.sign() > 0.0;
    let struck = (now.y - paddle.at) / reach;
    let is_level = struck.abs() <= 1.0;
    let hits = heading && was_short && is_past && is_level;
    if !hits {
        return now;
    }
    let speed = (now.speed() * SPEED_UP).min(TOP_SPEED);
    let angle = struck * HIT_ANGLE;
    let faster = speed / SERVE_SPEED;
    let spin = -(paddle.rising / PADDLE_SPEED) * FULL_SPIN * faster * faster;

    Ball {
        x: side.sign().mul_add(-BALL_HALF, face),
        y: now.y,
        across: -side.sign() * speed * angle.cos(),
        up: speed * angle.sin(),
        spin,
    }
}
