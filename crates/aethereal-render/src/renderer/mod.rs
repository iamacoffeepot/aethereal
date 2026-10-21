//! The renderer: the root that owns the GPU, makes what actors ask it for, and draws each frame (ADR-0021).

use crate::Gpu;
use crate::made::{Ids, Made, Makers, Released};
use crate::mesh::{self, Counted, CreateMesh, MeshId, Meshed, VertexLayout};
use crate::texture::{self, CreateTexture, TextureId, Textured};
use aethereal_actor::{
    Actor, Beginning, Carries, ChildOf, Ended, Ending, Fault, Given, Handle, Handles, Level, On, Protocol, Render,
    Resource, ToSender, Top, Turn,
};
use aethereal_data::{ActorId, Bound, Kind, KindName, Plain};

/// The renderer, the one actor that touches the GPU. Actors ask it through [`Rendering`].
#[derive(Bound)]
pub struct Renderer {
    gpu: Handle<Gpu>,
    kept: Handle<Kept>,
}

impl Kind for Renderer {
    const NAME: KindName = KindName::new("aether.render");
}

/// The renderer is a root, made by the chassis that opened the GPU (ADR-0021, ADR-0022).
impl ChildOf for Renderer {
    type Parent = Top;
}

/// What asking the renderer at its typed path, `path::<Renderer>(())`, gets: as asking it through [`Rendering`] does
/// (ADR-0022).
impl Carries<CreateMesh> for Renderer {
    type Answers = (Meshed,);
}

impl Carries<CreateTexture> for Renderer {
    type Answers = (Textured,);
}

/// What the renderer is made from: the GPU the chassis opened.
#[derive(Bound)]
pub struct RendererConfig {
    pub gpu: Given<Gpu>,
}

impl Kind for RendererConfig {
    const NAME: KindName = KindName::new("aether.render.config");
}

/// What an actor asks the renderer: to make a mesh or a texture, or to release one it made.
pub struct Rendering;

impl Protocol for Rendering {
    type Kinds = (CreateMesh, CreateTexture, Release);
}

impl Carries<CreateMesh> for Rendering {
    type Answers = (Meshed,);
}

impl Carries<CreateTexture> for Rendering {
    type Answers = (Textured,);
}

impl Carries<Release> for Rendering {
    type Answers = ();
}

/// Releases a mesh or a texture before its maker ends. Only its maker may; what another asks is logged and left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Release {
    Mesh(MeshId),
    Texture(TextureId),
}

impl Kind for Release {
    const NAME: KindName = KindName::new("aether.render.release");
}

/// What the renderer has made, and who made each.
#[derive(Default)]
struct Kept {
    ids: Ids,
    makers: Makers,
    meshes: Made<Mesh>,
    textures: Made<Texture>,
}

impl Resource for Kept {}

/// A mesh on the GPU.
#[expect(dead_code, reason = "drawn once the renderer draws what actors place (ADR-0021)")]
struct Mesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    counted: Counted,
    layout: VertexLayout,
}

/// A texture on the GPU, and the view a program samples it through.
#[expect(dead_code, reason = "drawn with once programs bind textures (ADR-0021)")]
struct Texture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Actor for Renderer {
    type Config = RendererConfig;
    type Key = ();
    type Handles = (On<Render>, CreateMesh, CreateTexture, Release, Ended);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: RendererConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let gpu = turn.take(config.gpu)?;
        let kept = turn.keep(Kept::default())?;
        turn.subscribe::<Render>();

        Ok(Self { gpu, kept })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<On<Render>> for Renderer {
    type Sends = ();

    fn handle(&mut self, _: Render, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let gpu = turn.open(&mut self.gpu)?;
        // A frame the surface has no texture for is skipped; the next asks again.
        let wgpu::CurrentSurfaceTexture::Success(frame) = gpu.surface().get_current_texture() else {
            return Ok(());
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let attachment = wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
        };
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(attachment)],
            ..Default::default()
        }));
        gpu.queue().submit([encoder.finish()]);
        gpu.queue().present(frame);

        Ok(())
    }
}

impl Handles<CreateMesh> for Renderer {
    type Sends = (ToSender<Meshed>,);

    fn handle(&mut self, create: CreateMesh, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        // Copied out of the store: the bytes are read through the turn, as the GPU is opened through it.
        let vertices = turn.read(&create.vertices).to_vec();
        let indices = turn.read(&create.indices).to_vec();
        let counted = match mesh::check(&create.layout, &vertices, &indices) {
            Ok(counted) => counted,
            Err(why) => {
                turn.reply(&Meshed::Refused(why))?;
                return Ok(());
            }
        };

        let gpu = turn.open(&mut self.gpu)?;
        let mesh = Mesh {
            vertices: upload(gpu, &vertices, wgpu::BufferUsages::VERTEX),
            indices: upload(gpu, &indices, wgpu::BufferUsages::INDEX),
            counted,
            layout: create.layout,
        };
        let id = self.made(turn, |kept, id, maker| kept.meshes.keep(id, maker, mesh))?;
        turn.reply(&Meshed::Made(MeshId { id }))?;

        Ok(())
    }
}

impl Handles<CreateTexture> for Renderer {
    type Sends = (ToSender<Textured>,);

    fn handle(&mut self, create: CreateTexture, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let CreateTexture { width, height, format, pixels } = create;
        let pixels = turn.read(&pixels).to_vec();
        let most = turn.open(&mut self.gpu)?.device().limits().max_texture_dimension_2d;
        if let Err(why) = texture::check(width, height, format, &pixels, most) {
            turn.reply(&Textured::Refused(why))?;
            return Ok(());
        }

        let gpu = turn.open(&mut self.gpu)?;
        let size = wgpu::Extent3d { width, height, depth_or_array_layers: 1 };
        let made = gpu.device().create_texture(&wgpu::TextureDescriptor {
            label: None,
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: format.wgpu(),
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * format.bytes()),
            rows_per_image: Some(height),
        };
        gpu.queue().write_texture(made.as_image_copy(), &pixels, layout, size);
        let view = made.create_view(&wgpu::TextureViewDescriptor::default());
        let texture = Texture { texture: made, view };
        let id = self.made(turn, |kept, id, maker| kept.textures.keep(id, maker, texture))?;
        turn.reply(&Textured::Made(TextureId { id }))?;

        Ok(())
    }
}

impl Handles<Release> for Renderer {
    type Sends = ();

    fn handle(&mut self, release: Release, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let kept = turn.open(&mut self.kept)?;
        let released = match release {
            Release::Mesh(mesh) => kept.meshes.release(mesh.id, by).map(drop),
            Release::Texture(texture) => kept.textures.release(texture.id, by).map(drop),
        };
        match released {
            Released::Freed(()) => {}
            Released::NotOwner => turn.log(Level::Warn, format_args!("{release:?} was not released: another made it")),
            Released::Unknown => turn.log(Level::Warn, format_args!("{release:?} was not released: no such thing")),
        }

        Ok(())
    }
}

impl Handles<Ended> for Renderer {
    type Sends = ();

    fn handle(&mut self, ended: Ended, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let maker = ended.actor().id();
        let kept = turn.open(&mut self.kept)?;
        let watched = kept.makers.ended(maker);
        if watched {
            kept.meshes.release_all(maker);
            kept.textures.release_all(maker);
        }

        Ok(())
    }
}

impl Renderer {
    /// Keeps what was just made for the sender of the mail being handled, under a new id, and watches the sender
    /// when it is the first thing it has made, so what it made is released when it ends. Answers the id.
    fn made<S>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        keep: impl FnOnce(&mut Kept, u64, ActorId),
    ) -> Result<u64, Fault> {
        let maker = turn.sender();
        let kept = turn.open(&mut self.kept)?;
        let id = kept.ids.next();
        keep(kept, id, maker.id());
        let first = kept.makers.made(maker.id());
        if first {
            turn.watch(maker);
        }

        Ok(id)
    }
}

/// A buffer of `usage` on the GPU, holding `bytes`.
fn upload(gpu: &Gpu, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    let buffer = gpu.device().create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes.len() as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    gpu.queue().write_buffer(&buffer, 0, bytes);

    buffer
}
