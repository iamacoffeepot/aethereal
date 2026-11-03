//! A piece of mail as the engine carries it (ADR-0012).

use aethereal_actor::{Due, MailId, Payload, SeatNumber, Tie, Tied, Time};
use aethereal_data::KindId;

/// A piece of mail as the engine carries it: what it is, the seat it comes from, and the chain it belongs to
/// (ADR-0012).
///
/// It holds nothing the engine keeps beside it: the stream's entry names the
/// actor it is for and the mail's id, its sender and that sender's count. It
/// does not carry its cause, which nothing reads from mail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Envelope {
    /// The seat of the actor that sent it.
    pub sender_seat: SeatNumber,
    /// The first mail of the chain this one belongs to.
    pub root: MailId,
    /// How far down its chain it is: one more than the mail whose turn sent it.
    pub depth: u32,
    /// The id of its kind.
    pub kind: KindId,
    /// Where its contents, the bytes of that kind, are: in the arena of the worker that sent it, or their own.
    pub bytes: Payload,
    /// What it is to a request, and when a request it is stops being waited for (ADR-0017, ADR-0041).
    pub tying: Tying,
}

/// What a piece of mail is to a request, as an envelope keeps it, in the eight bytes its [`Tie`] alone took before
/// requests had deadlines: a request's tie names the request by its count, which is the mail's own and its entry
/// keeps, so in a request's place it keeps when the request is due (ADR-0017, ADR-0041).
///
/// So no piece of mail is carried in more bytes for a deadline, and mail that asks nothing carries none. Of the top two
/// bits, the top marks a request, and the next what the engine says of a request that was not answered, as a tie's do;
/// a request's other bits are nought for one due `Never`, and one more than the nanoseconds of its time for one due
/// then. A time later than [`LAST_DUE`] is kept as that time, which no host's clock reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tying(u64);

/// The bit that marks a request.
const ASKS: u64 = 1 << 63;

/// The bit that marks what the engine says of a request that was not answered.
const UNANSWERED: u64 = 1 << 62;

/// The latest time a request's deadline is kept at: some 292 years past the host's first moment.
pub const LAST_DUE: Time = Time::from_nanos(ASKS - 2);

impl Tying {
    /// Mail tied as `tie`, due as `due` when it is a request.
    #[must_use]
    pub fn of(tie: Tie, due: Due) -> Self {
        let bits = match (tie.read(), due) {
            (Tied::Tell, _) => 0,
            (Tied::Answers(count), _) => count,
            (Tied::Unanswered(count), _) => count | UNANSWERED,
            (Tied::Asks(_), Due::Never) => ASKS,
            (Tied::Asks(_), Due::At(at)) => ASKS | (at.min(LAST_DUE).nanos() + 1),
        };

        Self(bits)
    }

    /// Its tie, for the mail whose sender counted it `count`.
    #[must_use]
    pub const fn tie(self, count: u64) -> Tie {
        match self.0 {
            0 => Tie::TELL,
            asks if asks & ASKS != 0 => Tie::asks(count),
            unanswered if unanswered & UNANSWERED != 0 => Tie::unanswered(unanswered & !UNANSWERED),
            answers => Tie::answers(answers),
        }
    }

    /// When the request it is stops being waited for: `Never` for mail that asks nothing.
    #[must_use]
    pub const fn due(self) -> Due {
        let is_request = self.0 & ASKS != 0;
        let after = self.0 & !ASKS;
        let states_time = after != 0;
        let has_time = is_request && states_time;
        if !has_time {
            return Due::Never;
        }

        Due::At(Time::from_nanos(after - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tying_gives_back_the_tie_and_the_deadline_it_was_made_of() {
        // Catches a request's count not taken from its mail, an answer's or a notice's count lost, a deadline read a
        // nanosecond off, `Never` read as a time, and a far deadline wrapped into an early one.
        const COUNT: u64 = 9;
        let at = Time::from_nanos(5);
        let ties = [Tie::TELL, Tie::answers(4), Tie::unanswered(4), Tie::asks(COUNT)];
        let dues = [Due::Never, Due::At(Time::ZERO), Due::At(at)];

        for tie in ties {
            for due in dues {
                let tying = Tying::of(tie, due);
                let is_request = matches!(tie.read(), Tied::Asks(_));
                let kept = if is_request {
                    due
                } else {
                    Due::Never
                };

                assert_eq!((tying.tie(COUNT), tying.due()), (tie, kept), "{tie:?} {due:?}");
            }
        }
        let far = Tying::of(Tie::asks(COUNT), Due::At(Time::from_nanos(u64::MAX)));
        assert_eq!(far.due(), Due::At(LAST_DUE));
    }
}
