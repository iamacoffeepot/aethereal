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
//! [`ResizeWindow`] and [`MoveWindow`] (ADR-0047); and to let a window's input method compose, or stop it,
//! [`AllowIme`], while a text field there has focus (ADR-0051). The root gives the window its id, asks its host through its controller, [`Windowing`], and answers
//! once the host has said, through the root's one source, what it did; the chassis takes the requests and says so
//! with the [`Hosting`] it kept.
//!
//! Any actor may also ask a window's actor to perform input, [`Perform`]: what a person does at the window, sent to
//! its subscribers as though its host had heard it (ADR-0046). The `tools` feature's `send_input` asks it for an agent.
//!
//! An actor takes a window's key focus by asking its actor, [`TakeKeyFocus`], and gives it up, [`ReleaseKeyFocus`]:
//! while it holds it, the window's keys and typed text are sent to it alone, and it is told as it gains and loses it,
//! [`KeyFocusGained`] and [`KeyFocusLost`] (ADR-0053). Both chassis push what they hear through the window root, so
//! both are routed by it.

mod event;
mod focus;
mod host;
mod perform;
#[cfg(feature = "tools")]
mod tool;
mod window;
mod windows;

pub use event::{Button, Event, Mode, Subscribe, Surface, Unsubscribe};
pub use focus::{KeyFocusGained, KeyFocusHolder, KeyFocusLost, ReleaseKeyFocus, TakeKeyFocus};
pub use host::{Hosting, InitialWindows, Opening, WindowFeed};
pub use perform::{Input, MOST_INPUTS, MOST_NAME_BYTES, MOST_TEXT_BYTES, Perform, Performed, Unperformed};
#[cfg(feature = "tools")]
pub use tool::{InputTool, InputToolConfig, SendInput, WindowTool, WindowToolConfig};
pub use window::{Window, WindowConfig, WindowId};
pub use windows::{
    AllowIme, CloseWindow, Closed, Closing, Heard, ImeAllowed, Initial, ListWindows, Listed, Logical, ModeSet,
    MoveWindow, Moved, OpenWindow, Opened, Place, ResizeWindow, Resized, SetMode, Unclosed, Unmade, Unopened, Unplaced,
    Unset, WindowList, Windowing, Windows, WindowsConfig,
};
