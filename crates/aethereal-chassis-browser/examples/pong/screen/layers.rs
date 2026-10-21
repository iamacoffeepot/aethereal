//! What each of the screen's three placements shows, as records of rectangles: the court, drawn once and never
//! written again; the board, the score and the words over the court, written when they change; and the play, the
//! paddles and the ball, written for every court the game sends.

use super::glyphs::{self, Line};
use super::records::{Colour, Plane, Records};
use crate::court::{
    BALL_HALF, Court, HALF_HEIGHT, HALF_WIDTH, PADDLE_HALF_HEIGHT, PADDLE_HALF_WIDTH, PADDLE_X, Play, Score, Side,
};
use aethereal_data::Plain;

/// The floor of the court, and the panel the board's words stand on.
const FLOOR: Colour = [12, 15, 22, 255];

/// The court's walls and its centre line.
const LINES: Colour = [54, 62, 80, 255];

/// The player's paddle, lightly cool.
const PLAYER: Colour = [205, 235, 255, 255];

/// The opponent's paddle, lightly warm.
const OPPONENT: Colour = [255, 222, 205, 255];

/// The ball.
const BALL: Colour = [255, 255, 255, 255];

/// The score.
const SCORE: Colour = [196, 204, 220, 255];

/// The board's headline.
const HEADLINE: Colour = [255, 255, 255, 255];

/// The board's hint below its headline.
const HINT: Colour = [150, 160, 182, 255];

/// Half the thickness of a wall, outside the court.
const WALL_HALF: f32 = 0.008;

/// Half the width of the centre line.
const CENTRE_HALF_WIDTH: f32 = 0.006;

/// How many dashes the centre line is.
const DASHES: u8 = 18;

/// The part of the space each dash takes that it fills.
const DASH_FILLED: f32 = 0.55;

/// How far apart the dashes are.
const PITCH: f32 = 2.0 * HALF_HEIGHT / DASHES as f32;

/// The gaps between the dashes the board's panel runs from and to, counted from the bottom wall: it ends in gaps, so
/// it hides whole dashes.
const PANEL_GAPS: (u8, u8) = (7, 12);

/// The most records the court holds.
pub const COURT_CAPACITY: u32 = 32;

/// The most records the board holds.
pub const BOARD_CAPACITY: u32 = 1024;

/// The most records the play holds: two paddles and the ball.
pub const PLAY_CAPACITY: u32 = 3;

/// The court: its floor, its walls, and its dashed centre line.
pub fn court() -> Records {
    let mut records = Records::new();
    records.rect(0.0, 0.0, HALF_WIDTH, HALF_HEIGHT, Plane::Floor, FLOOR);
    for wall in [-1.0_f32, 1.0] {
        records.rect(0.0, wall * (HALF_HEIGHT + WALL_HALF), HALF_WIDTH, WALL_HALF, Plane::Lines, LINES);
    }
    for dash in 0..DASHES {
        let y = (gap(dash) + gap(dash + 1)) * 0.5;
        records.rect(0.0, y, CENTRE_HALF_WIDTH, PITCH * DASH_FILLED * 0.5, Plane::Lines, LINES);
    }

    records
}

/// The height of the middle of the `at`th gap between the centre line's dashes, counted from the bottom wall.
fn gap(at: u8) -> f32 {
    f32::from(at).mul_add(PITCH, -HALF_HEIGHT)
}

/// The paddles where `court` has them, and the ball while the match goes on.
pub fn play(court: &Court) -> Records {
    let mut records = Records::new();
    records.rect(-PADDLE_X, court.left, PADDLE_HALF_WIDTH, PADDLE_HALF_HEIGHT, Plane::Play, PLAYER);
    records.rect(PADDLE_X, court.right, PADDLE_HALF_WIDTH, PADDLE_HALF_HEIGHT, Plane::Play, OPPONENT);
    let is_over = matches!(court.play, Play::Won { .. });
    if !is_over {
        records.rect(court.ball.x, court.ball.y, BALL_HALF, BALL_HALF, Plane::Play, BALL);
    }

    records
}

/// What the board says beside the score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Banner {
    /// Nothing: the match goes on.
    Blank,
    /// That `winner` won the match, and how to play again.
    Won { winner: Side },
}

/// The board: the score, and the banner over the court.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Board {
    score: Score,
    banner: Banner,
}

impl Board {
    /// The board before a point is played.
    pub const OPENING: Self = Self { score: Score::LOVE, banner: Banner::Blank };

    /// The board as `court` stands.
    pub const fn of(court: &Court) -> Self {
        let banner = match court.play {
            Play::Won { winner } => Banner::Won { winner },
            Play::Serving { .. } | Play::Rally => Banner::Blank,
        };

        Self { score: court.score, banner }
    }

    /// The board's records: each side's points above its half of the court, and over the middle, once the match is
    /// won, who won it on a panel that hides the centre line, and how to play again.
    pub fn records(self) -> Records {
        let mut records = Records::new();
        let points = Line { x: 0.25, top: 0.66, block: 0.03 };
        let left = self.score.left.to_string();
        let right = self.score.right.to_string();
        glyphs::write(&mut records, &left, Line { x: -points.x, ..points }, Plane::Text, SCORE);
        glyphs::write(&mut records, &right, points, Plane::Text, SCORE);
        let Banner::Won { winner } = self.banner else {
            return records;
        };

        let headline = match winner {
            Side::Left => "YOU WIN",
            Side::Right => "CPU WINS",
        };
        let (bottom, top) = (gap(PANEL_GAPS.0), gap(PANEL_GAPS.1));
        records.rect(0.0, (top + bottom) * 0.5, 0.62, (top - bottom) * 0.5, Plane::Panel, FLOOR);
        glyphs::write(&mut records, headline, Line { x: 0.0, top: 0.14, block: 0.024 }, Plane::Text, HEADLINE);
        glyphs::write(&mut records, "SPACE TO PLAY AGAIN", Line { x: 0.0, top: -0.04, block: 0.01 }, Plane::Text, HINT);

        records
    }
}
