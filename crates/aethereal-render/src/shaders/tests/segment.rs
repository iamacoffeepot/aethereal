//! The WGSL `Segment` run on a GPU, against `aethereal-math`'s `Segment` on the table of cases both follow.

// The test says which GPU ran the segment, or that there was none and the run was skipped.
#![allow(clippy::print_stderr)]

use super::{program, validate};
use aethereal_math::cases::segment::{CROSSINGS, POINTS};
use aethereal_math::{Side, Vec2};
use wgpu::util::DeviceExt;

/// A program that asks the package's `Segment` each case of the table, one case to an invocation: what a segment
/// answers about a point into `point_answers`, and whether two segments cross, as 1 or 0, into `crossing_answers`.
const EVALUATE: &str = "
import aethereal_render::segment::{Segment, along, closest_point, contains_point, crosses, distance_to, side};

struct PointCase {
    segment: Segment,
    point: vec2<f32>,
}

struct PointAnswer {
    closest: vec2<f32>,
    foot: f32,
    gap: f32,
    sign: i32,
    contained: u32,
}

struct CrossingCase {
    segment: Segment,
    other: Segment,
}

@group(0) @binding(0) var<storage, read> point_cases: array<PointCase>;
@group(0) @binding(1) var<storage, read_write> point_answers: array<PointAnswer>;
@group(0) @binding(2) var<storage, read> crossing_cases: array<CrossingCase>;
@group(0) @binding(3) var<storage, read_write> crossing_answers: array<u32>;

@compute @workgroup_size(64)
fn evaluate(@builtin(global_invocation_id) id: vec3<u32>) {
    let index = id.x;
    if index < arrayLength(&point_cases) {
        let asked = point_cases[index];
        point_answers[index] = PointAnswer(
            closest_point(asked.segment, asked.point),
            along(asked.segment, asked.point),
            distance_to(asked.segment, asked.point),
            side(asked.segment, asked.point),
            u32(contains_point(asked.segment, asked.point)),
        );
    }
    if index < arrayLength(&crossing_cases) {
        let asked = crossing_cases[index];
        crossing_answers[index] = u32(crosses(asked.segment, asked.other));
    }
}
";

/// The words of one `PointAnswer` or one `CrossingCase`.
const POINT_WORDS: usize = 6;

/// How far a length or a distance the GPU measures may lie from the CPU's: WGSL lets `sqrt` and division round a few
/// units in the last place, and every answer of the table is below 8.
const TOLERANCE: f32 = 1e-5;

/// What the GPU answered about the table's points and crossings.
struct Answers {
    points: Vec<u32>,
    crossings: Vec<u32>,
}

fn bytes(floats: impl IntoIterator<Item = f32>) -> Vec<u8> {
    floats.into_iter().flat_map(f32::to_le_bytes).collect()
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes.chunks_exact(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect()
}

fn point_cases() -> Vec<u8> {
    let floats = POINTS.iter().flat_map(|case| {
        let Vec2 { x: start_x, y: start_y } = case.segment.start;
        let Vec2 { x: end_x, y: end_y } = case.segment.end;
        [start_x, start_y, end_x, end_y, case.point.x, case.point.y]
    });
    bytes(floats)
}

fn crossing_cases() -> Vec<u8> {
    let floats = CROSSINGS.iter().flat_map(|case| {
        let ends = [case.segment.start, case.segment.end, case.other.start, case.other.end];
        ends.into_iter().flat_map(Vec2::to_array)
    });
    bytes(floats)
}

/// The side the shader's `i32` names.
fn side(sign: u32) -> Side {
    match sign {
        1 => Side::Positive,
        0 => Side::On,
        0xffff_ffff => Side::Negative,
        _ => panic!("the shader answered the side {sign:#x}, which is none of its three"),
    }
}

/// A buffer the shader writes `size` bytes into, and one it is copied into for reading back.
fn written(device: &wgpu::Device, size: usize) -> (wgpu::Buffer, wgpu::Buffer) {
    let size = u64::try_from(size).expect("a table's answers fit a buffer");
    let storage = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readable = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    (storage, readable)
}

/// Reads back `buffer`, whose copy the last submission made, waiting on the device for it.
fn read(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Vec<u32> {
    buffer.map_async(wgpu::MapMode::Read, .., |mapped| mapped.expect("the answers map for reading"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("the device finishes the evaluation");
    let view = buffer.get_mapped_range(..).expect("the mapped answers are readable");
    words(&view)
}

/// Runs `wgsl`'s `evaluate` over the table on `device`, one invocation to a case.
fn evaluated(device: &wgpu::Device, queue: &wgpu::Queue, wgsl: &str) -> Answers {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("evaluate"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    let input = |contents: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents,
            usage: wgpu::BufferUsages::STORAGE,
        })
    };
    let points = input(&point_cases());
    let crossings = input(&crossing_cases());
    let (point_answers, point_readable) = written(device, POINTS.len() * POINT_WORDS * 4);
    let (crossing_answers, crossing_readable) = written(device, CROSSINGS.len() * 4);
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: points.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: point_answers.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: crossings.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: crossing_answers.as_entire_binding() },
        ],
    });

    let cases = u32::try_from(POINTS.len().max(CROSSINGS.len())).expect("the table's cases fit a dispatch");
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(cases.div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&point_answers, 0, &point_readable, 0, None);
    encoder.copy_buffer_to_buffer(&crossing_answers, 0, &crossing_readable, 0, None);
    queue.submit([encoder.finish()]);

    Answers { points: read(device, &point_readable), crossings: read(device, &crossing_readable) }
}

/// A device on a Vulkan adapter, or `None` where the machine has none, as CI's does not. Only Vulkan is asked for, so
/// the device that draws nothing, which the renderer's other tests use, is never taken for a GPU.
fn vulkan() -> Option<(wgpu::Device, wgpu::Queue)> {
    let described = wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    };
    let instance = wgpu::Instance::new(described);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    eprintln!("the WGSL segment runs on {}", adapter.get_info().name);
    let opened = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()));
    Some(opened.expect("a device opens on the adapter found"))
}

// The program that asks the package's `Segment` for the table links and validates, so the struct, its functions, and
// its side constants cross the package's edge on any machine, with or without a GPU.
#[test]
fn a_program_asks_the_segment_across_the_package_edge() {
    validate("the program evaluating the segment", &program(EVALUATE));
}

// The WGSL `Segment` answers each case of the table as `aethereal-math`'s does: the lengths, distances, and nearest
// points within `TOLERANCE`, and the sides, the containment, and the crossings exactly, as the table's coordinates make
// every product they rest on exact. A port that rounds, clips, or signs differently from the CPU fails here. Without a
// Vulkan adapter it says so and passes, as CI has none.
#[test]
fn the_wgsl_segment_answers_the_table_as_the_cpu_segment_does() {
    let Some((device, queue)) = vulkan() else {
        eprintln!("skipped: no Vulkan adapter, so the WGSL segment is not run");
        return;
    };
    let answers = evaluated(&device, &queue, &program(EVALUATE));

    for (case, answer) in POINTS.iter().zip(answers.points.chunks_exact(POINT_WORDS)) {
        let (segment, point) = (case.segment, case.point);
        let closest = Vec2::new(f32::from_bits(answer[0]), f32::from_bits(answer[1]));
        let along = f32::from_bits(answer[2]);
        let distance = f32::from_bits(answer[3]);
        let expected_closest = segment.closest_point(point);
        let along_matches = (along - segment.along(point)).abs() < TOLERANCE;
        let distance_matches = (distance - segment.distance_to(point)).abs() < TOLERANCE;
        let closest_matches = (closest - expected_closest).length() < TOLERANCE;
        assert!(along_matches, "the GPU's along of {case:?} is {along}, the CPU's {}", segment.along(point));
        assert!(
            distance_matches,
            "the GPU's distance of {case:?} is {distance}, the CPU's {}",
            segment.distance_to(point)
        );
        assert!(closest_matches, "the GPU's closest point of {case:?} is {closest:?}, the CPU's {expected_closest:?}");
        assert_eq!(side(answer[4]), segment.side(point), "the GPU's side of {case:?}");
        assert_eq!(answer[5] == 1, segment.contains_point(point), "the GPU's containment of {case:?}");
    }
    for (case, &crossed) in CROSSINGS.iter().zip(&answers.crossings) {
        assert_eq!(crossed == 1, case.segment.crosses(case.other), "the GPU's crossing of {case:?}");
    }
}
