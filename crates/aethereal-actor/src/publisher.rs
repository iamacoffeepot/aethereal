//! The right to say which blob a kind is (ADR-0054).

use crate::{Gift, Given, Resource};

/// The right to move a kind's head, which names the blob a spawn from the head is made from: a resource only the
/// engine makes, one for each engine (ADR-0009, ADR-0054).
///
/// Its chassis hands it to one root's config as the program bootstraps,
/// and the root asks `turn.publish` with it. The engine keeps no record of
/// who holds it: the handle is the right, so no other actor changes which
/// code a kind runs. It is given and taken as any resource is.
///
/// Its one field is private, so no code outside this crate makes one: the
/// engine's comes only inside the gift it holds for a root.
pub struct Publisher(());

impl Resource for Publisher {}

impl Publisher {
    /// The engine's publisher under the ticket `ticket`, as the gift the engine holds for a root's config, and what
    /// names it on its way (ADR-0010). For the engine alone, which hands its one publisher out once: the value stays
    /// inside the gift, where no code reaches it to keep a second.
    #[must_use]
    pub fn handed(ticket: u64) -> (Gift, Given<Self>) {
        Gift::handed(ticket, Self(()))
    }
}
