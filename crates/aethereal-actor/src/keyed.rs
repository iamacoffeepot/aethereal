//! An actor whose state is split into sections, one for each key, whose turns of different keys run side by side
//! (spike: keyed sections).
//!
//! Spike code: the shape a macro would write, written by hand. See `docs/spikes/keyed-sections.md`.

use crate::answering::Token;
use crate::{
    Actor, ActorRef, Answering, Anyone, ChildOf, Declared, ErasedActorRef, Fault, Handled, Item, SectionKey, Turn,
    Turning,
};
use aethereal_data::{Kind, KindId};
use alloc::boxed::Box;
use core::any::Any;
use core::hash::{BuildHasherDefault, Hasher};
use std::collections::HashMap;

/// How an actor of this type finds the key of what it handles as `K`, from `From`: for mail of a kind, the mail
/// itself, so the key belongs to the receiver, not the kind, and the same kind can go unkeyed to an actor that is not
/// split; for an [`Answer<A, C>`](crate::Answer), the context `C` its asker keeps, which the asker reads as it asks to
/// find the section the answer goes back to, as `#[answer(key = ..)]` would declare (spike: keyed sections, return
/// stamps).
pub trait KeyOf<K>: Sectioned {
    /// What the key is read from.
    type From;

    /// The key of `from`.
    fn key(from: &Self::From) -> u64;
}

/// A target that names the type of a keyed receiver, so a send through it works out the key of what it sends.
pub trait KeyedTarget<K> {
    /// The section `mail` is for.
    fn section(mail: &K) -> SectionKey;
}

impl<T: KeyOf<K, From = K>, K> KeyedTarget<K> for ActorRef<T> {
    fn section(mail: &K) -> SectionKey {
        SectionKey::of(T::key(mail))
    }
}

/// An actor whose state is split into sections, one for each key. Its handlers are handed the section of their mail's
/// key to change, and the actor itself only to read: what it fixed as it began.
///
/// `Sync`, since every worker that runs one of its sections in a step reads it at once. A section need not be flat: a
/// failed turn ends the whole actor, as any failed turn does (ADR-0009), so no copy of it is kept to put back, and it
/// may hold what is not `Copy`, such as a held request.
pub trait Sectioned: Actor + ChildOf + Sync {
    /// One key's state.
    type Section: Default + Send + 'static;

    /// Hands `item` to the handler of its kind, with `section`, the section of the mail's key: what [`hand_section`]
    /// does for one handler.
    fn section(&self, section: &mut Self::Section, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault>;

    /// Whether `section` is no different from a section never made: at its default, holding nothing. The engine drops
    /// such a section once its turns have run and it neither waits on an answer nor owes one.
    fn is_unused(section: &Self::Section) -> bool;
}

/// Whether `item` is mail of kind `K`.
#[must_use]
pub fn is_of<K: Kind>(item: &Item<'_>) -> bool {
    match item {
        Item::Mail(mail) => mail.kind() == KindId::of::<K>(),
        _ => false,
    }
}

/// Whether `item` is what became of a request this actor asked, to be answered with `A`.
#[must_use]
pub fn is_answer_of<A: Kind>(item: &Item<'_>) -> bool {
    match item {
        Item::Answer { asking, .. } | Item::Unanswered { asking, .. } => asking.answer == KindId::of::<A>(),
        _ => false,
    }
}

/// A section's handler of what it handles as `H`: the actor to read, the section to change, what it is handed, and
/// its turn. It answers as an ordinary handler of `H` does: `()` for a tell or an answer, the answer `A`, a `Later<A>`
/// its hold made, or a `Reply<A>`.
pub type SectionHandler<A, H, R> =
    for<'a, 't> fn(&A, &mut <A as Sectioned>::Section, <H as Handled>::Item<'a>, &mut Turn<'t, A>) -> Result<R, Fault>;

/// Hands `item`, what `A` handles as `H`, to `handler`, with the actor `actor` to read and `section` to change, in a
/// turn of its own, and sends what it answers now: what a macro would write for a handler that declares its key.
pub fn hand_section<A: Sectioned, H: Handled, R: Answering>(
    actor: &A,
    section: &mut A::Section,
    item: Item<'_>,
    turning: Turning<'_>,
    handler: SectionHandler<A, H, R>,
) -> Result<(), Fault> {
    let taken = match H::take(item) {
        Ok(taken) => taken,
        Err(untaken) => return turning.untaken(untaken),
    };
    let sender = ErasedActorRef::new(turning.source());
    let mut turn = turning.declaring::<A, Anyone>(Declared::of::<A>(), sender);
    handler(actor, section, taken, &mut turn)?.send(&mut turn, Token::new())?;

    turn.settled()
}

/// One part's sections of one keyed actor, by key.
pub struct SectionStore<S> {
    cells: HashMap<u64, S, BuildHasherDefault<KeyBits>>,
}

/// The hash of a key: its bits spread by a multiply, as an author's keys are often small numbers in a row, which a
/// table hashed by their own bits would put in a row of slots with one tag. Spike: the engine's `IdBits` hashes ids,
/// which are already spread.
#[derive(Default)]
pub struct KeyBits(u64);

impl Hasher for KeyBits {
    fn finish(&self) -> u64 {
        self.0.wrapping_mul(0x9e37_79b9_7f4a_7c15)
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*byte);
        }
    }

    fn write_u64(&mut self, bits: u64) {
        self.0 = bits;
    }
}

impl<S> SectionStore<S> {
    /// The section of `key`, when it is not at its default.
    #[must_use]
    pub fn get(&self, key: u64) -> Option<&S> {
        self.cells.get(&key)
    }

    /// How many sections it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// A keyed actor as the engine runs it without naming its type: shared by every worker that runs one of its sections,
/// and handed the sections of the part it runs.
pub trait RunsSections: Send + Sync {
    /// What the actor's type declares.
    fn declared(&self) -> &'static Declared;

    /// An empty store of its sections, for a part that has none yet.
    fn store(&self) -> Box<dyn Any + Send>;

    /// The turn of `item`, for the section of `key`, found in `store`, a store [`Self::store`] made: a section at its
    /// default is made for a key's first item.
    fn turn(&self, store: &mut (dyn Any + Send), key: u64, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault>;

    /// Drops the section of `key` from `store` when it is unused: for the engine, once a run of the section's turns has
    /// left it neither waiting on an answer nor owing one. A section that waits or owes is never handed here, so the
    /// context of an answer still to come is never dropped with it.
    fn release(&self, store: &mut (dyn Any + Send), key: u64);
}

/// A keyed actor, as the engine runs it: the actor, to read, and whether a section left unused is dropped, which
/// looks at it after every run. `DROPS` is off only to measure what that costs.
pub struct SectionRunner<A, const DROPS: bool> {
    actor: A,
}

impl<A, const DROPS: bool> SectionRunner<A, DROPS> {
    /// The runner of `actor`.
    pub const fn new(actor: A) -> Self {
        Self { actor }
    }
}

impl<A: Sectioned, const DROPS: bool> SectionRunner<A, DROPS> {
    /// The store `store` is, as this runner's actor made it.
    fn sections(store: &mut (dyn Any + Send)) -> &mut SectionStore<A::Section> {
        let Some(store) = store.downcast_mut::<SectionStore<A::Section>>() else {
            unreachable!("a part's store of an actor's sections is the one that actor made");
        };

        store
    }
}

impl<A: Sectioned, const DROPS: bool> RunsSections for SectionRunner<A, DROPS> {
    fn declared(&self) -> &'static Declared {
        Declared::of::<A>()
    }

    fn store(&self) -> Box<dyn Any + Send> {
        Box::new(SectionStore::<A::Section> { cells: HashMap::default() })
    }

    fn turn(&self, store: &mut (dyn Any + Send), key: u64, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        let section = Self::sections(store).cells.entry(key).or_default();

        self.actor.section(section, item, turning)
    }

    fn release(&self, store: &mut (dyn Any + Send), key: u64) {
        if !DROPS {
            return;
        }
        let cells = &mut Self::sections(store).cells;
        let is_unused = cells.get(&key).is_some_and(A::is_unused);
        if is_unused {
            cells.remove(&key);
        }
    }
}
