//! The stock widgets (ADR-0056: Stock widgets): the containers `Row`, `Column`, `Grid`, and `Stack`; the content
//! `Text`, `Image`, and `Bar`; and the interaction `Button`, `Scroll`, `TextInput`, `Toggle`, `Slider`, `Tabs` with
//! its `Tab`, and `Popup` and `Tooltip`, whose content draws in their screen's top layer. Each is a primitive written as a program's own would be, and a screen brings them in with the prelude.

mod bar;
mod button;
mod containers;
mod image;
mod popup;
mod press;
mod scroll;
mod slider;
mod tabs;
mod text;
mod text_input;
mod toggle;
mod tooltip;

pub use bar::{Bar, Fill};
pub use button::Button;
pub use containers::{Column, Grid, Row, Stack};
pub use image::{Image, Region, Texture};
pub use popup::{Inside, Opened, Popped, Popup, Side};
pub(crate) use popup::{anchor, anchors, contains};
pub use press::Press;
pub use scroll::{Drag, Scroll, Scrolling};
pub use slider::{Slider, Sliding};
pub use tabs::{Header, Tab, Tabbing, Tabs};
pub use text::Text;
pub use text_input::{Editing, TextInput};
pub use toggle::Toggle;
pub use tooltip::{DELAY, Tip, Tooltip};
