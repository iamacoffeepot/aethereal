//! The window capability (ADR-0024, ADR-0027): what a host's windows hear, brought in through one source to the root
//! [`Windows`], and sent from there to the actor of the window it was heard in, a [`Window`], which sends it to the
//! actors that subscribed.
//!
//! It names no windowing library and no page: a chassis opens its windows with [`Opening`] as the program is set up,
//! and pushes what each hears onto its [`WindowFeed`], in the window's surface pixels and the W3C UI Events' names
//! for the keys.
//!
//! Once the program runs, an actor asks the root for a window, [`OpenWindow`], to close one, [`CloseWindow`], and to
//! show one in a [`Mode`], [`SetMode`] (ADR-0027); for every open window, [`ListWindows`]; and to resize or move one,
//! [`ResizeWindow`] and [`MoveWindow`] (ADR-0047). The root gives the window its id, asks its host through its controller, [`Windowing`], and answers
//! once the host has said, through the root's one source, what it did; the chassis takes the requests and says so
//! with the [`Hosting`] it kept.
//!
//! Any actor may also ask a window's actor to perform input, [`Perform`]: what a person does at the window, sent to
//! its subscribers as though its host had heard it (ADR-0046). The `tools` feature's `send_input` asks it for an agent.

mod event;
mod host;
mod perform;
#[cfg(feature = "tools")]
mod tool;
mod window;
mod windows;

pub use event::{Button, Event, Mode, Subscribe, Surface, Unsubscribe};
pub use host::{Hosting, InitialWindows, Opening, WindowFeed};
pub use perform::{Input, MOST_INPUTS, MOST_NAME_BYTES, Perform, Performed, Unperformed};
#[cfg(feature = "tools")]
pub use tool::{InputTool, InputToolConfig, SendInput, WindowTool, WindowToolConfig};
pub use window::{Window, WindowConfig, WindowId};
pub use windows::{
    CloseWindow, Closed, Closing, Heard, Initial, ListWindows, Listed, Logical, ModeSet, MoveWindow, Moved, OpenWindow,
    Opened, Place, ResizeWindow, Resized, SetMode, Unclosed, Unmade, Unopened, Unplaced, Unset, WindowList, Windowing,
    Windows, WindowsConfig,
};
