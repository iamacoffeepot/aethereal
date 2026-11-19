//! What the engine keeps beside an actor of the requests it holds, until it answers them or lets them go (ADR-0017).

#[cfg(test)]
mod tests;

use super::{REQUESTS_KEPT, ask};
use crate::Context;
use crate::room::{trim, trim_table};
use aethereal_actor::{Asked, Fault, NotOwed, Owed, Owes, Seated, Tie, holders};
use aethereal_data::KindId;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::mem;

/// The debts an actor holds, beside it in its seat: each request it held and has not answered (ADR-0017). Most actors
/// never hold one, and every actor's seat holds one of these, so one that has never held holds no more than a
/// pointer's width.
#[derive(Default)]
pub(super) struct Debts {
    held: Holds,
}

/// Whether an actor has held a request yet.
#[derive(Default)]
enum Holds {
    /// It has never held one.
    #[default]
    Never,
    /// It has: what it owes now, behind a pointer.
    Owes(Box<Owing>),
}

/// The debts an actor that has held a request owes now, each under the request's id, so a turn that holds or answers
/// one finds its place without a walk over the others, however many it owes.
///
/// Each table gives back its room past [`REQUESTS_KEPT`] requests' once it is emptied.
#[derive(Default)]
struct Owing {
    debts: FxHashMap<RequestId, Debt>,
    /// The debts the turn being filed has answered, in the order it answered them, for it to put back when it then
    /// answers one it does not owe. Emptied once each turn is filed.
    answered: Vec<(RequestId, Debt)>,
}

/// A request's id: its asker's id and its count of the request, which name one request for the life of the engine
/// (ADR-0008, ADR-0012).
type RequestId = (u64, u64);

/// One request an actor holds, and what the engine says of it if it is let go of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Debt {
    /// Who asked, and its count of the request.
    pub(super) asker: Seated,
    pub(super) request: u64,
    /// The kind it is to be answered with.
    answer: KindId,
    /// How many values hold it: held requests, and bytes they were written into.
    holders: i64,
    /// The request's chain, where what the engine says of it goes.
    pub(super) chain: Context,
}

impl Debts {
    /// Files what the closing turn of the actor `me`, at `chain`, did to its debts, taken out of `asked`: a debt it
    /// answered is not owed, and the rest are left for the engine to say the actor ended owing. A closing turn that
    /// answered a debt its actor does not hold fails with [`NotOwed`], and one that failed files nothing (ADR-0017).
    #[cold]
    #[inline(never)]
    pub(super) fn settle_closing(
        &mut self,
        me: Seated,
        chain: Context,
        asked: &mut Asked,
        outcome: &mut Result<(), Fault>,
    ) {
        if outcome.is_ok() {
            *outcome = self.file(me, asked.owes(), chain).map_err(Fault::from);
        }
        asked.take_owes().for_each(drop);
    }

    /// Files what the turn of the actor `me` did to its debts, taken out of `asked`: a request it held is owed, and a
    /// debt it answered is not. The turn handled mail tied as `tie` from `source`, at `chain`, having sent `sent`
    /// pieces of mail before it, and ended as `outcome` says. A turn that answered a debt it does not hold, of the
    /// kind it answered with, fails with [`NotOwed`], and a turn that failed files nothing. Then its asker is told
    /// what became of the request the turn handled, as for a turn that held nothing (ADR-0017).
    #[cold]
    #[inline(never)]
    pub(super) fn settle(
        &mut self,
        me: Seated,
        (source, tie, chain, sent): (Seated, Tie, Context, u64),
        asked: &mut Asked,
        outcome: &mut Result<(), Fault>,
    ) {
        if outcome.is_ok() {
            *outcome = self.file(me, asked.owes(), chain).map_err(Fault::from);
        }
        // What the turn held is read before it is taken: a request it held is answered later.
        ask::owed(me, source, tie, outcome, sent, asked);
        asked.take_owes().for_each(drop);
    }

    /// Files what a turn of `me` at `chain` did to its debts, `owes`, in place: or that it answered one it does not
    /// hold, or held one it owes already, when it changes none of them.
    fn file(&mut self, me: Seated, owes: &[Owes], chain: Context) -> Result<(), NotOwed> {
        match &mut self.held {
            Holds::Owes(owing) => owing.file(me, owes, chain),
            // An actor is given room for its debts once a turn of it holds one and is filed whole.
            Holds::Never => {
                let mut owing = Box::<Owing>::default();
                owing.file(me, owes, chain)?;
                self.held = Holds::Owes(owing);

                Ok(())
            }
        }
    }

    /// Changes how many values hold the debt `owed` names by `by`, and answers with it when none is left: it is let go
    /// of, and no longer owed. Nothing for a debt it does not owe, one answered or let go of already.
    pub(super) fn change(&mut self, owed: Owed, by: i64) -> Option<Debt> {
        let Holds::Owes(owing) = &mut self.held else {
            return None;
        };
        let Entry::Occupied(mut filed) = owing.debts.entry(request_id(owed.asker, owed.request)) else {
            return None;
        };
        // The asker's seat is part of what a held request names, as its id is.
        if filed.get().asker != owed.asker {
            return None;
        }
        let debt = filed.get_mut();
        debt.holders += by;
        if debt.holders > 0 {
            return None;
        }
        let debt = filed.remove();
        trim_table(&mut owing.debts, REQUESTS_KEPT);

        Some(debt)
    }

    /// Whether it still owes the request `asker` asked as its `request`th piece of mail.
    pub(super) fn owes(&self, asker: Seated, request: u64) -> bool {
        let Holds::Owes(owing) = &self.held else {
            return false;
        };

        owing.debts.get(&request_id(asker, request)).is_some_and(|debt| debt.asker == asker)
    }

    /// Takes every debt still owed: for an actor that has ended, whose askers are each told so.
    pub(super) fn take_all(&mut self) -> Vec<Debt> {
        match mem::take(&mut self.held) {
            Holds::Owes(owing) => owing.into_debts(),
            Holds::Never => Vec::new(),
        }
    }
}

impl Owing {
    /// Every debt it owes, in the order its map holds them, which its hash, the same on every run, makes the same on
    /// every run of the same steps.
    #[cold]
    #[inline(never)]
    fn into_debts(self) -> Vec<Debt> {
        self.debts.into_values().collect()
    }

    /// Files what a turn of `me` at `chain` did to its debts, `owes`, as [`Self::file_in_order`] does, then gives back
    /// the room of each of its tables that is empty.
    fn file(&mut self, me: Seated, owes: &[Owes], chain: Context) -> Result<(), NotOwed> {
        let filed = self.file_in_order(me, owes, chain);
        // Emptied either way: let go of once the turn is filed, or put back from as it failed.
        self.answered.clear();
        trim(&mut self.answered, REQUESTS_KEPT);
        trim_table(&mut self.debts, REQUESTS_KEPT);

        filed
    }

    /// Files what a turn of `me` at `chain` did to its debts, `owes`, in the order it did it: a request it held is owed,
    /// and a debt it answered is not. At the first it does not owe, a debt it answered that it does not hold of that
    /// kind or a request it held that is owed already, what it filed before is put back, so a turn that fails with
    /// [`NotOwed`] changes no debt.
    fn file_in_order(&mut self, me: Seated, owes: &[Owes], chain: Context) -> Result<(), NotOwed> {
        for (at, done) in owes.iter().enumerate() {
            let filed = match *done {
                Owes::Holds { owed, answer } => self.hold(owed, answer, chain),
                Owes::Answers { owed, answer } => self.answer(me, owed, answer),
            };
            if let Err(not_owed) = filed {
                self.put_back(&owes[..at]);

                return Err(not_owed);
            }
        }

        Ok(())
    }

    /// Files the request `owed` names as owed, to be answered with a kind `answer` names, on the chain `chain`: unless
    /// it is owed already.
    fn hold(&mut self, owed: Owed, answer: KindId, chain: Context) -> Result<(), NotOwed> {
        let Entry::Vacant(place) = self.debts.entry(request_id(owed.asker, owed.request)) else {
            return Err(NotOwed);
        };
        place.insert(Debt { asker: owed.asker, request: owed.request, answer, holders: 1, chain });

        Ok(())
    }

    /// Answers the debt `owed` names, by `me` with a kind `answer` names, keeping it to be put back: unless `me` does
    /// not owe it, or owes it another kind.
    fn answer(&mut self, me: Seated, owed: Owed, answer: KindId) -> Result<(), NotOwed> {
        let Entry::Occupied(filed) = self.debts.entry(request_id(owed.asker, owed.request)) else {
            return Err(NotOwed);
        };
        if !filed.get().answers(me, owed, answer) {
            return Err(NotOwed);
        }
        self.answered.push(filed.remove_entry());

        Ok(())
    }

    /// Puts back what a turn filed of `owes`, the last first, as it was before the turn: a request it held is not
    /// owed, and a debt it answered is owed again.
    fn put_back(&mut self, owes: &[Owes]) {
        for done in owes.iter().rev() {
            match *done {
                Owes::Holds { owed, .. } => {
                    self.debts.remove(&request_id(owed.asker, owed.request));
                }
                Owes::Answers { .. } => {
                    if let Some((id, debt)) = self.answered.pop() {
                        self.debts.insert(id, debt);
                    }
                }
            }
        }
    }
}

impl Debt {
    /// Whether answering `owed`, filed under this debt's request, with a kind `answer` names, by the actor `me`,
    /// answers this debt.
    fn answers(&self, me: Seated, owed: Owed, answer: KindId) -> bool {
        let is_mine = owed.holder.id == me.id;
        let is_kind = answer == self.answer;

        is_mine && is_kind
    }
}

/// The id of the request `asker` asked as its `request`th piece of mail.
const fn request_id(asker: Seated, request: u64) -> RequestId {
    (asker.id.bits(), request)
}

/// Sums the changes `changes` holds to how many values hold each debt, in place: each debt once, in the order of the
/// debts, and none whose changes come to nothing, as they change nothing. So a debt handed from one value to another
/// in a part is never seen with none.
pub(super) fn sum(changes: &mut Vec<holders::Change>) {
    changes.sort_unstable_by_key(|(owed, _)| order(*owed));
    changes.dedup_by(|(owed, by), (kept, total)| {
        let is_same = owed == kept;
        if is_same {
            *total += *by;
        }

        is_same
    });
    changes.retain(|(_, by)| *by != 0);
}

/// Where the debt `owed` names falls among debts: by its holder's seat, then its request's id. Every field of it
/// counts, so two debts fall together only when they are one.
const fn order(owed: Owed) -> (u64, u64, u64, u64, u64) {
    let Owed { holder, asker, request } = owed;

    (holder.seat.number(), holder.id.bits(), asker.id.bits(), request, asker.seat.number())
}
