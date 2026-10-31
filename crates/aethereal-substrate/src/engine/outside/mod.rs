//! The outside: the seat the host speaks through, which takes whatever it is handed onto a queue the host reads between
//! steps, and sends between steps as a source's events are brought (ADR-0036).

use super::{Engine, Holding, ROOTS, parity};
use crate::{Life, lives, table};
use aethereal_actor::{
    Actor, Actors, Asking, Beginning, ChildOf, Closing, Code, Declared, Ended, Ending, Fault, Item, Mail, MailId,
    Mailed, Owed, PinSource, Pinning, Preparing, Refused, Role, Runs, Seated, Tie, Tied, Top, Turning, Unanswered, Why,
    Written,
};
use aethereal_data::{ActorPath, Decode, DecodeError, Encode, EncodeError, Kind, KindId, PathSegment, Plain};
use core::any::Any;
use crossbeam_queue::SegQueue;
use std::iter;
use std::mem;
use std::sync::Arc;

/// The outside's type, which declares that it handles nothing: what the table knows it by. Its code is [`Taking`].
#[derive(Plain)]
#[kind(name = "aether.outside")]
struct OutsideSeat;

impl ChildOf for OutsideSeat {
    type Parent = Top;
}

impl Actor for OutsideSeat {
    type Config = Self;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(seat: Self, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(seat)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What the table knows the outside by: a type that handles nothing, and that every kind may be cast to, as its code
/// takes whatever it is handed.
static DECLARED: Declared = Declared::taking_every_kind(OutsideSeat::NAME, KindId::of::<OutsideSeat>());

/// What a piece of mail that reached the outside is to a request (ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reached {
    /// Mail that asks for nothing.
    Told,
    /// A request asked of the outside, which it holds.
    Asked(Owed),
    /// The answer to the outside's request of this count.
    Answered(u64),
    /// What the engine says of the outside's request of this count, which was not answered.
    Unanswered(u64),
}

/// A piece of mail the outside took, still written, with the pins its bytes hold.
pub struct Arrival {
    kind: KindId,
    bytes: Vec<u8>,
    pins: Vec<Pinning>,
    sender: Seated,
    root: MailId,
    depth: u32,
    reached: Reached,
    limit: usize,
}

impl Arrival {
    /// `mail`, copied out with the pins its bytes hold, which is to a request what `reached` says.
    fn of(mail: Mail<'_>, reached: Reached, limit: usize) -> Self {
        let mut pins = Vec::new();
        mail.pinned().each(|at, pin| pins.push((at, pin.clone())));
        let (kind, sender, root, depth) = (mail.kind(), mail.sender_seated(), mail.root(), mail.depth());
        let bytes = mail.bytes().to_vec();

        Self { kind, bytes, pins, sender, root, depth, reached, limit }
    }

    /// The id of its kind.
    #[must_use]
    pub const fn kind(&self) -> KindId {
        self.kind
    }

    /// What it is to a request.
    #[must_use]
    pub const fn reached(&self) -> Reached {
        self.reached
    }

    /// Its contents, read as `K`, as the mail an actor is handed reads.
    pub fn read<K: Kind + Decode<Actors>>(&self) -> Result<Option<K>, DecodeError> {
        let pins = PinSource::lying(&self.bytes, &self.pins);

        Mail::new(self.kind, &self.bytes, &pins, self.sender, self.root, self.depth, self.limit).read()
    }
}

/// The outside's code: every piece of mail is taken as it came, and every request asked of it is held.
struct Taking {
    arrivals: Arc<SegQueue<Arrival>>,
    limit: usize,
}

impl Taking {
    /// Notes `mail`, which is to a request what `reached` says.
    fn take(&self, mail: Mail<'_>, reached: Reached) {
        self.arrivals.push(Arrival::of(mail, reached, self.limit));
    }
}

impl Runs for Taking {
    fn declared(&self) -> &'static Declared {
        &DECLARED
    }

    fn prepares(&self) -> bool {
        false
    }

    fn prepare(&mut self, _: Preparing<'_>) {}

    fn turn(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        match item {
            Item::Mail(mail) => {
                let me = turning.me();
                let source = turning.source();
                let Tied::Asks(request) = turning.tie().read() else {
                    self.take(mail, Reached::Told);
                    return Ok(());
                };
                let (mut hosted, _) = turning.hosted(self.declared(), Role::Kind(mail.kind()));
                // What it is answered with is the asker's to say, and read as it is answered: the debt's own kind is
                // read only for a turn that answers, and the outside answers between steps.
                hosted.hold(KindId::of::<Unanswered>())?;
                self.take(mail, Reached::Asked(Owed { holder: me, asker: source, request }));
            }
            Item::Answer { mail, asking } => self.take(mail, Reached::Answered(asking.count)),
            Item::Unanswered { mail, asking } => self.take(mail, Reached::Unanswered(asking.count)),
            // The outside watches nothing.
            Item::Ended(_) => {}
        }

        Ok(())
    }

    fn end(self: Box<Self>, _: Closing<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// How a turn came to run (ADR-0013).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ran {
    /// Of an item that was waiting as its step began.
    Waiting,
    /// Of mail handed straight to its receiver within the step.
    Hop,
    /// As its actor's closing turn.
    Closing,
}

/// What a turn was handed.
pub enum Handed {
    /// A piece of mail.
    Mail(Arrival),
    /// The ending of an actor it watches.
    Ended(Ended),
    /// Nothing: a closing turn, and why it runs.
    Closing(Why),
}

/// One turn the engine ran, as it recorded it: nothing is sent or counted for it, so a turn runs the same whether it is
/// recorded or not (ADR-0036).
pub struct Turned {
    pub actor: Seated,
    pub path: ActorPath,
    pub step: u64,
    /// How far down its chain what the turn handled lies.
    pub depth: u32,
    pub ran: Ran,
    pub handed: Handed,
    pub outcome: Result<(), Fault>,
}

impl Turned {
    /// Whether it ran as a hop.
    #[must_use]
    pub fn is_hop(&self) -> bool {
        self.ran == Ran::Hop
    }

    /// Whether it failed.
    #[must_use]
    pub const fn failed(&self) -> bool {
        self.outcome.is_err()
    }
}

/// Whether a step's turns are recorded.
#[derive(Clone, Copy)]
pub enum Recording {
    Off,
    /// They are, and the step is this one.
    At(u64),
}

/// A turn's item, noted before it runs.
pub enum Noted {
    Unrecorded,
    Recorded { step: u64, depth: u32, handed: Handed },
}

impl Recording {
    /// The recording of the step `step`, when the engine `is_on`.
    pub(super) const fn of(is_on: bool, step: u64) -> Self {
        if is_on {
            Self::At(step)
        } else {
            Self::Off
        }
    }

    /// `item`, as the turn of `me` from `source`, tied as `tie`, is about to be handed it.
    pub(super) fn note(self, item: &Item<'_>, me: Seated, source: Seated, tie: Tie, depth: u32, limit: usize) -> Noted {
        let Self::At(step) = self else {
            return Noted::Unrecorded;
        };
        let handed = match item {
            Item::Mail(mail) => {
                let reached = match tie.read() {
                    Tied::Asks(request) => Reached::Asked(Owed { holder: me, asker: source, request }),
                    Tied::Tell | Tied::Answers(_) | Tied::Unanswered(_) => Reached::Told,
                };

                Handed::Mail(Arrival::of(*mail, reached, limit))
            }
            Item::Answer { mail, asking } => Handed::Mail(Arrival::of(*mail, Reached::Answered(asking.count), limit)),
            Item::Unanswered { mail, asking } => {
                Handed::Mail(Arrival::of(*mail, Reached::Unanswered(asking.count), limit))
            }
            Item::Ended(ended) => Handed::Ended(ended.clone()),
        };

        Noted::Recorded { step, depth, handed }
    }

    /// Records the closing turn of `me`, at `path` and `depth`, which runs for `why` and ended as `outcome` says.
    pub(super) fn closing(
        self,
        me: Seated,
        (path, depth): (&ActorPath, u32),
        why: Why,
        outcome: &Result<(), Fault>,
        turned: &mut Vec<Turned>,
    ) {
        if let Self::At(step) = self {
            let (ran, handed, outcome, path) = (Ran::Closing, Handed::Closing(why), outcome.clone(), path.clone());
            turned.push(Turned { actor: me, path, step, depth, ran, handed, outcome });
        }
    }
}

impl Noted {
    /// Records the turn of `me`, at `path`, it was noted for, which ran as `ran` and ended as `outcome` says.
    pub(super) fn finish(
        self,
        me: Seated,
        path: &ActorPath,
        ran: Ran,
        outcome: &Result<(), Fault>,
        turned: &mut Vec<Turned>,
    ) {
        if let Self::Recorded { step, depth, handed } = self {
            let (path, outcome) = (path.clone(), outcome.clone());
            turned.push(Turned { actor: me, path, step, depth, ran, handed, outcome });
        }
    }
}

/// Why a reply from the outside was not sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unsent {
    /// It does not encode.
    Unwritten(EncodeError),
    /// The asker waits on an answer of the kind `wanted`.
    OtherAnswer { wanted: KindId, found: KindId },
}

impl Engine {
    /// Seats the outside, and answers with who it is and the queue what it takes goes onto.
    pub fn seat_outside(&mut self) -> Result<(Seated, Arc<SegQueue<Arrival>>), Refused> {
        let arrivals = Arc::new(SegQueue::new());
        let taking = Taking { arrivals: Arc::clone(&arrivals), limit: self.read_limit };
        let segment = PathSegment::new(OutsideSeat::NAME, &());
        let declared = &DECLARED;
        self.recording = true;
        self.add_root_made(&segment, Code::Linked(declared), Vec::new(), |_, _, _, _, _, _| Ok(Box::new(taking)))?;
        let seated = table::reach(&self.table, &ActorPath::from_root(&segment), declared.id());

        Ok((seated, arrivals))
    }

    /// Records the turns of every step from now on, without seating the outside: for a test of the engine's mechanics,
    /// whose seats are its own to place.
    pub fn record_turns(&mut self) {
        self.recording = true;
    }

    /// How many pieces of mail the actor `seated` names has sent, as of the last step.
    #[must_use]
    pub fn sent(&self, seated: Seated) -> u64 {
        self.lives.shard(seated.seat.number()).life(seated).map_or(0, Life::sent)
    }

    /// Takes the turns recorded since this was last asked, each part's in the order they ran, the parts in order.
    pub fn take_turned(&mut self) -> Vec<Turned> {
        let mut turned = mem::take(&mut self.turned);
        turned.sort_by_key(|turn| (turn.step, self.part(turn.actor.seat)));

        turned
    }

    /// The state of the actor `seated` names, as of the last step, when one of type `T` sits there and its code is here.
    #[must_use]
    pub fn state<T: 'static>(&self, seated: Seated) -> Option<&T> {
        let held = self.actors[self.part(seated.seat)].get(seated)?;
        let Holding::Here(actor) = &held.actor else {
            return None;
        };
        let actor: &dyn Any = actor.as_ref();

        actor.downcast_ref()
    }

    /// Sends `mail` from the outside `by` to `to`, tied as `tie` says of its count, the root of a chain of its own, and
    /// answers with its count. It is read in the next step, as what is brought is (ADR-0010, ADR-0012).
    pub fn send_outside<K: Kind + Encode<Actors>>(
        &mut self,
        by: Seated,
        to: Seated,
        mail: &K,
        tie: impl FnOnce(u64) -> Tie,
    ) -> Result<u64, EncodeError> {
        let parity = parity(self.step);
        let mut arena = self.arena(parity, ROOTS, 0);
        let bytes = arena.write_mail(mail);
        self.arenas[parity][ROOTS] = arena;
        let bytes = bytes?;
        let count = self.sent(by) + 1;
        self.lives.fold_records([lives::sent(by, count)], &[]);
        let mailed = Mailed { by, to, count, kind: KindId::of::<K>(), bytes, tie: tie(count) };
        self.fold_brought(vec![mailed], Vec::new(), parity, |mailed| MailId {
            sender: mailed.by.id,
            count: mailed.count,
        });

        Ok(count)
    }

    /// Asks `mail` of `to` from the outside `by`, to be answered with a kind `answer` names, and answers with the
    /// request's count: filed beside the outside, as a turn's ask is (ADR-0017).
    pub fn ask_outside<K: Kind + Encode<Actors>>(
        &mut self,
        by: Seated,
        to: Seated,
        mail: &K,
        answer: KindId,
    ) -> Result<u64, EncodeError> {
        let count = self.send_outside(by, to, mail, Tie::asks)?;
        let part = self.part(by.seat);
        if let Some(held) = self.actors[part].get_mut(by) {
            held.waiting.file(iter::once(Asking { count, answer, context: Written::default(), lent: Vec::new() }));
        }

        Ok(count)
    }

    /// Answers the debt `owed` of the outside with `mail`: refused when its asker waits on another kind.
    pub fn answer_outside<K: Kind + Encode<Actors>>(&mut self, owed: Owed, mail: &K) -> Result<(), Unsent> {
        let found = KindId::of::<K>();
        let asker = self.actors[self.part(owed.asker.seat)].get(owed.asker);
        if let Some(wanted) = asker.and_then(|held| held.waiting.answer_of(owed.request))
            && wanted != found
        {
            return Err(Unsent::OtherAnswer { wanted, found });
        }
        self.send_outside(owed.holder, owed.asker, mail, |_| Tie::answers(owed.request)).map_err(Unsent::Unwritten)?;
        self.forgive(owed);

        Ok(())
    }

    /// Says to the asker of the debt `owed` of the outside what the engine would say of a request it never answered.
    pub fn unanswered_outside(&mut self, owed: Owed, why: Unanswered) -> Result<(), EncodeError> {
        self.send_outside(owed.holder, owed.asker, &why, |_| Tie::unanswered(owed.request))?;
        self.forgive(owed);

        Ok(())
    }

    /// Lets go of the debt `owed` of the outside, which has been said what became of it.
    fn forgive(&mut self, owed: Owed) {
        let part = self.part(owed.holder.seat);
        if let Some(held) = self.actors[part].get_mut(owed.holder) {
            // The one thing that holds it is what the host holds.
            let _ = held.debts.change(owed, -1);
        }
    }
}
