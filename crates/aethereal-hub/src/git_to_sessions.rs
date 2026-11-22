//! The hub's root that tells the sessions where the hub's repository stands (ADR-0057, ADR-0059). Neither `Git` nor
//! `Sessions` depends on the other: the hub, which names both, lists this after them, and it depends on both.
//!
//! ```text
//! begin ──> HearReadiness to Git
//! Readiness, at once and as it changes ──> the sessions' Repository ──> Sessions
//! ```
//!
//! What it tells `Sessions` is where it stands, so `Sessions` refuses a start the repository cannot serve before any
//! container is made, whose bootstrap would fail; a program that lists no such root leaves `Sessions` doing without.

use aethereal_actor::{ActorRef, Beginning, Fault, Turn, actor, path};
use aethereal_data::Plain;
use aethereal_git::{Git, HearReadiness, Readiness};
use aethereal_sessions::{Repository, Sessions};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};

/// The hub's root that tells `Sessions` where `Git`'s repository stands. It keeps nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.hub.git_to_sessions")]
pub struct GitToSessions;

/// It stands up on every host that lists `Git` and `Sessions`.
impl<H: Host> Bootstrap<H> for GitToSessions {
    const SECTION: &'static str = "git_to_sessions";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Self)?;

        Ok(())
    }
}

#[actor(depends(Git, Sessions))]
impl GitToSessions {
    /// Asks `Git` to tell it where the repository stands.
    fn begin(root: Self, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.to(path::<Git>(())).send(&HearReadiness)?;

        Ok(root)
    }

    /// Where the repository stands, as `Git` tells it: told `Sessions`, in the sessions' words.
    #[tell]
    fn readiness(&mut self, readiness: Readiness, turn: &mut Turn<'_, Self, ActorRef<Git>>) -> Result<(), Fault> {
        let repository = match readiness {
            Readiness::Making => Repository::Making,
            Readiness::Ready => Repository::Ready,
            Readiness::Unmade => Repository::Unmade,
        };
        turn.to(path::<Sessions>(())).send(&repository)?;

        Ok(())
    }
}
