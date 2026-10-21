//! A frame as the renderer walks it at each `Render`: its description, the shape of each pass, and its targets made at
//! the size of its end.

use crate::frame::{Description, PassSpec, TargetSpec};
use crate::pass::PassName;
use crate::shape::{ColourFormat, DepthFormat, Shape};

/// A frame, checked and made: what it does, the shape of each of its passes, and its targets, each the size of its
/// end as it was last drawn.
pub(super) struct Walk {
    description: Description,
    shapes: Vec<Shape>,
    targets: Vec<wgpu::TextureView>,
    width: u32,
    height: u32,
}

impl Walk {
    /// The renderer's own main frame of a window whose surface is of `end`, its targets made at one pixel until the
    /// first frame's size is known.
    pub fn own(device: &wgpu::Device, end: ColourFormat) -> Self {
        let description = Description::own();
        let shapes = description.shapes(end);
        let targets = made(device, &description.targets, 1, 1);

        Self { description, shapes, targets, width: 1, height: 1 }
    }

    /// How many passes it runs.
    pub fn passes(&self) -> u32 {
        // A frame's passes are a handful, far below what a `u32` counts.
        u32::try_from(self.description.passes.len()).unwrap_or(u32::MAX)
    }

    /// Each pass in the order it runs, with its shape.
    pub fn steps(&self) -> impl Iterator<Item = (&PassSpec, Shape)> {
        self.description.passes.iter().zip(self.shapes.iter().copied())
    }

    /// The shape of the pass `name`: none when it has no such pass.
    pub fn shape_of(&self, name: PassName) -> Option<Shape> {
        self.steps().find(|(pass, _)| pass.name == name).map(|(_, shape)| shape)
    }

    /// The target at `at`, which its check found among its targets.
    pub fn target(&self, at: u32) -> &wgpu::TextureView {
        &self.targets[at as usize]
    }

    /// Makes its targets again when its end is drawn at another size, so each is the end's size.
    pub fn fit(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let resized = (width, height) != (self.width, self.height);
        if resized {
            self.targets = made(device, &self.description.targets, width, height);
            (self.width, self.height) = (width, height);
        }
    }
}

/// Each of `targets`, made `width` by `height`.
fn made(device: &wgpu::Device, targets: &[TargetSpec], width: u32, height: u32) -> Vec<wgpu::TextureView> {
    targets
        .iter()
        .map(|target| {
            let format = match target {
                TargetSpec::Depth => DepthFormat::Depth32Float.wgpu(),
            };
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        })
        .collect()
}
