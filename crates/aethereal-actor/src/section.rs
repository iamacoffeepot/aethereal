//! A guest module's `aethereal.declared` section: what each actor type its code makes declares, as plain data
//! (ADR-0004, ADR-0013, ADR-0015).
//!
//! A guest writes it when it is built, from the same [`Declared`] a native
//! program reads, by a writer that runs in a constant. A host reads it back as
//! the plain types here, without making an instance.

use crate::{Declared, Handler, Role, Sending, Target};
use aethereal_data::{KindId, KindName, Plain};
use alloc::boxed::Box;
use alloc::vec::Vec;

/// The name of the section.
pub const NAME: &str = "aethereal.declared";

/// One actor type a guest makes, as its section holds it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Made {
    pub name: KindName,
    pub id: KindId,
    pub begins: Vec<Send>,
    pub handlers: Vec<Handles>,
    pub ends: Vec<Send>,
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
}

/// How many bytes the section of `types` takes.
#[must_use]
pub const fn len(types: &[&Declared]) -> usize {
    write(types, &mut [])
}

/// The section of `types`, in `N` bytes, which [`len`] says it takes.
///
/// # Panics
///
/// Panics, stopping the build, when `N` is not what [`len`] says.
#[must_use]
pub const fn bytes<const N: usize>(types: &[&Declared]) -> [u8; N] {
    let mut out = [0; N];
    let written = write(types, &mut out);
    assert!(written == N, "a section is written in the bytes its length says");

    out
}

/// Writes the section of `types` into `out` as far as it has room, and answers with how many bytes it takes.
const fn write(types: &[&Declared], out: &mut [u8]) -> usize {
    let mut at = count(out, 0, types.len());
    let mut index = 0;
    while index < types.len() {
        let declared = types[index];
        at = name(out, at, declared.name());
        at = id(out, at, declared.id());
        at = sendings(out, at, declared.begins());
        at = handlers(out, at, declared.handlers());
        at = sendings(out, at, declared.ends());
        index += 1;
    }

    at
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
    let text = name.as_str().as_bytes();
    let at = count(out, at, text.len());

    put(out, at, text)
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
        };
        index += 1;
    }

    at
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
        };
        at = sendings(out, at, handler.sends());
        index += 1;
    }

    at
}

impl Made {
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
            };

            Handler::new(role, leak_sends(&handles.sends))
        });
        let handlers: &'static [Handler] = Box::leak(handlers.collect::<Box<[Handler]>>());
        let declared = Declared::new(name.clone(), self.id, leak_sends(&self.begins), handlers, leak_sends(&self.ends));

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
        };

        Sending::new(send.kind, to)
    });

    Box::leak(sendings.collect::<Box<[Sending]>>())
}
