//! The atlas's texture: one `Rgba8` texture, not sRGB, as a distance is read as it was written, owned by the render
//! root, with the group the glyph program binds it in. Made again larger, or packed again, as the atlas's bookkeeping
//! says, and written a glyph at a time as each glyph's job finishes (ADR-0052).

use super::{Arranged, Slot};
use crate::stats;

/// The atlas's texture, its view, and the group binding it with the sampler the glyph program reads it through.
pub struct AtlasTexture {
    texture: wgpu::Texture,
    group: wgpu::BindGroup,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl AtlasTexture {
    /// An empty texture `side` texels a side, bound by `layout` with `sampler`.
    #[must_use]
    pub fn new(device: &wgpu::Device, side: u32, layout: wgpu::BindGroupLayout, sampler: wgpu::Sampler) -> Self {
        let texture = made(device, side);
        let group = group(device, &layout, &texture, &sampler);

        Self { texture, group, layout, sampler }
    }

    /// The bytes of the texture as it is made now (ADR-0053).
    #[must_use]
    pub fn bytes(&self) -> u64 {
        stats::texture_bytes(&self.texture)
    }

    /// The group the glyph program binds the atlas in, group 0.
    #[must_use]
    pub const fn group(&self) -> &wgpu::BindGroup {
        &self.group
    }

    /// Changes the texture as `arranged` says: made again, at its side, with what it held copied over, or with the
    /// glyphs kept copied to where they lie now.
    pub fn arrange(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, arranged: &Arranged) {
        let (side, moves) = match arranged {
            Arranged::Kept => return,
            Arranged::Grown { side } => {
                let old = self.texture.size();
                let whole = super::Move { from: (0, 0), to: (0, 0), width: old.width, height: old.height };
                (*side, vec![whole])
            }
            Arranged::Packed { side, moves } => (*side, moves.clone()),
        };
        let texture = made(device, side);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        for moved in moves {
            let at = |(x, y): (u32, u32), texture| wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            };
            let extent = wgpu::Extent3d { width: moved.width, height: moved.height, depth_or_array_layers: 1 };
            encoder.copy_texture_to_texture(at(moved.from, &self.texture), at(moved.to, &texture), extent);
        }
        queue.submit([encoder.finish()]);
        self.group = group(device, &self.layout, &texture, &self.sampler);
        self.texture = texture;
    }

    /// Writes a glyph's field, `rgb`, three bytes a texel row by row, where `slot` lies, its alpha opaque.
    pub fn write(&self, queue: &wgpu::Queue, slot: Slot, rgb: &[u8]) {
        if slot.field.is_empty() {
            return;
        }
        let rgba: Vec<u8> = rgb.chunks_exact(3).flat_map(|texel| [texel[0], texel[1], texel[2], u8::MAX]).collect();
        let (width, height) = (slot.field.width, slot.field.height);
        let at = wgpu::TexelCopyTextureInfo {
            texture: &self.texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x: slot.x, y: slot.y, z: 0 },
            aspect: wgpu::TextureAspect::All,
        };
        let layout =
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(width * 4), rows_per_image: Some(height) };
        queue.write_texture(at, &rgba, layout, wgpu::Extent3d { width, height, depth_or_array_layers: 1 });
    }
}

/// An empty atlas texture `side` texels a side, sampled, written, and copied to and from.
fn made(device: &wgpu::Device, side: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// The group binding `texture` at 0 and `sampler` at 1, by `layout`.
fn group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &wgpu::Texture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
        ],
    })
}
