//! The context a turn's effects were emitted in (ADR-0011, ADR-0012).

use aethereal_actor::MailId;

/// The context a turn's effects were emitted in: the chain of the item the turn was reacting to (ADR-0012).
///
/// Every effect of one turn shares it, and a view folds each effect with it.
/// What a turn sends belongs to the chain the context names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// The first mail of the chain the turn belongs to.
    pub root: MailId,
    /// How far down the chain the mail the turn sends is: one more than the mail it handled, and one for a root's
    /// `begin`.
    pub depth: u32,
}
