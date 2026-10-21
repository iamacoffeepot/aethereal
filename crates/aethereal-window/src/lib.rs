//! The window capability (ADR-0024): what a host's windows hear, brought in through one source to the root
//! [`Windows`], and sent from there to the actor of the window it was heard in, a [`Window`], which sends it to the
//! actors that subscribed.
//!
//! It names no windowing library and no page: a chassis opens its windows with [`Opening`] as the program is set up,
//! and pushes what each hears onto its [`WindowFeed`], in the window's surface pixels and the W3C UI Events' names
//! for the keys.

mod event;
mod host;
mod window;
mod windows;

pub use event::{Button, Event, Subscribe, Surface, Unsubscribe};
pub use host::{Opening, WindowFeed};
pub use window::{Window, WindowConfig, WindowId};
pub use windows::{Heard, Opened, Unopened, Windows, WindowsConfig};
