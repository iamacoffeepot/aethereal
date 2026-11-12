//! The stock widgets of the library's first stage (ADR-0056: Stock widgets): the containers `Row`, `Column`, `Grid`,
//! and `Stack`, and the content `Text`, `Image`, and `Bar`. Each is a primitive written as a program's own would be,
//! and a screen brings them in with the prelude.

mod bar;
mod containers;
mod image;
mod text;

pub use bar::{Bar, Fill};
pub use containers::{Column, Grid, Row, Stack};
pub use image::{Image, Region, Texture};
pub use text::Text;
