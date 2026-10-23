//! The host's side of the window capability: the windows a chassis opens as it sets the program up, each given its id
//! here; the feed each one's events are pushed onto; and what the chassis keeps once the program runs, to take the
//! window root's requests and say what it did with each (ADR-0024, ADR-0027).

use crate::event::{Event, Mode, Surface};
use crate::window::{WindowConfig, WindowId};
use crate::windows::{Closing, Heard, Initial, Unclosed, Unopened, Unset, Windowing, WindowsConfig};
use aethereal_actor::{Controller, Given, Requests, Source};
use aethereal_substrate::{Closed, Feed};
use core::iter;

/// The windows a chassis opens as it sets the program up, the source their events come in through, and the
/// controller the window root asks for windows through, until it makes the window root's config of them.
pub struct Opening {
    source: Given<Source<Heard>>,
    feed: Feed<Heard>,
    control: Given<Controller<Windowing>>,
    requests: Requests<Windowing>,
    windows: Vec<Initial>,
}

impl Opening {
    /// Opening windows whose events come in through the source the engine made, `(source, feed)`, as
    /// [`Setup::source`](aethereal_substrate::Setup::source) answers, for a window root that asks for windows through
    /// the controller it made, `(control, requests)`, as [`Setup::controller`](aethereal_substrate::Setup::controller)
    /// answers.
    #[must_use]
    pub fn new(
        (source, feed): (Given<Source<Heard>>, Feed<Heard>),
        (control, requests): (Given<Controller<Windowing>>, Requests<Windowing>),
    ) -> Self {
        Self { source, feed, control, requests, windows: Vec::new() }
    }

    /// Opens a window whose `surface`, `mode`, and `focused` stand as given, under the next id, and answers with the
    /// feed its events are pushed onto.
    pub fn open(&mut self, surface: Surface, mode: Mode, focused: bool) -> WindowFeed {
        // A host opens some windows, never more than a `u64` counts.
        let window = WindowId::nth(self.windows.len() as u64);
        self.windows.push(Initial { window, config: WindowConfig { surface, mode, focused } });

        WindowFeed { window, feed: self.feed.clone() }
    }

    /// The window root's config, its source and controller and every window opened, in the order opened, each
    /// window's close button closing it; and what the chassis keeps to take the root's requests and answer them. A
    /// program whose close buttons only ask sets [`WindowsConfig::closing`] to [`Closing::ByProgram`].
    #[must_use]
    pub fn config(self) -> (WindowsConfig, Hosting) {
        let Self { source, feed, control, requests, windows } = self;
        let config = WindowsConfig { source, control, windows, closing: Closing::ByButton };

        (config, Hosting { requests, feed })
    }
}

/// What the chassis keeps of the window capability once the program runs: its end of the window root's controller,
/// which it takes the root's requests from between steps, and the root's feed, onto which it says what it did with
/// each, and through which a window opened on request is fed, by the [`WindowFeed`] its opening answers.
pub struct Hosting {
    requests: Requests<Windowing>,
    feed: Feed<Heard>,
}

impl Hosting {
    /// Takes what the window root asked since the last take, in the order its turns asked.
    #[must_use]
    pub fn take(&self) -> Vec<Windowing> {
        self.requests.take()
    }

    /// Pushes that the host opened `window`, as the root asked, shown windowed, its `surface` and `focused` standing as
    /// given, and answers with the feed its events are pushed onto: the root hears that it opened before anything it
    /// heard.
    ///
    /// # Errors
    ///
    /// [`Closed`] when the window root's source has been dropped.
    pub fn opened(&self, window: WindowId, surface: Surface, focused: bool) -> Result<WindowFeed, Closed> {
        self.feed.push(Heard::Opened { window, surface, focused })?;

        Ok(WindowFeed { window, feed: self.feed.clone() })
    }

    /// Pushes that the host did not open `window`, as the root asked, and why.
    ///
    /// # Errors
    ///
    /// [`Closed`] when the window root's source has been dropped.
    pub fn unopened(&self, window: WindowId, why: Unopened) -> Result<(), Closed> {
        self.feed.push(Heard::Unopened { window, why })
    }

    /// Pushes that the host closed `window`, as the root asked or of its own accord: what is pushed for the window
    /// after reaches nothing.
    ///
    /// # Errors
    ///
    /// [`Closed`] when the window root's source has been dropped.
    pub fn closed(&self, window: WindowId) -> Result<(), Closed> {
        self.feed.push(Heard::Closed { window })
    }

    /// Pushes that the host did not close `window`, as the root asked, and why: it stays open.
    ///
    /// # Errors
    ///
    /// [`Closed`] when the window root's source has been dropped.
    pub fn unclosed(&self, window: WindowId, why: Unclosed) -> Result<(), Closed> {
        self.feed.push(Heard::Unclosed { window, why })
    }

    /// Pushes that the host shows `window` in the mode the root asked, its `surface` as it draws it now. The window's
    /// `Moded`, when its mode changed, is pushed onto its feed first.
    ///
    /// # Errors
    ///
    /// [`Closed`] when the window root's source has been dropped.
    pub fn set(&self, window: WindowId, surface: Surface) -> Result<(), Closed> {
        self.feed.push(Heard::Set { window, surface })
    }

    /// Pushes that the host did not show `window` in the mode the root asked, and why.
    ///
    /// # Errors
    ///
    /// [`Closed`] when the window root's source has been dropped.
    pub fn unset(&self, window: WindowId, why: Unset) -> Result<(), Closed> {
        self.feed.push(Heard::Unset { window, why })
    }

    /// Answers each request the window root asked since the last take as a host that opens, closes, and sets the mode
    /// of no window does, as a page does: [`Unopened::NoHost`], [`Unclosed::NoHost`], and [`Unset::NoHost`].
    ///
    /// # Errors
    ///
    /// [`Closed`] when the window root's source has been dropped.
    pub fn refuse(&self) -> Result<(), Closed> {
        for request in self.take() {
            match request {
                Windowing::Open { window, .. } => self.unopened(window, Unopened::NoHost)?,
                Windowing::Close { window } => self.unclosed(window, Unclosed::NoHost)?,
                Windowing::SetMode { window, .. } => self.unset(window, Unset::NoHost)?,
            }
        }

        Ok(())
    }
}

/// Where the host pushes what one window heard, for the window root to send to that window's actor.
#[derive(Clone)]
pub struct WindowFeed {
    window: WindowId,
    feed: Feed<Heard>,
}

impl WindowFeed {
    /// The window, by its id: its actor's key.
    #[must_use]
    pub const fn window(&self) -> WindowId {
        self.window
    }

    /// Pushes `event`, heard in this window, for the next bring.
    ///
    /// # Errors
    ///
    /// [`Closed`] when the window root's source has been dropped.
    pub fn push(&self, event: Event) -> Result<(), Closed> {
        self.feed.push(Heard::Event { window: self.window, event })
    }
}

/// The windows a host opened as it set the program up, in the order opened: what the host's bootstrap of the window
/// root provides the roots bootstrapped after it, so a root is given the window it draws on (ADR-0029). There is at
/// least one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitialWindows {
    first: WindowId,
    others: Vec<WindowId>,
}

impl InitialWindows {
    /// The windows `first`, then `others`, in the order opened.
    #[must_use]
    pub const fn new(first: WindowId, others: Vec<WindowId>) -> Self {
        Self { first, others }
    }

    /// The first window opened.
    #[must_use]
    pub const fn first(&self) -> WindowId {
        self.first
    }

    /// Every window opened, in the order opened.
    pub fn all(&self) -> impl Iterator<Item = WindowId> {
        iter::once(self.first).chain(self.others.iter().copied())
    }
}
