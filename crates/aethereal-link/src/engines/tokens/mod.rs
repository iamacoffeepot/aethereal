//! The tokens the hub mints, each good for one `Hello`, and what is asked of `Engines` about them (ADR-0040).

#[cfg(test)]
mod tests;

use super::{Spawn, SpawnEngine, Spawned};
use crate::Token;
use aethereal_actor::{ActorRef, Held, Time};
use aethereal_data::{Bound, Plain};
use aethereal_store::{Binary, StoreHash};
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
    /// An engine the hub spawned.
    Spawned,
    /// Nothing the hub holds: it never minted the token, the token expired, its spawn ended, or a `Hello` brought it
    /// already.
    Unknown,
}

/// What `Engines` keeps for a token it draws: the request for it, answered once it is drawn and the hub listens.
#[derive(Debug, Bound)]
#[kind(name = "aether.link.minting")]
pub enum Minting {
    /// A [`MintToken`]'s, answered with the token.
    External { held: Held<Minted> },
    /// A [`SpawnEngine`]'s, whose engine is spawned with the token.
    Spawn { held: Held<Spawn>, spawn: SpawnEngine },
}

/// What a token held was minted for.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
pub(super) enum Purpose {
    /// An engine started outside the hub, which may link with it before `expires_at`.
    External { expires_at: Time },
    /// The engine of the spawn `spawned`, asked for as `binary`, a copy of the build of `hash`, which may link with it
    /// until the spawn ends and [`forget`](Tokens::forget)s it.
    Spawn { spawned: ActorRef<Spawned>, binary: Binary, hash: StoreHash },
}

impl Purpose {
    /// Whether it may still link an engine at `now`: a spawn's token lasts as long as its spawn, which forgets it as it
    /// ends.
    fn is_live_at(&self, now: Time) -> bool {
        match self {
            Self::External { expires_at } => *expires_at > now,
            Self::Spawn { .. } => true,
        }
    }
}

/// The tokens `Engines` has minted that no `Hello` has brought, each with what it is for (ADR-0040). A token for an
/// engine started outside the hub expires: it is refused when a `Hello` brings it once expired, and dropped as the next
/// is minted. A spawn's lasts until its spawn ends.
#[derive(Debug, Default, Bound)]
pub(super) struct Tokens {
    held: BTreeMap<Token, Purpose>,
}

impl Tokens {
    /// Keeps `token`, minted at `now` for an engine started outside the hub, until `within` has passed: first
    /// dropping each such token expired at `now`, then the oldest of them while `most` or more are kept, so at most
    /// `most` are kept once it is, and it always is. A spawn's token is neither dropped nor counted.
    pub(super) fn mint_external(&mut self, token: Token, now: Time, within: Duration, most: usize) {
        self.held.retain(|_, purpose| purpose.is_live_at(now));
        while self.external().count() >= most {
            let oldest = self.external().min_by_key(|(_, expires_at)| *expires_at).map(|(token, _)| token);
            let Some(oldest) = oldest else {
                break;
            };
            self.held.remove(&oldest);
        }
        self.held.insert(token, Purpose::External { expires_at: now + within });
    }

    /// Keeps `token` for the spawn `spawned`, asked for as `binary`, which runs a copy of the build of `hash`, until a
    /// `Hello` brings it or the spawn ends and [`forget`](Self::forget)s it.
    pub(super) fn mint_spawn(&mut self, token: Token, spawned: ActorRef<Spawned>, binary: Binary, hash: StoreHash) {
        self.held.insert(token, Purpose::Spawn { spawned, binary, hash });
    }

    /// Takes `token` out, as a `Hello` that brought it at `now` does: what it was for, or none when it was never held,
    /// has been taken or forgotten, or has expired.
    pub(super) fn take(&mut self, token: Token, now: Time) -> Option<Purpose> {
        let purpose = self.held.remove(&token)?;

        purpose.is_live_at(now).then_some(purpose)
    }

    /// Each token kept for an engine started outside the hub, with when it expires.
    fn external(&self) -> impl Iterator<Item = (Token, Time)> + '_ {
        self.held.iter().filter_map(|(token, purpose)| match purpose {
            Purpose::External { expires_at } => Some((*token, *expires_at)),
            Purpose::Spawn { .. } => None,
        })
    }

    /// Lets go of `token`, whose spawn has ended: no `Hello` links with it from now on.
    pub(super) fn forget(&mut self, token: Token) {
        self.held.remove(&token);
    }
}
