//! A part of a view kept under actors' seats (ADR-0011).

use crate::Part;
use aethereal_actor::Seated;
use aethereal_data::ActorId;
use std::num::NonZeroUsize;

/// What one part of a view keeps for each actor, under the actor's seat (ADR-0011).
///
/// A part holds every `parts`th seat, so the seats it holds are numbered in
/// a list, front to back, and finding an actor is reading one place in it.
/// A seat is taken again once its actor has gone, so the id of the actor kept
/// in each seat is kept with it: a lookup for any other actor finds nothing,
/// and keeping a value for a new actor in the seat drops the old one's.
pub struct BySeat<V> {
    /// The seats this part holds, in order.
    seats: Vec<Slot<V>>,
    /// How many parts the view has: the step between the seats this part holds.
    parts: NonZeroUsize,
}

/// One seat of a part.
enum Slot<V> {
    /// Nothing is kept for any actor in this seat.
    Empty,
    /// The value kept for the actor `id`, which sits in this seat.
    Taken { id: ActorId, value: V },
}

impl<V> BySeat<V> {
    /// What is kept for `actor`, when it is the actor kept in its seat.
    #[must_use]
    pub fn get(&self, actor: Seated) -> Option<&V> {
        match self.seats.get(self.place(actor)) {
            Some(Slot::Taken { id, value }) if *id == actor.id => Some(value),
            _ => None,
        }
    }

    /// What is kept for `actor`, to change, when it is the actor kept in its seat.
    pub fn get_mut(&mut self, actor: Seated) -> Option<&mut V> {
        let place = self.place(actor);
        match self.seats.get_mut(place) {
            Some(Slot::Taken { id, value }) if *id == actor.id => Some(value),
            _ => None,
        }
    }

    /// What is kept for `actor`, kept first as `make` makes it when the seat is empty or another actor is kept in
    /// it, whose value is dropped.
    pub fn entry(&mut self, actor: Seated, make: impl FnOnce() -> V) -> &mut V {
        let place = self.place(actor);
        if place >= self.seats.len() {
            self.seats.resize_with(place + 1, || Slot::Empty);
        }

        let slot = &mut self.seats[place];
        let is_kept = matches!(slot, Slot::Taken { id, .. } if *id == actor.id);
        if !is_kept {
            *slot = Slot::Taken { id: actor.id, value: make() };
        }
        let Slot::Taken { value, .. } = slot else {
            unreachable!("the seat was just taken");
        };

        value
    }

    /// Drops what is kept for `actor`, when it is the actor kept in its seat.
    pub fn remove(&mut self, actor: Seated) {
        let place = self.place(actor);
        let is_kept = matches!(self.seats.get(place), Some(Slot::Taken { id, .. }) if *id == actor.id);
        if is_kept {
            self.seats[place] = Slot::Empty;
        }
    }

    /// Where `actor`'s seat lies in this part's list.
    fn place(&self, actor: Seated) -> usize {
        actor.seat.index() / self.parts.get()
    }
}

impl<V> Part for BySeat<V> {
    fn empty(parts: NonZeroUsize) -> Self {
        Self { seats: Vec::new(), parts }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethereal_actor::SeatNumber;

    /// The actor `bits` names, in the seat numbered `seat`.
    const fn seated(bits: u64, seat: u64) -> Seated {
        Seated { id: ActorId::new(bits), seat: SeatNumber::new(seat) }
    }

    #[test]
    fn seat_taken_again_answers_only_for_the_actor_in_it() {
        // Catches a lookup that reads a seat without its id, so a gone actor reads its successor's value, a new
        // actor that keeps its predecessor's value, and a removal by a gone actor that empties its successor's seat.
        let Some(parts) = NonZeroUsize::new(3) else {
            panic!("three is not zero");
        };
        let mut part = BySeat::empty(parts);
        let (gone, next) = (seated(1, 7), seated(2, 7));
        *part.entry(gone, || 0) += 5;
        assert_eq!(part.get(gone), Some(&5));

        *part.entry(next, || 0) += 1;
        assert_eq!(part.get(next), Some(&1));
        assert_eq!(part.get(gone), None);

        part.remove(gone);
        assert_eq!(part.get(next), Some(&1));
        part.remove(next);
        assert_eq!(part.get(next), None);
    }
}
