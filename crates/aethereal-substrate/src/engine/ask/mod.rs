//! What the engine keeps beside an asker while its requests wait for their answers, and how it hands an answer back
//! with what the asker kept (ADR-0017).

#[cfg(test)]
mod tests;

use aethereal_actor::{Asked, Asking, Fault, Item, Mail, Tie, Tied};

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
    /// The answer to a request it asked, and the context it kept for it, which it keeps until the turn has run.
    Kept(&'w [u8]),
    /// An answer to no request it waits on, which no turn is run for.
    Stray,
}

impl Waiting {
    /// Settles what the turn of an item tied as `tie`, which returned `outcome`, leaves: the context its answer was
    /// handed with is let go of, and the requests it asked are filed from `asked`, none of a turn that failed, whose
    /// mail was never sent.
    #[allow(
        clippy::inline_always,
        reason = "once for every turn: not inlined, it cost a piece of mail 2 percent, measured"
    )]
    #[inline(always)]
    pub(super) fn settle(&mut self, tie: Tie, asked: &mut Asked, outcome: &Result<(), Fault>) {
        if let Tied::Answers(count) = tie.read() {
            self.forget(count);
        }
        // Most turns ask nothing.
        if !asked.has_asked() {
            return;
        }
        let asks = asked.take_asks();
        if outcome.is_ok() {
            self.file(asks);
        }
    }

    /// Lets go of the context the request of the count `count` kept, once its answer has been handled.
    #[cold]
    fn forget(&mut self, count: u64) {
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
        let Tied::Answers(count) = tie.read() else {
            return Answering::Not;
        };
        let Asks::Waits(filed) = &self.asks else {
            return Answering::Stray;
        };
        let kept = filed.asks.iter().find(|asking| asking.count == count);

        kept.map_or(Answering::Stray, |asking| Answering::Kept(&asking.context))
    }
}

/// `mail`, handed to an asker that is `answering` it: an answer with the context it kept, or the mail as it came.
#[allow(clippy::inline_always, reason = "once for every item, read inline in the loop that hands it")]
#[inline(always)]
pub(super) const fn item<'a>(mail: Mail<'a>, answering: Answering<'a>) -> Item<'a> {
    match answering {
        Answering::Kept(context) => Item::Answer { mail, context },
        Answering::Not | Answering::Stray => Item::Mail(mail),
    }
}
