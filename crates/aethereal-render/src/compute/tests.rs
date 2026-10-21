//! A compute program's check: what WebGPU would refuse as the pipeline is made is refused as the compute program is
//! asked for, and its workgroup size is read from its shader.

use super::*;
use crate::program::{Access, BindingKind};

/// A compute program that moves each particle of a field by its speed and a pull it reads, over a step of time, its
/// speed gathered in workgroup memory on the way.
const WGSL: &str = "
struct Particle { at: vec4<f32>, speed: vec4<f32> }
struct Field { count: u32, particles: array<Particle> }
struct Clock { seconds: f32 }

@group(0) @binding(0) var<uniform> clock: Clock;
@group(0) @binding(1) var<storage, read_write> field: Field;
@group(0) @binding(2) var<storage, read> pull: array<vec4<f32>>;

var<workgroup> speeds: array<vec4<f32>, 64>;

@compute @workgroup_size(64)
fn simulate(@builtin(global_invocation_id) at: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    if at.x >= arrayLength(&field.particles) {
        return;
    }
    speeds[local] = field.particles[at.x].speed + pull[0];
    field.particles[at.x].at += speeds[local] * clock.seconds;
}
";

/// The compute program [`WGSL`] is, with what it binds. The field's count, padded to the particles' alignment, is 16
/// bytes, and a particle 32.
fn compute() -> CreateCompute {
    let bindings = vec![
        Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: 4 } },
        Binding { group: 0, binding: 1, kind: BindingKind::Storage { bytes: 48, access: Access::Write } },
        Binding { group: 0, binding: 2, kind: BindingKind::Storage { bytes: 16, access: Access::Read } },
    ];

    CreateCompute { wgsl: String::from(WGSL), entry: String::from("simulate"), bindings }
}

/// [`compute`] with its WGSL's `from` written as `to`.
fn rewritten(from: &str, to: &str) -> CreateCompute {
    let mut compute = compute();
    assert!(compute.wgsl.contains(from), "the WGSL holds {from}");
    compute.wgsl = compute.wgsl.replace(from, to);

    compute
}

/// What the check answers `compute`, under the limits every WebGPU device has.
fn checked(compute: &CreateCompute) -> Result<[u32; 3], ProgramRefused> {
    check(compute, &wgpu::Limits::default())
}

#[test]
fn a_compute_stage_is_made_with_the_workgroup_size_its_shader_declares() {
    // Catches a workgroup size taken as given rather than read, or read before its constants are worked out, which a
    // dispatch would count its workgroups by.
    let constant = rewritten("@workgroup_size(64)", "@workgroup_size(SIDE, SIDE)");
    let constant = CreateCompute { wgsl: format!("const SIDE = 8u;\n{}", constant.wgsl), ..constant };

    assert_eq!(checked(&compute()), Ok([64, 1, 1]));
    assert_eq!(checked(&constant), Ok([8, 8, 1]));
}

#[test]
fn a_workgroup_size_a_pipeline_overrides_is_refused() {
    // Catches a size read from an override's default, which the pipeline could run at another.
    let overridden = rewritten("@workgroup_size(64)", "@workgroup_size(side)");
    let overridden = CreateCompute { wgsl: format!("override side: u32 = 64u;\n{}", overridden.wgsl), ..overridden };

    assert_eq!(checked(&overridden), Err(ProgramRefused::Overrides));
}

#[test]
fn an_entry_point_that_is_missing_or_not_a_compute_stage_is_refused() {
    // Catches an entry point found by its name alone: a fragment stage made into a compute pipeline.
    let mut missing = compute();
    missing.entry = String::from("settle");
    let mut fragment = rewritten(
        "var<workgroup>",
        "@fragment\nfn shade() -> @location(0) vec4<f32> {\n    return vec4(1.0);\n}\n\nvar<workgroup>",
    );
    fragment.entry = String::from("shade");

    assert_eq!(checked(&missing), Err(ProgramRefused::Entry { stage: Stage::Compute, name: String::from("settle") }));
    assert_eq!(checked(&fragment), Err(ProgramRefused::Entry { stage: Stage::Compute, name: String::from("shade") }));
}

#[test]
fn storage_listed_with_another_access_than_its_shader_declares_is_refused() {
    // Catches access taken from the listing alone: storage the shader writes bound read-only, or storage it only
    // reads bound to write, each a layout WebGPU refuses as the pipeline is made. A binding the shader uses that is
    // not listed is refused as a program's is.
    let mut written_as_read = compute();
    written_as_read.bindings[1].kind = BindingKind::Storage { bytes: 48, access: Access::Read };
    let mut read_as_written = compute();
    read_as_written.bindings[2].kind = BindingKind::Storage { bytes: 16, access: Access::Write };
    let mut unlisted = compute();
    unlisted.bindings.remove(0);

    assert_eq!(checked(&written_as_read), Err(ProgramRefused::Mismatch { group: 0, binding: 1 }));
    assert_eq!(checked(&read_as_written), Err(ProgramRefused::Mismatch { group: 0, binding: 2 }));
    assert_eq!(checked(&unlisted), Err(ProgramRefused::Unbound { group: 0, binding: 0 }));
}

#[test]
fn a_workgroup_larger_than_the_gpu_runs_is_refused() {
    // Catches a workgroup checked against one limit of the four: along each dimension, then in all. WebGPU's least
    // device runs 256 along x and y, 64 along z, and 256 in all.
    let sized = |size: &str| checked(&rewritten("@workgroup_size(64)", &format!("@workgroup_size({size})")));

    assert_eq!(sized("16, 16, 1"), Ok([16, 16, 1]));
    assert_eq!(sized("512"), Err(ProgramRefused::Workgroup { size: [512, 1, 1] }));
    assert_eq!(sized("1, 1, 128"), Err(ProgramRefused::Workgroup { size: [1, 1, 128] }));
    assert_eq!(sized("16, 16, 2"), Err(ProgramRefused::Workgroup { size: [16, 16, 2] }));
}

#[test]
fn workgroup_memory_past_what_the_gpu_gives_is_refused_counted_as_webgpu_counts_it() {
    // Catches workgroup memory left unchecked, or counted without rounding each variable up to sixteen bytes, which
    // lets through what WebGPU refuses. WebGPU's least device gives a workgroup 16384 bytes.
    let held = |speeds: u32| {
        let wgsl = WGSL.replace("vec4<f32>, 64>", &format!("vec4<f32>, {speeds}>")).replace(
            "var<workgroup> speeds",
            "var<workgroup> first: u32;\nvar<workgroup> second: u32;\nvar<workgroup> speeds",
        );
        let wgsl = wgsl.replace("speeds[local] = ", "first = 1u;\n    second = 1u;\n    speeds[local] = ");
        checked(&CreateCompute { wgsl, ..compute() })
    };

    assert_eq!(held(1022), Ok([64, 1, 1]));
    assert_eq!(held(1023), Err(ProgramRefused::WorkgroupMemory { bytes: 16_400 }));
}

#[test]
fn more_storage_than_a_stage_binds_is_refused() {
    // Catches a compute program's bindings left unchecked against what the GPU binds in a stage, as a program's are.
    let mut many = compute();
    let more = (3..=10).map(|binding| Binding {
        group: 0,
        binding,
        kind: BindingKind::Storage { bytes: 4, access: Access::Read },
    });
    many.bindings.extend(more);

    assert_eq!(
        checked(&many),
        Err(ProgramRefused::Many { kind: BindingKind::Storage { bytes: 0, access: Access::Read } })
    );
}
