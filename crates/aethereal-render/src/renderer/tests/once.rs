//! A dispatch asked to run once (ADR-0026), on the device that draws nothing: run in the first cycle that draws its
//! frame and never again, answered as never run when its frame goes for good, and counted among its pass's dispatches
//! while it waits.

use super::*;
use crate::dispatch::{Dispatch, Invocations};

/// A compute program that counts up each element of the field it writes, in workgroups of 64.
const COUNT: &str = "
@group(0) @binding(0) var<storage, read_write> field: array<u32>;

@compute @workgroup_size(64)
fn count(@builtin(global_invocation_id) at: vec3<u32>) {
    if at.x < arrayLength(&field) {
        field[at.x] = field[at.x] + 1u;
    }
}
";

/// [`COUNT`], made and kept in `kept`, with a field of 64 elements it writes, both made by [`MAKER`]: their ids.
fn counting(gpu: &Gpu, kept: &mut Kept) -> (ComputeId, StorageId) {
    let create = CreateCompute {
        wgsl: blob(),
        entry: String::from("count"),
        bindings: vec![Binding {
            group: 0,
            binding: 0,
            kind: BindingKind::Storage { bytes: 4, access: Access::Write },
        }],
    };
    let Ok(checked) = compute::check(&create, COUNT, &gpu.device().limits()) else {
        panic!("the counting program is checked");
    };
    let compute = kept.ids.next();
    kept.computes.keep(compute, MAKER, compute_pipeline(gpu.device(), &create, COUNT, checked));
    let storage = kept.ids.next();
    let buffer = upload(gpu, &[0; 256], STORAGE_USAGE);
    kept.storage.keep(storage, MAKER, Storage { buffer, size: 256 });

    (ComputeId { id: compute }, StorageId { id: storage })
}

/// A dispatch of `compute` over each element of `field`, which it writes, in the pass [`Compute`] of `frame`.
fn counts(frame: Frame, (compute, field): (ComputeId, StorageId)) -> CreateDispatch {
    let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Storage(field) }];

    CreateDispatch { join: Dispatch::to::<Compute>(frame, compute, bindings), size: Invocations::PerElement(field) }
}

/// The dispatch `create` asks for, asked to run once.
fn run_once(create: CreateDispatch) -> RunDispatch {
    RunDispatch { join: create.join, size: create.size }
}

/// Asks `kept` to run `run` once, for [`MAKER`], as the renderer does: checked and kept until it runs. Its id.
fn asked_once(device: &wgpu::Device, kept: &mut Kept, run: RunDispatch) -> u64 {
    let Ok(made) = kept.checked_dispatch(device, &run.dispatch(), MAKER) else {
        panic!("the dispatch to run once is made");
    };
    let id = kept.ids.next();
    kept.once.keep(id, MAKER, made);
    kept.reorder();

    id
}

/// What `kept` runs in one cycle that draws into `windows`, submitted: the dispatches asked to run once that ran, as
/// the renderer answers them, dropped from what is kept.
fn cycle(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    kept: &mut Kept,
    windows: &[(WindowId, wgpu::TextureView)],
) -> Vec<u64> {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let once = kept.run_all(&mut encoder, windows, None, &mut Tally::Off);
    queue.submit([encoder.finish()]);
    kept.ran_once(&once);

    once
}

/// The window `window`'s frame, as handed a `Render`.
fn handed(device: &wgpu::Device, window: WindowId) -> [(WindowId, wgpu::TextureView); 1] {
    let [(_, view)] = surface(device);

    [(window, view)]
}

#[test]
fn a_dispatch_run_once_runs_in_the_first_cycle_that_draws_its_frame_and_never_again() {
    // Catches one run in a cycle that drew another window and not its own, answered while it has not run, kept and
    // run again in every cycle after, as a dispatch made until released is, or forgotten without running.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue.clone());
    let second = nth_window(1);
    let mut kept = kept_for(&device, &[window(), second]);
    let counting = counting(&gpu, &mut kept);
    let run = run_once(counts(Frame::Main(second), counting));
    let once = asked_once(&device, &mut kept, run);

    let undrawn = cycle(&device, &queue, &mut kept, &handed(&device, window()));
    let drawn = cycle(&device, &queue, &mut kept, &handed(&device, second));
    let after = cycle(&device, &queue, &mut kept, &handed(&device, second));

    assert_eq!(undrawn, Vec::<u64>::new());
    assert_eq!(drawn, vec![once]);
    assert_eq!(after, Vec::<u64>::new());
}

#[test]
fn a_dispatch_run_once_whose_frame_goes_for_good_never_runs_and_one_whose_frame_is_replaced_waits() {
    // Catches one left waiting for a window that closed or a frame released, which no cycle draws again, or one given
    // up as soon as its window's main frame is replaced by one without its pass, which it waits through.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue.clone());
    let mut kept = kept(&device);
    let counting = counting(&gpu, &mut kept);
    let main = PassSpec::of::<Main>(Does::Draw {
        into: Target::End,
        depth: DepthUse::Off,
        load: Load::Clear,
        reads: Vec::new(),
    });
    let onto = CreateFrame {
        ends: Ends::Texture { size: Size { width: 4, height: 4 }, format: TextureFormat::Rgba8 },
        targets: Vec::new(),
        passes: vec![PassSpec::of::<Compute>(Does::Compute), main.clone()],
    };
    let Ok(Framed::Texture { frame, .. }) = kept.frame(&device, onto, MAKER) else {
        panic!("the frame onto a texture is made");
    };
    let on_window = asked_once(&device, &mut kept, run_once(counts(Frame::Main(window()), counting)));
    let on_texture = asked_once(&device, &mut kept, run_once(counts(Frame::Texture(frame), counting)));
    let uncomputed = CreateFrame { ends: Ends::Window(window()), targets: Vec::new(), passes: vec![main] };
    assert!(matches!(kept.frame(&device, uncomputed, MAKER), Ok(Framed::Made(_))));

    let replaced = kept.unjoined();
    assert_eq!(kept.release(Release::Frame(frame), MAKER), Owned::Mine(()));
    let released = kept.unjoined();
    assert!(kept.closed(window()));
    let closed = kept.unjoined();
    let ran = cycle(&device, &queue, &mut kept, &[]);

    assert_eq!(replaced, Vec::new());
    assert_eq!(released, vec![(on_texture, Unrun::Released)]);
    assert_eq!(closed, vec![(on_window, Unrun::Closed)]);
    assert_eq!(ran, Vec::<u64>::new());
}

#[test]
fn a_dispatch_run_once_counts_among_its_pass_s_dispatches_until_it_has_run() {
    // Catches one left out of the order of what writes storage while it waits, so a dispatch writing the same storage
    // in its pass is made and the two run in no order, or one still counted once it has run and gone.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue.clone());
    let mut kept = kept(&device);
    let counting = counting(&gpu, &mut kept);
    let writes = counts(Frame::Main(window()), counting);
    asked_once(&device, &mut kept, run_once(writes.clone()));

    let waiting = dispatch::check(&writes, MAKER, &kept).map(drop);
    cycle(&device, &queue, &mut kept, &handed(&device, window()));
    let gone = dispatch::check(&writes, MAKER, &kept).map(drop);

    assert_eq!(waiting, Err(DispatchRefused::Written));
    assert_eq!(gone, Ok(()));
}
