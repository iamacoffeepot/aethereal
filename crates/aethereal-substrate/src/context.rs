//! The context a turn's effects were emitted in (ADR-0011, ADR-0012).

use aethereal_actor::MailId;

/// The context a turn's effects were emitted in: the item the turn was reacting to (ADR-0012).
///
/// Every effect of one turn shares it, and a view folds each effect with it.
/// What a turn sends belongs to the chain the context names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// The mail the turn handled, or for an ending the mail that brought it about. None for a root's `begin`,
    /// which nothing caused: fixed when the context is made.
    pub cause: Option<MailId>,
    /// The first mail of the chain the turn belongs to.
    pub root: MailId,
}
