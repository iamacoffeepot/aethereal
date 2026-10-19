//! What the engine keeps beside an asker while its requests wait for their answers, and how it hands an answer back
//! with what the asker kept (ADR-0017).

#[cfg(test)]
mod tests;

use aethereal_actor::{Asked, Asking, Fault, Item, Mail, Seated, Tie, Tied, Unanswered};

/// The contexts an actor's requests keep until they are answered, beside it in its seat, under each request's count
/// (ADR-0017). Dropped with the actor.
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

/// The requests an actor that has asked waits on now, each with its context.
#[derive(Default)]
struct Filed {
    asks: Vec<Asking>,
}

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
    /// its answer was handed with is let go of, and the requests it asked are filed from `asked`, none of a turn that
    /// failed, whose mail was never sent. What the `begin`s of the children it stood up asked is left in `asked`, to
    /// be filed beside each as it is seated.
    pub(super) fn settle(&mut self, me: Seated, tie: Tie, asked: &mut Asked, outcome: &Result<(), Fault>) {
        // Most mail is no answer.
        if !tie.is_tell() {
            self.forget(tie);
        }
        // Most turns ask nothing.
        if !asked.has_asked() {
            return;
        }
        if outcome.is_ok() {
            self.file(asked.take_asks_by(me));
        } else {
            asked.take_asks().for_each(drop);
        }
    }

    /// What a new actor waits on: the requests its `begin` asked.
    pub(super) fn of_begin(asks: impl Iterator<Item = Asking>) -> Self {
        let mut waiting = Self::default();
        waiting.file(asks);

        waiting
    }

    /// Lets go of the context the request mail tied as `tie` answers kept, once its answer has been handled.
    #[cold]
    fn forget(&mut self, tie: Tie) {
        let (Tied::Answers(count) | Tied::Unanswered(count)) = tie.read() else {
            return;
        };
        if let Asks::Waits(filed) = &mut self.asks
            && let Some(index) = filed.asks.iter().position(|asking| asking.count == count)
        {
            filed.asks.swap_remove(index);
        }
    }

    /// Files the requests a turn asked, each with its context.
    fn file(&mut self, asks: impl Iterator<Item = Asking>) {
        let mut asks = asks.peekable();
        // Most turns ask nothing.
        if asks.peek().is_none() {
            return;
        }
        if matches!(self.asks, Asks::Never) {
            self.asks = Asks::Waits(Box::default());
        }
        if let Asks::Waits(filed) = &mut self.asks {
            filed.asks.extend(asks);
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
        let Some(asking) = filed.asks.iter().find(|asking| asking.count == count) else {
            return Answering::Stray;
        };
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
/// or `Unreplied`. The mail is added to what the turn sent, after it, as `me`'s next; of a failed turn it is all that
/// is sent (ADR-0017).
#[cold]
#[inline(never)]
pub(super) fn owed(me: Seated, source: Seated, tie: Tie, outcome: &Result<(), Fault>, sent: u64, asked: &mut Asked) {
    let Tied::Asks(request) = tie.read() else {
        return;
    };
    let (why, count) = if outcome.is_err() {
        // What a failed turn sent is dropped, its counts with it.
        (Unanswered::Failed, sent + 1)
    } else if asked.has_answered(me, source, request) {
        return;
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
