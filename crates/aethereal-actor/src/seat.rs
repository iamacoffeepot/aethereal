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

/// The seats one part of the engine gives the children its turns spawn (ADR-0011, ADR-0013).
///
/// The engine keeps its seats in parts, a part holding every `stride`th
/// number from its own. Each part gives seats of its own, its free ones first
/// and then new numbers, while it has room, so a child sits beside the parent
/// that spawned it. Past that, it gives the seats in other parts the engine set
/// aside for it as the step began, and then its own again. What a part gives
/// depends only on the parts, never on how many workers run them, so a step
/// seats the same children in the same seats on any number of workers.
#[derive(Debug, Default)]
pub struct Seats {
    /// The part's free seats.
    free: Vec<SeatNumber>,
    /// The part's next new number, first; then, while the seats are lent to a root's `begin`, every other part's.
    next: Vec<u64>,
    /// How far apart one part's numbers are: one for each part.
    stride: u64,
    /// How many more children the part may seat in its own seats in this step.
    room: usize,
    /// The seats in other parts set aside for the children past its room, taken last first.
    aside: Vec<SeatNumber>,
    /// Who gives the seats: the part, or the host of a guest, whose seats are its part's (ADR-0015).
    giver: Giver,
}

/// Who gives a turn's children their seats.
#[derive(Debug, Default, Clone, Copy)]
enum Giver {
    /// The part the seats are of, from its own and those set aside for it.
    #[default]
    Part,
    /// A guest's host, from the seats of the part its actor sits in: `take` asks it for one, and `give_back` hands
    /// one back that a child that never began was given.
    Host { take: fn() -> SeatNumber, give_back: fn(SeatNumber) },
}

impl Seats {
    /// The seats of the part whose first new number is `first`, of parts numbered every `stride`th: none free, and
    /// no room until a step gives it some.
    #[must_use]
    pub fn of_part(first: u64, stride: u64) -> Self {
        Self { free: Vec::new(), next: vec![first], stride, room: 0, aside: Vec::new(), giver: Giver::Part }
    }

    /// No free seat, and new numbers from zero, with room for every child: the seats of an engine kept in one part.
    #[must_use]
    pub fn one_part() -> Self {
        Self { free: Vec::new(), next: vec![0], stride: 1, room: usize::MAX, aside: Vec::new(), giver: Giver::Part }
    }

    /// Seats a guest's host gives, from the seats of the part its actor sits in: `take` asks it for one, and
    /// `give_back` hands back one a child that never began was given (ADR-0015).
    #[must_use]
    pub fn of_host(take: fn() -> SeatNumber, give_back: fn(SeatNumber)) -> Self {
        Self { giver: Giver::Host { take, give_back }, ..Self::default() }
    }

    /// Whether a guest's host gives them.
    #[must_use]
    pub const fn is_of_host(&self) -> bool {
        matches!(self.giver, Giver::Host { .. })
    }

    /// Whether `seat` is one of this part's own, and not one set aside for it in another part.
    #[must_use]
    pub fn is_own(&self, seat: SeatNumber) -> bool {
        let stride = self.stride.max(1);

        seat.number() % stride == self.next[0] % stride
    }

    /// Gives the part room for `room` children in its own seats, for the step beginning.
    pub const fn make_room(&mut self, room: usize) {
        self.room = room;
    }

    /// Sets `seat`, in another part, aside for a child past the part's room.
    pub fn set_aside(&mut self, seat: SeatNumber) {
        self.aside.push(seat);
    }

    /// How many seats in other parts are set aside for the part.
    #[must_use]
    pub const fn aside(&self) -> usize {
        self.aside.len()
    }

    /// Lends a root's `begin` the new numbers of every other part, `others`, in the order of the parts, to seat the
    /// children past the part's room in whichever has numbered fewest.
    pub fn lend(&mut self, others: impl IntoIterator<Item = u64>) {
        self.next.extend(others);
    }

    /// Takes back the other parts' new numbers lent by [`Self::lend`], in the order they were lent.
    pub fn take_lent(&mut self) -> impl Iterator<Item = u64> + '_ {
        self.next.drain(1..)
    }

    /// Frees `seat`, of this part, once the actor in it has gone, to be given again.
    pub fn free(&mut self, seat: SeatNumber) {
        self.free.push(seat);
    }

    /// A seat of this part's own, for the engine to set aside for another part's children.
    pub fn own(&mut self) -> SeatNumber {
        self.free.pop().unwrap_or_else(|| {
            let seat = SeatNumber(self.next[0]);
            self.next[0] += self.stride.max(1);

            seat
        })
    }

    /// The part's next new number.
    #[must_use]
    pub fn own_next(&self) -> u64 {
        self.next[0]
    }

    /// Sets the part's next new number to `next`, where a root's `begin` left it.
    pub fn set_next(&mut self, next: u64) {
        self.next[0] = next;
    }

    /// The seats the part holds and has not given: its free ones and those set aside for it in other parts, and its
    /// next new number.
    pub fn held(&self) -> (impl Iterator<Item = SeatNumber> + '_, u64) {
        (self.free.iter().chain(&self.aside).copied(), self.next[0])
    }

    /// The part's free seats, and its new numbers, for a test to read.
    #[must_use]
    pub fn into_parts(self) -> (Vec<SeatNumber>, Vec<u64>) {
        (self.free, self.next)
    }

    /// A seat for a new actor: the part's own while it has room, then one set aside in another part, then, while the
    /// seats are lent to a root's `begin`, a new number of the part that has numbered fewest, and past all of those
    /// the part's own again.
    pub fn take(&mut self) -> SeatNumber {
        if let Giver::Host { take, .. } = self.giver {
            return take();
        }
        if self.room > 0 {
            self.room -= 1;

            return self.own();
        }
        if let Some(seat) = self.aside.pop() {
            return seat;
        }
        let Some(next) = self.next.iter_mut().skip(1).min() else {
            return self.own();
        };
        let seat = SeatNumber(*next);
        *next += self.stride.max(1);

        seat
    }

    /// Takes back the seat of a child that never began: among the part's own when it is one, else among those set
    /// aside.
    pub fn give_back(&mut self, seat: SeatNumber) {
        if let Giver::Host { give_back, .. } = self.giver {
            give_back(seat);
            return;
        }
        if self.is_own(seat) {
            self.free.push(seat);
        } else {
            self.aside.push(seat);
        }
    }
}
