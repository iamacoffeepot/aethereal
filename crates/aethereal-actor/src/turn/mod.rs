//! A turn: what an actor asks of the engine while it handles one item (ADR-0009).

mod hosted;
mod refused;
#[cfg(test)]
mod tests;

pub use hosted::{Broke, FalseSend, FalseWatch, Forged, Garbled, Hosted, Trapped};
pub use refused::Refused;

use crate::resource;
use crate::{
    Actor, ActorRef, Actors, ActorsWriter, Allows, Arena, Asking, Asks, Carries, Children, Claim, Claimed, Code,
    Declared, Effect, ErasedActorRef, Fault, Gift, Given, Giver, Handle, Handled, Handles, Instances, Keeping, Keeps,
    Kinds, Level, Line, Listed, MailId, Mailed, Marker, Payload, Protocol, ProtocolRef, Puts, Reaches, Resource, Role,
    Runs, SeatNumber, Seated, Seats, Seen, Sends, Table, Target, Tie, Tied, To, ToAny, ToSender, ToType, ToWithoutMail,
    Watches, What, Within, WithinWriter,
};
#[cfg(feature = "std")]
use crate::{Birth, BlobHash, BlobRef, Config, config_bytes};
use aethereal_data::{ActorId, ActorPath, Encode, EncodeError, Kind, KindId, PathSegment, PathTooDeep, Writer};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
#[cfg(feature = "std")]
use core::convert::Infallible;
use core::fmt;
use core::marker::PhantomData;
use core::mem;
use core::str;

/// What one turn has asked for, in the lists it fills as it runs: the mail it sent, the lines it logged, its effects,
/// and the resources it gave, in the order it asked for each; and the children it stood up (ADR-0009, ADR-0010).
///
/// The engine makes one for a turn and takes it back when the turn returns.
/// Nothing in it has happened yet: the engine alone applies what is asked.
pub struct Asked {
    buffers: Buffers,
    stood: Vec<Stood>,
    /// The seats of the children in `stood`, and what their types declare, under the bits of their ids, to find
    /// whether one stands at a key, where it sits, and what it is in a lookup.
    stood_seats: BTreeMap<u64, (SeatNumber, &'static Declared)>,
    /// Where the bytes of the mail the turn sends, and the text of the lines it logs, are written.
    arena: Arena,
    /// The seats the children the turn spawns are given.
    seats: Seats,
    /// The blobs its worker has put in the step, and the room it has left to put more (ADR-0014).
    puts: Puts,
    /// The tickets of the resources the mail being written holds, noted as it is written: empty between sends.
    carrying: Vec<u64>,
    /// What a guest's code claimed of its resources, in the order it claimed it, for its host to hold it to: always
    /// empty in native code, whose resources move as it asks (ADR-0015).
    claims: Vec<Claim>,
    /// The requests the turn running asked, with the context each keeps, for the engine to file once the turn has
    /// run (ADR-0017): empty between turns.
    asks: Vec<Asking>,
}

/// The buffers of what turns ask for: kept by a worker over its share of a step, and filled by each turn in turn.
#[derive(Default)]
pub struct Buffers {
    /// The mail sent, in the order sent.
    pub mailed: Vec<Mailed>,
    /// The lines logged, in the order logged.
    pub lines: Vec<Line>,
    /// Everything else asked for: the effects on the lives of actors, in the order asked for.
    pub effects: Vec<Effect>,
    /// The resources given, each with where it is carried, in the order given (ADR-0010).
    pub gifts: Vec<Gift>,
}

/// How long each of the buffers was at some moment: where what was asked after it begins.
#[derive(Clone, Copy)]
struct Marks {
    mailed: usize,
    effects: usize,
}

impl Buffers {
    /// How long the buffers a failed `begin` takes back from are.
    fn marks(&self) -> Marks {
        Marks { mailed: self.mailed.len(), effects: self.effects.len() }
    }

    /// Whether every buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let no_mail = self.mailed.is_empty();
        let no_lines = self.lines.is_empty();
        let no_effects = self.effects.is_empty();
        let no_gifts = self.gifts.is_empty();

        no_mail && no_lines && no_effects && no_gifts
    }
}

impl Asked {
    /// Nothing asked for yet, with mail written to an arena of its own, children seated as in an engine kept in one
    /// part, and blobs put with no bound on their bytes, for no engine: none numbers itself `u32::MAX`.
    #[must_use]
    pub fn new() -> Self {
        Self::writing(Arena::default(), Seats::one_part(), Puts::new(u32::MAX, 0, usize::MAX))
    }

    /// Nothing asked for yet, with mail written to `arena`, after what it already holds, children seated from
    /// `seats`, and blobs put into `puts`.
    #[must_use]
    pub fn writing(arena: Arena, seats: Seats, puts: Puts) -> Self {
        Self::continuing(Buffers::default(), arena, seats, puts)
    }

    /// Nothing asked for yet, in `buffers`, emptied and kept for their room, with mail written to `arena`, after what
    /// it already holds, children seated from `seats`, and blobs put into `puts`, after what it already holds.
    #[must_use]
    pub fn continuing(buffers: Buffers, arena: Arena, seats: Seats, puts: Puts) -> Self {
        debug_assert!(buffers.is_empty(), "a turn begins with its buffers emptied");

        let (carrying, claims, asks) = (Vec::new(), Vec::new(), Vec::new());
        let stood = Vec::new();

        Self { buffers, stood, stood_seats: BTreeMap::new(), arena, seats, puts, carrying, claims, asks }
    }

    /// Begins the next turn, once what the last asked for has been taken out of the buffers. The children the last
    /// turn stood up have been taken with [`Self::take_stood`].
    ///
    /// The engine keeps one of these for a worker's whole share of a step, and not one for each turn, so a turn
    /// moves nothing in or out, and every buffer holds only what the turn running asked for.
    pub fn next_turn(&mut self) {
        debug_assert!(self.buffers.is_empty(), "a turn begins with its buffers emptied");
        // Most turns stand up no child, and clearing even an empty map walks it.
        if !self.stood_seats.is_empty() {
            self.stood_seats.clear();
        }
    }

    /// Empties everything asked for, the arena's bytes among it, keeping the room of each: for a guest, whose host has
    /// read what it asked for after each call (ADR-0015).
    #[cfg(feature = "guest")]
    pub(crate) fn empty(&mut self) {
        self.buffers.mailed.clear();
        self.buffers.lines.clear();
        self.buffers.effects.clear();
        self.buffers.gifts.clear();
        self.claims.clear();
        self.stood.clear();
        self.stood_seats.clear();
        self.arena.clear(0);
    }

    /// Lets go of the pins the mail written holds, keeping the mail: for a guest, whose host pins what its mail refers
    /// to itself (ADR-0015).
    #[cfg(feature = "guest")]
    pub(crate) fn let_go_of_pins(&mut self) {
        self.arena.let_go();
    }

    /// How many children have been stood up since the buffers were last emptied: for a guest, whose turns of a batch
    /// all add to them (ADR-0015).
    #[cfg(feature = "guest")]
    pub(crate) fn stood_count(&self) -> usize {
        self.stood.len()
    }

    /// Whether this turn has asked for nothing in any buffer.
    #[must_use]
    pub fn asked_nothing(&self) -> bool {
        self.buffers.is_empty()
    }

    /// Whether the turn running has asked anything (ADR-0017).
    #[must_use]
    pub fn has_asked(&self) -> bool {
        !self.asks.is_empty()
    }

    /// Takes the requests the turn running asked, each with its context, in the order asked: for the engine, which
    /// files them beside the asker once its turn has run, and drops them when it failed (ADR-0017).
    pub fn take_asks(&mut self) -> impl Iterator<Item = Asking> + '_ {
        self.asks.drain(..)
    }

    /// Takes the children this turn stood up, each in the order its `begin` finished.
    pub fn take_stood(&mut self) -> Vec<Stood> {
        mem::take(&mut self.stood)
    }

    /// Every list, earlier turns' entries first, for the engine to add its own to, route, and empty.
    pub fn buffers_mut(&mut self) -> &mut Buffers {
        &mut self.buffers
    }

    /// The seats children are given, for the engine to give back those of children that did not stay.
    pub fn seats_mut(&mut self) -> &mut Seats {
        &mut self.seats
    }

    /// The seats children are given, the blobs' puts, and the arena mail is written to, lent together: to a batch
    /// prepared ahead, whose turns' children and blobs its host asks for in calls out, and whose hop's pins lie in
    /// the arena (ADR-0015), and to a child its host begins apart.
    pub fn lent(&mut self) -> (&mut Seats, &mut Puts, &Arena) {
        (&mut self.seats, &mut self.puts, &self.arena)
    }

    /// The seats children are given, the blobs' puts, and the resources the turn has given, lent together: to a batch
    /// prepared ahead, or a child begun apart, whose host may hand one of those resources to a child of other code
    /// (ADR-0010, ADR-0015).
    pub fn lent_gifts(&mut self) -> (&mut Seats, &mut Puts, &Arena, &mut Vec<Gift>) {
        (&mut self.seats, &mut self.puts, &self.arena, &mut self.buffers.gifts)
    }

    /// What a guest's code claimed of its resources since what it asked for was last emptied, in the order claimed:
    /// for its library, which writes them for its host (ADR-0015).
    #[must_use]
    pub fn claims(&self) -> &[Claim] {
        &self.claims
    }

    /// Takes in what `other` asked for: a child's `begin` its host ran apart from the turn that spawned it, while the
    /// turn's code waited on a call out (ADR-0015). Its mail and lines are copied into this arena, each piece of mail
    /// with pins of its own, and its effects and the children it stood up follow this turn's own.
    #[cfg(feature = "std")]
    pub fn take_in(&mut self, other: Self) {
        self.take_lines(&other);
        let Self { buffers, stood, stood_seats, arena, .. } = other;
        for mailed in buffers.mailed {
            self.copy_mail(&arena, mailed);
        }
        self.buffers.effects.extend(buffers.effects);
        self.buffers.gifts.extend(buffers.gifts);
        self.stood.extend(stood);
        self.stood_seats.extend(stood_seats);
    }

    /// Copies the lines `other` logged into this arena, after this turn's own: what a child's `begin` its host ran
    /// apart leaves when it was refused, as a refused child's lines are kept.
    pub fn take_lines(&mut self, other: &Self) {
        for line in &other.buffers.lines {
            if let Ok(text) = self.arena.write_text(other.text(line)) {
                self.buffers.lines.push(Line { text, ..*line });
            }
        }
    }

    /// Copies `mailed`, whose bytes lie in `from`, into this arena, piece by piece, each holding pins of its own on
    /// the blobs it refers to.
    #[cfg(feature = "std")]
    fn copy_mail(&mut self, from: &Arena, mailed: Mailed) {
        for (index, piece) in (0_u64..).zip(mailed.bytes.each_piece()) {
            let Some(bytes) = from.read(piece) else {
                continue;
            };
            let copied = self.arena.write_holding(|mut out, held| {
                let base = u32::try_from(out.len()).unwrap_or(u32::MAX);
                out.extend_from_slice(bytes);
                from.pin_copied(piece, base, held);

                (out, Ok::<(), Infallible>(()))
            });
            let Ok(copied) = copied;
            let count = mailed.count + index;
            self.buffers.mailed.push(Mailed { count, bytes: copied, ..mailed });
        }
    }

    /// The arena the turn's mail and lines were written to.
    #[must_use]
    pub const fn arena(&self) -> &Arena {
        &self.arena
    }

    /// The mail this turn sent, in the order sent.
    #[must_use]
    pub fn mailed(&self) -> &[Mailed] {
        &self.buffers.mailed
    }

    /// The lines this turn logged, in the order logged.
    #[must_use]
    pub fn lines(&self) -> &[Line] {
        &self.buffers.lines
    }

    /// The text of `line`, one this arena holds.
    #[must_use]
    pub fn text(&self, line: &Line) -> &str {
        let bytes = self.arena.read(line.text).unwrap_or_default();

        str::from_utf8(bytes).unwrap_or_default()
    }

    /// This turn's other effects, in the order they were asked for.
    #[must_use]
    pub fn effects(&self) -> &[Effect] {
        &self.buffers.effects
    }

    /// The buffers, the children stood up, each child in the order its `begin` finished, the arena the mail was
    /// written to, the seats not given out, and the blobs put.
    #[must_use]
    pub fn into_parts(self) -> (Buffers, Vec<Stood>, Arena, Seats, Puts) {
        (self.buffers, self.stood, self.arena, self.seats, self.puts)
    }

    /// The seat of the child stood up in this turn under the id `id`, when there is one.
    fn stood_seat(&self, id: ActorId) -> Option<SeatNumber> {
        self.stood_seats.get(&id.bits()).map(|(seat, _)| *seat)
    }

    /// What the type of the child stood up in this turn under the id `id` declares, when there is one.
    fn stood_declared(&self, id: ActorId) -> Option<&'static Declared> {
        self.stood_seats.get(&id.bits()).map(|(_, declared)| *declared)
    }
}

impl Default for Asked {
    /// As [`Asked::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// A child stood up in a turn and held aside until the engine applies its spawn (ADR-0009).
///
/// Until then it exists for no one but its parent.
pub struct Stood {
    me: Seated,
    path: ActorPath,
    above: Above,
    actor: Box<dyn Runs>,
    /// What its `begin` kept and took (ADR-0010).
    keeping: Keeping,
}

/// What stands directly above an actor: the actor [`To::Parent`] names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Above {
    /// Nothing: the actor is a root, made for the chassis.
    Chassis,
    /// The actor that spawned it, its seat, and what its type declares, which a child's `begin` cannot read from the
    /// table while its parent is still being stood up.
    Actor(Seated, &'static Declared),
    /// The actor that spawned it, and its seat, of a type the code running this one does not hold: a parent of other
    /// code, as a guest sees it. Its sends to it are checked by its host (ADR-0015).
    Elsewhere(Seated),
}

impl Stood {
    /// The child's id.
    #[must_use]
    pub const fn id(&self) -> ActorId {
        self.me.id
    }

    /// The child, and the seat it was given.
    #[must_use]
    pub const fn seated(&self) -> Seated {
        self.me
    }

    /// Where the child stands.
    #[must_use]
    pub const fn path(&self) -> &ActorPath {
        &self.path
    }

    /// The actor that stood the child up.
    #[must_use]
    pub const fn above(&self) -> Above {
        self.above
    }

    /// What the child's type declares (ADR-0013).
    #[must_use]
    pub fn declared(&self) -> &'static Declared {
        self.actor.declared()
    }

    /// The child itself, for the engine to run.
    #[must_use]
    pub fn into_actor(self) -> Box<dyn Runs> {
        self.actor
    }

    /// The child taken apart, for the engine to seat it: who it is and where it sits, its path, what stands above
    /// it, the child itself, and what it keeps.
    #[must_use]
    pub fn into_parts(self) -> (Seated, ActorPath, Above, Box<dyn Runs>, Keeping) {
        (self.me, self.path, self.above, self.actor, self.keeping)
    }
}

/// Whose turn it is, and what every kind of turn works with.
struct Seat<'a> {
    /// The actor, and its seat.
    me: Seated,
    path: &'a ActorPath,
    /// What stands above the actor, kept when it was stood up, so a send to its parent reads no path.
    above: Above,
    table: &'a dyn Table,
    /// The children the actor made before this turn's step, whose keys it may not use again.
    made: &'a dyn Children,
    asked: &'a mut Asked,
    /// What the actor keeps, which its resource calls reach (ADR-0010).
    keeps: Keeps<'a>,
    /// How many pieces of mail this actor has sent, this turn's included.
    sent: u64,
    /// What the actor's type declares (ADR-0013).
    declared: &'static Declared,
    /// Which of its turns this is, named when a send breaks the declaration.
    during: During,
    /// The last send of the turn that was checked against the declaration, if it is still the turn's last effect.
    checked: Checked,
    /// What the turn owes the source of the item it handles (ADR-0017).
    owing: Owing,
}

/// What a turn owes the source of the item it handles (ADR-0017).
///
/// It is read only when the turn replies, so a turn that does not reply reads nothing of it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Owing {
    /// What the item is to a request, as its mail says: an answer when it asks one.
    Item(Tie),
    /// Nothing more: it has answered the request.
    Answered,
}

impl Owing {
    /// What a turn that owes nothing owes.
    pub const NOTHING: Self = Self::Item(Tie::TELL);

    /// What a reply to the source carries, and what the turn owes after it: refused once the request is answered.
    fn reply(self) -> Result<(Tie, Self), EncodeError> {
        let Self::Item(tie) = self else {
            return Err(EncodeError::Answered);
        };
        let replied = match tie.read() {
            Tied::Asks(count) => (Tie::answers(count), Self::Answered),
            Tied::Tell | Tied::Answers(_) => (Tie::TELL, self),
        };

        Ok(replied)
    }
}

/// Which send of a turn was last checked against the actor's declaration (ADR-0013): a send like it that follows it
/// straight after need not be checked again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Checked {
    /// No send of the turn has been checked.
    Nothing,
    /// The send that was the last in the mail buffer when it held `mailed` pieces, checked against `target`.
    Last { mailed: usize, target: Target },
}

/// Which of an actor's turns runs: named when a send breaks the declaration (ADR-0013).
#[derive(Clone, Copy)]
enum During {
    Begin,
    Handling(Role),
    End,
}

impl fmt::Display for During {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Begin => write!(f, "its begin"),
            Self::Handling(Role::Kind(kind)) => write!(f, "its handler of {kind:?}"),
            Self::Handling(Role::Ended) => write!(f, "its handler of endings"),
            Self::Handling(Role::Others) => write!(f, "its fallback"),
            Self::Handling(Role::Answer(kind)) => write!(f, "its handler of answers of {kind:?}"),
            Self::End => write!(f, "its closing turn"),
        }
    }
}

impl Seat<'_> {
    /// Adds the effect by which this actor asks for `what`.
    fn ask(&mut self, what: What) {
        self.asked.buffers.effects.push(Effect::new(self.me, what));
    }

    /// A reference to this actor.
    const fn me(&self) -> ErasedActorRef {
        ErasedActorRef::new(self.me)
    }

    /// A reference to the live actor `path` names.
    fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.table.resolve(path).map(ErasedActorRef::new)
    }

    /// The reference, to the tuple of kinds `P`, when the declaration of its actor's type lists every kind of `P`:
    /// none when one is missing, or the table holds no such actor (ADR-0016). A type this turn cannot read itself is
    /// read by the table.
    fn cast<P: Kinds>(&self, reference: ErasedActorRef) -> Option<ProtocolRef<P>> {
        let id = reference.id();
        let lists = self
            .declared_at(id)
            .map_or_else(|| self.table.lists(id, P::KINDS), |declared| declared.lists_all(P::KINDS));

        lists.then(|| ProtocolRef::new(reference.seated()))
    }

    /// A handle that logs as this actor and does nothing else.
    const fn logger(&mut self) -> Logger<'_> {
        Logger { me: self.me, asked: self.asked }
    }

    /// Puts `bytes` as a blob and answers with the first pin on it: refused when they would take this worker past
    /// its share of the budget for blobs (ADR-0014).
    #[cfg(feature = "std")]
    fn put(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Refused> {
        self.asked.puts.pinned(bytes)
    }

    /// The bytes of the blob `blob` pins: put by this worker in this step, or in the store.
    ///
    /// # Panics
    ///
    /// Panics when neither holds it, which a pin rules out.
    #[cfg(feature = "std")]
    fn read(&self, blob: &BlobRef) -> &[u8] {
        let id = blob.id();
        let Some(bytes) = self.asked.puts.read(id).or_else(|| self.table.blob(id)) else {
            panic!("blob store: the blob {id:?} is pinned and not held");
        };

        bytes
    }

    /// The content hash of the blob `blob` pins: the one the store kept, or one computed now and noted for the store
    /// to keep at the fold (ADR-0014).
    #[cfg(feature = "std")]
    fn hash(&mut self, blob: &BlobRef) -> BlobHash {
        let id = blob.id();
        if let Some(kept) = self.table.blob_hash(id) {
            return kept;
        }
        let hash = BlobHash::of(self.read(blob));
        self.asked.puts.hashed(id, hash);

        hash
    }

    /// Adds the effect that watches the actor `of` refers to, with how this turn sees it: live when the table held
    /// it as the step began or it was stood up since, and ended otherwise, since a reference's actor existed
    /// (ADR-0008). The turn's declaration lists the watch, and its type handles endings, as it built (ADR-0013).
    fn watch(&mut self, of: ErasedActorRef) {
        let id = of.seated().id;
        let was_held = self.table.held(id);
        let is_stood = self.asked.stood_seat(id).is_some();
        let seen = if was_held || is_stood {
            Seen::Live
        } else {
            Seen::Ended
        };
        self.ask(What::Watch { of, seen });
    }

    /// Encodes `mail` and adds the effect that sends it to the actor `to` names, which `target` says it may reach.
    ///
    /// Nothing is sent when `to` names no actor this turn can see: a parent no longer live, or a root's parent.
    /// Whether the actor it names is live when the mail would be handed to it is the engine's to see, and mail to
    /// one that is not is dropped there.
    fn send<K: Kind + Encode<Actors>>(&mut self, to: &To, target: Target, mail: &K) -> Result<(), EncodeError> {
        let Some(to) = self.receiver(to) else {
            return Ok(());
        };
        // A send to the receiver and of the kind of the turn's last, made by the same declared send, was checked with
        // it. A reply or a send through a reference is never checked, so one that went before proves nothing.
        let is_like_last = self.last_send().is_some_and(|(last, kind)| last == to && kind == KindId::of::<K>());
        let was_checked = self.checked == Checked::Last { mailed: self.asked.buffers.mailed.len(), target };
        let is_checked = is_like_last && was_checked;
        if !is_checked {
            self.check(to, target, KindId::of::<K>());
        }
        self.write_after(to, mail, is_like_last, Tie::TELL)?;
        self.checked = Checked::Last { mailed: self.asked.buffers.mailed.len(), target };

        Ok(())
    }

    /// The receiver and kind of this turn's last mail, when this actor sent it.
    fn last_send(&self) -> Option<(Seated, KindId)> {
        let last = self.asked.buffers.mailed.last()?;
        let is_mine = last.by.id == self.me.id;

        is_mine.then_some((last.to, last.kind))
    }

    /// Sends `mail` to the actor `to` names, which is checked to be of type `T` (ADR-0013).
    fn send_typed<T: Actor + Handles<K>, K: Kind + Encode<Actors> + Handled>(
        &mut self,
        to: &To,
        mail: &K,
    ) -> Result<(), EncodeError> {
        self.send(to, <ToType<T, K> as Marker>::SENDING.to(), mail)
    }

    /// Sends `mail` to the actor `to` names, which is checked to list every kind of `P` (ADR-0013).
    fn send_any<P: Listed, K: Kind + Encode<Actors>>(&mut self, to: &To, mail: &K) -> Result<(), EncodeError> {
        self.send(to, <ToAny<P, K> as Marker>::SENDING.to(), mail)
    }

    /// Encodes `mail` and adds the effect that sends it to the actor `to`, which this actor reaches as it is: one a
    /// typed reference proves, or the sender.
    fn write<K: Kind + Encode<Actors>>(&mut self, to: Seated, mail: &K) -> Result<(), EncodeError> {
        // A reply goes on its own: a turn rarely answers one sender back to back.
        self.write_after(to, mail, false, Tie::TELL)
    }

    /// Replies `mail` to `to`, the source of the item handled: the answer to it, when it is a request this turn has
    /// not answered yet, and refused when it has (ADR-0017).
    fn reply<K: Kind + Encode<Actors>>(&mut self, to: Seated, mail: &K) -> Result<(), EncodeError> {
        let (tie, owing) = self.owing.reply()?;
        self.write_after(to, mail, false, tie)?;
        self.owing = owing;

        Ok(())
    }

    /// Asks `mail` of `to`, keeping `context` until it is answered: a request, filed by the engine once the turn has
    /// run (ADR-0017).
    fn request<K: Kind + Encode<Actors>, C: Encode<Within>>(
        &mut self,
        to: Seated,
        mail: &K,
        context: &C,
    ) -> Result<(), EncodeError> {
        let mut out = Writer::<Within>::new(WithinWriter::new());
        context.encode(&mut out)?;
        let context = out.into_bytes();
        let count = self.sent + 1;
        self.write_after(to, mail, false, Tie::asks(count))?;
        self.asked.asks.push(Asking { count, context });

        Ok(())
    }

    /// As [`Self::write`], knowing whether `to` and the kind are those of the turn's last send.
    fn write_after<K: Kind + Encode<Actors>>(
        &mut self,
        to: Seated,
        mail: &K,
        is_like_last: bool,
        tie: Tie,
    ) -> Result<(), EncodeError> {
        let bytes = self.encode(mail)?;
        self.carry(to)?;
        self.sent += 1;
        let kind = KindId::of::<K>();
        // Mail of one kind to one receiver, sent back to back by sends the turn named, is one run: one effect, one item
        // in its stream.
        if is_like_last
            && let Some(last) = self.asked.buffers.mailed.last_mut()
            && last.count + u64::from(last.bytes.pieces()) == self.sent
            && last.tie.is_tell()
            && let Some(joined) = last.bytes.joined(bytes)
        {
            last.bytes = joined;
            return Ok(());
        }
        self.asked.buffers.mailed.push(Mailed { by: self.me, to, count: self.sent, kind, bytes, tie });

        Ok(())
    }

    /// Encodes `mail` once and sends it to each of `to`, which this actor reaches as each is: one a typed reference
    /// proves. Each is its own piece of mail, with its own count, in the order `to` gives, as if each were sent alone;
    /// all of them read the one copy of its bytes, and of the pins it holds.
    fn write_to_all<K: Kind + Encode<Actors>>(
        &mut self,
        to: impl ExactSizeIterator<Item = Seated>,
        mail: &K,
    ) -> Result<(), EncodeError> {
        // Nothing is written for no one.
        if to.len() == 0 {
            return Ok(());
        }
        let bytes = self.encode(mail)?;
        // One resource has one owner, so it goes to no more than one receiver (ADR-0010).
        if !self.asked.carrying.is_empty() {
            self.asked.carrying.clear();
            return Err(EncodeError::NotCarried { name: "Given" });
        }
        let kind = KindId::of::<K>();
        for to in to {
            self.sent += 1;
            self.asked.buffers.mailed.push(Mailed { by: self.me, to, count: self.sent, kind, bytes, tie: Tie::TELL });
        }

        Ok(())
    }

    /// Writes `mail` into the worker's arena, with the pins on the blobs it refers to, and answers where it lies. The
    /// tickets of the resources it holds are noted, for [`Self::carry`].
    fn encode<K: Kind + Encode<Actors>>(&mut self, mail: &K) -> Result<Payload, EncodeError> {
        #[cfg(feature = "std")]
        let carrying = &mut self.asked.carrying;
        #[cfg(feature = "std")]
        let bytes = self.asked.arena.write_holding(|bytes, held| {
            let mut out = Writer::<Actors>::appending(bytes, ActorsWriter::carrying(held, carrying));
            let written = mail.encode(&mut out);

            (out.into_bytes(), written)
        });
        #[cfg(not(feature = "std"))]
        let bytes = self.asked.arena.write(|bytes| {
            let mut out = Writer::<Actors>::appending(bytes, ActorsWriter::new());
            let written = mail.encode(&mut out);

            (out.into_bytes(), written)
        });
        // Mail that did not write carries nothing it began to write.
        if bytes.is_err() {
            self.asked.carrying.clear();
        }

        bytes
    }

    /// Carries each resource the mail just written holds to `to`, with that mail, the next this actor sends: refused
    /// when one is not a resource this turn gave and no other mail carries (ADR-0010).
    #[inline]
    fn carry(&mut self, to: Seated) -> Result<(), EncodeError> {
        // Most mail carries no resource.
        if self.asked.carrying.is_empty() {
            return Ok(());
        }
        let carried = match self.keeps {
            Keeps::Here(_) => {
                let mail = MailId { sender: self.me.id, count: self.sent + 1 };
                resource::carry(&mut self.asked.buffers.gifts, &self.asked.carrying, self.me, to, mail)
            }
            // A guest's sends are counted by its host, so what it claims names the send by its place in the call.
            Keeps::Claimed => {
                let send = self.asked.buffers.mailed.iter().map(|mailed| mailed.bytes.pieces()).sum();
                self.claim_carried(Claimed::Carried { send })
            }
        };
        self.asked.carrying.clear();

        carried
    }

    /// Claims each resource the bytes just written hold as gone with `what`: refused, claiming none, when one is not a
    /// resource this guest's turn claimed as given and nothing has carried or taken since (ADR-0015).
    fn claim_carried(&mut self, what: Claimed) -> Result<(), EncodeError> {
        let claims = &self.asked.claims;
        let every = self.asked.carrying.iter().all(|ticket| resource::is_loose_claim(claims, self.me, *ticket));
        if !every {
            return Err(EncodeError::NotCarried { name: "Given" });
        }
        let me = self.me;
        self.asked.claims.extend(self.asked.carrying.iter().map(|&ticket| Claim { by: me, ticket, what }));

        Ok(())
    }

    /// Hands the resources `carried` names to the config of a child of other code: each must be one this turn gave and
    /// nothing carries. Natively they stay this actor's gifts for the child to take; a guest claims them as given in
    /// the config, and its host hands them to the child as it begins it (ADR-0015).
    #[cfg(feature = "std")]
    fn hand_config(&mut self, carried: &[u64]) -> Result<(), EncodeError> {
        // Most configs hold no resource.
        if carried.is_empty() {
            return Ok(());
        }
        match self.keeps {
            Keeps::Here(_) => {
                let by = Giver::Actor(self.me);
                let gifts = &self.asked.buffers.gifts;
                let every = carried.iter().all(|ticket| gifts.iter().any(|gift| gift.is_loose(by, *ticket)));
                if every {
                    Ok(())
                } else {
                    Err(EncodeError::NotCarried { name: "Given" })
                }
            }
            Keeps::Claimed => {
                self.asked.carrying.clear();
                self.asked.carrying.extend_from_slice(carried);
                let claimed = self.claim_carried(Claimed::Configured);
                self.asked.carrying.clear();

                claimed
            }
        }
    }

    /// Keeps `value` as this actor's own, under a ticket no other resource has had, and answers with the handle to it
    /// (ADR-0010).
    fn keep<R: Resource>(&mut self, value: R) -> Result<Handle<R>, Fault> {
        let ticket = self.asked.puts.ticket();

        self.keeps.keep(ticket, value)
    }

    /// The resource `handle` names, this actor's own.
    fn open<R: Resource>(&mut self, handle: &Handle<R>) -> Result<&mut R, Fault> {
        self.keeps.open(handle)
    }

    /// Gives up the resource `handle` names, and answers with what names it on its way: dropped at the end of the
    /// turn unless mail carries it, or a child this turn stands up takes it from its config.
    fn give<R: Resource>(&mut self, handle: Handle<R>) -> Result<Given<R>, Fault> {
        self.keeps.give(self.me, handle, &mut self.asked.buffers.gifts, &mut self.asked.claims)
    }

    /// Takes the resource `given` names, which came with the mail being handled, from `source`, as this actor's own.
    fn take_arrived<R: Resource>(&mut self, given: Given<R>, source: Seated) -> Result<Handle<R>, Fault> {
        self.keeps.take_arrived(self.me, given, source, &mut self.asked.claims)
    }

    /// Takes the resource `given` names, which this actor's config holds, as its own: given by the actor above it, or
    /// by the chassis to a root.
    fn take_config<R: Resource>(&mut self, given: Given<R>) -> Result<Handle<R>, Fault> {
        let by = Giver::above(self.above);

        self.keeps.take_config(self.me, given, by, &mut self.asked.buffers.gifts, &mut self.asked.claims)
    }

    /// Stops the engine when the actor `to` names is not one `target` reaches: the send breaks this actor's
    /// declaration (ADR-0013). A send to the sender reaches it by how it was made.
    ///
    /// # Panics
    ///
    /// Panics when the receiver's type is not the one `target` names, or does not list the kinds it names.
    fn check(&self, to: Seated, target: Target, kind: KindId) {
        // A reply reaches its receiver by how it was made, so nothing is looked up for it.
        if target == Target::Sender {
            return;
        }
        // An actor a reference names that the table no longer holds has ended: the mail is dropped where it would be
        // handed over, and there is no type left to check it against (ADR-0008).
        let Some(receiver) = self.declared_at(to.id) else {
            return;
        };
        let reaches = match target {
            Target::Sender => true,
            Target::Type(_, id) => receiver.id() == id,
            Target::Handling(kinds) => receiver.lists_all(kinds),
            Target::Watched => false,
        };
        let receiver = receiver.name();
        assert!(
            reaches,
            "false declaration: an actor of type {} sent {kind:?} in {} to {to:?}, of type {receiver:?}, which \
             its declaration of {target:?} does not reach",
            self.declared.name(),
            self.during,
        );
    }

    /// What the type of the actor `id` stands for declares: this actor, its parent, a child this turn stood up, or one
    /// the table holds. An actor's own `begin`, and its parent's, may run before the table holds either.
    fn declared_at(&self, id: ActorId) -> Option<&'static Declared> {
        let is_me = id == self.me.id;
        if is_me {
            return Some(self.declared);
        }
        if let Above::Actor(parent, declared) = self.above {
            let is_parent = id == parent.id;
            if is_parent {
                return Some(declared);
            }
        }

        self.asked.stood_declared(id).or_else(|| self.table.declared(id))
    }

    /// The actor `to` names, beside this one, and its seat: none for a parent this turn cannot see, or a child that
    /// was never made.
    #[inline]
    fn receiver(&self, to: &To) -> Option<Seated> {
        match to {
            To::Parent => self.parent(),
            To::Child(kind) => self.child(&PathSegment::new(kind.clone(), &())),
            To::ChildInstanced(kind, key) => self.child(&PathSegment::instanced(kind.clone(), key.clone())),
            To::Held(reference) => Some(reference.seated()),
            // A turn puts the source of its item in its place before it asks.
            To::Sender => None,
        }
    }

    /// The actor directly above this one, and its seat, when it is live.
    fn parent(&self) -> Option<Seated> {
        let parent = match self.above {
            Above::Chassis => return None,
            Above::Actor(parent, _) | Above::Elsewhere(parent) => parent,
        };
        let is_live = self.table.is_live(parent.id);

        is_live.then_some(parent)
    }

    /// The child `segment` names beneath this actor, and its seat, when it was made: stood up in this turn, or
    /// before.
    fn child(&self, segment: &PathSegment) -> Option<Seated> {
        let id = self.table.child(self.me.id, segment);
        let seat = self.asked.stood_seat(id).or_else(|| self.table.child_seat(id))?;

        Some(Seated { id, seat })
    }

    /// Stands up a child of type `C` under `key`: adds its spawn effect, then runs its `begin` here, lent `instances`.
    /// A guest begins only its own types here, and asks its host to begin a child of any other (ADR-0015).
    fn spawn<C: Actor>(
        &mut self,
        instances: &mut Instances,
        key: &C::Key,
        config: C::Config,
    ) -> Result<ActorRef<C>, Refused> {
        let segment = PathSegment::new(C::NAME, key);
        let (path, id) = self.free_child(&segment)?;
        let declared = Declared::of::<C>();
        #[cfg(feature = "std")]
        if !instances.is_own(declared) {
            let guests = instances.guests();
            let config = config_bytes(&config).map_err(|_| Refused::NotMade)?;
            let from = Code::Linked(declared);

            return self.spawn_other(instances, id, &segment, path, from, &config, |config, birth| {
                guests.begin_linked(declared, config, birth)
            });
        }
        let (me, before) = self.ask_spawn(id, &segment, Code::Linked(declared));

        let stood_before = self.asked.stood.len();
        let above = Above::Actor(self.me, self.declared);
        let table = self.table;
        let asked = &mut *self.asked;
        let mut keeping = Keeping::default();
        // A child begun in a guest's code claims what it keeps, as its parent does (ADR-0015).
        let keeps = match self.keeps {
            Keeps::Here(_) => Keeps::Here(&mut keeping),
            Keeps::Claimed => Keeps::Claimed,
        };
        let child = Seat {
            me,
            path: &path,
            above,
            table,
            made: &(),
            asked,
            keeps,
            sent: 0,
            declared,
            during: During::Begin,
            checked: Checked::Nothing,
            owing: Owing::NOTHING,
        };
        let began = C::begin(config, &mut Beginning { seat: child, instances, sends: PhantomData });
        match began {
            Ok(actor) => {
                self.stand(me, path, above, declared, Box::new(actor), keeping);

                Ok(ActorRef::new(me))
            }
            Err(fault) => {
                self.take_back(before, stood_before, me.seat);

                Err(Refused::Began(fault))
            }
        }
    }

    /// Stands up a child of type `C` under `key` whose code is the blob `code` pins: adds its spawn effect, then has
    /// the blob's host run its `begin` here, in its part's instance of the blob, lent `instances` (ADR-0009,
    /// ADR-0015). Refused when the blob makes no actor of `C`'s type, as `C` declares it.
    #[cfg(feature = "std")]
    fn spawn_from<C: Actor>(
        &mut self,
        instances: &mut Instances,
        code: &BlobRef,
        key: &C::Key,
        config: &C::Config,
    ) -> Result<ActorRef<C>, Refused> {
        let segment = PathSegment::new(C::NAME, key);
        let (path, id) = self.free_child(&segment)?;
        let guests = instances.guests();
        let made = guests.made(code.id(), Declared::of::<C>()).ok_or(Refused::NotMade)?;
        let config = config_bytes(config).map_err(|_| Refused::NotMade)?;

        self.spawn_other(instances, id, &segment, path, Code::Blob(made), &config, |config, birth| {
            guests.begin(made, config, birth)
        })
    }

    /// Stands up the child `id`, which `segment` names beneath this actor at `path`, from `from`, code the turn does
    /// not run, made from `config`: adds its spawn effect, then has `begin` begin it where its code runs from the
    /// config's bytes, lent `instances`. The child is taken back when its `begin` was refused.
    #[cfg(feature = "std")]
    #[allow(clippy::too_many_arguments, reason = "the child, what it is made from, and what begins it, each its own")]
    fn spawn_other<C: Actor>(
        &mut self,
        instances: &mut Instances,
        id: ActorId,
        segment: &PathSegment,
        path: ActorPath,
        from: Code,
        config: &Config,
        begin: impl FnOnce(&[u8], Birth<'_>) -> Result<Box<dyn Runs>, Refused>,
    ) -> Result<ActorRef<C>, Refused> {
        let declared = from.declared();
        let claims_before = self.asked.claims.len();
        self.hand_config(&config.carried).map_err(|_| Refused::NotMade)?;
        let (me, before) = self.ask_spawn(id, segment, from);

        let stood_before = self.asked.stood.len();
        let above = Above::Actor(self.me, self.declared);
        let (refers, carried) = (&config.held, &config.carried);
        let mut keeping = Keeping::default();
        let birth = Birth {
            me,
            path: &path,
            above,
            table: self.table,
            asked: &mut *self.asked,
            instances,
            refers,
            keeping: &mut keeping,
            carried,
        };
        match begin(&config.bytes, birth) {
            Ok(actor) => {
                self.stand(me, path, above, declared, actor, keeping);

                Ok(ActorRef::new(me))
            }
            Err(refused) => {
                self.take_back(before, stood_before, me.seat);
                // A child refused took nothing, so what a guest claimed went in its config is its own to give again.
                self.asked.claims.truncate(claims_before);

                Err(refused)
            }
        }
    }

    /// The path and id of the child `segment` names beneath this actor, when its key is free: when this actor has
    /// made no child there, none its seat keeps from earlier steps, none live, and none stood up earlier in the step
    /// or in this turn (ADR-0008).
    fn free_child(&self, segment: &PathSegment) -> Result<(ActorPath, ActorId), Refused> {
        let path = self.path.child(segment).map_err(|PathTooDeep| Refused::TooDeep)?;
        let id = self.table.child(self.me.id, segment);
        let is_made = self.made.has_made(id);
        let is_live = self.table.child_seat(id).is_some();
        let is_stood = self.asked.stood_seat(id).is_some();
        let is_taken = is_made || is_live || is_stood;
        if is_taken {
            return Err(Refused::KeyTaken);
        }

        Ok((path, id))
    }

    /// Asks for the spawn of the child `id`, which `segment` names beneath this actor, from `from`, in a seat taken
    /// for it: answers with the child, and where the buffers stood before.
    fn ask_spawn(&mut self, id: ActorId, segment: &PathSegment, from: Code) -> (Seated, Marks) {
        let seat = self.asked.seats.take();
        let before = self.asked.buffers.marks();
        let spawn = segment.key().map_or(What::Spawn { from, seat }, |key| What::SpawnInstanced {
            from,
            key: key.clone(),
            seat,
        });
        self.ask(spawn);

        (Seated { id, seat }, before)
    }

    /// Stands up the child `me`, at `path`, beneath `above`, of the type `declared`, run by `actor` and keeping
    /// `keeping`, once its `begin` succeeded.
    fn stand(
        &mut self,
        me: Seated,
        path: ActorPath,
        above: Above,
        declared: &'static Declared,
        actor: Box<dyn Runs>,
        keeping: Keeping,
    ) {
        self.asked.stood_seats.insert(me.id.bits(), (me.seat, declared));
        self.asked.stood.push(Stood { me, path, above, actor, keeping });
    }

    /// Takes back a child that never began, in `seat`, and every child it stood up from the `stood_before`th: their
    /// seats are given back, and what they asked for since `before`, the spawn of the child among it, is taken back
    /// but for the lines they logged. What they gave is dropped with them, as what they kept and took is (ADR-0010).
    fn take_back(&mut self, before: Marks, stood_before: usize, seat: SeatNumber) {
        self.asked.buffers.mailed.truncate(before.mailed);
        self.asked.buffers.effects.truncate(before.effects);
        let mut gone = Vec::from([seat]);
        for below in self.asked.stood.drain(stood_before..) {
            self.asked.stood_seats.remove(&below.me.id.bits());
            self.asked.seats.give_back(below.me.seat);
            gone.push(below.me.seat);
        }
        self.asked.buffers.gifts.retain(|gift| match gift.by {
            Giver::Actor(by) => !gone.contains(&by.seat),
            Giver::Chassis => true,
        });
        self.asked.claims.retain(|claim| !gone.contains(&claim.by.seat));
        self.asked.seats.give_back(seat);
    }
}

/// What logs as one actor in its turn, and does nothing else (ADR-0009): lent by a turn to a helper that logs.
pub struct Logger<'t> {
    me: Seated,
    asked: &'t mut Asked,
}

impl Logger<'_> {
    /// Logs `text` at `level`, as the turn that lent this does. The line is kept when the turn fails. A text whose
    /// `Display` fails logs nothing.
    pub fn log(&mut self, level: Level, text: impl fmt::Display) {
        if let Ok(text) = self.asked.arena.write_text(text) {
            self.asked.buffers.lines.push(Line { by: self.me, level, text });
        }
    }
}

/// Where in its chain the item a turn handles came: the chain's first mail, and how far down it the item is
/// (ADR-0012).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chain {
    /// The first mail of the chain.
    pub root: MailId,
    /// How far down the chain the item is. Mail the turn sends is one deeper.
    pub depth: u32,
}

/// A turn as the engine opens it, before the handler it runs is known: what [`Runs::turn`] is given.
pub struct Turning<'a> {
    me: Seated,
    path: &'a ActorPath,
    above: Above,
    table: &'a dyn Table,
    made: &'a dyn Children,
    asked: &'a mut Asked,
    /// The instances of the actor's part, lent for the turn (ADR-0015).
    instances: &'a mut Instances,
    keeps: Keeps<'a>,
    sent: u64,
    source: Seated,
    chain: Chain,
    /// What the item is to a request (ADR-0017).
    tie: Tie,
}

impl<'a> Turning<'a> {
    /// The turn of the actor `me` names, at `path`, reading `table`, adding to `asked`, lent its part's `instances`
    /// and reaching what it keeps through `keeps`, which handles an item from the actor `source` names, at `chain`,
    /// and has sent `sent` pieces of mail before it, the item being to a request what `tie` says.
    ///
    /// For the engine. `table` answers for the step's start and for what this actor has spawned since.
    #[allow(clippy::too_many_arguments, reason = "what a turn is opened with, each fixed by the engine")]
    #[must_use]
    pub const fn new(
        me: Seated,
        path: &'a ActorPath,
        above: Above,
        table: &'a dyn Table,
        made: &'a dyn Children,
        asked: &'a mut Asked,
        instances: &'a mut Instances,
        keeps: Keeps<'a>,
        sent: u64,
        source: Seated,
        chain: Chain,
        tie: Tie,
    ) -> Self {
        Self { me, path, above, table, made, asked, instances, keeps, sent, source, chain, tie }
    }

    /// The source of the item the turn handles.
    #[must_use]
    pub const fn source(&self) -> Seated {
        self.source
    }

    /// Where the item the turn handles stands in its chain.
    #[must_use]
    pub const fn chain(&self) -> Chain {
        self.chain
    }

    /// The turn of the handler of `A` for `role`, which may send `S`.
    pub(crate) fn declaring<A: Actor, S: Sends>(self, role: Role) -> Turn<'a, S> {
        let Self { me, path, above, table, made, asked, instances, keeps, sent, source, chain, tie } = self;
        let declared = Declared::of::<A>();
        let seat = Seat {
            me,
            path,
            above,
            table,
            made,
            asked,
            keeps,
            sent,
            declared,
            during: During::Handling(role),
            checked: Checked::Nothing,
            owing: Owing::Item(tie),
        };

        Turn { seat, instances, source, chain, sends: PhantomData }
    }
}

/// What an actor asks of the engine while it handles one item, through a handler that may send `S` (ADR-0009,
/// ADR-0013).
///
/// Each ask adds an effect to the turn's buffer of its class, and nothing asked for happens
/// until the turn has returned `Ok`. Each read answers from the table of
/// actors as it stood when the turn's step began.
pub struct Turn<'a, S> {
    seat: Seat<'a>,
    /// The instances of the actor's part, which a child spawned from a blob begins in (ADR-0015).
    instances: &'a mut Instances,
    /// The actor the item being handled comes from: who [`To::Sender`] names.
    source: Seated,
    chain: Chain,
    sends: PhantomData<fn() -> S>,
}

impl<S> Turn<'_, S> {
    /// Watches the actor `of` refers to: when it ends, this actor is handed its ending, once, as an item.
    ///
    /// Watching an actor already watched changes nothing.
    ///
    /// It builds only when the turn's sends list [`Watches`](crate::Watches), which only a type that handles
    /// [`Ended`](crate::Ended) may (ADR-0013):
    ///
    /// ```compile_fail,E0277
    /// use aethereal_actor::{Actor, Beginning, Ending, Fault};
    /// use aethereal_data::{Bound, Kind, KindName, Plain};
    ///
    /// #[derive(Plain)]
    /// struct Nothing;
    ///
    /// impl Kind for Nothing {
    ///     const NAME: KindName = KindName::new("aether.doc.nothing");
    /// }
    ///
    /// #[derive(Bound)]
    /// struct Watcher;
    ///
    /// impl Kind for Watcher {
    ///     const NAME: KindName = KindName::new("aether.doc.watcher");
    /// }
    ///
    /// impl Actor for Watcher {
    ///     type Config = Nothing;
    ///     type Key = ();
    ///     type Handles = ();
    ///     type BeginSends = ();
    ///     type EndSends = ();
    ///
    ///     fn begin(_: Nothing, turn: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
    ///         let me = turn.me();
    ///         turn.watch(me);
    ///
    ///         Ok(Self)
    ///     }
    ///
    ///     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
    ///         Ok(())
    ///     }
    /// }
    /// ```
    pub fn watch<I>(&mut self, of: impl Into<ErasedActorRef>)
    where
        S: Allows<Watches, I>,
    {
        self.seat.watch(of.into());
    }

    /// Sends `mail` to the actor `to` names. It is delivered after the turn, in a later step.
    ///
    /// It builds only when the turn's sends list a [`ToType<T, K>`], and `T` is found from them when one names `K`.
    ///
    /// # Panics
    ///
    /// Panics when the actor named is not of type `T` (ADR-0013).
    pub fn send<T, K, I>(&mut self, to: To, mail: &K) -> Result<(), EncodeError>
    where
        T: Actor + Handles<K>,
        K: Kind + Encode<Actors> + Handled,
        S: Allows<ToType<T, K>, I>,
    {
        let to = self.named(to);
        self.seat.send_typed::<T, K>(&to, mail)
    }

    /// Sends `mail` to the actor `reference` refers to. What it reaches is proven, so nothing is checked when it
    /// is sent.
    ///
    /// It builds only when the turn's sends list what the reference [`Reaches`] by: [`ToType<T, K>`] for an
    /// [`ActorRef<T>`], and [`ToAny<P, K>`] for a [`ProtocolRef<P>`].
    pub fn send_to<R, K, I>(&mut self, reference: R, mail: &K) -> Result<(), EncodeError>
    where
        R: Reaches<K>,
        K: Kind + Encode<Actors>,
        S: Allows<R::Marker, I>,
    {
        self.seat.write(reference.seated(), mail)
    }

    /// Sends `mail` to each actor `references` refer to, in their order: each its own piece of mail, as if each were sent alone,
    /// and the bytes written once for all of them. What they reach is proven, so nothing is checked when it is sent.
    ///
    /// It builds only when the turn's sends list what the references [`Reaches`] by, as `send_to` does.
    pub fn send_to_all<R, K, I>(&mut self, references: &[R], mail: &K) -> Result<(), EncodeError>
    where
        R: Reaches<K>,
        K: Kind + Encode<Actors>,
        S: Allows<R::Marker, I>,
    {
        self.seat.write_to_all(references.iter().map(|reference| reference.seated()), mail)
    }

    /// Sends `mail` to the actor `to` names, whatever its type, when that type lists every kind of `P`.
    ///
    /// It builds only when the turn's sends list a [`ToAny<P, K>`], and `P` is found from them when one names `K`.
    ///
    /// # Panics
    ///
    /// Panics when the actor named does not list every kind of `P` (ADR-0013).
    pub fn send_to_any<P, K, I>(&mut self, to: To, mail: &K) -> Result<(), EncodeError>
    where
        P: Listed,
        K: Kind + Encode<Actors>,
        S: Allows<ToAny<P, K>, I>,
    {
        let to = self.named(to);
        self.seat.send_any::<P, K>(&to, mail)
    }

    /// The actor `to` names, with the source of the item being handled named by its reference.
    fn named(&self, to: To) -> To {
        match to {
            To::Sender => To::Held(ErasedActorRef::new(self.source)),
            named => named,
        }
    }

    /// Sends `mail` to the source of the item being handled. It is delivered after the turn, in a later step.
    ///
    /// It builds only when the turn's sends list [`ToSender<K>`].
    pub fn reply<K: Kind + Encode<Actors>, I>(&mut self, mail: &K) -> Result<(), EncodeError>
    where
        S: Allows<ToSender<K>, I>,
    {
        self.seat.reply(self.source, mail)
    }

    /// Asks `mail` of the actor `reference` refers to, keeping `context` until the request is answered, when the
    /// asker is handed what became of it with the context, as [`Answer<A, C>`](crate::Answer) (ADR-0017).
    ///
    /// It builds only when the turn's sends list [`Asks<P, K, C>`]: `P` answers `K` with one kind, and the actor
    /// handles that answer with `C`. What the reference reaches is proven, so nothing is checked when it is sent.
    pub fn ask_to<P, K, C, I>(&mut self, reference: ProtocolRef<P>, mail: &K, context: &C) -> Result<(), EncodeError>
    where
        P: Protocol + Carries<K>,
        K: Kind + Encode<Actors>,
        C: Kind + Encode<Within>,
        S: Allows<Asks<P, K, C>, I>,
    {
        self.seat.request(reference.seated(), mail, context)
    }

    /// Stands up a child of type `C` under `key`, running its `begin` here, and answers with a reference to it.
    ///
    /// The child takes its own mail. It is live once the engine applies this turn's effects.
    pub fn spawn<C: Actor>(&mut self, key: &C::Key, config: C::Config) -> Result<ActorRef<C>, Refused> {
        self.seat.spawn::<C>(self.instances, key, config)
    }

    /// Stands up a child of type `C` under `key` whose code is the blob `code` pins, running its `begin` here in the
    /// blob, and answers with a reference to it (ADR-0009). Refused when the blob makes no actor of `C`'s type.
    #[cfg(feature = "std")]
    pub fn spawn_from<C: Actor>(
        &mut self,
        code: &BlobRef,
        key: &C::Key,
        config: &C::Config,
    ) -> Result<ActorRef<C>, Refused> {
        self.seat.spawn_from::<C>(self.instances, code, key, config)
    }

    /// Keeps `value`, a resource this actor made, as its own, and answers with the handle to it (ADR-0010). Keeping
    /// is not an effect: it holds whether or not the turn goes on to fail.
    ///
    /// # Errors
    ///
    /// [`KeepsNothing`](crate::KeepsNothing) in a guest's code, which keeps nothing it makes.
    pub fn keep<R: Resource>(&mut self, value: R) -> Result<Handle<R>, Fault> {
        self.seat.keep(value)
    }

    /// The resource `handle` names, for this turn to use directly: only this actor holds the handle.
    ///
    /// # Errors
    ///
    /// [`NotHeld`](crate::NotHeld) when this actor holds no resource of type `R` under the handle's ticket, and
    /// [`KeepsNothing`](crate::KeepsNothing) in a guest's code.
    pub fn open<R: Resource>(&mut self, handle: &mut Handle<R>) -> Result<&mut R, Fault> {
        self.seat.open(handle)
    }

    /// Gives up the resource `handle` names, and answers with what names it on its way to a new owner, to send in
    /// mail. The resource goes with the one piece of mail that holds it, and when none does by the end of the turn,
    /// or the turn fails, it is dropped.
    ///
    /// # Errors
    ///
    /// [`NotHeld`](crate::NotHeld) when this actor holds no resource of type `R` under the handle's ticket. A guest's
    /// code holds no values, so its give is a claim its host holds it to when it hands the turn back, and a false one
    /// fails the turn there with the same fault (ADR-0015).
    pub fn give<R: Resource>(&mut self, handle: Handle<R>) -> Result<Given<R>, Fault> {
        self.seat.give(handle)
    }

    /// Takes the resource `given` names as this actor's own, and answers with the handle to it. It is taken in the
    /// turn of the mail it came with; what that turn does not take is dropped, and recorded in this actor's log.
    ///
    /// # Errors
    ///
    /// [`NotHeld`](crate::NotHeld) when no resource of type `R` came under that ticket with the mail being handled,
    /// or it was taken already. In a guest's code the take is a claim its host holds it to, as a give is.
    pub fn take<R: Resource>(&mut self, given: Given<R>) -> Result<Handle<R>, Fault> {
        self.seat.take_arrived(given, self.source)
    }

    /// Closes this actor. Its closing turn runs in the next step.
    pub fn close(&mut self) {
        self.seat.ask(What::Close);
    }

    /// Logs `text` at `level`. The line is kept when this turn fails.
    ///
    /// The text is written as `Display` shows it straight into the worker's arena, so a line built with
    /// `format_args!` allocates nothing.
    pub fn log(&mut self, level: Level, text: impl fmt::Display) {
        self.seat.logger().log(level, text);
    }

    /// A handle that logs as this actor and does nothing else: for a helper that logs and should not send.
    pub const fn logger(&mut self) -> Logger<'_> {
        self.seat.logger()
    }

    /// Puts `bytes` as a blob, which nobody changes from then on, and answers with the first reference to it
    /// (ADR-0014). It is refused when the bytes would take this worker past its share of the budget for blobs.
    #[cfg(feature = "std")]
    pub fn put(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Refused> {
        self.seat.put(bytes)
    }

    /// The bytes of the blob `blob` refers to.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn read(&self, blob: &BlobRef) -> &[u8] {
        self.seat.read(blob)
    }

    /// The content hash of the blob `blob` refers to: BLAKE3 over its bytes, computed the first time it is asked for
    /// and kept with the blob from the step's fold on.
    #[cfg(feature = "std")]
    pub fn hash(&mut self, blob: &BlobRef) -> BlobHash {
        self.seat.hash(blob)
    }

    /// A reference to this actor.
    #[must_use]
    pub const fn me(&self) -> ErasedActorRef {
        self.seat.me()
    }

    /// The source of the item being handled: the sender of mail, or the actor that ended.
    #[must_use]
    pub const fn sender(&self) -> ErasedActorRef {
        ErasedActorRef::new(self.source)
    }

    /// Where in its chain the item being handled came (ADR-0012).
    #[must_use]
    pub const fn chain(&self) -> Chain {
        self.chain
    }

    /// A reference to the live actor `path` names, as of the step's start.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.seat.resolve(path)
    }

    /// The actor `reference` refers to, as a reference to the tuple of kinds `P`, when its type lists every kind of
    /// `P`: none when one is missing (ADR-0016). It is read once, here, and the reference it answers with is not
    /// checked again.
    #[must_use]
    pub fn cast<P: Kinds>(&self, reference: impl Into<ErasedActorRef>) -> Option<ProtocolRef<P>> {
        self.seat.cast(reference.into())
    }
}

/// What an actor asks of the engine in its `begin`, which may send `S`: a first turn, which offers less than a turn
/// (ADR-0009, ADR-0013).
///
/// It offers no close: an actor does not end in the turn that makes it. It is handling no mail, so it has no
/// sender to send to.
pub struct Beginning<'a, S> {
    seat: Seat<'a>,
    /// The instances of the actor's part, which a child spawned from a blob begins in (ADR-0015).
    instances: &'a mut Instances,
    sends: PhantomData<fn() -> S>,
}

impl<'a, S: Sends> Beginning<'a, S> {
    /// The `begin` of the root of type `A` that `me` names, at `path`, reading `table`, adding to `asked`, lent its
    /// part's `instances`, and keeping what it keeps in `keeps`.
    ///
    /// For the engine, which makes a root for the chassis. A child's is made by the turn that spawns it, but for a
    /// guest's, which [`Self::beneath`] makes.
    #[must_use]
    pub fn new<A: Actor<BeginSends = S>>(
        me: Seated,
        path: &'a ActorPath,
        table: &'a dyn Table,
        asked: &'a mut Asked,
        instances: &'a mut Instances,
        keeps: Keeps<'a>,
    ) -> Self {
        let declared = Declared::of::<A>();
        let during = During::Begin;
        let seat = Seat {
            me,
            path,
            above: Above::Chassis,
            table,
            made: &(),
            asked,
            keeps,
            sent: 0,
            declared,
            during,
            checked: Checked::Nothing,
            owing: Owing::NOTHING,
        };

        Self { seat, instances, sends: PhantomData }
    }

    /// The `begin` of an actor of type `A` that `me` names, at `path`, beneath `above`, reading `table`, adding to
    /// `asked`, lent `instances`, and keeping what it keeps in `keeps`: for a guest's library, which begins an actor
    /// its host stood up from the guest's blob, beneath a parent of any code or none, and for a guest's host, which
    /// begins a native child of a guest (ADR-0015).
    #[must_use]
    pub fn beneath<A: Actor<BeginSends = S>>(
        me: Seated,
        path: &'a ActorPath,
        above: Above,
        table: &'a dyn Table,
        asked: &'a mut Asked,
        instances: &'a mut Instances,
        keeps: Keeps<'a>,
    ) -> Self {
        let mut beginning = Self::new::<A>(me, path, table, asked, instances, keeps);
        beginning.seat.above = above;

        beginning
    }
}

impl<S> Beginning<'_, S> {
    /// Sends `mail` to the actor `to` names, with the mail of the turn this `begin` runs in. It is delivered after the
    /// turn, in a later step.
    ///
    /// It builds only when the turn's sends list a [`ToType<T, K>`], and `T` is found from them when one names `K`.
    ///
    /// # Panics
    ///
    /// Panics when the actor named is not of type `T` (ADR-0013).
    pub fn send<T, K, I>(&mut self, to: ToWithoutMail, mail: &K) -> Result<(), EncodeError>
    where
        T: Actor + Handles<K>,
        K: Kind + Encode<Actors> + Handled,
        S: Allows<ToType<T, K>, I>,
    {
        let to = To::from(to);
        self.seat.send_typed::<T, K>(&to, mail)
    }

    /// Sends `mail` to the actor `reference` refers to, with the mail of the turn this `begin` runs in. What it reaches is proven, so nothing is checked when it
    /// is sent.
    ///
    /// It builds only when the turn's sends list what the reference [`Reaches`] by: [`ToType<T, K>`] for an
    /// [`ActorRef<T>`], and [`ToAny<P, K>`] for a [`ProtocolRef<P>`].
    pub fn send_to<R, K, I>(&mut self, reference: R, mail: &K) -> Result<(), EncodeError>
    where
        R: Reaches<K>,
        K: Kind + Encode<Actors>,
        S: Allows<R::Marker, I>,
    {
        self.seat.write(reference.seated(), mail)
    }

    /// Sends `mail` to each actor `references` refer to, in their order, with the mail of the turn this `begin` runs in: each its own piece of mail, as if each were sent alone,
    /// and the bytes written once for all of them. What they reach is proven, so nothing is checked when it is sent.
    ///
    /// It builds only when the turn's sends list what the references [`Reaches`] by, as `send_to` does.
    pub fn send_to_all<R, K, I>(&mut self, references: &[R], mail: &K) -> Result<(), EncodeError>
    where
        R: Reaches<K>,
        K: Kind + Encode<Actors>,
        S: Allows<R::Marker, I>,
    {
        self.seat.write_to_all(references.iter().map(|reference| reference.seated()), mail)
    }

    /// Sends `mail` to the actor `to` names, whatever its type, when that type lists every kind of `P`, with the mail
    /// of the turn this `begin` runs in.
    ///
    /// It builds only when the turn's sends list a [`ToAny<P, K>`], and `P` is found from them when one names `K`.
    ///
    /// # Panics
    ///
    /// Panics when the actor named does not list every kind of `P` (ADR-0013).
    pub fn send_to_any<P, K, I>(&mut self, to: ToWithoutMail, mail: &K) -> Result<(), EncodeError>
    where
        P: Listed,
        K: Kind + Encode<Actors>,
        S: Allows<ToAny<P, K>, I>,
    {
        let to = To::from(to);
        self.seat.send_any::<P, K>(&to, mail)
    }

    /// Keeps `value`, a resource this actor made, as its own, and answers with the handle to it (ADR-0010). If this
    /// `begin` then fails, the actor was never live, and what it kept is dropped with it.
    ///
    /// # Errors
    ///
    /// [`KeepsNothing`](crate::KeepsNothing) in a guest's code, which keeps nothing it makes.
    pub fn keep<R: Resource>(&mut self, value: R) -> Result<Handle<R>, Fault> {
        self.seat.keep(value)
    }

    /// The resource `handle` names, for this turn to use directly.
    ///
    /// # Errors
    ///
    /// [`NotHeld`](crate::NotHeld) when this actor holds no resource of type `R` under the handle's ticket, and
    /// [`KeepsNothing`](crate::KeepsNothing) in a guest's code.
    pub fn open<R: Resource>(&mut self, handle: &mut Handle<R>) -> Result<&mut R, Fault> {
        self.seat.open(handle)
    }

    /// Gives up the resource `handle` names, and answers with what names it on its way: to send in mail, or to put
    /// in the config of a child this `begin` stands up. Dropped when neither has it by the end of the turn.
    ///
    /// # Errors
    ///
    /// [`NotHeld`](crate::NotHeld) when this actor holds no resource of type `R` under the handle's ticket. A guest's
    /// code holds no values, so its give is a claim its host holds it to when it hands the turn back, and a false one
    /// fails the turn there with the same fault (ADR-0015).
    pub fn give<R: Resource>(&mut self, handle: Handle<R>) -> Result<Given<R>, Fault> {
        self.seat.give(handle)
    }

    /// Takes the resource `given` names, from this actor's config, as its own, and answers with the handle to it.
    /// What the config holds and this `begin` does not take is dropped.
    ///
    /// # Errors
    ///
    /// [`NotHeld`](crate::NotHeld) when the actor above this one, or the chassis for a root, gave no resource of type
    /// `R` under that ticket in this turn, or it was taken already. In a guest's code the take is a claim its host
    /// holds it to, as a give is.
    pub fn take<R: Resource>(&mut self, given: Given<R>) -> Result<Handle<R>, Fault> {
        self.seat.take_config(given)
    }

    /// Stands up a child of type `C` under `key`, running its `begin` here, and answers with a reference to it.
    pub fn spawn<C: Actor>(&mut self, key: &C::Key, config: C::Config) -> Result<ActorRef<C>, Refused> {
        self.seat.spawn::<C>(self.instances, key, config)
    }

    /// Stands up a child of type `C` under `key` whose code is the blob `code` pins, running its `begin` here in the
    /// blob, and answers with a reference to it (ADR-0009). Refused when the blob makes no actor of `C`'s type.
    #[cfg(feature = "std")]
    pub fn spawn_from<C: Actor>(
        &mut self,
        code: &BlobRef,
        key: &C::Key,
        config: &C::Config,
    ) -> Result<ActorRef<C>, Refused> {
        self.seat.spawn_from::<C>(self.instances, code, key, config)
    }

    /// Watches the actor `of` refers to: when it ends, this actor is handed its ending, once, as an item.
    ///
    /// It builds only when the `begin`'s sends list [`Watches`](crate::Watches), which only a type that handles
    /// [`Ended`](crate::Ended) may (ADR-0013):
    ///
    /// ```compile_fail,E0080
    /// use aethereal_actor::{Actor, Beginning, Ending, Fault, Watches};
    /// use aethereal_data::{Bound, Kind, KindName, Plain};
    ///
    /// #[derive(Plain)]
    /// struct Nothing;
    ///
    /// impl Kind for Nothing {
    ///     const NAME: KindName = KindName::new("aether.doc.nothing");
    /// }
    ///
    /// #[derive(Bound)]
    /// struct Watcher;
    ///
    /// impl Kind for Watcher {
    ///     const NAME: KindName = KindName::new("aether.doc.watcher");
    /// }
    ///
    /// impl Actor for Watcher {
    ///     type Config = Nothing;
    ///     type Key = ();
    ///     type Handles = ();
    ///     type BeginSends = (Watches,);
    ///     type EndSends = ();
    ///
    ///     fn begin(_: Nothing, turn: &mut Beginning<'_, (Watches,)>) -> Result<Self, Fault> {
    ///         let me = turn.me();
    ///         turn.watch(me);
    ///
    ///         Ok(Self)
    ///     }
    ///
    ///     fn end(self, _: &mut Ending<'_, ()>) -> Result<(), Fault> {
    ///         Ok(())
    ///     }
    /// }
    ///
    /// let declared = aethereal_actor::Declared::of::<Watcher>();
    /// ```
    pub fn watch<I>(&mut self, of: impl Into<ErasedActorRef>)
    where
        S: Allows<Watches, I>,
    {
        self.seat.watch(of.into());
    }

    /// Logs `text` at `level`. The line is kept when this turn fails.
    ///
    /// The text is written as `Display` shows it straight into the worker's arena, so a line built with
    /// `format_args!` allocates nothing.
    pub fn log(&mut self, level: Level, text: impl fmt::Display) {
        self.seat.logger().log(level, text);
    }

    /// A handle that logs as this actor and does nothing else: for a helper that logs and should not send.
    pub const fn logger(&mut self) -> Logger<'_> {
        self.seat.logger()
    }

    /// Puts `bytes` as a blob, which nobody changes from then on, and answers with the first reference to it
    /// (ADR-0014). It is refused when the bytes would take this worker past its share of the budget for blobs.
    #[cfg(feature = "std")]
    pub fn put(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Refused> {
        self.seat.put(bytes)
    }

    /// The bytes of the blob `blob` refers to.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn read(&self, blob: &BlobRef) -> &[u8] {
        self.seat.read(blob)
    }

    /// The content hash of the blob `blob` refers to: BLAKE3 over its bytes, computed the first time it is asked for
    /// and kept with the blob from the step's fold on.
    #[cfg(feature = "std")]
    pub fn hash(&mut self, blob: &BlobRef) -> BlobHash {
        self.seat.hash(blob)
    }

    /// A reference to this actor.
    #[must_use]
    pub const fn me(&self) -> ErasedActorRef {
        self.seat.me()
    }

    /// A reference to the live actor `path` names, as of the step's start.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.seat.resolve(path)
    }

    /// The actor `reference` refers to, as a reference to the tuple of kinds `P`, when its type lists every kind of
    /// `P`: none when one is missing (ADR-0016). It is read once, here, and the reference it answers with is not
    /// checked again.
    #[must_use]
    pub fn cast<P: Kinds>(&self, reference: impl Into<ErasedActorRef>) -> Option<ProtocolRef<P>> {
        self.seat.cast(reference.into())
    }
}

/// Why an actor's closing turn runs (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The actor asked to close.
    Closed,
    /// A turn of the actor's own returned a fault. Its state is whatever that turn left.
    Faulted,
}

/// A closing turn as the engine opens it, before the actor's type is known: what [`Runs::end`] is given.
pub struct Closing<'a> {
    me: Seated,
    path: &'a ActorPath,
    above: Above,
    table: &'a dyn Table,
    asked: &'a mut Asked,
    /// The instances of the actor's part, lent for the turn (ADR-0015).
    instances: &'a mut Instances,
    keeps: Keeps<'a>,
    sent: u64,
    why: Why,
}

impl<'a> Closing<'a> {
    /// The closing turn of the actor `me` names, at `path`, lent its part's `instances` and reaching what it keeps
    /// through `keeps`, which runs because of `why`, after the actor has sent `sent` pieces of mail.
    ///
    /// For the engine.
    #[allow(clippy::too_many_arguments, reason = "what a closing turn is opened with, each fixed by the engine")]
    #[must_use]
    pub const fn new(
        me: Seated,
        path: &'a ActorPath,
        above: Above,
        table: &'a dyn Table,
        asked: &'a mut Asked,
        instances: &'a mut Instances,
        keeps: Keeps<'a>,
        sent: u64,
        why: Why,
    ) -> Self {
        Self { me, path, above, table, asked, instances, keeps, sent, why }
    }

    /// The closing turn of an actor of type `A`.
    pub(crate) fn declaring<A: Actor>(self) -> Ending<'a, A::EndSends> {
        let Self { me, path, above, table, asked, keeps, sent, why, .. } = self;
        let declared = Declared::of::<A>();
        let seat = Seat {
            me,
            path,
            above,
            table,
            made: &(),
            asked,
            keeps,
            sent,
            declared,
            during: During::End,
            checked: Checked::Nothing,
            owing: Owing::NOTHING,
        };

        Ending { seat, why, sends: PhantomData }
    }
}

/// What an actor asks of the engine in its closing turn, which may send `S` (ADR-0009, ADR-0013).
///
/// It offers no spawn and no close. It is handling no mail, so it has no sender to send to.
pub struct Ending<'a, S> {
    seat: Seat<'a>,
    why: Why,
    sends: PhantomData<fn() -> S>,
}

impl<S> Ending<'_, S> {
    /// Why this closing turn runs.
    #[must_use]
    pub const fn why(&self) -> Why {
        self.why
    }

    /// Sends `mail` to the actor `to` names, as the actor's last mail. It is delivered after the turn, in a later step.
    ///
    /// It builds only when the turn's sends list a [`ToType<T, K>`], and `T` is found from them when one names `K`.
    ///
    /// # Panics
    ///
    /// Panics when the actor named is not of type `T` (ADR-0013).
    pub fn send<T, K, I>(&mut self, to: ToWithoutMail, mail: &K) -> Result<(), EncodeError>
    where
        T: Actor + Handles<K>,
        K: Kind + Encode<Actors> + Handled,
        S: Allows<ToType<T, K>, I>,
    {
        let to = To::from(to);
        self.seat.send_typed::<T, K>(&to, mail)
    }

    /// Sends `mail` to the actor `reference` refers to, as the actor's last mail. What it reaches is proven, so nothing is checked when it
    /// is sent.
    ///
    /// It builds only when the turn's sends list what the reference [`Reaches`] by: [`ToType<T, K>`] for an
    /// [`ActorRef<T>`], and [`ToAny<P, K>`] for a [`ProtocolRef<P>`].
    pub fn send_to<R, K, I>(&mut self, reference: R, mail: &K) -> Result<(), EncodeError>
    where
        R: Reaches<K>,
        K: Kind + Encode<Actors>,
        S: Allows<R::Marker, I>,
    {
        self.seat.write(reference.seated(), mail)
    }

    /// Sends `mail` to each actor `references` refer to, in their order, as the actor's last mail: each its own piece of mail, as if each were sent alone,
    /// and the bytes written once for all of them. What they reach is proven, so nothing is checked when it is sent.
    ///
    /// It builds only when the turn's sends list what the references [`Reaches`] by, as `send_to` does.
    pub fn send_to_all<R, K, I>(&mut self, references: &[R], mail: &K) -> Result<(), EncodeError>
    where
        R: Reaches<K>,
        K: Kind + Encode<Actors>,
        S: Allows<R::Marker, I>,
    {
        self.seat.write_to_all(references.iter().map(|reference| reference.seated()), mail)
    }

    /// Sends `mail` to the actor `to` names, whatever its type, when that type lists every kind of `P`, as the
    /// actor's last mail.
    ///
    /// It builds only when the turn's sends list a [`ToAny<P, K>`], and `P` is found from them when one names `K`.
    ///
    /// # Panics
    ///
    /// Panics when the actor named does not list every kind of `P` (ADR-0013).
    pub fn send_to_any<P, K, I>(&mut self, to: ToWithoutMail, mail: &K) -> Result<(), EncodeError>
    where
        P: Listed,
        K: Kind + Encode<Actors>,
        S: Allows<ToAny<P, K>, I>,
    {
        let to = To::from(to);
        self.seat.send_any::<P, K>(&to, mail)
    }

    /// The resource `handle` names, for a last use before it is dropped with this actor.
    ///
    /// # Errors
    ///
    /// [`NotHeld`](crate::NotHeld) when this actor holds no resource of type `R` under the handle's ticket, and
    /// [`KeepsNothing`](crate::KeepsNothing) in a guest's code.
    pub fn open<R: Resource>(&mut self, handle: &mut Handle<R>) -> Result<&mut R, Fault> {
        self.seat.open(handle)
    }

    /// Gives up the resource `handle` names, to send in this actor's last mail, so it outlives the actor. Dropped when
    /// no mail has it by the end of the turn.
    ///
    /// # Errors
    ///
    /// [`NotHeld`](crate::NotHeld) when this actor holds no resource of type `R` under the handle's ticket. A guest's
    /// code holds no values, so its give is a claim its host holds it to when it hands the turn back, and a false one
    /// fails the turn there with the same fault (ADR-0015).
    pub fn give<R: Resource>(&mut self, handle: Handle<R>) -> Result<Given<R>, Fault> {
        self.seat.give(handle)
    }

    /// Logs `text` at `level`. The line is kept when this turn fails.
    ///
    /// The text is written as `Display` shows it straight into the worker's arena, so a line built with
    /// `format_args!` allocates nothing.
    pub fn log(&mut self, level: Level, text: impl fmt::Display) {
        self.seat.logger().log(level, text);
    }

    /// A handle that logs as this actor and does nothing else: for a helper that logs and should not send.
    pub const fn logger(&mut self) -> Logger<'_> {
        self.seat.logger()
    }

    /// Puts `bytes` as a blob, which nobody changes from then on, and answers with the first reference to it
    /// (ADR-0014). It is refused when the bytes would take this worker past its share of the budget for blobs.
    #[cfg(feature = "std")]
    pub fn put(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Refused> {
        self.seat.put(bytes)
    }

    /// The bytes of the blob `blob` refers to.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn read(&self, blob: &BlobRef) -> &[u8] {
        self.seat.read(blob)
    }

    /// The content hash of the blob `blob` refers to: BLAKE3 over its bytes, computed the first time it is asked for
    /// and kept with the blob from the step's fold on.
    #[cfg(feature = "std")]
    pub fn hash(&mut self, blob: &BlobRef) -> BlobHash {
        self.seat.hash(blob)
    }

    /// A reference to this actor.
    #[must_use]
    pub const fn me(&self) -> ErasedActorRef {
        self.seat.me()
    }

    /// A reference to the live actor `path` names, as of the step's start.
    #[must_use]
    pub fn resolve(&self, path: &ActorPath) -> Option<ErasedActorRef> {
        self.seat.resolve(path)
    }

    /// The actor `reference` refers to, as a reference to the tuple of kinds `P`, when its type lists every kind of
    /// `P`: none when one is missing (ADR-0016). It is read once, here, and the reference it answers with is not
    /// checked again.
    #[must_use]
    pub fn cast<P: Kinds>(&self, reference: impl Into<ErasedActorRef>) -> Option<ProtocolRef<P>> {
        self.seat.cast(reference.into())
    }
}
