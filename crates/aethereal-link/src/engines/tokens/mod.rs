//! The tokens the hub mints, each good for one `Hello`, and what is asked of `Engines` about them (ADR-0040).

#[cfg(test)]
mod tests;

use crate::Token;
use aethereal_actor::{Held, Time};
use aethereal_data::{Bound, Plain};
use core::time::Duration;
use std::collections::BTreeMap;

/// Asks `Engines`, at `path::<Engines>(())`, for a token for an engine started outside the hub, answered with
/// [`Minted`] once the hub listens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.mint_token")]
pub struct MintToken;

/// A token minted for an engine started outside the hub, and exactly what to start that engine with: `--hub`, the
/// host engines dial, `--port`, the port the hub listens on, and `--hub-token`, the token. It is good for one link,
/// within the hub's `token_within_millis`.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.minted")]
pub struct Minted {
    pub token: Token,
    pub args: Vec<String>,
}

/// Asks `Engines`, by an engine whose `Hello` brought `token`, to take the token out of its table, answered with what
/// it was for. A token is good for one `Hello`: once taken, it is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.take_token")]
pub struct TakeToken {
    pub token: Token,
}

/// What a token taken was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.token_for")]
pub enum TokenFor {
    /// An engine started outside the hub.
    External,
    /// Nothing the hub holds: it never minted the token, the token expired, or a `Hello` brought it already.
    Unknown,
}

/// What `Engines` keeps for a token it draws: the request for it, answered once it is drawn and the hub listens.
#[derive(Debug, Bound)]
#[kind(name = "aether.link.minting")]
pub struct Minting {
    pub(super) held: Held<Minted>,
}

/// What a token held was minted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Purpose {
    /// An engine started outside the hub, which may link with it before `expires_at`.
    External { expires_at: Time },
}

impl Purpose {
    /// When it expires.
    const fn expires_at(self) -> Time {
        match self {
            Self::External { expires_at } => expires_at,
        }
    }
}

/// The tokens `Engines` has minted that no `Hello` has brought, each with what it is for (ADR-0040). A token expired
/// is refused when a `Hello` brings it, and dropped as the next is minted.
#[derive(Debug, Default, Plain)]
pub(super) struct Tokens {
    held: BTreeMap<Token, Purpose>,
}

impl Tokens {
    /// Keeps `token`, minted at `now` for an engine started outside the hub, until `within` has passed: first
    /// dropping each token expired at `now`, then the oldest while `most` or more are kept, so at most `most` are kept
    /// once it is, and it always is.
    pub(super) fn mint_external(&mut self, token: Token, now: Time, within: Duration, most: usize) {
        self.held.retain(|_, purpose| purpose.expires_at() > now);
        while self.held.len() >= most {
            let oldest = self.held.iter().min_by_key(|(_, purpose)| purpose.expires_at()).map(|(token, _)| *token);
            let Some(oldest) = oldest else {
                break;
            };
            self.held.remove(&oldest);
        }
        self.held.insert(token, Purpose::External { expires_at: now + within });
    }

    /// Takes `token` out, as a `Hello` that brought it at `now` does: what it was for, or that it is unknown when it
    /// was never held, has been taken, or has expired.
    pub(super) fn take(&mut self, token: Token, now: Time) -> TokenFor {
        let Some(purpose) = self.held.remove(&token) else {
            return TokenFor::Unknown;
        };
        let expired = purpose.expires_at() <= now;
        if expired {
            return TokenFor::Unknown;
        }

        match purpose {
            Purpose::External { .. } => TokenFor::External,
        }
    }
}
