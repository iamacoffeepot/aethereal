//! What the engine keeps beside an asker while its requests and jobs wait for their answers, and how it hands an
//! answer back with what the asker kept (ADR-0017, ADR-0018).

#[cfg(test)]
mod tests;
#[cfg(test)]
mod unreadable;

use super::REQUESTS_KEPT;
use super::due::Expiring;
use crate::room::trim_table;
use aethereal_actor::{Asked, Asking, Awaited, Due, Fault, Item, JobId, Mail, MailId, Seated, Tie, Tied, Unanswered};
use rustc_hash::FxHashMap;

/// The contexts an actor's requests and jobs keep until they are answered, beside it in its seat, under each one's
/// count (ADR-0017, ADR-0018). Dropped with the actor, once the host is asked to stop each of its jobs.
///
/// Most actors never ask, and every actor's seat holds one, so one that has never asked holds no more than a
/// pointer's width.
#[derive(Default)]
pub(super) struct Waiting {
    asks: Asks,
}

/// Whether an actor has asked anything yet.
#[derive(Default)]
enum Asks {
    /// It has never asked.
    #[default]
    Never,
    /// It has: what it waits on now, behind a pointer.
    Waits(Box<Filed>),
}

/// The requests and jobs an actor that has asked waits on now, each with its context, under its count: so an answer
/// finds its context without a walk over the others, however many the actor waits on.
type Filed = FxHashMap<u64, Awaited>;

/// What an item is to the asker it is handed to.
#[derive(Clone, Copy)]
pub(super) enum Answering<'w> {
    /// No answer: the item goes as it came.
    Not,
    /// The answer to a request it asked, and what was kept of the request, which is kept until the turn has run.
    Kept(&'w Asking),
    /// What the engine says of a request it asked that was not answered, and what was kept of the request, which is
    /// kept until the turn has run.
    Unanswered(&'w Asking),
    /// An answer to no request it waits on, which no turn is run for.
    Stray,
}

impl Waiting {
    /// Settles what the turn of the actor `me` of an item tied as `tie`, which returned `outcome`, leaves: the context
    /// its answer was handed with is let go of, the requests it asked are filed from `asked`, and the contexts of the
    /// jobs it cancelled are let go of, so no turn is handed their answers (ADR-0018); none of a turn that failed, whose
    /// mail was never sent. What the `begin`s of the children it stood up asked is left in `asked`, to
    /// be filed beside each as it is seated. Each request it files with a deadline is added to `expiring` (ADR-0041).
    pub(super) fn settle(
        &mut self,
        me: Seated,
        tie: Tie,
        asked: &mut Asked,
        outcome: &Result<(), Fault>,
        expiring: &mut Vec<Expiring>,
    ) {
        // Most mail is no answer.
        if !tie.is_tell() {
            self.forget(tie);
        }
        // Most turns ask nothing.
        if asked.has_asked() {
            if outcome.is_ok() {
                self.file(me, asked.take_asks_by(me), expiring);
            } else {
                asked.take_asks().for_each(drop);
            }
        }
        // Most turns cancel nothing. A job may be cancelled in the turn that ran it, so it is filed first.
        if outcome.is_ok() {
            for job in &asked.buffers().cancelled {
                self.forget_count(job.mail().count);
            }
        }
    }

    /// What the new actor `me` waits on: the requests its `begin` asked, each with a deadline added to `expiring`.
    pub(super) fn of_begin(me: Seated, asks: impl Iterator<Item = Awaited>, expiring: &mut Vec<Expiring>) -> Self {
        let mut waiting = Self::default();
        waiting.file(me, asks, expiring);

        waiting
    }

    /// Lets go of the context the request mail tied as `tie` answers kept, once its answer has been handled.
    #[cold]
    fn forget(&mut self, tie: Tie) {
        let (Tied::Answers(count) | Tied::Unanswered(count)) = tie.read() else {
            return;
        };
        self.forget_count(count);
    }

    /// Lets go of the context the request of the count `count` kept, when it still waits on it, and the room of the
    /// requests it waited on when it waits on none now.
    fn forget_count(&mut self, count: u64) {
        if let Asks::Waits(filed) = &mut self.asks {
            filed.remove(&count);
            trim_table(filed, REQUESTS_KEPT);
        }
    }

    /// Files the requests a turn of the actor `me` asked, each with its context, and adds each with a deadline to
    /// `expiring`, for the engine to keep by when it is due (ADR-0041).
    ///
    /// Kept out of line: every actor's birth calls it, through [`Self::of_begin`], and most file nothing. Inlined there,
    /// the map's insert made a spawn dearer, by about 3 percent of a piece of mail in `churn`, measured.
    #[inline(never)]
    pub(super) fn file(&mut self, me: Seated, asks: impl Iterator<Item = Awaited>, expiring: &mut Vec<Expiring>) {
        let mut asks = asks.peekable();
        // Most turns ask nothing.
        if asks.peek().is_none() {
            return;
        }
        if matches!(self.asks, Asks::Never) {
            self.asks = Asks::Waits(Box::default());
        }
        if let Asks::Waits(filed) = &mut self.asks {
            for awaited in asks {
                let asking = awaited.asking();
                if let Due::Before(at) = asking.due {
                    expiring.push(Expiring::at(at, me, asking.count));
                }
                filed.insert(asking.count, awaited);
            }
        }
    }

    /// Files again, under `notice`, the request of the count `count` it still waits on, whose deadline has come:
    /// `notice` is the count of what the engine says of it, which hands it its context, and an answer to the request
    /// that comes later finds nothing filed under its count, and is dropped (ADR-0041). Answers whether it still
    /// waited on it.
    pub(super) fn expire(&mut self, count: u64, notice: u64) -> bool {
        let Asks::Waits(filed) = &mut self.asks else {
            return false;
        };
        let Some(mut awaited) = filed.remove(&count) else {
            return false;
        };
        awaited.asking_mut().count = notice;
        filed.insert(notice, awaited);

        true
    }

    /// The kind it waits on the request of the count `count` to be answered with, when it still waits on it.
    #[cfg(any(test, feature = "harness"))]
    pub(super) fn answer_of(&self, count: u64) -> Option<aethereal_data::KindId> {
        let Asks::Waits(filed) = &self.asks else {
            return None;
        };

        filed.get(&count).map(|awaited| awaited.asking().answer)
    }

    /// Adds to `cancelled` each job the actor `me` still waits on as it ends, so the host is asked to stop each it was
    /// handed and the engine drops each still waiting: no turn of the actor is handed their answers, and a job that
    /// never finishes on its own would otherwise hold what it was lent and given (ADR-0018). One its closing turn
    /// cancelled, already there, is not added again.
    pub(super) fn cancel_jobs(&self, me: Seated, cancelled: &mut Vec<JobId>) {
        let Asks::Waits(filed) = &self.asks else {
            return;
        };
        let closing = cancelled.len();
        for awaited in filed.values() {
            let Awaited::Job(asking) = awaited else {
                continue;
            };
            let job = JobId::new(MailId { sender: me.id, count: asking.count });
            // Most closing turns cancel none, so the walk over what they did is short.
            if !cancelled[..closing].contains(&job) {
                cancelled.push(job);
            }
        }
    }

    /// What mail tied as `tie` is to the actor: when it is an answer, the context it kept for the request it answers,
    /// which it keeps until [`Self::settle`].
    #[allow(clippy::inline_always, reason = "once for every item, read inline in the loop that hands it")]
    #[inline(always)]
    pub(super) fn answering(&self, tie: Tie) -> Answering<'_> {
        // Most mail is no answer.
        if tie.is_tell() {
            return Answering::Not;
        }
        self.answering_tied(tie)
    }

    /// As [`Self::answering`], for mail tied to a request.
    #[inline(never)]
    fn answering_tied(&self, tie: Tie) -> Answering<'_> {
        let (count, is_answer) = match tie.read() {
            Tied::Tell | Tied::Asks(_) => return Answering::Not,
            Tied::Answers(count) => (count, true),
            Tied::Unanswered(count) => (count, false),
        };
        let Asks::Waits(filed) = &self.asks else {
            return Answering::Stray;
        };
        let Some(awaited) = filed.get(&count) else {
            return Answering::Stray;
        };
        let asking = awaited.asking();
        if is_answer {
            Answering::Kept(asking)
        } else {
            Answering::Unanswered(asking)
        }
    }
}

/// `mail`, handed to an asker that is `answering` it: an answer, or what the engine says of a request that was not,
/// with the context it kept; or the mail as it came.
#[allow(clippy::inline_always, reason = "once for every item, read inline in the loop that hands it")]
#[inline(always)]
pub(super) const fn item<'a>(mail: Mail<'a>, answering: Answering<'a>) -> Item<'a> {
    match answering {
        Answering::Kept(asking) => Item::Answer { mail, asking },
        Answering::Unanswered(asking) => Item::Unanswered { mail, asking },
        Answering::Not | Answering::Stray => Item::Mail(mail),
    }
}

/// Says to `source`, the asker, what became of its request to `me`, when the mail tied as `tie` is one and the turn that
/// handled it, which had sent `sent` pieces of mail before it and ended as `outcome` says, did not answer it: `Failed`,
/// `Unreadable` when its handler never ran as the request did not read, or `Unreplied`. The mail is added to what the
/// turn sent, after it, as `me`'s next; of a failed turn it is all that is sent (ADR-0017).
#[cold]
#[inline(never)]
pub(super) fn owed(me: Seated, source: Seated, tie: Tie, outcome: &Result<(), Fault>, sent: u64, asked: &mut Asked) {
    let Tied::Asks(request) = tie.read() else {
        return;
    };
    let (why, count) = if outcome.is_err() {
        // What a failed turn sent is dropped, its counts with it.
        (Unanswered::Failed, sent + 1)
    } else if asked.has_answered(me, sent, source, request) {
        return;
    } else if asked.was_unread(me) {
        (Unanswered::Unreadable, sent + 1)
    } else {
        (Unanswered::Unreplied, asked.last_sent(me).unwrap_or(sent) + 1)
    };
    let mailed = asked.unanswered(me, source, count, request, why);
    asked.buffers_mut().mailed.push(mailed);
}

/// The order, in its asker's stream, of what the engine says of the request of the count `request` to an actor that
/// had ended or was gone when it was to be handed to it: after any mail that actor sent, which its count orders,
/// and before its ending. Each request is answered once, so no two share one.
pub(super) const fn ended_order(request: u64) -> u64 {
    (1 << 63) | request
}
