//! The renderer (ADR-0021): the root that owns the GPU the chassis opens, and makes what is drawn for the actors that
//! ask it. Every actor, native or a guest, reaches the GPU through it, and everything it makes is checked as it is made,
//! so nothing it draws can fail.

#[cfg(test)]
mod asked;
#[cfg(feature = "tools")]
mod capture;
pub mod compute;
pub mod dispatch;
pub mod draw_list;
pub mod effect;
pub mod frame;
mod gpu;
pub mod instances;
mod made;
pub mod mapping;
pub mod mesh;
pub mod pass;
pub mod placement;
pub mod program;
pub mod readback;
#[cfg(feature = "tools")]
mod record;
mod renderer;
pub mod shaders;
pub mod shape;
pub mod storage;
pub mod surface;
#[cfg(feature = "tools")]
mod target;
pub mod texture;
pub mod timing;
pub mod uniforms;

#[cfg(feature = "tools")]
pub use capture::{Capture, CaptureTool, Crop};
pub use gpu::Gpu;
#[cfg(feature = "tools")]
pub use record::{Record, RecordTool, RecordToolConfig, Recorded};
pub use renderer::{Release, Renderer, RendererConfig, Rendering};
#[cfg(feature = "tools")]
pub use target::{Pixel, ReadTarget, TargetRead, TargetTool, TargetToolConfig, Values};
