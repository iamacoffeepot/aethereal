//! Where an actor sits in the engine that runs it (ADR-0011).

use aethereal_data::ActorId;
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

/// The seats a worker gives the children its turns spawn: free ones first, then new numbers of its own (ADR-0011).
///
/// The engine gives each worker some of the free seats as a step begins, and
/// new numbers that no other worker gives: from `next`, every `stride`th. It
/// takes back what is left when the step is applied.
#[derive(Debug, Default)]
pub struct Seats {
    free: Vec<SeatNumber>,
    /// The next new number this worker gives.
    next: u64,
    /// How far apart this worker's new numbers are: one for each worker, so no two give the same.
    stride: u64,
}

impl Seats {
    /// The seats `free`, then new numbers from `next`, every `stride`th.
    #[must_use]
    pub const fn new(free: Vec<SeatNumber>, next: u64, stride: u64) -> Self {
        Self { free, next, stride }
    }

    /// The free seats not given out, for the engine to take back.
    #[must_use]
    pub fn into_free(self) -> Vec<SeatNumber> {
        self.free
    }

    /// A seat for a new actor: a free one, or the next new number.
    pub fn take(&mut self) -> SeatNumber {
        self.free.pop().unwrap_or_else(|| {
            let seat = SeatNumber(self.next);
            self.next += self.stride.max(1);

            seat
        })
    }

    /// Takes back the seat of a child that never began.
    pub fn give_back(&mut self, seat: SeatNumber) {
        self.free.push(seat);
    }
}
