//! The window capability (ADR-0024, ADR-0027): what a host's windows hear, brought in through one source to the root
//! [`Windows`], and sent from there to the actor of the window it was heard in, a [`Window`], which sends it to the
//! actors that subscribed.
//!
//! It names no windowing library and no page: a chassis opens its windows with [`Opening`] as the program is set up,
//! and pushes what each hears onto its [`WindowFeed`], in the window's surface pixels and the W3C UI Events' names
//! for the keys.
//!
//! Once the program runs, an actor asks the root for a window, [`OpenWindow`], and to close one, [`CloseWindow`]
//! (ADR-0027). The root gives the window its id, asks its host through its controller, [`Windowing`], and answers
//! once the host has said, through the root's one source, what it did; the chassis takes the requests and says so
//! with the [`Hosting`] it kept.

mod event;
mod host;
mod window;
mod windows;

pub use event::{Button, Event, Subscribe, Surface, Unsubscribe};
pub use host::{Hosting, Opening, WindowFeed};
pub use window::{Window, WindowConfig, WindowId};
pub use windows::{
    CloseWindow, Closed, Closing, Heard, Initial, Logical, OpenWindow, Opened, Unclosed, Unmade, Unopened, Windowing,
    Windows, WindowsConfig,
};
