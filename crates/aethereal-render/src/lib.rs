//! The renderer (ADR-0021): the root that owns the GPU the chassis opens, and makes what is drawn for the actors that
//! ask it. Every actor, native or a guest, reaches the GPU through it, and everything it makes is checked as it is made,
//! so nothing it draws can fail.

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
mod renderer;
pub mod shape;
pub mod storage;
pub mod texture;
pub mod timing;
pub mod uniforms;

pub use gpu::Gpu;
pub use renderer::{Release, Renderer, RendererConfig, Rendering};
