//! The host's side of the window capability: the windows a chassis opens as it sets the program up, each given its id
//! here, and the feed each one's events are pushed onto.

use crate::event::{Event, Surface};
use crate::window::{WindowConfig, WindowId};
use crate::windows::{Heard, Opened, WindowsConfig};
use aethereal_actor::{Given, Source};
use aethereal_substrate::{Closed, Feed};

/// The windows a chassis opens as it sets the program up, and the source their events come in through, until it
/// makes the window root's config of them.
pub struct Opening {
    source: Given<Source<Heard>>,
    feed: Feed<Heard>,
    windows: Vec<Opened>,
}

impl Opening {
    /// Opening windows whose events come in through the source the engine made, `(source, feed)`, as
    /// [`Setup::source`](aethereal_substrate::Setup::source) answers.
    #[must_use]
    pub fn new((source, feed): (Given<Source<Heard>>, Feed<Heard>)) -> Self {
        Self { source, feed, windows: Vec::new() }
    }

    /// Opens a window whose `surface` and `focused` stand as given, under the next id, and answers with the feed its
    /// events are pushed onto.
    pub fn open(&mut self, surface: Surface, focused: bool) -> WindowFeed {
        // A host opens some windows, never more than a `u64` counts.
        let window = WindowId::nth(self.windows.len() as u64);
        self.windows.push(Opened { window, config: WindowConfig { surface, focused } });

        WindowFeed { window, feed: self.feed.clone() }
    }

    /// The window root's config: its source, and every window opened, in the order opened.
    #[must_use]
    pub fn config(self) -> WindowsConfig {
        WindowsConfig { source: self.source, windows: self.windows }
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
        self.feed.push(Heard { window: self.window, event })
    }
}
