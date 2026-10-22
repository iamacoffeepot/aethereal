//! A frame as the renderer walks it at each `Render`: its description, what each pass does, made, with the shape of
//! each pass that draws and what it reads, and its targets made at the sizes their scales make of its end's.

use super::read_layout;
use crate::effect::Read;
use crate::frame::{CreateFrame, DepthUse, Does, Frame, FrameId, Load, PassRefused, Size, Target, TargetSpec};
use crate::pass::PassName;
use crate::placement::{Fits, Resource};
use crate::shape::{ColourFormat, Samples, Shape};
use aethereal_window::WindowId;

/// A frame, checked and made: what it does, where it ends, what each pass does with what it was made with, the
/// textures and the storage its effects bind, and its targets, each of the size its scale makes of its end's as it was
/// last drawn, `size`.
pub(super) struct Walk {
    create: CreateFrame,
    end: End,
    steps: Vec<Step>,
    binds: Vec<u64>,
    stored: Vec<(PassName, u64)>,
    targets: Vec<Image>,
    size: Size,
}

/// One target of a frame, made: one texture, which a pass draws into and a later pass reads, or for a target of
/// floats of four samples, the four a pass draws into and the one each pass that draws into it resolves them into as
/// it ends, which a later pass reads (ADR-0025). A depth of four samples is one texture, never read.
pub(super) enum Image {
    Single(wgpu::TextureView),
    Resolved { samples: wgpu::TextureView, resolved: wgpu::TextureView },
}

impl Image {
    /// What a pass draws into, or tests and writes as its depth.
    pub const fn drawn(&self) -> &wgpu::TextureView {
        match self {
            Self::Single(view) | Self::Resolved { samples: view, .. } => view,
        }
    }

    /// What a pass that draws into it resolves its samples into as it ends: none for one texture.
    pub const fn resolved(&self) -> Option<&wgpu::TextureView> {
        match self {
            Self::Single(_) => None,
            Self::Resolved { resolved, .. } => Some(resolved),
        }
    }

    /// What a later pass reads: the one texture, or the samples resolved.
    pub const fn read(&self) -> &wgpu::TextureView {
        match self {
            Self::Single(view) | Self::Resolved { resolved: view, .. } => view,
        }
    }
}

/// Where a frame ends: on its window, into the frame of the window's surface the chassis hands each `Render`, or on a
/// texture of its own, kept under the id `texture`.
pub(super) enum End {
    Window(WindowId),
    Texture { texture: u64, view: wgpu::TextureView },
}

/// What one pass does, made, with the shape of a pass that draws.
pub(super) enum Step {
    /// The placements that join it draw into `into`, using depth as `depth` says, beginning from what `load` says, each
    /// with a program made against `fits`, binding the targets it reads in `read`, bound at group 0 when it reads any.
    Draw { into: Target, depth: DepthUse, load: Load, fits: Fits, read: ReadGroup },
    /// An effect drawn over the whole of its target.
    Effect { effect: Effected, shape: Shape },
    /// The dispatches that join it run, and it draws into nothing.
    Compute,
}

/// An effect pass, made: the effect's pipeline, the targets it reads bound in its first group, the bind groups of
/// what it binds besides, and what it draws into, beginning from what `load` says.
pub(super) struct Effected {
    pub pipeline: wgpu::RenderPipeline,
    pub read: ReadGroup,
    pub groups: Vec<wgpu::BindGroup>,
    pub into: Target,
    pub load: Load,
}

/// The targets a pass reads, by their places among the frame's targets, and the bind group they are bound in through
/// `layout`, made again when the targets are.
pub(super) struct ReadGroup {
    pub reads: Vec<u32>,
    pub layout: wgpu::BindGroupLayout,
    pub group: wgpu::BindGroup,
}

impl ReadGroup {
    /// The targets at `reads` of `targets`, bound in order through `layout`.
    pub fn new(device: &wgpu::Device, layout: wgpu::BindGroupLayout, reads: Vec<u32>, targets: &[Image]) -> Self {
        let group = read_group(device, &layout, &reads, targets);

        Self { reads, layout, group }
    }
}

/// What an effect pass is made from as the frame is made: what it does, whether an earlier pass wrote what it draws
/// into, and the frame's targets.
#[derive(Clone, Copy)]
pub(super) struct Making<'a> {
    pub does: &'a Does,
    pub written: bool,
    pub targets: &'a [Image],
}

impl Walk {
    /// The frame `create`, which ends on `end`, of `format` and `size`, its targets made at the sizes their scales make
    /// of it and each effect pass made by `effect`: refused as `effect` refuses a pass, at its place.
    pub fn new(
        device: &wgpu::Device,
        create: CreateFrame,
        (end, format): (End, ColourFormat),
        size: Size,
        mut effect: impl FnMut(Making<'_>) -> Result<Effected, PassRefused>,
    ) -> Result<Self, (u32, PassRefused)> {
        let targets = made(device, &create.targets, size);
        let mut written: Vec<Target> = Vec::new();
        let mut steps = Vec::with_capacity(create.passes.len());
        for (pass, at) in create.passes.iter().zip(0..) {
            let step = match &pass.does {
                Does::Draw { into, depth, load, reads } => {
                    // The check found each target it reads among the frame's, and none the end.
                    let at: Vec<u32> = reads.iter().filter_map(|read| texture_of(*read)).collect();
                    let kinds: Vec<Read> = at.iter().map(|at| create.targets[*at as usize].read()).collect();
                    let read = ReadGroup::new(device, read_layout(device, &kinds), at, &targets);
                    let fits = Fits { shape: create.shape(*into, *depth, format), reads: kinds };
                    Step::Draw { into: *into, depth: *depth, load: *load, fits, read }
                }
                Does::Effect { into, .. } => {
                    let making = Making { does: &pass.does, written: written.contains(into), targets: &targets };
                    let made = effect(making).map_err(|why| (at, why))?;
                    Step::Effect { effect: made, shape: create.shape(*into, DepthUse::Off, format) }
                }
                Does::Compute => Step::Compute,
            };
            steps.push(step);
            written.extend(pass.does.writes());
        }
        let bound: Vec<(PassName, Resource)> = create
            .passes
            .iter()
            .flat_map(|pass| match &pass.does {
                Does::Effect { bindings, .. } => bindings.iter().map(|bind| (pass.name, bind.to)).collect(),
                Does::Draw { .. } | Does::Compute => Vec::new(),
            })
            .collect();
        let binds = bound
            .iter()
            .filter_map(|(_, to)| match to {
                Resource::Texture(texture) => Some(texture.id),
                Resource::Uniforms(_) | Resource::Storage(_) | Resource::Sampler(_) => None,
            })
            .collect();
        let stored = bound
            .iter()
            .filter_map(|(pass, to)| match to {
                Resource::Storage(storage) => Some((*pass, storage.id)),
                Resource::Uniforms(_) | Resource::Texture(_) | Resource::Sampler(_) => None,
            })
            .collect();

        Ok(Self { create, end, steps, binds, stored, targets, size })
    }

    /// The texture it ends on, by its id: none for a frame that ends on a window.
    pub const fn texture(&self) -> Option<u64> {
        match self.end {
            End::Window(_) => None,
            End::Texture { texture, .. } => Some(texture),
        }
    }

    /// The frame it is, kept under `id`: the main frame of the window it ends on, or the frame of that id.
    pub const fn frame(&self, id: u64) -> Frame {
        match self.end {
            End::Window(window) => Frame::Main(window),
            End::Texture { .. } => Frame::Texture(FrameId { id }),
        }
    }

    /// What it draws its end into: its own texture, or for a frame that ends on a window, the frame of the window's
    /// surface among `windows`, those handed this `Render`, each beside its window. None when its window was handed
    /// none.
    pub fn end<'a>(&'a self, windows: &'a [(WindowId, wgpu::TextureView)]) -> Option<&'a wgpu::TextureView> {
        match &self.end {
            End::Window(shown) => windows.iter().find(|(window, _)| window == shown).map(|(_, view)| view),
            End::Texture { view, .. } => Some(view),
        }
    }

    /// The textures its effects bind, by their ids.
    pub fn binds(&self) -> &[u64] {
        &self.binds
    }

    /// The storage its effects bind, by its id, each beside the pass that binds it, which reads it.
    pub fn stored(&self) -> &[(PassName, u64)] {
        &self.stored
    }

    /// How many passes it runs.
    pub fn passes(&self) -> u32 {
        // A frame's passes are a handful, far below what a `u32` counts.
        u32::try_from(self.steps.len()).unwrap_or(u32::MAX)
    }

    /// Each pass in the order it runs: its name, and what it does, made.
    pub fn steps(&self) -> impl Iterator<Item = (PassName, &Step)> {
        self.create.passes.iter().zip(&self.steps).map(|(pass, step)| (pass.name, step))
    }

    /// What a program that joins the pass `name` is made against: none when it has no such pass, or it is a compute
    /// pass, which draws into nothing. An effect pass reads nothing a program joining it would.
    pub fn fits(&self, name: PassName) -> Option<Fits> {
        match self.steps().find(|(pass, _)| *pass == name)? {
            (_, Step::Draw { fits, .. }) => Some(fits.clone()),
            (_, Step::Effect { shape, .. }) => Some(Fits { shape: *shape, reads: Vec::new() }),
            (_, Step::Compute) => None,
        }
    }

    /// Whether it has a compute pass `name`.
    pub fn computes(&self, name: PassName) -> bool {
        self.steps().any(|(pass, step)| pass == name && matches!(step, Step::Compute))
    }

    /// The target at `at`, which its check found among its targets.
    pub fn target(&self, at: u32) -> &Image {
        &self.targets[at as usize]
    }

    /// Makes its targets again when its end is drawn at another size, `size`, so each is the size its scale makes of
    /// it, both textures of one of four samples, and the bind groups its passes read them through.
    pub fn fit(&mut self, device: &wgpu::Device, size: Size) {
        if size == self.size {
            return;
        }
        self.targets = made(device, &self.create.targets, size);
        self.size = size;
        for step in &mut self.steps {
            let read = match step {
                Step::Draw { read, .. } | Step::Effect { effect: Effected { read, .. }, .. } => read,
                Step::Compute => continue,
            };
            read.group = read_group(device, &read.layout, &read.reads, &self.targets);
        }
    }
}

/// The place of `target` among a frame's targets: none for its end.
const fn texture_of(target: Target) -> Option<u32> {
    match target {
        Target::Texture(at) => Some(at),
        Target::End => None,
    }
}

/// The bind group of the targets at `reads` of `targets`, in order, each as a later pass reads it, through `layout`.
fn read_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    reads: &[u32],
    targets: &[Image],
) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry<'_>> = (0..)
        .zip(reads)
        .map(|(binding, at)| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(targets[*at as usize].read()),
        })
        .collect();

    device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries })
}

/// Each of `targets`, made at the size its scale makes of the end's, `end`, to draw into and read: one of floats of
/// four samples as the four and the one they are resolved into, and one of depth of four as the four alone, as it is
/// never read.
fn made(device: &wgpu::Device, targets: &[TargetSpec], end: Size) -> Vec<Image> {
    targets
        .iter()
        .map(|target| {
            let format = target.colour().map_or(wgpu::TextureFormat::Depth32Float, ColourFormat::wgpu);
            let Size { width, height } = end.scaled(target.scale);
            let texture = |sample_count, usage| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: None,
                        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
            let read = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            let drawn = wgpu::TextureUsages::RENDER_ATTACHMENT;
            let four = Samples::Four.count();
            match (target.colour(), target.samples) {
                (_, Samples::One) => Image::Single(texture(1, read)),
                (Some(_), Samples::Four) => {
                    Image::Resolved { samples: texture(four, drawn), resolved: texture(1, read) }
                }
                (None, Samples::Four) => Image::Single(texture(four, drawn)),
            }
        })
        .collect()
}
