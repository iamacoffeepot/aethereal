//! An actor whose state is split into sections, one for each key, whose turns of different keys run side by side
//! (spike: keyed sections).
//!
//! Spike code: the shape a macro would write, written by hand. See `docs/spikes/keyed-sections.md`.

use crate::{
    Actor, ActorRef, Actors, Anyone, ChildOf, Declared, ErasedActorRef, Fault, Handled, Item, SectionKey, Turn, Turning,
};
use aethereal_data::{Decode, Kind, KindId};
use alloc::boxed::Box;
use core::any::Any;
use core::hash::{BuildHasherDefault, Hasher};
use std::collections::HashMap;

/// How an actor of this type finds the key of mail of kind `K`: the key belongs to the receiver, not the kind, so the
/// same kind can go unkeyed to an actor that is not split.
pub trait KeyOf<K>: Sectioned {
    /// The key of `mail`.
    fn key(mail: &K) -> u64;
}

/// A target that names the type of a keyed receiver, so a send through it works out the key of what it sends.
pub trait KeyedTarget<K> {
    /// The section `mail` is for.
    fn section(mail: &K) -> SectionKey;
}

impl<T: KeyOf<K>, K> KeyedTarget<K> for ActorRef<T> {
    fn section(mail: &K) -> SectionKey {
        SectionKey::of(T::key(mail))
    }
}

/// An actor whose state is split into sections, one for each key. Its handlers are handed the section of their mail's
/// key to change, and the actor itself only to read: what it fixed as it began.
///
/// `Sync`, since every worker that runs one of its sections in a step reads it at once.
pub trait Sectioned: Actor + ChildOf + Sync {
    /// One key's state. Flat, so a turn's copy of it, kept to put back when the turn fails, is a copy of its bytes.
    type Section: Copy + Default + PartialEq + Send + 'static;

    /// Hands `item` to the handler of its kind, with `section`, the section of the mail's key: what [`hand_section`]
    /// does for one handler. Answers with how many pieces of mail the section has sent, this turn's included.
    fn section(&self, section: &mut Self::Section, item: Item<'_>, turning: Turning<'_>) -> Result<u64, Fault>;
}

/// Whether `item` is mail of kind `K`.
#[must_use]
pub fn is_of<K: Kind>(item: &Item<'_>) -> bool {
    match item {
        Item::Mail(mail) => mail.kind() == KindId::of::<K>(),
        _ => false,
    }
}

/// A section's handler of mail of kind `K`: the actor to read, the section to change, the mail, and its turn.
pub type SectionHandler<A, K> = fn(&A, &mut <A as Sectioned>::Section, K, &mut Turn<'_, A>) -> Result<(), Fault>;

/// Hands `item`, mail of kind `K`, to `handler`, with the actor `actor` to read and `section` to change, in a turn of
/// its own: what a macro would write for a handler that declares its key. Answers with how many pieces of mail the
/// section has sent, this turn's included.
pub fn hand_section<A: Sectioned, K: Kind + Decode<Actors> + 'static>(
    actor: &A,
    section: &mut A::Section,
    item: Item<'_>,
    turning: Turning<'_>,
    handler: SectionHandler<A, K>,
) -> Result<u64, Fault> {
    let sent = turning.sent();
    let mail = match <K as Handled>::take(item) {
        Ok(mail) => mail,
        Err(untaken) => return turning.untaken(untaken).map(|()| sent),
    };
    let sender = ErasedActorRef::new(turning.source());
    let mut turn = turning.declaring::<A, Anyone>(Declared::of::<A>(), sender);
    handler(actor, section, mail, &mut turn)?;
    turn.settled()?;

    Ok(turn.sent_count())
}

/// One part's sections of one keyed actor, by key, each with the count of the mail it has sent.
pub struct SectionStore<S> {
    cells: HashMap<u64, Cell<S>, BuildHasherDefault<KeyBits>>,
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

/// One section, and how many pieces of mail it has sent: a section counts its own, so two of one actor sending side
/// by side give no two pieces one id.
struct Cell<S> {
    state: S,
    sent: u64,
}

impl<S> SectionStore<S> {
    /// The section of `key`, when it is not at its default.
    #[must_use]
    pub fn get(&self, key: u64) -> Option<&S> {
        self.cells.get(&key).map(|cell| &cell.state)
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

/// Where the count of a section's mail starts: its key in the high half, so the counts of two sections never meet,
/// and above the actor's own, which start at one. Spike: a key below `2^30`, and a section that sends below `2^32`.
const fn first_count(key: u64) -> u64 {
    (key + 1) << 32
}

/// A keyed actor as the engine runs it without naming its type: shared by every worker that runs one of its sections,
/// and handed the sections of the part it runs.
pub trait RunsSections: Send + Sync {
    /// What the actor's type declares.
    fn declared(&self) -> &'static Declared;

    /// An empty store of its sections, for a part that has none yet.
    fn store(&self) -> Box<dyn Any + Send>;

    /// The turn of `item`, for the section of `key`, found in `store`, a store [`Self::store`] made: a section at its
    /// default is made for a key's first mail, and dropped once a turn leaves it at its default and it has sent
    /// nothing.
    fn turn(&self, store: &mut (dyn Any + Send), key: u64, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault>;
}

/// A keyed actor, as the engine runs it: the actor, to read; whether each turn keeps a copy of its section from
/// before it, to put back when the turn fails; and whether a section left at its default is dropped, which compares it
/// with the default after every turn. `KEEPS` and `DROPS` are off only to measure what each costs.
pub struct SectionRunner<A, const KEEPS: bool, const DROPS: bool> {
    actor: A,
}

impl<A, const KEEPS: bool, const DROPS: bool> SectionRunner<A, KEEPS, DROPS> {
    /// The runner of `actor`.
    pub const fn new(actor: A) -> Self {
        Self { actor }
    }
}

impl<A: Sectioned, const KEEPS: bool, const DROPS: bool> RunsSections for SectionRunner<A, KEEPS, DROPS> {
    fn declared(&self) -> &'static Declared {
        Declared::of::<A>()
    }

    fn store(&self) -> Box<dyn Any + Send> {
        Box::new(SectionStore::<A::Section> { cells: HashMap::default() })
    }

    fn turn(&self, store: &mut (dyn Any + Send), key: u64, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        let Some(store) = store.downcast_mut::<SectionStore<A::Section>>() else {
            unreachable!("a part's store of an actor's sections is the one that actor made");
        };
        let cell =
            store.cells.entry(key).or_insert_with(|| Cell { state: A::Section::default(), sent: first_count(key) });
        let turning = turning.with_sent(cell.sent);
        let outcome = if KEEPS {
            // The rollback: the section as it was, copied to the stack, and put back over what a failed turn left.
            let kept = cell.state;
            let outcome = self.actor.section(&mut cell.state, item, turning);
            if outcome.is_err() {
                cell.state = kept;
            }

            outcome
        } else {
            self.actor.section(&mut cell.state, item, turning)
        };
        if let Ok(sent) = outcome {
            cell.sent = sent;
        }

        if !DROPS {
            return outcome.map(|_| ());
        }
        // A section at its default that has sent nothing is no different from one never made. One that has sent keeps
        // its count, so its next mail is not numbered as mail it sent before.
        let is_default = cell.state == A::Section::default();
        let is_unsent = cell.sent == first_count(key);
        let is_unused = is_default && is_unsent;
        if is_unused {
            store.cells.remove(&key);
        }

        outcome.map(|_| ())
    }
}
