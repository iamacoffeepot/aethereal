//! A guest module's `aethereal.declared` section: what each actor type its code makes declares, and the schema of its
//! state, as plain data (ADR-0004, ADR-0013, ADR-0015, ADR-0054).
//!
//! A guest writes it when it is built, from the same [`Declared`] a native
//! program reads, by a writer that runs in a constant. A host reads it back as
//! the plain types here, without making an instance, and checks it there.

mod state;
#[cfg(test)]
mod tests;

use crate::{Beneath, Declared, Handler, Role, Sending, Spawner, StateSchema, Target, TypeSegment, Unfit};
use aethereal_data::{Budget, Keyed, KindId, KindName, Plain, SchemaNodes, Shape, from_bytes};
use alloc::boxed::Box;
use alloc::vec::Vec;

/// The name of the section.
pub const NAME: &str = "aethereal.declared";

/// One actor type a guest makes, as its section holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Made {
    pub name: KindName,
    pub id: KindId,
    pub keyed: Key,
    pub spawner: Spawned,
    pub begins: Vec<Send>,
    pub handlers: Vec<Handles>,
    pub ends: Vec<Send>,
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

/// One send a handler may make, as a section holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Send {
    pub kind: KindId,
    pub to: To,
}

/// Where a declared send may go, as a section holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum To {
    Type { name: KindName, id: KindId },
    Sender,
    Handling { kinds: Vec<KindId> },
    Path { last: Segment, above: Vec<Segment> },
}

/// One segment of a typed path a send may target, as a section holds it: its last, then those above it to its root.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Segment {
    pub name: KindName,
    pub kind: KindId,
    pub keyed: Key,
}

/// One thing a type handles and what its handler may send, as a section holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Handles {
    pub role: Handled,
    pub sends: Vec<Send>,
}

/// What a handler handles, as a section holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Handled {
    Kind { kind: KindId },
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
    at = sendings(out, at, declared.begins());
    at = handlers(out, at, declared.handlers());

    sendings(out, at, declared.ends())
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

const fn sendings(out: &mut [u8], at: usize, sendings: &[Sending]) -> usize {
    let mut at = count(out, at, sendings.len());
    let mut index = 0;
    while index < sendings.len() {
        let sending = sendings[index];
        at = id(out, at, sending.kind());
        at = match sending.to() {
            Target::Type(type_name, type_id) => {
                let at = count(out, at, 0);
                let at = name(out, at, type_name);

                id(out, at, type_id)
            }
            Target::Sender => count(out, at, 1),
            Target::Handling(kinds) => {
                let mut at = count(out, at, 2);
                at = count(out, at, kinds.len());
                let mut kind = 0;
                while kind < kinds.len() {
                    at = id(out, at, kinds[kind]);
                    kind += 1;
                }

                at
            }
            Target::Path(last) => {
                let mut at = count(out, at, 3);
                at = segment(out, at, last);
                at = count(out, at, depth(last) - 1);
                let mut below = last;
                while let Beneath::Type(above) = below.beneath {
                    at = segment(out, at, above);
                    below = above;
                }

                at
            }
        };
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

                id(out, at, kind)
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
        at = sendings(out, at, handler.sends());
        index += 1;
    }

    at
}

impl Made {
    /// Whether its state's schema, with its name, makes the id it states: two types of one name and layout are one
    /// kind (ADR-0006).
    fn states_its_layout(&self) -> bool {
        KindId::from_description(&self.name, &self.state) == self.id
    }

    /// What the type declares, as the engine reads it, kept for as long as the process runs: a host keeps it for as
    /// long as it holds the blob that makes the type (ADR-0015).
    #[must_use]
    pub fn leak(&self) -> &'static Declared {
        let name: &'static KindName = Box::leak(Box::new(self.name.clone()));
        let handlers = self.handlers.iter().map(|handles| {
            let role = match handles.role {
                Handled::Kind { kind } => Role::Kind(kind),
                Handled::Ended => Role::Ended,
                Handled::Others => Role::Others,
                Handled::Answer { kind } => Role::Answer(kind),
                Handled::Stage { kind } => Role::Stage(kind),
            };

            Handler::new(role, leak_sends(&handles.sends))
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
        let begins = leak_sends(&self.begins);
        let ends = leak_sends(&self.ends);
        let state = StateSchema::Read(Box::leak(Box::new(self.state.clone())));
        let declared = Declared::new(name.clone(), self.id, state, keyed, spawner, begins, handlers, ends);

        Box::leak(Box::new(declared))
    }
}

/// The sends `sends` holds, as the engine reads them, kept for as long as the process runs.
fn leak_sends(sends: &[Send]) -> &'static [Sending] {
    let sendings = sends.iter().map(|send| {
        let to = match &send.to {
            To::Type { name, id } => Target::Type(Box::leak(Box::new(name.clone())), *id),
            To::Sender => Target::Sender,
            To::Handling { kinds } => Target::Handling(Box::leak(kinds.clone().into_boxed_slice())),
            To::Path { last, above } => Target::Path(leak_path(last, above)),
        };

        Sending::new(send.kind, to)
    });

    Box::leak(sendings.collect::<Box<[Sending]>>())
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
