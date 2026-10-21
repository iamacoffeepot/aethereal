//! A program's check: what WebGPU would refuse as the pipeline is made is refused as the program is asked for.

use super::*;
use crate::mesh::Attribute;
use crate::shape::ColourFormat;

/// A program that lifts each vertex by a camera and shades it from a texture through a sampler.
const WGSL: &str = "
struct Camera { view: mat4x4<f32> }

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var soft: sampler;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn lift(@location(0) position: vec3<f32>, @location(1) uv: vec2<f32>) -> Between {
    return Between(camera.view * vec4(position, 1.0), uv);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return textureSample(image, soft, between.uv);
}
";

/// The program [`WGSL`] is, with a layout of a position and a texture coordinate, and what it binds.
fn program() -> CreateProgram {
    let position = Attribute { location: 0, format: VertexFormat::Float32x3, offset: 0 };
    let uv = Attribute { location: 1, format: VertexFormat::Float32x2, offset: 12 };
    let bindings = vec![
        Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: 64 } },
        Binding { group: 1, binding: 0, kind: BindingKind::Texture },
        Binding { group: 1, binding: 1, kind: BindingKind::Sampler },
    ];

    CreateProgram {
        wgsl: String::from(WGSL),
        vertex: String::from("lift"),
        fragment: String::from("shade"),
        layout: VertexLayout { stride: 20, attributes: vec![position, uv] },
        instances: PerInstance::None,
        bindings,
        state: Fixed { blend: Blend::Opaque, depth: Depth::TestWrite, cull: Cull::Back },
        shape: Shape::with_depth(ColourFormat::Bgra8),
    }
}

/// [`program`] with its WGSL's `from` written as `to`.
fn rewritten(from: &str, to: &str) -> CreateProgram {
    let mut program = program();
    assert!(program.wgsl.contains(from), "the WGSL holds {from}");
    program.wgsl = program.wgsl.replace(from, to);

    program
}

/// What the check answers `program`, under the limits every WebGPU device has.
fn checked(program: &CreateProgram) -> Result<(), ProgramRefused> {
    check(program, &wgpu::Limits::default())
}

#[test]
fn a_program_whose_stages_read_what_is_given_them_is_made() {
    // Catches a check that refuses what WebGPU makes: a struct passed between the stages, a builtin beside the
    // locations, a vertex read with more components than its format gives, a binding listed that the shader leaves
    // unused, and one the shader declares and does not use, left unlisted.
    let mut wider = program();
    wider.layout.attributes[0].format = VertexFormat::Float32x2;
    let mut more = program();
    more.bindings.push(Binding { group: 2, binding: 0, kind: BindingKind::Sampler });
    let spare = rewritten("struct Camera", "@group(3) @binding(0) var spare: sampler;\nstruct Camera");

    assert_eq!([program(), wider, more, spare].map(|program| checked(&program)), [Ok(()), Ok(()), Ok(()), Ok(())]);
}

#[test]
fn wgsl_that_does_not_parse_or_is_not_valid_is_refused_with_why() {
    // Catches WGSL handed to the GPU unchecked: WebGPU would make an invalid pipeline, and wgpu natively stops.
    let unparsed = checked(&rewritten("fn lift", "fn lift("));
    let invalid = checked(&rewritten("return textureSample(image, soft, between.uv);", "return between.uv;"));
    let overridden = checked(&rewritten("struct Camera", "override scale: f32;\nstruct Camera"));

    assert!(matches!(unparsed, Err(ProgramRefused::Parse { message }) if message.contains("lift")));
    assert!(matches!(invalid, Err(ProgramRefused::Invalid { .. })));
    assert_eq!(overridden, Err(ProgramRefused::Overrides));
}

#[test]
fn an_entry_point_that_is_missing_or_of_the_other_stage_is_refused() {
    // Catches an entry point found by its name alone: the fragment stage named as the vertex stage.
    let mut missing = program();
    missing.fragment = String::from("paint");
    let mut swapped = program();
    swapped.vertex = String::from("shade");

    assert_eq!(checked(&missing), Err(ProgramRefused::Entry { stage: Stage::Fragment, name: String::from("paint") }));
    assert_eq!(checked(&swapped), Err(ProgramRefused::Entry { stage: Stage::Vertex, name: String::from("shade") }));
}

#[test]
fn a_vertex_stage_that_reads_what_the_layout_does_not_give_is_refused() {
    // Catches an input checked against no attribute, or against an attribute of another kind of number.
    let mut missing = program();
    missing.layout.attributes.pop();
    let mut integer = program();
    integer.layout.attributes[1].format = VertexFormat::Uint32;
    let mut bad_layout = program();
    bad_layout.layout.stride = 18;

    assert_eq!(checked(&missing), Err(ProgramRefused::Input { location: 1 }));
    assert_eq!(checked(&integer), Err(ProgramRefused::InputFormat { location: 1 }));
    assert_eq!(checked(&bad_layout), Err(ProgramRefused::Layout { why: MeshRefused::Stride { stride: 18 } }));
}

#[test]
fn a_vertex_stage_reads_per_instance_what_the_instance_layout_gives_at_locations_of_its_own() {
    // Catches inputs checked against the vertex layout alone, which refuses every program drawn per instance, or
    // instance attributes left unchecked, or two layouts read at one location, which WebGPU refuses as the pipeline
    // is made.
    let mut lifted =
        rewritten("@location(1) uv: vec2<f32>)", "@location(1) uv: vec2<f32>, @location(2) offset: vec3<f32>)");
    let offset = Attribute { location: 2, format: VertexFormat::Float32x3, offset: 0 };
    lifted.instances = PerInstance::Records(VertexLayout { stride: 12, attributes: vec![offset] });
    let mut unread = lifted.clone();
    unread.instances = PerInstance::None;
    let mut shared = program();
    let over = Attribute { location: 1, format: VertexFormat::Float32x2, offset: 0 };
    shared.instances = PerInstance::Records(VertexLayout { stride: 8, attributes: vec![over] });
    let mut bad_layout = lifted.clone();
    bad_layout.instances = PerInstance::Records(VertexLayout { stride: 6, attributes: vec![offset] });

    assert_eq!(checked(&lifted), Ok(()));
    assert_eq!(checked(&unread), Err(ProgramRefused::Input { location: 2 }));
    assert_eq!(checked(&shared), Err(ProgramRefused::Shared { location: 1 }));
    assert_eq!(checked(&bad_layout), Err(ProgramRefused::InstanceLayout { why: MeshRefused::Stride { stride: 6 } }));
}

#[test]
fn a_fragment_stage_that_reads_what_the_vertex_stage_does_not_write_is_refused() {
    // Catches stages checked apart: each valid alone, and the pipeline refused for what passes between them.
    let read = |input: &str| {
        let mut split = rewritten("fn shade(between: Between)", &format!("fn shade({input})"));
        split.wgsl = split.wgsl.replace("between.uv);", "uv);");
        checked(&split)
    };

    assert_eq!(read("@location(0) uv: vec2<f32>"), Ok(()));
    assert_eq!(read("@location(0) @interpolate(flat) uv: vec2<f32>"), Err(ProgramRefused::Stages { location: 0 }));
    assert_eq!(read("@location(1) uv: vec2<f32>"), Err(ProgramRefused::Stages { location: 1 }));
}

#[test]
fn a_fragment_stage_that_does_not_write_four_floats_of_colour_is_refused() {
    // Catches a colour of fewer channels than the target has, which WebGPU refuses.
    let three = rewritten(
        "-> @location(0) vec4<f32> {\n    return textureSample(image, soft, between.uv);",
        "-> @location(0) vec3<f32> {\n    return textureSample(image, soft, between.uv).rgb;",
    );

    assert_eq!(checked(&three), Err(ProgramRefused::Output));
}

#[test]
fn a_binding_the_shader_uses_unlisted_or_as_another_kind_is_refused() {
    // Catches a binding found by its place alone, a uniform block smaller than what the shader reads, and a storage
    // buffer, a texture that is not two-dimensional, and a comparing sampler let through as bindings a program makes.
    let mut unlisted = program();
    unlisted.bindings.remove(1);
    let mut small = program();
    small.bindings[0].kind = BindingKind::Uniform { bytes: 32 };
    let mut swapped = program();
    swapped.bindings[2].kind = BindingKind::Texture;
    let mut storage = rewritten("var<uniform> camera", "var<storage> camera");
    storage.bindings[0].kind = BindingKind::Uniform { bytes: 64 };
    let mut deep = rewritten("var image: texture_2d<f32>", "var image: texture_3d<f32>");
    deep.wgsl = deep.wgsl.replace("between.uv);", "vec3(between.uv, 0.0));");
    let mut comparing = rewritten(
        "@group(1) @binding(0) var image: texture_2d<f32>;\n@group(1) @binding(1) var soft: sampler;",
        "@group(1) @binding(1) var soft: sampler_comparison;\n@group(1) @binding(0) var image: texture_depth_2d;",
    );
    comparing.wgsl = comparing
        .wgsl
        .replace("textureSample(image, soft, between.uv)", "vec4(textureSampleCompare(image, soft, between.uv, 0.5))");

    assert_eq!(checked(&unlisted), Err(ProgramRefused::Unbound { group: 1, binding: 0 }));
    assert_eq!(checked(&small), Err(ProgramRefused::Mismatch { group: 0, binding: 0 }));
    assert_eq!(checked(&swapped), Err(ProgramRefused::Mismatch { group: 1, binding: 1 }));
    assert_eq!(checked(&storage), Err(ProgramRefused::Mismatch { group: 0, binding: 0 }));
    assert_eq!(checked(&deep), Err(ProgramRefused::Mismatch { group: 1, binding: 0 }));
    assert_eq!(checked(&comparing), Err(ProgramRefused::Mismatch { group: 1, binding: 1 }));
}

#[test]
fn bindings_the_gpu_would_not_bind_are_refused() {
    // Catches a layout WebGPU refuses: two bindings at one place, a group past the last, a uniform block of no
    // bytes or past what the GPU binds, and more textures than a stage binds.
    let mut repeated = program();
    repeated.bindings.push(Binding { group: 1, binding: 1, kind: BindingKind::Sampler });
    let mut past = program();
    past.bindings.push(Binding { group: 4, binding: 0, kind: BindingKind::Sampler });
    let mut empty = program();
    empty.bindings[0].kind = BindingKind::Uniform { bytes: 0 };
    let mut large = program();
    large.bindings[0].kind = BindingKind::Uniform { bytes: 65_537 };
    let mut many = program();
    many.bindings.extend((2..=17).map(|binding| Binding { group: 1, binding, kind: BindingKind::Texture }));

    assert_eq!(checked(&repeated), Err(ProgramRefused::Repeated { group: 1, binding: 1 }));
    assert_eq!(checked(&past), Err(ProgramRefused::Group { group: 4 }));
    assert_eq!(checked(&empty), Err(ProgramRefused::Size { group: 0, binding: 0 }));
    assert_eq!(checked(&large), Err(ProgramRefused::Size { group: 0, binding: 0 }));
    assert_eq!(checked(&many), Err(ProgramRefused::Many { kind: BindingKind::Texture }));
}

/// A program that places each vertex at a splat it reads from storage, in its vertex stage, by its instance, and
/// shades it the splat's colour, read again in its fragment stage.
const SPLATS: &str = "
struct Splat { at: vec4<f32>, colour: vec4<f32> }
struct Field { count: u32, splats: array<Splat> }

@group(0) @binding(0) var<storage, read> field: Field;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) @interpolate(flat) splat: u32,
}

@vertex
fn lift(@location(0) position: vec3<f32>, @builtin(instance_index) splat: u32) -> Between {
    return Between(vec4(position, 1.0) + field.splats[splat].at, splat);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return field.splats[between.splat].colour * f32(field.count);
}
";

/// The program [`SPLATS`] is, binding storage of `bytes`.
fn splats(bytes: u32) -> CreateProgram {
    let mut program = program();
    program.wgsl = String::from(SPLATS);
    program.layout.attributes.pop();
    program.bindings =
        vec![Binding { group: 0, binding: 0, kind: BindingKind::Storage { bytes, access: Access::Read } }];

    program
}

#[test]
fn storage_both_stages_only_read_is_bound_at_least_its_fixed_part_and_one_element() {
    // Catches storage refused as every storage buffer was, a runtime-sized array counted as nothing, which lets a
    // binding too small for one splat through, and the least size counted past one element, which refuses storage
    // WebGPU binds. The field's count, padded to the splats' alignment, is 16 bytes, and a splat 32.
    assert_eq!(checked(&splats(48)), Ok(()));
    assert_eq!(checked(&splats(1 << 20)), Ok(()));
    assert_eq!(checked(&splats(44)), Err(ProgramRefused::Mismatch { group: 0, binding: 0 }));
}

#[test]
fn storage_the_shader_writes_or_binds_as_another_kind_is_refused() {
    // Catches storage let through whatever the shader does with it: WebGPU binds a buffer a stage writes only as
    // writable storage, which a program does not bind, and storage and uniforms are bound apart.
    let mut written = splats(48);
    written.wgsl = written.wgsl.replace("var<storage, read>", "var<storage, read_write>").replace(
        "return field.splats[between.splat].colour",
        "field.count = 1u;\n    return field.splats[between.splat].colour",
    );
    let mut as_uniform = splats(48);
    as_uniform.bindings[0].kind = BindingKind::Uniform { bytes: 48 };
    let mut as_storage = program();
    as_storage.bindings[0].kind = BindingKind::Storage { bytes: 64, access: Access::Read };

    assert_eq!(checked(&written), Err(ProgramRefused::Mismatch { group: 0, binding: 0 }));
    assert_eq!(checked(&as_uniform), Err(ProgramRefused::Mismatch { group: 0, binding: 0 }));
    assert_eq!(checked(&as_storage), Err(ProgramRefused::Mismatch { group: 0, binding: 0 }));
}

#[test]
fn a_draw_program_listing_storage_to_write_is_refused_whatever_its_shader_declares() {
    // Catches a draw program let write storage, which WebGPU never binds for writing in a vertex stage, whether its
    // shader declares it written or only read (ADR-0026).
    let mut listed = splats(48);
    listed.bindings[0].kind = BindingKind::Storage { bytes: 48, access: Access::Write };
    let mut declared = listed.clone();
    declared.wgsl = declared.wgsl.replace("var<storage, read>", "var<storage, read_write>");

    assert_eq!(checked(&listed), Err(ProgramRefused::Writes { group: 0, binding: 0 }));
    assert_eq!(checked(&declared), Err(ProgramRefused::Writes { group: 0, binding: 0 }));
}

#[test]
fn storage_the_gpu_would_not_bind_is_refused() {
    // Catches storage sized against the uniform limit, or not at all, and storage buffers counted with uniforms or
    // not counted: WebGPU refuses the layout as the pipeline is made.
    let limits = wgpu::Limits::default();
    let most = u32::try_from(limits.max_storage_buffer_binding_size).expect("the default limit is under 4 GiB");
    let mut many = splats(48);
    let more = (1..=8).map(|binding| Binding {
        group: 1,
        binding,
        kind: BindingKind::Storage { bytes: 4, access: Access::Read },
    });
    many.bindings.extend(more);
    let mut uniforms_beside = splats(48);
    let uniforms = (1..=8).map(|binding| Binding { group: 1, binding, kind: BindingKind::Uniform { bytes: 4 } });
    uniforms_beside.bindings.extend(uniforms);

    assert_eq!(checked(&splats(0)), Err(ProgramRefused::Size { group: 0, binding: 0 }));
    assert_eq!(checked(&splats(most)), Ok(()));
    assert_eq!(checked(&splats(most + 4)), Err(ProgramRefused::Size { group: 0, binding: 0 }));
    assert_eq!(
        checked(&many),
        Err(ProgramRefused::Many { kind: BindingKind::Storage { bytes: 0, access: Access::Read } })
    );
    assert_eq!(checked(&uniforms_beside), Ok(()));
}

#[test]
fn a_program_into_whole_numbers_writes_them_and_blends_nothing() {
    // Catches a target of whole numbers drawn as colour, or blended into: WebGPU refuses each as the pipeline is made.
    let whole = Shape::with_depth(ColourFormat::R32Uint);
    let mut ids = rewritten(
        "-> @location(0) vec4<f32> {\n    return textureSample(image, soft, between.uv);",
        "-> @location(0) u32 {\n    return u32(textureSample(image, soft, between.uv).x * 255.0);",
    );
    ids.shape = whole;
    let mut floats = program();
    floats.shape = whole;
    let mut blended = ids.clone();
    blended.state.blend = Blend::Alpha;

    assert_eq!(checked(&ids), Ok(()));
    assert_eq!(checked(&floats), Err(ProgramRefused::Output));
    assert_eq!(checked(&blended), Err(ProgramRefused::Blend));
}

#[test]
fn a_program_testing_depth_in_a_shape_that_holds_none_is_refused() {
    // Catches a pipeline made with depth for a pass without it, which WebGPU refuses as the pass draws it.
    let mut depthless = program();
    depthless.shape = Shape::flat(ColourFormat::Bgra8);
    let mut untested = depthless.clone();
    untested.state.depth = Depth::Off;

    assert_eq!(checked(&depthless), Err(ProgramRefused::Depthless));
    assert_eq!(checked(&untested), Ok(()));
}
