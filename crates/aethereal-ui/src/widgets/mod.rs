//! The stock widgets (ADR-0056: Stock widgets): the containers `Row`, `Column`, `Grid`, and `Stack`; the content
//! `Text`, `Image`, and `Bar`; and the interaction `Button`, `TextInput`, `Toggle`, and `Slider`. Each is a primitive
//! written as a program's own would be, and a screen brings them in with the prelude.

mod bar;
mod button;
mod containers;
mod image;
mod press;
mod slider;
mod text;
mod text_input;
mod toggle;

pub use bar::{Bar, Fill};
pub use button::Button;
pub use containers::{Column, Grid, Row, Stack};
pub use image::{Image, Region, Texture};
pub use press::Press;
pub use slider::{Slider, Sliding};
pub use text::Text;
pub use text_input::{Editing, TextInput};
pub use toggle::Toggle;
