//! The chat a trusted engine's link carries (ADR-0057: The console is the chat): what the hub shows the engine, handed
//! to each actor of the engine that [`Listen`]s, and what its owner does, told to the root as the sessions' [`Chat`]
//! and written up as its frame.
//!
//! ```text
//! Down::Sessions / Show / Ask ──> kept (the tabs, the asks that wait) ──> ShowTabs / ShowEvents / ShowAsk ──> each listener
//! a listener's Chat ──> Up::Say / Ran / Answer / Interrupt / Open / Close / New / Archive ──> the hub
//! ```
//!
//! An untrusted engine is shown nothing, so its listeners hear nothing, and an engine's console shows no chat until
//! the hub has shown it the sessions.

use super::Link;
use crate::Up;
use crate::frames::{self, Written};
use aethereal_actor::{Anyone, Ended, Fault, Handles, Level, Receives, Speaks, Turn};
use aethereal_data::Plain;
use aethereal_sessions::{Chat, ShowAsk, ShowEvents, ShowTabs, Tab};

/// What a listener is shown: the kinds of the sessions' `Showing`, which the hub's `Engine` is told.
pub type Listening = (ShowTabs, ShowEvents, ShowAsk);

/// Asks the link to show this actor the chat its hub shows the engine: the tabs and the asks that wait as they stand,
/// and from then each change and each event of a tab the engine has open. The sender must handle each kind of
/// [`Listening`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.link.listen")]
pub struct Listen;

/// What the hub has shown the engine of the chat.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) enum Shown {
    /// Nothing: the hub has not shown the sessions, as it never does to an untrusted engine.
    Nothing,
    /// The sessions as the hub last listed them, and the asks it showed that wait still.
    Sessions { tabs: Vec<Tab>, asks: Vec<ShowAsk> },
}

impl Shown {
    /// The tabs the hub listed: every ask of a session that no longer waits on any is let go, as an ask answered at
    /// another engine is shown here only by its session's count.
    fn listed(&mut self, listed: &[Tab]) {
        let waiting = |ask: &&ShowAsk| listed.iter().any(|tab| tab.session == ask.session && tab.asking > 0);
        let asks = match self {
            Self::Nothing => Vec::new(),
            Self::Sessions { asks, .. } => asks.iter().filter(waiting).cloned().collect(),
        };
        *self = Self::Sessions { tabs: listed.to_vec(), asks };
    }

    /// An ask the hub showed: kept until its session waits on none.
    fn asked(&mut self, ask: ShowAsk) {
        if let Self::Sessions { asks, .. } = self {
            asks.push(ask);
        }
    }
}

impl Link {
    /// The sessions the hub listed, kept and shown to each listener.
    pub(super) fn tabs(&mut self, turn: &mut Turn<'_, Self>, tabs: Vec<Tab>) -> Result<(), Fault> {
        self.shown.listed(&tabs);
        let shown = ShowTabs { tabs };
        for listener in self.listeners.references() {
            turn.to(*listener).send(&shown)?;
        }

        Ok(())
    }

    /// Events of a tab the engine has open, shown to each listener.
    pub(super) fn events(&self, turn: &mut Turn<'_, Self>, shown: &ShowEvents) -> Result<(), Fault> {
        for listener in self.listeners.references() {
            turn.to(*listener).send(shown)?;
        }

        Ok(())
    }

    /// An ask that waits on the owner, kept and shown to each listener.
    pub(super) fn ask(&mut self, turn: &mut Turn<'_, Self>, ask: ShowAsk) -> Result<(), Fault> {
        for listener in self.listeners.references() {
            turn.to(*listener).send(&ask)?;
        }
        self.shown.asked(ask);

        Ok(())
    }
}

/// Adds the sender, and shows it the tabs and the asks that wait as they stand: nothing, when it listens already or the
/// hub has shown nothing.
impl Handles<Listen> for Link {
    type From = Speaks<Listening>;
    type Returns = ();

    fn handle(&mut self, _: Listen, turn: &mut Turn<'_, Self, Speaks<Listening>>) -> Result<(), Fault> {
        let listener = *turn.from();
        let added = self.listeners.subscribe(turn, listener);
        let Shown::Sessions { tabs, asks } = &self.shown else {
            return Ok(());
        };
        if !added {
            return Ok(());
        }
        turn.to(listener).send(&ShowTabs { tabs: tabs.clone() })?;
        for ask in asks {
            turn.to(listener).send(ask)?;
        }

        Ok(())
    }
}

/// What the engine's owner did, written up as its frame when the link is open: the hub takes it only from a trusted
/// engine. A frame too large for the link is not written, and logged.
impl Handles<Chat> for Link {
    type From = Anyone;
    type Returns = ();

    fn handle(&mut self, chat: Chat, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let written = frames::written(&Up::from(chat), self.config.most_bytes)?;
        let Written::Fits(bytes) = written else {
            turn.log(Level::Warn, "a chat frame is too large for the link, and was not written");
            return Ok(());
        };

        self.write(turn, bytes)
    }
}

/// A listener has ended, and is let go.
impl Receives<Ended> for Link {
    fn handle(&mut self, ended: Ended, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.listeners.ended(&ended);

        Ok(())
    }
}
