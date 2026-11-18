//! The hub's root that puts each trusted engine's chat through to the sessions (ADR-0057). A hub that runs sessions
//! lists it after `Engines` and `Sessions`, and one that runs none lists no such root, so neither `Engines` nor an
//! engine depends on the sessions.
//!
//! ```text
//! begin ──> HearTrusted to Engines
//! TrustedEngine { engine }, for each engine trusted already and each after ──> ChatTo { to: Sessions } to the engine
//! the engine ──> Joined, then each chat frame as Chatted, from its own actor ──> Sessions
//! ```
//!
//! It hands the engine `Sessions` and steps aside: the sessions take a `Chatted` only from the actor that joined as its
//! engine, so no engine speaks as another, and an engine joins and chats only once the hub trusts it, as it keeps
//! for itself. An engine trusted before it hears is told it all the same, as `Engines` tells it each trusted already.

use crate::engines::{ChatTo, Engines, HearTrusted, TrustedEngine};
use aethereal_actor::{Actor, ActorRef, Beginning, ChildOf, Ending, Fault, Handles, Level, Top, Turn, path};
use aethereal_data::Plain;
use aethereal_sessions::Sessions;
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};

/// The hub's root that tells each engine the hub trusts that its chat goes to `Sessions`. It keeps nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.chat_to_sessions")]
pub struct ChatToSessions;

impl ChildOf for ChatToSessions {
    type Parent = Top;
}

/// It stands up on every host that lists `Engines` and `Sessions`.
impl<H: Host> Bootstrap<H> for ChatToSessions {
    const SECTION: &'static str = "chat_to_sessions";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Self)?;

        Ok(())
    }
}

impl Actor for ChatToSessions {
    type Config = Self;
    type Key = ();
    type Handles = (TrustedEngine,);
    type Depends = (Engines, Sessions);
    type Spawns = ();

    /// Asks `Engines` to tell it each engine the hub trusts.
    fn begin(root: Self, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.to(path::<Engines>(())).send(&HearTrusted)?;

        Ok(root)
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<TrustedEngine> for ChatToSessions {
    type From = ActorRef<Engines>;
    type Returns = ();

    /// An engine the hub trusts, as `Engines` tells it: told its chat goes to `Sessions`.
    fn handle(&mut self, trusted: TrustedEngine, turn: &mut Turn<'_, Self, ActorRef<Engines>>) -> Result<(), Fault> {
        let Some(sessions) = turn.found(&path::<Sessions>(())) else {
            turn.log(Level::Warn, "a trusted engine's chat goes nowhere: the sessions are gone");
            return Ok(());
        };
        turn.to(trusted.engine).send(&ChatTo { to: sessions.protocol() })?;

        Ok(())
    }
}
