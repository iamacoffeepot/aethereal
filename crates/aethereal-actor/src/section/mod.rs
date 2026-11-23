//! A guest module's `aethereal.declared` section: what each actor type its code makes declares, and the schema of its
//! state, as plain data (ADR-0004, ADR-0013, ADR-0015, ADR-0054).
//!
//! A guest writes it when it is built, from the same [`Declared`] a native
//! program reads, by a writer that runs in a constant. A host reads it back as
//! the plain types here, without making an instance, and checks it there.

pub mod kinds;
mod state;
#[cfg(test)]
mod tests;

use crate::{Beneath, Declared, Handler, Role, Spawner, StateSchema, TypeSegment, Unfit};
use aethereal_data::{Budget, Keyed, KindId, KindName, Plain, SchemaNodes, Shape, from_bytes};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::iter;

/// The name of the section.
pub const NAME: &str = "aethereal.declared";

/// One actor type a guest makes, as its section holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Made {
    pub name: KindName,
    pub id: KindId,
    /// The kind an actor of it is configured by (ADR-0059).
    pub config: KindId,
    pub keyed: Key,
    pub spawner: Spawned,
    /// The typed path to each type it depends on (ADR-0059).
    pub depends: Vec<Depended>,
    /// The kind of each type it spawns (ADR-0059).
    pub spawns: Vec<KindId>,
    pub handlers: Vec<Handled>,
    /// The schema of an actor's state, its names included, which must be of the layout `id` is made from (ADR-0006,
    /// ADR-0054).
    pub state: SchemaNodes,
}

/// Whether a type has one actor or many, as a section holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Key {
    Singleton,
    Instanced,
}

/// Who may spawn an actor of a type, as a section holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Spawned {
    Nobody,
    Anyone,
    Own,
    Type { id: KindId },
}

/// The typed path to a type a type depends on, as a section holds it: its last segment, then those above it to its
/// root (ADR-0059).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Depended {
    pub last: Segment,
    pub above: Vec<Segment>,
}

/// One segment of a typed path, as a section holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Segment {
    pub name: KindName,
    pub kind: KindId,
    pub keyed: Key,
}

/// What a handler handles, as a section holds it, and for mail of a kind, the kinds it answers with, one or none
/// (ADR-0059).
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Handled {
    Kind { kind: KindId, answers: Vec<KindId> },
    Ended,
    Others,
    Answer { kind: KindId },
    Stage { kind: KindId },
}

/// How many shapes the largest state of `types` holds: the room [`len`] and [`bytes`] keep the nodes of a state in as
/// they write it, as a constant cannot allocate.
///
/// # Panics
///
/// Panics, as [`len`] does, when a type's state is one a host read.
#[must_use]
pub const fn room(types: &[&Declared]) -> usize {
    let mut most = 0;
    let mut index = 0;
    while index < types.len() {
        let shapes = state::shapes(built(types[index]));
        if shapes > most {
            most = shapes;
        }
        index += 1;
    }

    most
}

/// How many bytes the section of `types` takes, its states written in the room `ROOM`, which [`room`] says.
///
/// # Panics
///
/// Panics, stopping the build, when a type's state is one a host read, which is not written again, or `ROOM` is less
/// than [`room`] says.
#[must_use]
pub const fn len<const ROOM: usize>(types: &[&Declared]) -> usize {
    write::<ROOM>(types, &mut [])
}

/// The section of `types`, in `N` bytes, which [`len`] says it takes, its states written in the room `ROOM`, which
/// [`room`] says.
///
/// # Panics
///
/// Panics, stopping the build, when `N` is not what [`len`] says, or as [`len`] does.
#[must_use]
pub const fn bytes<const N: usize, const ROOM: usize>(types: &[&Declared]) -> [u8; N] {
    let mut out = [0; N];
    let written = write::<ROOM>(types, &mut out);
    assert!(written == N, "a section is written in the bytes its length says");

    out
}

/// What the section says of the one type `declared`, but for its state's schema: what a guest's host compares what a
/// guest holds of a type with, as each writes it the same way (ADR-0015). A spawn does not read a state's names, and
/// the id holds its layout (ADR-0006).
#[must_use]
pub fn of(declared: &Declared) -> Vec<u8> {
    let len = declared_type(&mut [], count(&mut [], 0, 1), declared);
    let mut out = alloc::vec![0; len];
    let at = count(&mut out, 0, 1);
    declared_type(&mut out, at, declared);

    out
}

/// The types the section `section` says its module makes, read within `budget`: refused, as its module is where it
/// is put, when it does not read back, its states' schemas among it, or a type's state is not of the layout its id is
/// made from (ADR-0006, ADR-0015).
///
/// # Errors
///
/// [`Unfit::Undeclared`] when it does not read back, and [`Unfit::Misstated`] for the first type whose state is of
/// another layout than its id's.
pub fn read(section: &[u8], budget: &mut Budget) -> Result<Vec<Made>, Unfit> {
    let made = from_bytes::<Vec<Made>>(section, budget).map_err(|_| Unfit::Undeclared)?;
    let misstated = made.iter().find(|made| !made.states_its_layout());
    if let Some(misstated) = misstated {
        return Err(Unfit::Misstated(misstated.name.clone()));
    }

    Ok(made)
}

/// Writes the section of `types` into `out` as far as it has room, its states in the room `ROOM`, and answers with how
/// many bytes it takes.
const fn write<const ROOM: usize>(types: &[&Declared], out: &mut [u8]) -> usize {
    let mut at = count(out, 0, types.len());
    let mut index = 0;
    while index < types.len() {
        let declared = types[index];
        at = declared_type(out, at, declared);
        at = state::write::<ROOM>(out, at, built(declared));
        index += 1;
    }

    at
}

/// Writes what the section says of the type `declared` at `at`, but for its state's schema, which follows it.
const fn declared_type(out: &mut [u8], at: usize, declared: &Declared) -> usize {
    let mut at = name(out, at, declared.name());
    at = id(out, at, declared.id());
    at = id(out, at, declared.config());
    at = match declared.keyed() {
        Keyed::Singleton => count(out, at, 0),
        Keyed::Instanced => count(out, at, 1),
    };
    at = match declared.spawner() {
        Spawner::Nobody => count(out, at, 0),
        Spawner::Anyone => count(out, at, 1),
        Spawner::Own => count(out, at, 2),
        Spawner::Type(type_id) => {
            let at = count(out, at, 3);

            id(out, at, type_id)
        }
    };
    at = depends(out, at, declared.depends());
    at = kinds(out, at, declared.spawns());
    handlers(out, at, declared.handlers())
}

/// The shape of the state of `declared`, as its code is built: what a section is written from.
///
/// # Panics
///
/// Panics when its state is one a host read from a section, which no build writes again.
const fn built(declared: &Declared) -> &'static Shape {
    match declared.state() {
        StateSchema::Built(shape) => shape,
        StateSchema::Read(_) => panic!("a section is written from the types its code is built with"),
    }
}

/// Writes `bytes` at `at`, when `out` has room, and answers with where what follows them goes.
const fn put(out: &mut [u8], at: usize, bytes: &[u8]) -> usize {
    let fits = out.len() >= at + bytes.len();
    if fits {
        let mut index = 0;
        while index < bytes.len() {
            out[at + index] = bytes[index];
            index += 1;
        }
    }

    at + bytes.len()
}

/// A count or a selector: an unsigned 32-bit number, little-endian (ADR-0002).
const fn count(out: &mut [u8], at: usize, count: usize) -> usize {
    assert!(count <= u32::MAX as usize, "a section counts less than u32::MAX of anything");
    #[allow(clippy::cast_possible_truncation, reason = "the count was checked to fit")]
    put(out, at, &(count as u32).to_le_bytes())
}

const fn id(out: &mut [u8], at: usize, id: KindId) -> usize {
    put(out, at, &id.plain_bytes())
}

const fn name(out: &mut [u8], at: usize, name: &KindName) -> usize {
    text(out, at, name.as_str())
}

/// Text: a count, then that many bytes of UTF-8 (ADR-0002).
const fn text(out: &mut [u8], at: usize, text: &str) -> usize {
    let bytes = text.as_bytes();
    let at = count(out, at, bytes.len());

    put(out, at, bytes)
}

/// Writes the typed path to each type a type depends on: its last segment, then those above it to its root, as
/// [`Depended`] reads them.
const fn depends(out: &mut [u8], at: usize, depends: &[&TypeSegment]) -> usize {
    let mut at = count(out, at, depends.len());
    let mut index = 0;
    while index < depends.len() {
        let last = depends[index];
        at = segment(out, at, last);
        at = count(out, at, depth(last) - 1);
        let mut below = last;
        while let Beneath::Type(above) = below.beneath {
            at = segment(out, at, above);
            below = above;
        }
        index += 1;
    }

    at
}

/// How many segments the typed path whose last is `segment` has.
const fn depth(segment: &TypeSegment) -> usize {
    let mut depth = 1;
    let mut segment = segment;
    while let Beneath::Type(above) = segment.beneath {
        depth += 1;
        segment = above;
    }

    depth
}

/// Writes each of `kinds`: the kind of each type a type spawns, as [`Made::spawns`] reads them, or of each answer a
/// handler answers with, as [`Handled::Kind`] reads them.
const fn kinds(out: &mut [u8], at: usize, kinds: &[KindId]) -> usize {
    let mut at = count(out, at, kinds.len());
    let mut index = 0;
    while index < kinds.len() {
        at = id(out, at, kinds[index]);
        index += 1;
    }

    at
}

/// Writes one segment of a typed path: its type's name and kind, and whether it is keyed, as [`Segment`] reads them.
const fn segment(out: &mut [u8], at: usize, segment: &TypeSegment) -> usize {
    let at = name(out, at, segment.name);
    let at = id(out, at, segment.kind);
    let keyed = match segment.keyed {
        Keyed::Singleton => 0,
        Keyed::Instanced => 1,
    };

    count(out, at, keyed)
}

const fn handlers(out: &mut [u8], at: usize, handlers: &[Handler]) -> usize {
    let mut at = count(out, at, handlers.len());
    let mut index = 0;
    while index < handlers.len() {
        let handler = &handlers[index];
        at = match handler.role() {
            Role::Kind(kind) => {
                let at = count(out, at, 0);
                let at = id(out, at, kind);

                kinds(out, at, handler.answers())
            }
            Role::Ended => count(out, at, 1),
            Role::Others => count(out, at, 2),
            Role::Answer(kind) => {
                let at = count(out, at, 3);

                id(out, at, kind)
            }
            Role::Stage(kind) => {
                let at = count(out, at, 4);

                id(out, at, kind)
            }
        };
        index += 1;
    }

    at
}

impl Made {
    /// Every kind it names: its own, its config's, its spawner's, each on the typed path to a type it depends on, each
    /// it spawns, and each its handlers handle and answer with. A kind may be named more than once.
    pub fn kinds(&self) -> impl Iterator<Item = KindId> + '_ {
        let spawner = match self.spawner {
            Spawned::Type { id } => Some(id),
            Spawned::Nobody | Spawned::Anyone | Spawned::Own => None,
        };

        [self.id, self.config]
            .into_iter()
            .chain(spawner)
            .chain(self.depends.iter().flat_map(Depended::kinds))
            .chain(self.spawns.iter().copied())
            .chain(self.handlers.iter().flat_map(Handled::kinds))
    }

    /// Whether its state's schema, with its name, makes the id it states: two types of one name and layout are one
    /// kind (ADR-0006).
    fn states_its_layout(&self) -> bool {
        KindId::from_description(&self.name, &self.state) == Ok(self.id)
    }

    /// What the type declares, as the engine reads it, kept for as long as the process runs: a host keeps it for as
    /// long as it holds the blob that makes the type (ADR-0015).
    #[must_use]
    pub fn leak(&self) -> &'static Declared {
        let name: &'static KindName = Box::leak(Box::new(self.name.clone()));
        let handlers = self.handlers.iter().map(|handled| match handled {
            Handled::Kind { kind, answers } => {
                let answers: &'static [KindId] = Box::leak(answers.clone().into_boxed_slice());

                Handler::answering(*kind, answers)
            }
            Handled::Ended => Handler::new(Role::Ended),
            Handled::Others => Handler::new(Role::Others),
            Handled::Answer { kind } => Handler::new(Role::Answer(*kind)),
            Handled::Stage { kind } => Handler::new(Role::Stage(*kind)),
        });
        let handlers: &'static [Handler] = Box::leak(handlers.collect::<Box<[Handler]>>());
        let keyed = match self.keyed {
            Key::Singleton => Keyed::Singleton,
            Key::Instanced => Keyed::Instanced,
        };
        let spawner = match self.spawner {
            Spawned::Nobody => Spawner::Nobody,
            Spawned::Anyone => Spawner::Anyone,
            Spawned::Own => Spawner::Own,
            Spawned::Type { id } => Spawner::Type(id),
        };
        let depends = self.depends.iter().map(|depended| leak_path(&depended.last, &depended.above));
        let depends: &'static [&'static TypeSegment] = Box::leak(depends.collect::<Box<[&'static TypeSegment]>>());
        let spawns: &'static [KindId] = Box::leak(self.spawns.clone().into_boxed_slice());
        let state = StateSchema::Read(Box::leak(Box::new(self.state.clone())));
        let declared =
            Declared::new(name.clone(), self.id, state, self.config, keyed, spawner, depends, spawns, handlers);

        Box::leak(Box::new(declared))
    }
}

impl Depended {
    /// The kind of each segment of the path, from its last to its root.
    fn kinds(&self) -> impl Iterator<Item = KindId> + '_ {
        iter::once(&self.last).chain(&self.above).map(|segment| segment.kind)
    }
}

impl Handled {
    /// The kind it handles and each it answers with, or none, for a handler of the end of an actor it watches, or of
    /// every other kind.
    fn kinds(&self) -> impl Iterator<Item = KindId> + '_ {
        let (kind, answers): (Option<KindId>, &[KindId]) = match self {
            Self::Kind { kind, answers } => (Some(*kind), answers),
            Self::Answer { kind } | Self::Stage { kind } => (Some(*kind), &[]),
            Self::Ended | Self::Others => (None, &[]),
        };

        kind.into_iter().chain(answers.iter().copied())
    }
}

/// The typed path whose last segment is `last`, with `above` from the one over it to its root, kept for as long as the
/// process runs.
fn leak_path(last: &Segment, above: &[Segment]) -> &'static TypeSegment {
    let beneath =
        above.iter().rev().fold(Beneath::Top, |beneath, segment| Beneath::Type(leak_segment(segment, beneath)));

    leak_segment(last, beneath)
}

/// The segment `segment` reads as, beneath `beneath`, kept for as long as the process runs.
fn leak_segment(segment: &Segment, beneath: Beneath) -> &'static TypeSegment {
    let name: &'static KindName = Box::leak(Box::new(segment.name.clone()));
    let keyed = match segment.keyed {
        Key::Singleton => Keyed::Singleton,
        Key::Instanced => Keyed::Instanced,
    };

    Box::leak(Box::new(TypeSegment { name, kind: segment.kind, keyed, beneath }))
}
