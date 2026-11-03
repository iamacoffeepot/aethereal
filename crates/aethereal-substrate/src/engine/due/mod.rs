//! Deadlines: the requests filed with one, kept by when each is due; what the engine says to the asker of each when it
//! comes; and the wake it asks of its host for the earliest, as a turn's `Sleep` is woken (ADR-0041).

#[cfg(test)]
mod tests;

use super::{Engine, ROOTS, parity};
use crate::lives;
use aethereal_actor::{Due, MailId, Mailed, SeatNumber, Seated, Tie, Time, Unanswered};
use aethereal_data::{ActorId, KindId};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::binary_heap::PeekMut;

/// A request filed with a deadline, as the engine keeps it: when it is due, its asker, and the asker's count of it.
///
/// Ordered by when it is due first, so the earliest is found first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Expiring {
    at: Time,
    seat: SeatNumber,
    asker: u64,
    count: u64,
}

impl Expiring {
    /// The request of the count `count`, filed beside `asker`, due at `at`.
    pub(super) const fn at(at: Time, asker: Seated, count: u64) -> Self {
        Self { at, seat: asker.seat, asker: asker.id.bits(), count }
    }

    /// The actor it is filed beside.
    const fn asker(self) -> Seated {
        Seated { id: ActorId::new(self.asker), seat: self.seat }
    }
}

/// The requests filed with a deadline, by when each is due, the earliest first (ADR-0041).
///
/// A request answered before its deadline stays here until the deadline comes, when it is found no longer filed and
/// passed over: an answer is handed to its asker in a worker, where the engine's own keeping is not reached.
#[derive(Default)]
pub(super) struct Deadlines {
    due: BinaryHeap<Reverse<Expiring>>,
}

impl Deadlines {
    /// Keeps each of `expiring`, emptying it.
    pub(super) fn keep(&mut self, expiring: &mut Vec<Expiring>) {
        // Most steps file no deadline.
        if !expiring.is_empty() {
            self.due.extend(expiring.drain(..).map(Reverse));
        }
    }

    /// When the earliest it keeps is due: nothing when it keeps none.
    fn earliest(&self) -> Option<Time> {
        self.due.peek().map(|Reverse(expiring)| expiring.at)
    }

    /// Takes out the earliest it keeps, when it is due by `until`.
    fn take_due(&mut self, until: Time) -> Option<Expiring> {
        let earliest = self.due.peek_mut()?;
        let is_due = earliest.0.at <= until;
        if !is_due {
            return None;
        }
        let Reverse(expiring) = PeekMut::pop(earliest);

        Some(expiring)
    }
}

/// The wake the engine has asked of its host for its earliest deadline (ADR-0041).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Alarm {
    /// None: no deadline waits, or the host had no runner of jobs when one did.
    Unset,
    /// Its host's timer wakes it at this time, as a turn's `Sleep` is woken, unless it was asked for an earlier one
    /// since.
    Set(Time),
    /// The host's runner answered a wake before its time came, as a host with no timer does: the engine asks for none
    /// again until the host hands it a runner, and expires what is due only as its steps run.
    Unkept,
}

impl Engine {
    /// Answers `Expired` every request filed with a deadline that has come by `now`, as the engine's own mail to its
    /// asker, from the asker, the first of a chain of its own, as a job's answer is: the request is filed again under
    /// the count of that mail, so the mail is handed its context and an answer that comes later is dropped (ADR-0041).
    /// Then asks its host to wake it for the earliest still to come.
    ///
    /// Run between steps, before a step's turns, and as the host's wake for a deadline is brought in.
    pub(super) fn expire(&mut self, now: Time) {
        // Most steps find nothing due.
        let has_come = self.deadlines.earliest().is_some_and(|at| at <= now);
        if has_come {
            self.expire_due(now);
        }
        self.arm();
    }

    /// As [`Self::expire`], once a deadline has come.
    #[cold]
    #[inline(never)]
    fn expire_due(&mut self, now: Time) {
        let parity = parity(self.step);
        let mut arena = self.arena(parity, ROOTS, 0);
        let mut mailed = Vec::new();
        while let Some(expiring) = self.deadlines.take_due(now) {
            let asker = expiring.asker();
            let Some(life) = self.lives.shard(asker.seat.number()).life(asker) else {
                continue;
            };
            let count = life.sent() + 1;
            let part = self.part(asker.seat);
            let Some(held) = self.actors[part].get_mut(asker) else {
                continue;
            };
            // A request answered before its deadline is no longer filed.
            if !held.waiting.expire(expiring.count, count) {
                continue;
            }
            let Ok(bytes) = arena.write_mail(&Unanswered::Expired) else {
                unreachable!("a reason why a request was not answered is a byte, and refers to nothing");
            };
            self.lives.fold_records([lives::sent(asker, count)], &[]);
            let (kind, tie) = (KindId::of::<Unanswered>(), Tie::unanswered(count));
            mailed.push(Mailed { by: asker, to: asker, count, kind, bytes, tie, due: Due::Never });
        }
        self.arenas[parity][ROOTS] = arena;
        // Each is the first of its chain: nothing the asker did in the step it is brought in caused it.
        self.fold_brought(mailed, Vec::new(), parity, |mailed| MailId { sender: mailed.by.id, count: mailed.count });
    }

    /// Asks the host to wake the engine at its earliest deadline, when it has not asked for that one or an earlier
    /// one already: a `Sleep` run on the host's runner of jobs, on the engine's own behalf, whose finish rings the
    /// engine's bell as a turn's does (ADR-0010, ADR-0018, ADR-0041).
    pub(super) fn arm(&mut self) {
        let Some(earliest) = self.deadlines.earliest() else {
            return;
        };
        let is_asked = match self.alarm {
            Alarm::Set(at) => at <= earliest,
            Alarm::Unkept => true,
            Alarm::Unset => false,
        };
        if is_asked {
            return;
        }
        if self.hand_out_alarm(earliest) {
            self.alarm = Alarm::Set(earliest);
        }
    }

    /// Takes in the host's wake for the deadline at `at`, which `rang` when its time came: what is due by then is
    /// expired, and the wake for the next is asked. A wake answered before its time is one the host cannot keep.
    pub(super) fn woken(&mut self, at: Time, rang: bool) {
        let is_current = self.alarm == Alarm::Set(at);
        if !rang {
            if is_current {
                self.alarm = Alarm::Unkept;
            }
            return;
        }
        if is_current {
            self.alarm = Alarm::Unset;
        }
        self.expire(at);
    }

    /// Asks again for the wake for the earliest deadline, with the runner of jobs the host has just handed the engine.
    pub(super) fn rearm(&mut self) {
        self.alarm = Alarm::Unset;
        self.arm();
    }
}
