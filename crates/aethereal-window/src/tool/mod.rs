//! The window capability's tools: `send_input`, which performs input on a window (ADR-0046), and `list_windows`,
//! `resize_window`, `move_window`, and `set_window_mode`, which ask the window root (ADR-0047).

mod input;
mod windows;

pub use input::{InputTool, InputToolConfig, SendInput};
pub use windows::{WindowTool, WindowToolConfig};
