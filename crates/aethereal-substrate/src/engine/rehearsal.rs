//! An engine for a host to time steps on, to measure what splitting one among workers costs (ADR-0011).

use super::Engine;
use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, ToType, Top, Turn};
use aethereal_data::{Kind, KindName, Plain};
use std::num::NonZeroUsize;

/// What each actor of a rehearsal sends itself, every step.
#[derive(Plain)]
struct Again;

impl Kind for Again {
    const NAME: KindName = KindName::new("aether.rehearsal.again");
}

/// An actor of a rehearsal: it sends itself [`Again`] as it begins, and again on each it is sent.
#[derive(Plain)]
struct Echo;

impl Kind for Echo {
    const NAME: KindName = KindName::new("aether.rehearsal.echo");
}

impl ChildOf for Echo {
    type Parent = Top;
}

impl Actor for Echo {
    type Config = Again;
    type Key = u64;
    type Handles = (Again,);
    type BeginSends = (ToType<Self, Again>,);
    type EndSends = ();

    fn begin(_: Again, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(turn.me()).send(&Again)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Again> for Echo {
    type Sends = (ToType<Self, Again>,);

    fn handle(&mut self, _: Again, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(turn.me()).send(&Again)?;

        Ok(())
    }
}

/// The bytes of arena a rehearsal's mail is written to: a piece of mail for each actor, each step, fits many times.
const REHEARSAL_MAIL_BYTES: usize = 1 << 20;

/// The actors a rehearsal has in each part. With one, a step's turns take so little that what the plan expects of them
/// is off by more than a split costs: measured on 2 to 16 workers, every count's cost came out about 15 µs more than
/// with eight, and with eight each count's came within about 6 µs of what steps of 120 actors passing mail paid.
const ACTORS_EACH_PART: usize = 8;

impl Engine {
    /// An engine in as many parts as this one, for a host to time steps on: [`ACTORS_EACH_PART`] actors in each
    /// part, each sending itself a piece of mail every step, for ever. Each of its steps runs turns in every part and
    /// folds records into every part, with turns that take little time, so what a step split among workers takes
    /// beyond the same step run whole is what the split costs (ADR-0011).
    ///
    /// # Panics
    ///
    /// Panics when an actor of it cannot begin, which sending itself one piece of mail never fails.
    #[must_use]
    pub fn rehearsal(&self) -> Self {
        let parts = NonZeroUsize::new(self.seating.len()).unwrap_or(NonZeroUsize::MIN);
        let mut rehearsal = Self::empty(parts, self.read_limit, REHEARSAL_MAIL_BYTES, 0, 0);
        // Each root sits in the part with fewest actors, so as many go in each part.
        for number in 0..parts.get() * ACTORS_EACH_PART {
            let key = u64::try_from(number).unwrap_or(u64::MAX);
            let Ok(_) = rehearsal.add_root::<Echo>(&key, Again) else {
                panic!("an actor of a rehearsal begins");
            };
        }

        rehearsal
    }
}
