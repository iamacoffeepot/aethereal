//! The court: its measures, which every actor of the game shares, and the mail the game sends the actors that follow
//! the play, and the mail its opponent steers by.
//!
//! The court is in its own units, `y` up as ADR-0005 has it: it runs from `-HALF_WIDTH` to `HALF_WIDTH` across and
//! from `-HALF_HEIGHT` to `HALF_HEIGHT` up, with the middle at nought, the player's paddle on the left and the
//! opponent's on the right.

use aethereal_data::{Kind, KindName, Plain};

/// Half the court's width: from the middle to the line a ball passes to score.
pub const HALF_WIDTH: f32 = 1.0;

/// Half the court's height: from the middle to a wall.
pub const HALF_HEIGHT: f32 = 0.75;

/// How far from the middle each paddle's centre stands, across.
pub const PADDLE_X: f32 = 0.92;

/// Half a paddle's width.
pub const PADDLE_HALF_WIDTH: f32 = 0.0125;

/// Half a paddle's height.
pub const PADDLE_HALF_HEIGHT: f32 = 0.11;

/// Half the side of the ball, which is square.
pub const BALL_HALF: f32 = 0.0125;

/// How fast the ball is served, in court units a second.
pub const SERVE_SPEED: f32 = 0.95;

/// The points that win a match.
pub const WINNING: u8 = 11;

/// One end of the court.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Side {
    /// The player's end.
    Left,
    /// The opponent's end.
    Right,
}

impl Side {
    /// The end across the court from this one.
    #[must_use]
    pub const fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    /// Which way along `x` is toward this end: minus one for the left, one for the right.
    #[must_use]
    pub const fn sign(self) -> f32 {
        match self {
            Self::Left => -1.0,
            Self::Right => 1.0,
        }
    }
}

/// Where the ball is, and how fast it goes along each axis, in court units a second.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Ball {
    pub x: f32,
    pub y: f32,
    pub across: f32,
    pub up: f32,
}

impl Ball {
    /// The ball at rest in the middle of the court.
    pub const RESTING: Self = Self { x: 0.0, y: 0.0, across: 0.0, up: 0.0 };

    /// Whether it is heading toward `side`.
    #[must_use]
    pub fn heads_to(self, side: Side) -> bool {
        self.across * side.sign() > 0.0
    }

    /// How fast it goes, in court units a second.
    #[must_use]
    pub fn speed(self) -> f32 {
        self.across.hypot(self.up)
    }
}

/// The points each side has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Score {
    pub left: u8,
    pub right: u8,
}

impl Score {
    /// No points to either.
    pub const LOVE: Self = Self { left: 0, right: 0 };

    /// The score with one more point to `side`.
    #[must_use]
    pub const fn to(self, side: Side) -> Self {
        match side {
            Side::Left => Self { left: self.left + 1, right: self.right },
            Side::Right => Self { left: self.left, right: self.right + 1 },
        }
    }

    /// The side that has won, or none while the match goes on.
    #[must_use]
    pub const fn winner(self) -> Option<Side> {
        if self.left >= WINNING {
            Some(Side::Left)
        } else if self.right >= WINNING {
            Some(Side::Right)
        } else {
            None
        }
    }
}

/// Where a match stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Play {
    /// The ball waits in the middle to be served toward `toward`, once the game's clock reaches `at_nanos`.
    Serving { toward: Side, at_nanos: u64 },
    /// The ball is in play.
    Rally,
    /// The match is over, and `winner` won it: Space or Enter plays again.
    Won { winner: Side },
}

/// The court as it stands after one of the game's ticks, sent to the actors beneath the game that follow the play:
/// `elapsed_nanos` into the game, the ball, each paddle's height, the score, and where the match stands.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Court {
    pub elapsed_nanos: u64,
    pub ball: Ball,
    pub left: f32,
    pub right: f32,
    pub score: Score,
    pub play: Play,
}

impl Kind for Court {
    const NAME: KindName = KindName::new("aether.example.pong.court");
}

impl Court {
    /// A court before its first tick: the paddles level, the ball resting in the middle, and no points to either.
    pub const OPENING: Self = Self {
        elapsed_nanos: 0,
        ball: Ball::RESTING,
        left: 0.0,
        right: 0.0,
        score: Score::LOVE,
        play: Play::Serving { toward: Side::Left, at_nanos: 0 },
    };
}

/// How the opponent moves its paddle: `direction` from minus one, down at full speed, to one, up at full speed. The
/// game moves the paddle so until it is sent another.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Steer {
    pub direction: f32,
}

impl Kind for Steer {
    const NAME: KindName = KindName::new("aether.example.pong.steer");
}
