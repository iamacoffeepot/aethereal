//! Whether the hub trusts an engine (ADR-0057): decided by the hub as the engine links, from who asked for its token or
//! its spawn, and never said by the engine.

use aethereal_data::Plain;
use aethereal_tools::Caller;

/// Whether the hub trusts an engine linked. An engine the owner started is trusted; one a session spawned or minted a
/// token for is not, as it runs code the session has just written, its link included (ADR-0057).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.trust")]
pub enum Trust {
    /// The owner started it: it is sent and may send chat.
    Trusted,
    /// The session numbered `session` started it: it keeps the link as it is, its tools and the hub's calls of them.
    Untrusted { session: u32 },
}

impl Trust {
    /// Whether it is trusted.
    #[must_use]
    pub const fn is_trusted(self) -> bool {
        matches!(self, Self::Trusted)
    }
}

/// An engine is trusted when the owner asked for it, and not when a session did.
impl From<Caller> for Trust {
    fn from(by: Caller) -> Self {
        match by {
            Caller::Owner => Self::Trusted,
            Caller::Session { session } => Self::Untrusted { session },
        }
    }
}
