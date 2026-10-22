//! How the ball flies between strokes: along its way, curved by its spin, which fades as it goes, and turned back off
//! the walls, which turn its spin away from them and take half of it. The game plays the ball by it, and the opponent reads by it where the
//! ball will reach its end.

use crate::court::{BALL_HALF, Ball, HALF_HEIGHT};

/// The highest the ball's centre goes, and the lowest below the middle: where its edge meets the wall.
const BALL_REACH: f32 = HALF_HEIGHT - BALL_HALF;

/// How fast the ball's spin fades: the spin left after a second is `e` to the minus this of what it was.
const SPIN_FADING: f32 = 0.15;

/// The steepest the ball goes however it curves: how fast it goes up or down, at the most, for each court unit a
/// second it goes across.
const STEEPEST: f32 = 1.4;

/// What is left of the ball's spin after a wall turns it.
const WALL_KEEPS: f32 = 0.5;

/// The stretch of time the ball is followed by when reading where it will go, in seconds.
const STRETCH_SECS: f32 = 0.002;

/// The most stretches the ball is followed for when reading where it will go: longer than the slowest ball takes to
/// cross the court.
const FARTHEST_STRETCHES: u16 = 4096;

/// The ball `secs` on along its way, through anything in it: its spin pulls its way up or down, never steeper than the
/// steepest it goes, and fades.
#[must_use]
pub fn ahead(ball: Ball, secs: f32) -> Ball {
    let steepest = ball.across.abs() * STEEPEST;
    let up = ball.spin.mul_add(secs, ball.up).clamp(-steepest, steepest);
    let spin = ball.spin * (-SPIN_FADING * secs).exp();

    Ball { x: ball.across.mul_add(secs, ball.x), y: up.mul_add(secs, ball.y), up, spin, ..ball }
}

/// The ball turned back off a wall it has gone into, by as far as it went in, half its spin turned away from the wall
/// so that it curves away from it.
#[must_use]
pub fn off_walls(ball: Ball) -> Ball {
    let into_top = ball.y > BALL_REACH;
    let into_bottom = ball.y < -BALL_REACH;
    if into_top {
        Ball { y: 2.0f32.mul_add(BALL_REACH, -ball.y), up: -ball.up.abs(), spin: -ball.spin.abs() * WALL_KEEPS, ..ball }
    } else if into_bottom {
        Ball {
            y: (-2.0f32).mul_add(BALL_REACH, -ball.y),
            up: ball.up.abs(),
            spin: ball.spin.abs() * WALL_KEEPS,
            ..ball
        }
    } else {
        ball
    }
}

/// The height at which `ball` reaches `x` across, followed a stretch at a time along its curve and off the walls: or
/// its height at the last stretch followed, when it never gets there.
#[must_use]
pub fn reaching(ball: Ball, x: f32) -> f32 {
    let way = ball.across.signum();
    let mut ball = ball;
    for _ in 0..FARTHEST_STRETCHES {
        let arrived = (x - ball.x) * way <= 0.0;
        if arrived {
            break;
        }
        ball = off_walls(ahead(ball, STRETCH_SECS));
    }

    ball.y
}
