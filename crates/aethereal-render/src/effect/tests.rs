//! An effect's check: what WebGPU would refuse as its pipeline is made, or as a frame draws it, is refused as it is
//! asked for.

use super::*;
use crate::asked::{Asked, blob};
use crate::program::{Access, BindingKind};
use crate::shape::ColourFormat;

/// An effect that reads a target of floats, one of whole numbers, and a depth, by the pixel it shades, and tints
/// what it reads by uniforms.
const WGSL: &str = "
@group(0) @binding(0) var colour: texture_2d<f32>;
@group(0) @binding(1) var ids: texture_2d<u32>;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(1) @binding(0) var<uniform> tint: vec4<f32>;

@fragment
fn glaze(@builtin(position) at: vec4<f32>, @location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(at.xy);
    let id = f32(textureLoad(ids, pixel, 0).x);
    let near = textureLoad(depth, pixel, 0);
    return textureLoad(colour, pixel, 0) * tint * id * near + vec4(uv, 0.0, 0.0);
}
";

/// The effect [`WGSL`] is, into the window's colour.
fn effect() -> Asked<CreateEffect> {
    let create = CreateEffect {
        wgsl: blob(),
        fragment: String::from("glaze"),
        reads: vec![Read::Float, Read::Uint, Read::Depth],
        bindings: vec![Binding { group: 1, binding: 0, kind: BindingKind::Uniform { bytes: 16 } }],
        blend: Blend::Opaque,
        shape: Shape::flat(ColourFormat::Bgra8),
    };

    Asked::new(create, WGSL)
}

/// What the check answers `effect`, under the limits every WebGPU device has.
fn checked(effect: &Asked<CreateEffect>) -> Result<(), ProgramRefused> {
    check(effect, &effect.wgsl, &wgpu::Limits::default())
}

#[test]
fn an_effect_reading_each_kind_of_target_as_it_declares_it_is_made() {
    // Catches a check that refuses what WebGPU makes: a target of whole numbers or of depth read as itself, the pixel
    // and the place across the target the renderer's vertex stage writes, and uniforms bound after the reads.
    assert_eq!(checked(&effect()), Ok(()));
}

#[test]
fn an_effect_reading_a_target_as_another_kind_or_past_what_it_reads_is_refused() {
    // Catches group 0 checked against nothing: a pipeline made with a layout of other sample types than the shader's
    // is refused by WebGPU, and a binding the frame never binds would fail as the effect is drawn.
    let mut as_floats = effect();
    as_floats.reads[1] = Read::Float;
    let mut fewer = effect();
    fewer.reads.pop();
    let mut depth_as_floats = effect();
    depth_as_floats.reads[2] = Read::Float;

    assert_eq!(checked(&as_floats), Err(ProgramRefused::Mismatch { group: 0, binding: 1 }));
    assert_eq!(checked(&fewer), Err(ProgramRefused::Unbound { group: 0, binding: 2 }));
    assert_eq!(checked(&depth_as_floats), Err(ProgramRefused::Mismatch { group: 0, binding: 2 }));
}

#[test]
fn an_effect_binding_in_group_0_or_holding_depth_is_refused() {
    // Catches what the renderer binds of the frame laid over by what the effect lists, and an effect drawn in a pass
    // with a depth target, which an effect pass never has.
    let mut reserved = effect();
    reserved.bindings.push(Binding { group: 0, binding: 3, kind: BindingKind::Sampler });
    let mut deep = effect();
    deep.shape = Shape::with_depth(ColourFormat::Bgra8);

    assert_eq!(checked(&reserved), Err(ProgramRefused::Reserved { binding: 3 }));
    assert_eq!(checked(&deep), Err(ProgramRefused::EffectDepth));
}

#[test]
fn an_effect_listing_storage_to_write_is_refused() {
    // Catches an effect let write storage, which only a compute program writes: its writes would land in no order
    // the frame's passes give (ADR-0026).
    let mut writes = effect();
    let written = BindingKind::Storage { bytes: 16, access: Access::Write };
    writes.bindings.push(Binding { group: 1, binding: 1, kind: written });

    assert_eq!(checked(&writes), Err(ProgramRefused::Writes { group: 1, binding: 1 }));
}

#[test]
fn an_effect_reading_more_targets_than_a_stage_binds_is_refused() {
    // Catches what an effect reads left out of the count of textures it binds: WebGPU refuses a layout of more than
    // its stage binds as the pipeline is made.
    let most = wgpu::Limits::default().max_sampled_textures_per_shader_stage;
    let mut many = effect();
    many.reads = vec![Read::Float; most as usize];
    many.bindings.push(Binding { group: 1, binding: 1, kind: BindingKind::Texture });

    assert_eq!(checked(&many), Err(ProgramRefused::Many { kind: BindingKind::Texture }));
}

#[test]
fn an_effect_reading_what_the_renderer_does_not_pass_it_is_refused() {
    // Catches the fragment stage checked against its own module only: WebGPU refuses a pipeline whose fragment stage
    // reads a location the vertex stage does not write.
    let mut more = effect();
    more.wgsl = more.wgsl.replace("@location(0) uv: vec2<f32>", "@location(1) uv: vec2<f32>");

    assert_eq!(checked(&more), Err(ProgramRefused::Stages { location: 1 }));
}

#[test]
fn an_effect_into_whole_numbers_writes_them_and_blends_nothing() {
    // Catches a target of whole numbers written as floats, which WebGPU refuses as the pipeline is made, and a blend
    // into one, which it refuses too.
    let wgsl = "
@group(0) @binding(0) var ids: texture_2d<u32>;

@fragment
fn copy(@builtin(position) at: vec4<f32>) -> @location(0) u32 {
    return textureLoad(ids, vec2<i32>(at.xy), 0).x;
}
";
    let copy = CreateEffect {
        wgsl: blob(),
        fragment: String::from("copy"),
        reads: vec![Read::Uint],
        bindings: Vec::new(),
        blend: Blend::Opaque,
        shape: Shape::flat(ColourFormat::R32Uint),
    };
    let copy = Asked::new(copy, wgsl);
    let mut blended = copy.clone();
    blended.blend = Blend::Additive;
    let mut into_floats = effect();
    into_floats.shape = Shape::flat(ColourFormat::R32Uint);

    assert_eq!(checked(&copy), Ok(()));
    assert_eq!(checked(&blended), Err(ProgramRefused::Blend));
    assert_eq!(checked(&into_floats), Err(ProgramRefused::Output));
}

#[test]
fn an_effect_id_is_had_only_for_what_the_effect_reads() {
    // Catches an effect typed by reads it was not made for, which would let the builder hand it targets of other
    // kinds.
    let made = MadeEffect { program: ProgramId { id: 7 }, reads: vec![Read::Float, Read::Uint] };

    assert!(EffectId::<(Float, Uint)>::of(&made).is_some());
    assert!(EffectId::<(Uint, Float)>::of(&made).is_none());
    assert!(EffectId::<(Float,)>::of(&made).is_none());
    assert!(EffectId::<(Float, Uint, Depth32)>::of(&made).is_none());
    assert!(EffectId::<()>::of(&made).is_none());
}

/// An effect that reads no target and shades by the place across its target and uniforms, as a sky does.
const SKY: &str = "
@group(1) @binding(0) var<uniform> horizon: vec4<f32>;

@fragment
fn sky(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    return horizon * uv.y;
}
";

/// The effect [`SKY`] is, into the window's colour, reading nothing and binding its uniforms at group 1.
fn sky() -> Asked<CreateEffect> {
    let create = CreateEffect {
        wgsl: blob(),
        fragment: String::from("sky"),
        reads: Vec::new(),
        bindings: vec![Binding { group: 1, binding: 0, kind: BindingKind::Uniform { bytes: 16 } }],
        blend: Blend::Opaque,
        shape: Shape::flat(ColourFormat::Bgra8),
    };

    Asked::new(create, SKY)
}

#[test]
fn an_effect_reading_nothing_binds_what_it_lists_from_group_1() {
    // Catches group 0 dropped from an effect that reads nothing, its listed bindings moved down into it: what it
    // lists begins at group 1 whatever it reads, so a shader that binds there is made, and one that reads a target
    // its effect does not, or lists a binding in group 0, is refused.
    let mut reading = sky();
    reading.wgsl = reading.wgsl.replace("@fragment", "@group(0) @binding(0) var scene: texture_2d<f32>;\n@fragment");
    reading.wgsl = reading.wgsl.replace("horizon * uv.y", "horizon * textureLoad(scene, vec2<i32>(uv), 0)");
    let mut moved = sky();
    moved.bindings[0].group = 0;
    moved.wgsl = moved.wgsl.replace("@group(1)", "@group(0)");

    assert_eq!(checked(&sky()), Ok(()));
    assert_eq!(checked(&reading), Err(ProgramRefused::Unbound { group: 0, binding: 0 }));
    assert_eq!(checked(&moved), Err(ProgramRefused::Reserved { binding: 0 }));
}

#[test]
fn an_effect_id_reading_nothing_is_had_only_for_an_effect_that_reads_nothing() {
    // Catches an effect typed as reading nothing when it reads a target, which would leave its group 0 unbound as
    // the frame draws it.
    let alone = MadeEffect { program: ProgramId { id: 7 }, reads: Vec::new() };

    assert!(EffectId::<()>::of(&alone).is_some());
    assert!(EffectId::<(Float,)>::of(&alone).is_none());
}
