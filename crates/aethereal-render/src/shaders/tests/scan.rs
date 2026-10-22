//! The workgroup scan and the ordered compaction: the programs that use them as the module documentation shows link and
//! validate, their Rust mirror of the three passes answers what a plain running sum and a filter answer, and, where the
//! machine has a GPU adapter, the shader answers it too.

// The GPU test says which adapter ran the scan, or that there was none and the run was skipped.
#![allow(clippy::print_stderr)]

use super::{program, validate};
use crate::shaders::WORKGROUP_SIZE;
use naga::front::wgsl;
use wgpu::util::DeviceExt;

/// The compaction across workgroups that the module documentation shows: three entry points, each a dispatch in a
/// compute pass of its own.
const COMPACTED: &str = "
import aethereal_render::scan::{WORKGROUP_SIZE, scan_from};
import aethereal_render::compact::place;

@group(0) @binding(0) var<storage, read> items: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read_write> totals: array<u32>;
@group(0) @binding(2) var<storage, read_write> kept: array<u32>;
@group(0) @binding(3) var<storage, read_write> count: u32;

fn keeps(at: u32) -> bool {
    return at < arrayLength(&items) && length(items[at]) < 0.75;
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn tally(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32,
         @builtin(workgroup_id) group: vec3<u32>) {
    let placed = place(keeps(id.x), local, 0u);
    if local == 0u {
        totals[group.x] = placed.end;
    }
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn offsets(@builtin(local_invocation_index) local: u32) {
    let groups = arrayLength(&totals);
    var carry = 0u;
    for (var first = 0u; first < groups; first += WORKGROUP_SIZE) {
        let at = first + local;
        var total = 0u;
        if at < groups {
            total = totals[at];
        }
        let summed = scan_from(total, local, carry);
        if at < groups {
            totals[at] = summed.exclusive;
        }
        carry = summed.total;
    }
    if local == 0u {
        count = carry;
    }
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn scatter(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32,
           @builtin(workgroup_id) group: vec3<u32>) {
    let keep = keeps(id.x);
    let placed = place(keep, local, totals[group.x]);
    if keep {
        kept[placed.at] = id.x;
    }
}
";

/// The test's own program, scanning and compacting at once in the three passes: the first scans each workgroup's
/// values and counts the odd ones it keeps, the second scans both workgroups' totals, a width at a time, into their
/// offsets and ends with the sums of all, and the third adds each workgroup's offset to its sums and writes the index
/// of each odd value in order. The number of elements rides a uniform, so none is a buffer of no bytes.
const SCANNED: &str = "
import aethereal_render::scan::{WORKGROUP_SIZE, scan, scan_from};
import aethereal_render::compact::place;

struct Count { elements: u32 }

@group(0) @binding(0) var<uniform> count: Count;
@group(0) @binding(1) var<storage, read> values: array<u32>;
@group(0) @binding(2) var<storage, read_write> sums: array<u32>;
@group(0) @binding(3) var<storage, read_write> totals: array<u32>;
@group(0) @binding(4) var<storage, read_write> kept_totals: array<u32>;
@group(0) @binding(5) var<storage, read_write> kept: array<u32>;
@group(0) @binding(6) var<storage, read_write> ends: array<u32, 2>;

fn value_at(at: u32) -> u32 {
    if at < count.elements {
        return values[at];
    }
    return 0u;
}

fn keeps(at: u32) -> bool {
    return (value_at(at) & 1u) == 1u;
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn each(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32,
        @builtin(workgroup_id) group: vec3<u32>) {
    let summed = scan(value_at(id.x), local);
    let placed = place(keeps(id.x), local, 0u);
    if id.x < count.elements {
        sums[id.x] = summed.exclusive;
    }
    if local == 0u {
        totals[group.x] = summed.total;
        kept_totals[group.x] = placed.end;
    }
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn across(@builtin(local_invocation_index) local: u32) {
    let groups = (count.elements + WORKGROUP_SIZE - 1u) / WORKGROUP_SIZE;
    var carry = 0u;
    var kept_carry = 0u;
    for (var first = 0u; first < groups; first += WORKGROUP_SIZE) {
        let at = first + local;
        var total = 0u;
        var kept_total = 0u;
        if at < groups {
            total = totals[at];
            kept_total = kept_totals[at];
        }
        let summed = scan_from(total, local, carry);
        let placed = scan_from(kept_total, local, kept_carry);
        if at < groups {
            totals[at] = summed.exclusive;
            kept_totals[at] = placed.exclusive;
        }
        carry = summed.total;
        kept_carry = placed.total;
    }
    if local == 0u {
        ends[0] = carry;
        ends[1] = kept_carry;
    }
}

@compute @workgroup_size(WORKGROUP_SIZE)
fn back(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32,
        @builtin(workgroup_id) group: vec3<u32>) {
    if id.x < count.elements {
        sums[id.x] += totals[group.x];
    }
    let keep = keeps(id.x);
    let placed = place(keep, local, kept_totals[group.x]);
    if keep {
        kept[placed.at] = id.x;
    }
}
";

/// What a buffer the shader writes holds where it wrote nothing.
const UNWRITTEN: u32 = u32::MAX;

/// The workgroup's width, as an index.
const WIDTH: usize = WORKGROUP_SIZE as usize;

/// The reference scan: each element's exclusive running sum, wrapping as the shader's `u32` wraps, and the sum of all.
fn exclusive(values: &[u32]) -> (Vec<u32>, u32) {
    let mut sum = 0_u32;
    let sums = values
        .iter()
        .map(|value| {
            let before = sum;
            sum = sum.wrapping_add(*value);
            before
        })
        .collect();

    (sums, sum)
}

/// The reference compaction: the index of each odd value, in order.
fn compacted(values: &[u32]) -> Vec<u32> {
    (0..).zip(values).filter(|(_, value)| *value % 2 == 1).map(|(at, _)| at).collect()
}

/// The shader's `scan` over one workgroup's values, as it runs: each step adds the sum `offset` places back, every
/// place reading before any writes, as the barrier between them makes it. Answers each place's inclusive sum.
fn workgroup(values: &[u32; WIDTH]) -> [u32; WIDTH] {
    let mut sums = *values;
    let mut offset = 1;
    while offset < WIDTH {
        let before = sums;
        for (local, sum) in sums.iter_mut().enumerate().skip(offset) {
            *sum = sum.wrapping_add(before[local - offset]);
        }
        offset <<= 1;
    }

    sums
}

/// What the three passes leave, as the Rust mirror or the GPU runs them: each element's exclusive sum, the sum of all,
/// the kept indices, and how many were kept.
#[derive(Debug, PartialEq, Eq)]
struct Passed {
    sums: Vec<u32>,
    total: u32,
    kept: Vec<u32>,
    count: u32,
}

impl Passed {
    /// What the reference scan and compaction answer for `values`.
    fn expected(values: &[u32]) -> Self {
        let (sums, total) = exclusive(values);
        let kept = compacted(values);
        let count = u32::try_from(kept.len()).unwrap_or(u32::MAX);

        Self { sums, total, kept, count }
    }
}

/// The scan and the compaction of `values` in the three passes, in Rust, as `SCANNED` runs them: a workgroup at a time,
/// each padded past the end with nothing, then the totals a width at a time with a carry, then each workgroup's offset
/// added back.
fn passed(values: &[u32]) -> Passed {
    let mut sums = Vec::with_capacity(values.len());
    let mut totals = Vec::new();
    let mut kept_totals = Vec::new();
    for chunk in values.chunks(WIDTH) {
        let mut padded = [0; WIDTH];
        padded[..chunk.len()].copy_from_slice(chunk);
        let inclusive = workgroup(&padded);
        let keeps = padded.map(|value| value % 2);
        sums.extend(chunk.iter().zip(inclusive).map(|(value, sum)| sum.wrapping_sub(*value)));
        totals.push(inclusive[WIDTH - 1]);
        kept_totals.push(workgroup(&keeps)[WIDTH - 1]);
    }

    let (offsets, total) = carried(&totals);
    let (kept_offsets, count) = carried(&kept_totals);

    let mut kept = vec![UNWRITTEN; values.len()];
    for (group, chunk) in values.chunks(WIDTH).enumerate() {
        for sum in &mut sums[group * WIDTH..group * WIDTH + chunk.len()] {
            *sum = sum.wrapping_add(offsets[group]);
        }
        let mut keeps = [0; WIDTH];
        for (keep, value) in keeps.iter_mut().zip(chunk) {
            *keep = value % 2;
        }
        let inclusive = workgroup(&keeps);
        for (local, keep) in keeps.iter().enumerate() {
            if *keep == 1 {
                let place = kept_offsets[group] + inclusive[local] - 1;
                kept[place as usize] = u32::try_from(group * WIDTH + local).unwrap_or(u32::MAX);
            }
        }
    }
    kept.truncate(count as usize);

    Passed { sums, total, kept, count }
}

/// The second pass: `totals` scanned a width at a time, each width from the carry the last answered. Answers each
/// total's exclusive sum, its workgroup's offset, and the last carry.
fn carried(totals: &[u32]) -> (Vec<u32>, u32) {
    let mut carry = 0_u32;
    let mut offsets = Vec::with_capacity(totals.len());
    for width in totals.chunks(WIDTH) {
        let mut padded = [0; WIDTH];
        padded[..width.len()].copy_from_slice(width);
        let inclusive = workgroup(&padded);
        offsets.extend(width.iter().zip(inclusive).map(|(total, sum)| carry.wrapping_add(sum.wrapping_sub(*total))));
        carry = carry.wrapping_add(inclusive[WIDTH - 1]);
    }

    (offsets, carry)
}

/// Which values a case keeps: its odd ones.
#[derive(Debug, Clone, Copy)]
enum Keeps {
    /// Some odd and some even, from the sequence.
    Some,
    /// Every value made odd.
    All,
    /// Every value made even.
    None,
}

/// The lengths each case runs at: none, one, a workgroup's width less one, exactly one and three of them, one past
/// one, and one past a width of widths, so the second pass walks two widths of totals.
const LENGTHS: [usize; 7] = [0, 1, WIDTH - 1, WIDTH, WIDTH + 1, 3 * WIDTH, WIDTH * WIDTH + 1];

/// `length` values from a fixed sequence of whole numbers across all of `u32`, so the sums wrap, kept as `keeps` says.
fn values(length: usize, keeps: Keeps) -> Vec<u32> {
    let mut next = 0x2545_f491_u32;
    (0..length)
        .map(|_| {
            next = next.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            match keeps {
                Keeps::Some => next,
                Keeps::All => next | 1,
                Keeps::None => next & !1,
            }
        })
        .collect()
}

/// Every case: each length, keeping some, all, and none.
fn cases() -> impl Iterator<Item = (usize, Keeps)> {
    LENGTHS.into_iter().flat_map(|length| [Keeps::Some, Keeps::All, Keeps::None].map(|keeps| (length, keeps)))
}

// The three-pass arithmetic answers the plain running sum and filter at every edge: the padding past the end, the
// offset each workgroup adds back, and the second pass's carry from one width of totals to the next.
#[test]
fn the_three_passes_scan_and_compact_as_a_running_sum_and_a_filter() {
    for (length, keeps) in cases() {
        let values = values(length, keeps);
        assert_eq!(passed(&values), Passed::expected(&values), "{length} values, keeping {keeps:?}");
    }
}

// A program compacting across workgroups as the documentation shows, and the test's own scanning one, link with the
// package and validate: the imported constant sizes the workgroup, and the barriers inside `scan` sit in control flow
// every invocation takes alike.
#[test]
fn the_three_pass_programs_link_into_wgsl_the_renderer_validates() {
    validate("the documented compaction", &program(COMPACTED));
    validate("the test's scan and compaction", &program(SCANNED));
}

// The workgroup size a program counts its dispatch by in Rust is the one the shader runs.
#[test]
fn the_rust_workgroup_size_is_the_shaders() {
    let linked = program(SCANNED);
    let module = wgsl::parse_str(&linked).unwrap_or_else(|error| panic!("{}", error.emit_to_string(&linked)));
    let each = module.entry_points.iter().find(|entry| entry.name == "each").map(|entry| entry.workgroup_size);

    assert_eq!(each, Some([WORKGROUP_SIZE, 1, 1]), "the linked shader's workgroup:\n{linked}");
}

/// A device on the machine's adapter, if it has one.
fn device() -> Result<(wgpu::Device, wgpu::Queue, String), String> {
    let mut described = wgpu::InstanceDescriptor::new_without_display_handle_from_env();
    described.backends &= wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(described);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .map_err(|error| error.to_string())?;
    let name = adapter.get_info().name;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
        .map_err(|error| error.to_string())?;

    Ok((device, queue, name))
}

/// The bytes of `words`.
fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_ne_bytes()).collect()
}

/// The three passes of `SCANNED`, made once on a device.
struct Scanner {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    passes: [wgpu::ComputePipeline; 3],
}

impl Scanner {
    fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let wgsl = program(SCANNED);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(wgsl.into()),
        });
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let written = wgpu::BufferBindingType::Storage { read_only: false };
        let entries = [
            entry(0, wgpu::BufferBindingType::Uniform),
            entry(1, wgpu::BufferBindingType::Storage { read_only: true }),
            entry(2, written),
            entry(3, written),
            entry(4, written),
            entry(5, written),
            entry(6, written),
        ];
        let layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: None, entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pass = |name| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(name),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(name),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };
        let passes = [pass("each"), pass("across"), pass("back")];

        Self { device, queue, layout, passes }
    }

    /// A buffer holding `words`, which the shader binds and the test copies out of.
    fn buffer(&self, words: &[u32], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &bytes(words),
            usage: usage | wgpu::BufferUsages::COPY_SRC,
        })
    }

    /// Runs the three passes over `values`, each in a compute pass of its own, and reads back what they left.
    fn run(&self, values: &[u32]) -> Passed {
        let elements = u32::try_from(values.len()).unwrap_or(u32::MAX);
        let groups = elements.div_ceil(WORKGROUP_SIZE);
        // No buffer is bound with no bytes: each holds at least one word, the values one past their end, which the
        // shader never reads as it reads only `count.elements`.
        let room = values.len().max(1);
        let unwritten = vec![UNWRITTEN; room];
        let unwritten_totals = vec![UNWRITTEN; (groups as usize).max(1)];
        let read = [values, &[0]].concat();
        let storage = wgpu::BufferUsages::STORAGE;
        let buffers = [
            self.buffer(&[elements, 0, 0, 0], wgpu::BufferUsages::UNIFORM),
            self.buffer(&read, storage),
            self.buffer(&unwritten, storage),
            self.buffer(&unwritten_totals, storage),
            self.buffer(&unwritten_totals, storage),
            self.buffer(&unwritten, storage),
            self.buffer(&[UNWRITTEN, UNWRITTEN], storage),
        ];
        let entries: Vec<wgpu::BindGroupEntry> = (0..)
            .zip(&buffers)
            .map(|(binding, buffer)| wgpu::BindGroupEntry { binding, resource: buffer.as_entire_binding() })
            .collect();
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.layout,
            entries: &entries,
        });

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        for (pass, workgroups) in self.passes.iter().zip([groups, 1, groups]) {
            let mut recording = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            recording.set_pipeline(pass);
            recording.set_bind_group(0, &group, &[]);
            recording.dispatch_workgroups(workgroups, 1, 1);
        }
        let sums = self.read(&mut encoder, &buffers[2]);
        let ends = self.read(&mut encoder, &buffers[6]);
        let kept = self.read(&mut encoder, &buffers[5]);
        self.queue.submit([encoder.finish()]);

        let ends = ends.words(&self.device);
        let count = ends[1];
        let mut kept = kept.words(&self.device);
        // What the compaction left past its count must be untouched.
        let past: Vec<u32> = kept.split_off((count as usize).min(kept.len()));
        assert!(past.iter().all(|word| *word == UNWRITTEN), "the compaction wrote past its count {count}");
        let mut sums = sums.words(&self.device);
        sums.truncate(values.len());

        Passed { sums, total: ends[0], kept, count }
    }

    /// Copies `buffer` out once what the encoder holds before has run, into a buffer the test maps.
    fn read(&self, encoder: &mut wgpu::CommandEncoder, buffer: &wgpu::Buffer) -> Readback {
        let copy = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: buffer.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &copy, 0, buffer.size());

        Readback(copy)
    }
}

/// A buffer copied out of one the shader wrote, to be mapped once the copy has run.
struct Readback(wgpu::Buffer);

impl Readback {
    /// Its words, once the GPU has finished what was submitted.
    fn words(&self, device: &wgpu::Device) -> Vec<u32> {
        self.0.map_async(wgpu::MapMode::Read, .., |mapped| mapped.expect("the copy maps"));
        device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU finishes");
        let view = self.0.get_mapped_range(..).expect("the copy is mapped");

        view.chunks_exact(4).map(|word| u32::from_ne_bytes([word[0], word[1], word[2], word[3]])).collect()
    }
}

// On a GPU, the shader's scan and compaction answer what the plain running sum and filter answer, at every edge: no
// elements, one, a workgroup's width and one either side of it, three widths, and one past a width of widths, keeping
// some, all, and none. A machine with no adapter, as CI's, skips it and says so.
#[test]
fn on_a_gpu_the_shader_scans_and_compacts_as_the_reference_does() {
    let (device, queue, adapter) = match device() {
        Ok(opened) => opened,
        Err(why) => {
            eprintln!("skipped: no GPU adapter to run the scan on: {why}");
            return;
        }
    };
    eprintln!("ran adapter={adapter:?}");
    let scanner = Scanner::new(device, queue);

    for (length, keeps) in cases() {
        let values = values(length, keeps);
        assert_eq!(scanner.run(&values), Passed::expected(&values), "{length} values, keeping {keeps:?}, on {adapter}");
    }
}
