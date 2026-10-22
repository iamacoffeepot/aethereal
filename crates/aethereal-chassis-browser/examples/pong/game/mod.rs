//! The game: the root that keeps the match. It moves the player's paddle by the keys its window hears and the
//! opponent's as the opponent steers it, plays the ball at each `Tick` by the time the tick carries, and sends the
//! court as it then stands to the actors beneath it that follow the play: the screen, and the opponent.

mod keys;
mod rally;

use crate::court::{Ball, Court, Play, Score, Side, Steer};
use crate::dice::Dice;
use crate::opponent::{Opponent, Seeded};
use crate::screen::{Screen, Shows};
use aethereal_actor::{
    Actor, ActorRef, Beginning, ChildOf, Ending, ErasedActorRef, Fault, Handles, Level, On, Path, Tells, Tick, ToType,
    Top, Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_window::{self as window, Event, Window, WindowId};
use keys::Keys;
use rally::{Paddle, Rolled};

/// How long the ball waits in the middle before the first serve of a match, in nanoseconds.
const OPENING_NANOS: u64 = 1_200_000_000;

/// How long the ball waits in the middle after a point before it is served, in nanoseconds.
const BETWEEN_NANOS: u64 = 900_000_000;

/// The game's config: the window whose keys move the player's paddle and that the screen draws on, and the seed of
/// the match's chance.
#[derive(Plain)]
#[kind(name = "aether.example.pong.match")]
pub struct Match {
    pub window: WindowId,
    pub seed: u64,
}

/// Why the game did not begin: an actor it stands up beneath it did not.
#[derive(Plain)]
#[kind(name = "aether.example.pong.unbegun")]
enum Unbegun {
    /// The screen.
    Screen,
    /// The opponent.
    Opponent,
}

/// The game: the court as it stands, the keys the player holds, the way the opponent last steered, the match's
/// chance, and the actors beneath it it sends the court to.
#[derive(Bound)]
#[kind(name = "aether.example.pong")]
pub struct Pong {
    court: Court,
    keys: Keys,
    steer: f32,
    dice: Dice,
    screen: ActorRef<Screen>,
    opponent: ActorRef<Opponent>,
}

impl ChildOf for Pong {
    type Parent = Top;
}

impl Actor for Pong {
    type Config = Match;
    type Key = ();
    type Handles = (On<Tick>, Event, Steer);
    type BeginSends = (Tells<Path<Window>, window::Subscribe>,);
    type EndSends = ();

    /// Stands up the screen and the opponent beneath it, subscribes to its window and to `Tick`, and sets the first
    /// serve toward a side the dice choose.
    fn begin(game: Match, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let mut dice = Dice::seeded(game.seed);
        let screen = match turn.spawn::<Screen>(&(), Shows { window: game.window }) {
            Ok(screen) => screen,
            Err(refused) => {
                turn.log(Level::Error, format_args!("the screen was refused: {refused}"));
                return Err(Fault::new(&Unbegun::Screen)?);
            }
        };
        let opponent = match turn.spawn::<Opponent>(&(), Seeded { dice: dice.split() }) {
            Ok(opponent) => opponent,
            Err(refused) => {
                turn.log(Level::Error, format_args!("the opponent was refused: {refused}"));
                return Err(Fault::new(&Unbegun::Opponent)?);
            }
        };

        turn.to(path::<Window>(game.window)).send(&window::Subscribe)?;
        turn.subscribe::<Tick>();

        Ok(Self {
            court: Court { play: Play::Serving { toward: dice.side(), at_nanos: OPENING_NANOS }, ..Court::OPENING },
            keys: Keys::NONE,
            steer: 0.0,
            dice,
            screen,
            opponent,
        })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Pong {
    /// The court once `delta_nanos` more of play have passed, `elapsed_nanos` into the game: both paddles moved, and
    /// the ball served, played, or waiting as the match stands.
    fn played(&mut self, elapsed_nanos: u64, delta_nanos: u64) -> Court {
        let secs = rally::seconds(delta_nanos);
        let left = rally::moved(self.court.left, self.keys.direction(), secs);
        let right = rally::moved(self.court.right, self.steer, secs);
        let (ball, score, play) = match self.court.play {
            Play::Serving { toward, at_nanos } if elapsed_nanos >= at_nanos => {
                (rally::serve(toward, &mut self.dice), self.court.score, Play::Rally)
            }
            Play::Rally => match rally::roll(
                self.court.ball,
                Paddle::between(self.court.left, left, secs),
                Paddle::between(self.court.right, right, secs),
                delta_nanos,
            ) {
                Rolled::On(ball) => (ball, self.court.score, Play::Rally),
                Rolled::Scored(side) => self.scored(side, elapsed_nanos),
            },
            Play::Serving { .. } | Play::Won { .. } => (self.court.ball, self.court.score, self.court.play),
        };

        Court { elapsed_nanos, ball, left, right, score, play }
    }

    /// The ball, the score, and the play once `side` scored, `elapsed_nanos` into the game: the ball back in the
    /// middle, to be served toward the side that conceded, or the match won.
    const fn scored(&self, side: Side, elapsed_nanos: u64) -> (Ball, Score, Play) {
        let score = self.court.score.to(side);
        let play = match score.winner() {
            Some(winner) => Play::Won { winner },
            None => Play::Serving { toward: side.other(), at_nanos: elapsed_nanos + BETWEEN_NANOS },
        };

        (Ball::RESTING, score, play)
    }

    /// Starts a new match once one is won: no points to either, and the first serve toward a side the dice choose.
    fn rematch(&mut self) {
        let Play::Won { .. } = self.court.play else {
            return;
        };
        let serving = Play::Serving { toward: self.dice.side(), at_nanos: self.court.elapsed_nanos + OPENING_NANOS };
        self.court = Court { score: Score::LOVE, play: serving, ..self.court };
    }
}

/// Plays the ball for the time since the last tick, and sends the court as it now stands to the screen and the
/// opponent.
impl Handles<On<Tick>> for Pong {
    type Sends = (ToType<Screen, Court>, ToType<Opponent, Court>);

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let court = self.played(tick.elapsed_nanos, tick.delta_nanos);
        self.court = court;
        turn.to(self.screen).send(&court)?;
        turn.to(self.opponent).send(&court)?;

        Ok(())
    }
}

/// Keeps which of the keys that move the paddle are held, lets them all go when the window loses the keyboard, and
/// starts a match on Space or Enter once one is won.
impl Handles<Event> for Pong {
    type Sends = ();

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        match event {
            Event::KeyDown { code, repeat, .. } => {
                self.keys = self.keys.pressed(&code);
                let restarts = !repeat && keys::starts(&code);
                if restarts {
                    self.rematch();
                }
            }
            Event::KeyUp { code, .. } => self.keys = self.keys.released(&code),
            Event::FocusLost => self.keys = Keys::NONE,
            _ => {}
        }

        Ok(())
    }
}

/// Takes the way the opponent steers its paddle, from the opponent alone.
impl Handles<Steer> for Pong {
    type Sends = ();

    fn handle(&mut self, steer: Steer, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let is_opponent = turn.sender() == ErasedActorRef::from(self.opponent);
        if !is_opponent {
            turn.log(Level::Warn, "a steer from an actor other than the opponent was left: only the opponent steers");
            return Ok(());
        }
        self.steer = steer.direction;

        Ok(())
    }
}
