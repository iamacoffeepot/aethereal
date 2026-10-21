//! The renderer (ADR-0021): the root that owns the GPU the chassis opens, and makes what is drawn for the actors that
//! ask it. Every actor, native or a guest, reaches the GPU through it, and everything it makes is checked as it is made,
//! so nothing it draws can fail.

mod gpu;
pub mod instances;
mod made;
pub mod mesh;
pub mod program;
mod renderer;
pub mod texture;
pub mod uniforms;

pub use gpu::Gpu;
pub use renderer::{Release, Renderer, RendererConfig, Rendering};
