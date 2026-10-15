//! Where an actor sits in the engine that runs it (ADR-0011).

use aethereal_data::ActorId;
use alloc::vec;
use alloc::vec::Vec;

/// The number of the seat an actor sits in, in the engine that runs it (ADR-0011).
///
/// The engine keeps what it holds of each actor by this number, and an actor
/// keeps its seat for as long as the engine runs it. Once the actor has gone
/// the seat is taken again, so a seat names an actor only beside that actor's
/// id. It means nothing outside the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SeatNumber(u64);

impl SeatNumber {
    /// The seat numbered `number`.
    #[must_use]
    pub const fn new(number: u64) -> Self {
        Self(number)
    }

    /// The seat's number.
    #[must_use]
    pub const fn number(self) -> u64 {
        self.0
    }

    /// The seat's place in a list of seats.
    #[must_use]
    pub const fn index(self) -> usize {
        #[allow(clippy::cast_possible_truncation, reason = "a seat is numbered by its place in a list")]
        let index = self.0 as usize;

        index
    }
}

/// An actor, and the seat it sits in: how the engine names an actor (ADR-0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Seated {
    pub id: ActorId,
    pub seat: SeatNumber,
}

/// The seats a worker gives the children its turns spawn: free ones first, then new numbers (ADR-0011).
///
/// The engine keeps its seats in parts, a part holding every `stride`th
/// number from its own. As a step begins it gives each worker the free seats
/// of some parts and the next new number of each, and takes back what is left
/// when the step is applied. A new seat goes in whichever of the worker's parts
/// has numbered fewest, so new seats spread across them and no two workers give
/// the same number.
#[derive(Debug, Default)]
pub struct Seats {
    free: Vec<SeatNumber>,
    /// The next new number of each part this worker numbers seats in.
    next: Vec<u64>,
    /// How far apart one part's numbers are: one for each part.
    stride: u64,
}

impl Seats {
    /// The seats `free`, then new numbers in the parts whose next numbers are `next`, every `stride`th.
    #[must_use]
    pub const fn new(free: Vec<SeatNumber>, next: Vec<u64>, stride: u64) -> Self {
        Self { free, next, stride }
    }

    /// No free seat, and new numbers from zero: the seats of an engine kept in one part.
    #[must_use]
    pub fn one_part() -> Self {
        Self { free: Vec::new(), next: vec![0], stride: 1 }
    }

    /// The free seats not given out, and the next new number of each part, for the engine to take back.
    #[must_use]
    pub fn into_parts(self) -> (Vec<SeatNumber>, Vec<u64>) {
        (self.free, self.next)
    }

    /// A seat for a new actor: a free one, or the next new number of the part that has numbered fewest.
    ///
    /// # Panics
    ///
    /// Panics when no seat is free and the worker numbers seats in no part: the engine gives every worker a part.
    pub fn take(&mut self) -> SeatNumber {
        self.free.pop().unwrap_or_else(|| {
            let Some(next) = self.next.iter_mut().min() else {
                panic!("a worker numbers new seats in at least one part");
            };
            let seat = SeatNumber(*next);
            *next += self.stride.max(1);

            seat
        })
    }

    /// Takes back the seat of a child that never began.
    pub fn give_back(&mut self, seat: SeatNumber) {
        self.free.push(seat);
    }
}
