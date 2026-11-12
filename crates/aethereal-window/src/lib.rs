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
//! Any actor may also ask a window's actor to perform input, [`Perform`]: what a person does at the window, sent on
//! as though its host had heard it (ADR-0046). The `tools` feature's `send_input` asks it for an agent.
//!
//! A window's pointer events also cascade through what was drawn under the pointer (ADR-0055): the renderer subscribes
//! to each window's probes, [`SubscribeProbes`], is sent each pointer event's place, [`Probed`], and answers what each
//! frame drew there, [`Picked`]; the window then sends each event, as [`Pointed`], to the actors whose draw lists hold
//! the entries it reaches, from the top, until one blocks. Its keys and typed text go only through the keys the draw
//! lists declare: the renderer sends it the `Keys` entries a frame lists, [`KeysListed`], the last called first, and
//! the window sends each key, as [`Keyed`], to the actor of each entry whose [`KeySet`] holds it, from the top, until
//! one blocks, and typed text where the key that typed it went. A raw subscriber hears neither.

mod event;
mod host;
mod keys;
mod perform;
mod pointing;
#[cfg(feature = "tools")]
mod tool;
mod window;
mod windows;

pub use event::{Button, Event, Mode, Subscribe, Surface, Unsubscribe};
pub use host::{Hosting, InitialWindows, Opening, WindowFeed};
pub use keys::{KeyEvent, KeyHearer, KeySet, Keyed, KeysEntry, KeysListed};
pub use perform::{Input, MOST_INPUTS, MOST_NAME_BYTES, MOST_TEXT_BYTES, Perform, Performed, Unperformed};
pub use pointing::{Hearer, Picked, Pointed, PointerEvent, Pressed, Probed, Reach, SubscribeProbes};
#[cfg(feature = "tools")]
pub use tool::{InputTool, InputToolConfig, SendInput, WindowTool, WindowToolConfig};
pub use window::{Window, WindowConfig, WindowId};
pub use windows::{
    AllowIme, CloseWindow, Closed, Closing, Heard, ImeAllowed, Initial, ListWindows, Listed, Logical, ModeSet,
    MoveWindow, Moved, OpenWindow, Opened, Place, ResizeWindow, Resized, SetMode, Unclosed, Unmade, Unopened, Unplaced,
    Unset, WindowList, Windowing, Windows, WindowsConfig,
};
