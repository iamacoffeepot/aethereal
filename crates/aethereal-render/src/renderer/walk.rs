//! A frame as the renderer walks it at each `Render`: its description, what each pass does, made, with the shape of
//! each pass that draws, and its targets made at the size of its end.

use crate::frame::{CreateFrame, DepthUse, Does, Load, PassRefused, Target, TargetSpec};
use crate::pass::PassName;
use crate::placement::Resource;
use crate::shape::{ColourFormat, Shape};

/// A frame, checked and made: what it does, where it ends, what each pass does with what it was made with, the
/// textures and the storage its effects bind, and its targets, each the size of its end as it was last drawn.
pub(super) struct Walk {
    create: CreateFrame,
    end: End,
    steps: Vec<Step>,
    binds: Vec<u64>,
    stored: Vec<(PassName, u64)>,
    targets: Vec<wgpu::TextureView>,
    width: u32,
    height: u32,
}

/// Where a frame ends: on its window, whose surface gives each `Render` what it draws into, or on a texture of its
/// own, kept under the id `texture`.
pub(super) enum End {
    Window,
    Texture { texture: u64, view: wgpu::TextureView },
}

/// What one pass does, made, with the shape of a pass that draws.
pub(super) enum Step {
    /// The placements that join it draw into `into`, using depth as `depth` says, beginning from what `load` says.
    Draw { into: Target, depth: DepthUse, load: Load, shape: Shape },
    /// An effect drawn over the whole of its target.
    Effect { effect: Effected, shape: Shape },
    /// The dispatches that join it run, and it draws into nothing.
    Compute,
}

/// An effect pass, made: the effect's pipeline, the targets it reads and the bind group they are bound in, made again
/// when the targets are, the bind groups of what it binds besides, and what it draws into, beginning from what `load`
/// says.
pub(super) struct Effected {
    pub pipeline: wgpu::RenderPipeline,
    pub reads: Vec<u32>,
    pub layout: wgpu::BindGroupLayout,
    pub read: wgpu::BindGroup,
    pub groups: Vec<wgpu::BindGroup>,
    pub into: Target,
    pub load: Load,
}

/// What an effect pass is made from as the frame is made: what it does, whether an earlier pass wrote what it draws
/// into, and the frame's targets.
#[derive(Clone, Copy)]
pub(super) struct Making<'a> {
    pub does: &'a Does,
    pub written: bool,
    pub targets: &'a [wgpu::TextureView],
}

impl Walk {
    /// The frame `create`, which ends on `end`, of `format` and `width` by `height`, its targets made at that size and
    /// each effect pass made by `effect`: refused as `effect` refuses a pass, at its place.
    pub fn new(
        device: &wgpu::Device,
        create: CreateFrame,
        (end, format): (End, ColourFormat),
        (width, height): (u32, u32),
        mut effect: impl FnMut(Making<'_>) -> Result<Effected, PassRefused>,
    ) -> Result<Self, (u32, PassRefused)> {
        let targets = made(device, &create.targets, width, height);
        let mut written: Vec<Target> = Vec::new();
        let mut steps = Vec::with_capacity(create.passes.len());
        for (pass, at) in create.passes.iter().zip(0..) {
            let step = match &pass.does {
                Does::Draw { into, depth, load } => {
                    Step::Draw { into: *into, depth: *depth, load: *load, shape: create.shape(*into, *depth, format) }
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

        Ok(Self { create, end, steps, binds, stored, targets, width, height })
    }

    /// The texture it ends on, by its id: none for a frame that ends on a window.
    pub const fn texture(&self) -> Option<u64> {
        match self.end {
            End::Window => None,
            End::Texture { texture, .. } => Some(texture),
        }
    }

    /// What it draws its end into, given the window's this `Render` for a frame that ends on a window.
    pub const fn end<'a>(&'a self, window: &'a wgpu::TextureView) -> &'a wgpu::TextureView {
        match &self.end {
            End::Window => window,
            End::Texture { view, .. } => view,
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

    /// The shape of the pass `name`: none when it has no such pass, or it is a compute pass, which draws into nothing.
    pub fn shape_of(&self, name: PassName) -> Option<Shape> {
        match self.steps().find(|(pass, _)| *pass == name)? {
            (_, Step::Draw { shape, .. } | Step::Effect { shape, .. }) => Some(*shape),
            (_, Step::Compute) => None,
        }
    }

    /// Whether it has a compute pass `name`.
    pub fn computes(&self, name: PassName) -> bool {
        self.steps().any(|(pass, step)| pass == name && matches!(step, Step::Compute))
    }

    /// The target at `at`, which its check found among its targets.
    pub fn target(&self, at: u32) -> &wgpu::TextureView {
        &self.targets[at as usize]
    }

    /// Makes its targets again when its end is drawn at another size, so each is the end's size, and the bind groups
    /// its effects read them through.
    pub fn fit(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let resized = (width, height) != (self.width, self.height);
        if !resized {
            return;
        }
        self.targets = made(device, &self.create.targets, width, height);
        (self.width, self.height) = (width, height);
        for step in &mut self.steps {
            if let Step::Effect { effect, .. } = step {
                effect.read = read_group(device, &effect.layout, &effect.reads, &self.targets);
            }
        }
    }
}

/// The bind group of the targets at `reads` of `targets`, in order, through `layout`.
pub(super) fn read_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    reads: &[u32],
    targets: &[wgpu::TextureView],
) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry<'_>> = (0..)
        .zip(reads)
        .map(|(binding, at)| wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(&targets[*at as usize]),
        })
        .collect();

    device.create_bind_group(&wgpu::BindGroupDescriptor { label: None, layout, entries: &entries })
}

/// Each of `targets`, made `width` by `height`, to draw into and read.
fn made(device: &wgpu::Device, targets: &[TargetSpec], width: u32, height: u32) -> Vec<wgpu::TextureView> {
    targets
        .iter()
        .map(|target| {
            let format = target.colour().map_or(wgpu::TextureFormat::Depth32Float, ColourFormat::wgpu);
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        })
        .collect()
}
