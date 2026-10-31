//! The outside: the seat the host speaks through, which takes whatever it is handed onto a queue the host reads between
//! steps, and sends between steps as a source's events are brought (ADR-0036).

use super::{Engine, ROOTS, parity};
use crate::{Life, lives, table};
use aethereal_actor::{
    Actor, Actors, Asking, Beginning, ChildOf, Closing, Code, Declared, Ending, Fault, Item, Mail, MailId, Mailed,
    Owed, PinSource, Pinning, Preparing, Refused, Role, Runs, Seated, Tie, Tied, Top, Turning, Unanswered, Written,
};
use aethereal_data::{ActorPath, Decode, DecodeError, Encode, EncodeError, Kind, KindId, PathSegment, Plain};
use crossbeam_queue::SegQueue;
use std::iter;
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
    /// The id of its kind.
    pub const fn kind(&self) -> KindId {
        self.kind
    }

    /// What it is to a request.
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
        let mut pins = Vec::new();
        mail.pinned().each(|at, pin| pins.push((at, pin.clone())));
        let (kind, sender, root, depth) = (mail.kind(), mail.sender_seated(), mail.root(), mail.depth());
        let bytes = mail.bytes().to_vec();
        self.arrivals.push(Arrival { kind, bytes, pins, sender, root, depth, reached, limit: self.limit });
    }
}

impl Runs for Taking {
    fn declared(&self) -> &'static Declared {
        Declared::of::<OutsideSeat>()
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
        let declared = Declared::of::<OutsideSeat>();
        self.add_root_made(&segment, Code::Linked(declared), Vec::new(), |_, _, _, _, _, _| Ok(Box::new(taking)))?;
        let seated = table::reach(&self.table, &ActorPath::from_root(&segment), declared.id());

        Ok((seated, arrivals))
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
        let sent = self.lives.shard(by.seat.number()).life(by).map_or(0, Life::sent);
        let count = sent + 1;
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
