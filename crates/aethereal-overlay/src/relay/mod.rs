//! The console's chat over the engine's link (ADR-0057: The console is the chat): [`ChatRelay`], a root of its own that
//! a program with both the overlay and a link lists after them, so the overlay depends on no link and a program with
//! none lists no relay. As it begins it tells the overlay it [`Relays`] the chat, and listens to the link; what the
//! link shows it is told to the overlay, and what the overlay's owner does is told to the link, which writes it up to
//! the hub.
//!
//! ```text
//! begin ──> Relays to the overlay ──> the overlay tells it each Chat from then on
//!       ──> Listen to the link
//! the link's ShowTabs / ShowEvents / ShowAsk ──> the overlay
//! the overlay's Chat ──> the link ──> Up::Say / Ran / Answer / Interrupt / Open / Close / New / Archive ──> the hub
//! ```
//!
//! It takes what it relays only from the actor whose it is: the shows from the link, and the chat from the overlay.

use crate::overlay::Overlay;
use aethereal_actor::{Actor, ActorRef, Beginning, ChildOf, Ending, Fault, Handles, Top, Turn, path};
use aethereal_data::Plain;
use aethereal_link::{Link, Listen};
use aethereal_sessions::{Chat, ShowAsk, ShowEvents, ShowTabs};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};

/// Told to the overlay by an actor that carries its chat to the engine's link: the sender, which must handle the
/// sessions' `Chat`, is told what the owner does from then on, and the console shows the chat once the hub lists the
/// sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.relays")]
pub struct Relays;

/// The root that carries the console's chat between the overlay and the engine's link. It keeps nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.overlay.chat_relay")]
pub struct ChatRelay;

impl ChildOf for ChatRelay {
    type Parent = Top;
}

/// It stands up on every host that lists the overlay and a link.
impl<H: Host> Bootstrap<H> for ChatRelay {
    const SECTION: &'static str = "chat_relay";
    type Settings = ();
    type Needs = ();
    type Provides = ();

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Self)?;

        Ok(())
    }
}

impl Actor for ChatRelay {
    type Config = Self;
    type Key = ();
    type Handles = (ShowTabs, ShowEvents, ShowAsk, Chat);
    type Depends = (Overlay, Link);
    type Spawns = ();

    /// Tells the overlay it relays the chat, then listens to the link, so the overlay holds it before the link shows
    /// anything.
    fn begin(relay: Self, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.to(path::<Overlay>(())).send(&Relays)?;
        turn.to(path::<Link>(())).send(&Listen)?;

        Ok(relay)
    }

    fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The sessions the hub lists, told to the overlay.
impl Handles<ShowTabs> for ChatRelay {
    type From = ActorRef<Link>;

    fn handle(&mut self, shown: ShowTabs, turn: &mut Turn<'_, Self, ActorRef<Link>>) -> Result<(), Fault> {
        turn.to(path::<Overlay>(())).send(&shown)?;

        Ok(())
    }
}

/// Events of a tab open, told to the overlay.
impl Handles<ShowEvents> for ChatRelay {
    type From = ActorRef<Link>;

    fn handle(&mut self, shown: ShowEvents, turn: &mut Turn<'_, Self, ActorRef<Link>>) -> Result<(), Fault> {
        turn.to(path::<Overlay>(())).send(&shown)?;

        Ok(())
    }
}

/// An ask that waits on the owner, told to the overlay.
impl Handles<ShowAsk> for ChatRelay {
    type From = ActorRef<Link>;

    fn handle(&mut self, ask: ShowAsk, turn: &mut Turn<'_, Self, ActorRef<Link>>) -> Result<(), Fault> {
        turn.to(path::<Overlay>(())).send(&ask)?;

        Ok(())
    }
}

/// What the owner did at the console, told to the link.
impl Handles<Chat> for ChatRelay {
    type From = ActorRef<Overlay>;

    fn handle(&mut self, chat: Chat, turn: &mut Turn<'_, Self, ActorRef<Overlay>>) -> Result<(), Fault> {
        turn.to(path::<Link>(())).send(&chat)?;

        Ok(())
    }
}
