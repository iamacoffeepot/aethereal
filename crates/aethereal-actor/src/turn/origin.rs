//! Where the code of a child spawned from a blob comes from (ADR-0054).

use crate::Bytes;

/// Which blob a child spawned from a blob is made from, as `turn.spawn_from` is told (ADR-0054).
///
/// A parent of a library's kind names only the kind and takes its head, so updating that kind is one publish for
/// every parent that makes it; a blob the parent holds is for code no library publishes, as a test's or the chassis's
/// is. The child runs that blob from then on, whatever the head does after.
#[derive(Debug, Clone, Copy)]
pub enum Origin<'a> {
    /// The head the heads view shows for the child's kind as the spawning turn's step began: refused with
    /// [`Refused::NoHead`](crate::Refused::NoHead) when the kind has none.
    Head,
    /// The blob this reference pins, which the parent holds.
    Blob(&'a Bytes),
}
