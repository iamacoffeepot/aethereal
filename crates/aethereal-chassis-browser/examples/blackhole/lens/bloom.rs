//! The bloom's effects: the bright part of the light traced small kept, blurred along each row, then along each
//! column onto the glow's frame's end, and laid over the light on the window with the tone map.

use aethereal_actor::BlobRef;
use aethereal_data::Plain;
use aethereal_render::effect::{CreateEffect, EffectId, Read};
use aethereal_render::frame::builder::Float;
use aethereal_render::placement::{Bind, Resource};
use aethereal_render::program::{Binding, BindingKind, Blend};
use aethereal_render::shape::{ColourFormat, Shape};
use aethereal_render::texture::TextureId;

/// The effect that keeps the bright part.
pub const BRIGHT_WGSL: &str = include_str!("../shaders/bright.wgsl");

/// The two effects that blur, across and down.
pub const BLUR_WGSL: &str = include_str!("../shaders/blur.wgsl");

/// The effect that lays the glow over the light and maps it to the screen.
pub const COMPOSITE_WGSL: &str = include_str!("../shaders/composite.wgsl");

/// The bloom's effects, each reading one target of floats.
#[derive(Debug, Clone, Copy, Plain)]
pub struct Effects {
    pub bright: EffectId<(Float,)>,
    pub across: EffectId<(Float,)>,
    pub down: EffectId<(Float,)>,
    pub composite: EffectId<(Float,)>,
}

/// The effect of `fragment` of the WGSL `wgsl` holds, reading one target of floats and binding `bindings` after it,
/// drawing into a target of `colour`.
fn effect(wgsl: BlobRef, fragment: &str, bindings: Vec<Binding>, colour: ColourFormat) -> CreateEffect {
    CreateEffect {
        wgsl,
        fragment: String::from(fragment),
        reads: vec![Read::Float],
        bindings,
        blend: Blend::Opaque,
        shape: Shape::flat(colour),
    }
}

/// The effect that keeps the bright part of the light traced small, into a target of floats.
pub fn bright(wgsl: BlobRef) -> CreateEffect {
    effect(wgsl, "bright", Vec::new(), ColourFormat::Rgba16Float)
}

/// The effect that blurs along each row, into a target of floats.
pub fn across(wgsl: BlobRef) -> CreateEffect {
    effect(wgsl, "across", Vec::new(), ColourFormat::Rgba16Float)
}

/// The effect that blurs along each column, onto the glow's frame's end: a texture of bytes, as every frame that ends
/// on a texture ends.
pub fn down(wgsl: BlobRef) -> CreateEffect {
    effect(wgsl, "down", Vec::new(), ColourFormat::Rgba8)
}

/// The effect that lays the glow over the light onto a window drawn in `window`: written in linear light where the
/// window encodes sRGB itself, and encoded where it does not.
pub fn composite(wgsl: BlobRef, window: ColourFormat) -> CreateEffect {
    let encodes = matches!(window, ColourFormat::Rgba8Srgb | ColourFormat::Bgra8Srgb);
    let fragment = if encodes {
        "shown"
    } else {
        "encoded"
    };
    let bindings = vec![Binding { group: 1, binding: 0, kind: BindingKind::Texture }];

    effect(wgsl, fragment, bindings, window)
}

/// What the composite binds: the glow, `glow`. It blends the glow's texels itself, as an effect's sampler is not made
/// for it.
pub fn composited(glow: TextureId) -> Vec<Bind> {
    vec![Bind { group: 1, binding: 0, to: Resource::Texture(glow) }]
}
