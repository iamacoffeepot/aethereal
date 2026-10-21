//! A frame as the renderer walks it at each `Render`: its description, the shape of each pass, what each pass does,
//! made, and its targets made at the size of its end.

use crate::frame::{CreateFrame, DepthUse, Does, Load, PassRefused, Target, TargetSpec};
use crate::pass::PassName;
use crate::placement::Resource;
use crate::shape::{ColourFormat, Shape};

/// A frame, checked and made: what it does, where it ends, the shape of each of its passes, what each pass does with
/// what it was made with, the textures its effects bind, and its targets, each the size of its end as it was last
/// drawn.
pub(super) struct Walk {
    create: CreateFrame,
    end: End,
    shapes: Vec<Shape>,
    steps: Vec<Step>,
    binds: Vec<u64>,
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

/// What one pass does, made.
pub(super) enum Step {
    /// The placements that join it draw into `into`, using depth as `depth` says, beginning from what `load` says.
    Draw { into: Target, depth: DepthUse, load: Load },
    /// An effect drawn over the whole of its target.
    Effect(Effected),
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

/// What a pass is made from as the frame is made: what it does, whether an earlier pass wrote what it draws into, and
/// the frame's targets.
#[derive(Clone, Copy)]
pub(super) struct Making<'a> {
    pub does: &'a Does,
    pub written: bool,
    pub targets: &'a [wgpu::TextureView],
}

impl Walk {
    /// The frame `create`, which ends on `end`, of `format` and `width` by `height`, its targets made at that size and
    /// each pass made by `step`: refused as `step` refuses a pass, at its place.
    pub fn new(
        device: &wgpu::Device,
        create: CreateFrame,
        (end, format): (End, ColourFormat),
        (width, height): (u32, u32),
        mut step: impl FnMut(Making<'_>) -> Result<Step, PassRefused>,
    ) -> Result<Self, (u32, PassRefused)> {
        let shapes = create.shapes(format);
        let targets = made(device, &create.targets, width, height);
        let mut written: Vec<Target> = Vec::new();
        let mut steps = Vec::with_capacity(create.passes.len());
        for (pass, at) in create.passes.iter().zip(0..) {
            let into = pass.does.writes();
            let making = Making { does: &pass.does, written: written.contains(&into), targets: &targets };
            steps.push(step(making).map_err(|why| (at, why))?);
            written.push(into);
        }
        let binds = create
            .passes
            .iter()
            .flat_map(|pass| match &pass.does {
                Does::Effect { bindings, .. } => bindings.as_slice(),
                Does::Draw { .. } => &[],
            })
            .filter_map(|bind| match bind.to {
                Resource::Texture(texture) => Some(texture.id),
                Resource::Uniforms(_) | Resource::Storage(_) | Resource::Sampler(_) => None,
            })
            .collect();

        Ok(Self { create, end, shapes, steps, binds, targets, width, height })
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

    /// How many passes it runs.
    pub fn passes(&self) -> u32 {
        // A frame's passes are a handful, far below what a `u32` counts.
        u32::try_from(self.steps.len()).unwrap_or(u32::MAX)
    }

    /// Each pass in the order it runs: its name, its shape, and what it does, made.
    pub fn steps(&self) -> impl Iterator<Item = (PassName, Shape, &Step)> {
        self.create
            .passes
            .iter()
            .zip(&self.shapes)
            .zip(&self.steps)
            .map(|((pass, shape), step)| (pass.name, *shape, step))
    }

    /// The shape of the pass `name`: none when it has no such pass.
    pub fn shape_of(&self, name: PassName) -> Option<Shape> {
        self.steps().find(|(pass, ..)| *pass == name).map(|(_, shape, _)| shape)
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
            if let Step::Effect(effect) = step {
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
