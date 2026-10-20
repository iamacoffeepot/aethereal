//! What the engine keeps beside an actor of the requests it holds, until it answers them or lets them go (ADR-0017).

#[cfg(test)]
mod tests;

use super::ask;
use crate::Context;
use aethereal_actor::{Asked, Fault, NotOwed, Owed, Owes, Seated, Tie, holders};
use aethereal_data::KindId;
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

/// The debts an actor that has held a request owes now.
#[derive(Default)]
struct Owing {
    debts: Vec<Debt>,
}

/// What filing a turn's debts comes to.
enum Filed {
    /// The debts owed after it.
    Owes(Vec<Debt>),
    /// It answered a debt its actor does not hold.
    NotOwed,
}

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
            match self.filed(me, asked.owes(), chain) {
                Filed::Owes(debts) => self.held = Holds::Owes(Box::new(Owing { debts })),
                Filed::NotOwed => *outcome = Err(NotOwed.into()),
            }
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
            match self.filed(me, asked.owes(), chain) {
                Filed::Owes(debts) => self.held = Holds::Owes(Box::new(Owing { debts })),
                Filed::NotOwed => *outcome = Err(NotOwed.into()),
            }
        }
        // What the turn held is read before it is taken: a request it held is answered later.
        ask::owed(me, source, tie, outcome, sent, asked);
        asked.take_owes().for_each(drop);
    }

    /// The debts owed once what a turn of `me` at `chain` did to them, `owes`, is filed: or that it answered one it
    /// does not hold.
    fn filed(&self, me: Seated, owes: &[Owes], chain: Context) -> Filed {
        let mut after = self.owed().to_vec();
        for owes in owes {
            match *owes {
                Owes::Holds { owed: Owed { asker, request, .. }, answer } => {
                    after.push(Debt { asker, request, answer, holders: 1, chain });
                }
                Owes::Answers { owed: answered, answer } => {
                    let Some(at) = after.iter().position(|debt| debt.answers(me, answered, answer)) else {
                        return Filed::NotOwed;
                    };
                    after.swap_remove(at);
                }
            }
        }

        Filed::Owes(after)
    }

    /// Changes how many values hold the debt `owed` names by `by`, and answers with it when none is left: it is let go
    /// of, and no longer owed. Nothing for a debt it does not owe, one answered or let go of already.
    pub(super) fn change(&mut self, owed: Owed, by: i64) -> Option<Debt> {
        let Holds::Owes(owing) = &mut self.held else {
            return None;
        };
        let debts = &mut owing.debts;
        let at = debts.iter().position(|debt| debt.asker == owed.asker && debt.request == owed.request)?;
        debts[at].holders += by;
        let is_let_go = debts[at].holders <= 0;

        is_let_go.then(|| debts.swap_remove(at))
    }

    /// Whether it still owes the request `asker` asked as its `request`th piece of mail.
    pub(super) fn owes(&self, asker: Seated, request: u64) -> bool {
        self.owed().iter().any(|debt| debt.asker == asker && debt.request == request)
    }

    /// Takes every debt still owed: for an actor that has ended, whose askers are each told so.
    pub(super) fn take_all(&mut self) -> Vec<Debt> {
        match mem::take(&mut self.held) {
            Holds::Owes(owing) => owing.debts,
            Holds::Never => Vec::new(),
        }
    }

    /// The debts owed now.
    fn owed(&self) -> &[Debt] {
        match &self.held {
            Holds::Owes(owing) => &owing.debts,
            Holds::Never => &[],
        }
    }
}

impl Debt {
    /// Whether answering `owed` with a kind `answer` names, by the actor `me`, answers this debt.
    fn answers(&self, me: Seated, owed: Owed, answer: KindId) -> bool {
        let is_mine = owed.holder.id == me.id;
        let is_asker = owed.asker.id == self.asker.id;
        let is_request = owed.request == self.request;
        let is_kind = answer == self.answer;

        is_mine && is_asker && is_request && is_kind
    }
}

/// The changes to how many values hold each debt that `changes` holds, summed for each debt, and emptied: each debt
/// once, so one handed from one value to another in a part is never seen with none.
pub(super) fn summed(changes: &mut Vec<holders::Change>) -> Vec<holders::Change> {
    let mut summed: Vec<holders::Change> = Vec::new();
    for (owed, by) in changes.drain(..) {
        match summed.iter_mut().find(|(seen, _)| *seen == owed) {
            Some((_, total)) => *total += by,
            None => summed.push((owed, by)),
        }
    }

    summed
}
