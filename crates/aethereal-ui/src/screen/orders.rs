//! The mail a screen's library sends (ADR-0056): what each of its turns decided, in order, sent through the screen's
//! turn by [`Orders::send`]. What a send carries as a blob is put there, so the library's decisions are plain values.
//! A tooltip's wait is a `Sleep` job the turn runs (ADR-0018), timed from the turn's step: a job is mail to no actor, so
//! no send lists it, and it builds only for a screen that handles its answer, as [`handles!`](crate::handles!) says.
//! A child the screen places is told its size through the reference the screen made it with, as any actor that takes
//! [`Resize`] is reached, so one send serves every child of every type.

use super::input::ImeAsked;
use super::layers::{Instancing, Placing, TextPlacing, Texting};
use super::pictures::{Picturing, Texturing};
use super::popups::Tipping;
use super::program::{Asked, Meshing, MetricsAsked, Programming, Uniforming, Whitening};
use crate::child::{Resize, Resizes};
use crate::draw::{FRAME_BYTES, PLACE, SHADE, corner_layout, corners, fill_layout, record_layout, triangles};
use crate::pixels::Pixels;
use aethereal_actor::{Answer, DependsOn, Fault, Level, ProtocolRef, Receives, Sleep, Slept, Turn, path};
use aethereal_render::draw_list::{DrawList, Entry};
use aethereal_render::instances::Instanced;
use aethereal_render::instances::{CreateInstances, InstancesId, WriteInstances};
use aethereal_render::mesh::Meshed;
use aethereal_render::mesh::{CreateMesh, VertexLayout};
use aethereal_render::placement::Place;
use aethereal_render::placement::Placed;
use aethereal_render::program::Programmed;
use aethereal_render::program::{
    Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance,
};
use aethereal_render::shape::Shape;
use aethereal_render::text::{CreateText, Font, MoveText, PlaceText, ReadMetrics, WriteText};
use aethereal_render::text::{FontMetrics, TextMade, TextPlaced};
use aethereal_render::texture::Textured;
use aethereal_render::texture::{CreateTexture, TextureFormat};
use aethereal_render::uniforms::Uniformed;
use aethereal_render::uniforms::{CreateUniforms, UniformsId, WriteUniforms};
use aethereal_render::{Release, Renderer};
use aethereal_window::{AllowIme, ImeAllowed, Windows};
use core::time::Duration;

/// A screen's type, as its library's orders are sent on its behalf: it depends on the renderer and the windows, found
/// at `R` and `W` in its `Depends`, which the compiler infers, and handles the answer to each ask the library makes,
/// with the ask's context (ADR-0059). A screen's `Depends` is [`depends!`](crate::depends!) of its own.
pub trait Ordering<R, W>:
    DependsOn<Renderer, R>
    + DependsOn<Windows, W>
    + Receives<Answer<Meshed, Meshing>>
    + Receives<Answer<Programmed, Programming>>
    + Receives<Answer<Uniformed, Uniforming>>
    + Receives<Answer<Textured, Texturing>>
    + Receives<Answer<FontMetrics, MetricsAsked>>
    + Receives<Answer<Instanced, Instancing>>
    + Receives<Answer<Placed, Placing>>
    + Receives<Answer<TextMade, Texting>>
    + Receives<Answer<TextPlaced, TextPlacing>>
    + Receives<Answer<ImeAllowed, ImeAsked>>
    + Receives<Answer<Slept, Tipping>>
{
}

impl<A, R, W> Ordering<R, W> for A where
    A: DependsOn<Renderer, R>
        + DependsOn<Windows, W>
        + Receives<Answer<Meshed, Meshing>>
        + Receives<Answer<Programmed, Programming>>
        + Receives<Answer<Uniformed, Uniforming>>
        + Receives<Answer<Textured, Texturing>>
        + Receives<Answer<FontMetrics, MetricsAsked>>
        + Receives<Answer<Instanced, Instancing>>
        + Receives<Answer<Placed, Placing>>
        + Receives<Answer<TextMade, Texting>>
        + Receives<Answer<TextPlaced, TextPlacing>>
        + Receives<Answer<ImeAllowed, ImeAsked>>
        + Receives<Answer<Slept, Tipping>>
{
}

/// What one of a screen's turns decided to send, in order.
#[derive(Debug, Default)]
pub struct Orders {
    orders: Vec<Order>,
}

/// One send, or one line logged.
#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    Mesh(Meshing),
    Program(Programming),
    Uniforms(Uniforming, Vec<u8>),
    White(Whitening),
    /// The texture of a picture, made of its pixels.
    Picture(Picturing, Pixels),
    Metrics(MetricsAsked),
    Instances {
        context: Instancing,
        layout: VertexLayout,
        capacity: u32,
        records: Vec<u8>,
    },
    Place(Place, Placing),
    Text(CreateText, Texting),
    PlaceText(PlaceText, TextPlacing),
    WriteInstances {
        instances: InstancesId,
        first: u32,
        records: Vec<u8>,
    },
    WriteUniforms {
        uniforms: UniformsId,
        bytes: Vec<u8>,
    },
    WriteText(WriteText),
    MoveText(MoveText),
    Release(Release),
    List(Vec<Entry>),
    AllowIme(AllowIme),
    /// The size a child the screen places lays out to, mailed to it.
    Resize {
        child: ProtocolRef<Resizes>,
        resize: Resize,
    },
    /// A sleep of `delay` from the turn's step, for a tooltip's wait.
    Sleep {
        delay: Duration,
        context: Tipping,
    },
    Log(Level, String),
}

impl Orders {
    /// What was decided, in order.
    #[cfg(test)]
    pub(crate) fn orders(&self) -> &[Order] {
        &self.orders
    }

    /// Whether nothing was decided.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub(super) fn ask_mesh(&mut self, context: Meshing) {
        self.orders.push(Order::Mesh(context));
    }

    pub(super) fn ask_program(&mut self, context: Programming) {
        self.orders.push(Order::Program(context));
    }

    pub(super) fn ask_uniforms(&mut self, context: Uniforming, bytes: Vec<u8>) {
        self.orders.push(Order::Uniforms(context, bytes));
    }

    pub(super) fn ask_white(&mut self, context: Whitening) {
        self.orders.push(Order::White(context));
    }

    pub(super) fn ask_picture(&mut self, context: Picturing, pixels: Pixels) {
        self.orders.push(Order::Picture(context, pixels));
    }

    pub(super) fn ask_metrics(&mut self, font: Font) {
        self.orders.push(Order::Metrics(MetricsAsked { font }));
    }

    pub(super) fn ask_instances(&mut self, context: Instancing, layout: VertexLayout, capacity: u32, records: Vec<u8>) {
        self.orders.push(Order::Instances { context, layout, capacity, records });
    }

    pub(super) fn ask_place(&mut self, place: Place, context: Placing) {
        self.orders.push(Order::Place(place, context));
    }

    pub(super) fn ask_text(&mut self, text: CreateText, context: Texting) {
        self.orders.push(Order::Text(text, context));
    }

    pub(super) fn ask_place_text(&mut self, place: PlaceText, context: TextPlacing) {
        self.orders.push(Order::PlaceText(place, context));
    }

    pub(super) fn write_instances(&mut self, instances: InstancesId, first: u32, records: Vec<u8>) {
        self.orders.push(Order::WriteInstances { instances, first, records });
    }

    pub(super) fn write_uniforms(&mut self, uniforms: UniformsId, bytes: Vec<u8>) {
        self.orders.push(Order::WriteUniforms { uniforms, bytes });
    }

    pub(super) fn write_text(&mut self, write: WriteText) {
        self.orders.push(Order::WriteText(write));
    }

    pub(super) fn move_text(&mut self, moved: MoveText) {
        self.orders.push(Order::MoveText(moved));
    }

    pub(super) fn release(&mut self, release: Release) {
        self.orders.push(Order::Release(release));
    }

    pub(super) fn list(&mut self, entries: Vec<Entry>) {
        self.orders.push(Order::List(entries));
    }

    pub(super) fn allow_ime(&mut self, allow: AllowIme) {
        self.orders.push(Order::AllowIme(allow));
    }

    pub(super) fn tell_size(&mut self, child: ProtocolRef<Resizes>, resize: Resize) {
        self.orders.push(Order::Resize { child, resize });
    }

    pub(super) fn sleep(&mut self, delay: Duration, context: Tipping) {
        self.orders.push(Order::Sleep { delay, context });
    }

    pub(super) fn log(&mut self, level: Level, line: String) {
        self.orders.push(Order::Log(level, line));
    }

    /// Sends each, in order, through `turn`, putting what a send carries as a blob: one whose blob was not put is not
    /// sent, and is logged. A sleep runs as a job of the turn's, until its delay past the turn's step.
    ///
    /// # Errors
    ///
    /// A fault when a send or a job was not written.
    pub fn send<A: Ordering<R, W>, R, W>(self, turn: &mut Turn<'_, A>) -> Result<(), Fault> {
        for order in self.orders {
            let renderer = path::<Renderer>(());
            match order {
                Order::Mesh(context) => {
                    let (Ok(vertices), Ok(indices)) = (turn.put(corners()), turn.put(triangles())) else {
                        turn.log(Level::Error, "the screen's square was not made: its bytes were not put");
                        continue;
                    };
                    let mesh = CreateMesh { layout: corner_layout(), vertices, indices };
                    turn.to(renderer).ask(&mesh, &context)?;
                }
                Order::Program(context) => {
                    let Ok(wgsl) = turn.put(Vec::from(context.asked.wgsl())) else {
                        turn.log(Level::Error, "the screen's program was not made: its text was not put");
                        continue;
                    };
                    turn.to(renderer).ask(&program(wgsl, context.shape, context.asked), &context)?;
                }
                Order::Uniforms(context, bytes) => {
                    let Ok(bytes) = turn.put(bytes) else {
                        turn.log(Level::Error, "the screen's uniform was not made: its bytes were not put");
                        continue;
                    };
                    turn.to(renderer).ask(&CreateUniforms { bytes }, &context)?;
                }
                Order::White(context) => {
                    let Ok(pixels) = turn.put(vec![u8::MAX; 4]) else {
                        turn.log(Level::Error, "the screen's white pixel was not made: its bytes were not put");
                        continue;
                    };
                    let white = CreateTexture { width: 1, height: 1, format: TextureFormat::Rgba8, pixels };
                    turn.to(renderer).ask(&white, &Texturing::White(context))?;
                }
                Order::Picture(context, pixels) => {
                    let Ok(texture) = turn.put(pixels.rgba().to_vec()).map(|bytes| texture_of(&pixels, bytes)) else {
                        turn.log(Level::Error, "a picture of the screen was not made: its pixels were not put");
                        continue;
                    };
                    turn.to(renderer).ask(&texture, &Texturing::Picture(context))?;
                }
                Order::Metrics(context) => {
                    turn.to(renderer).ask(&ReadMetrics { font: context.font }, &context)?;
                }
                Order::Instances { context, layout, capacity, records } => {
                    let Ok(records) = turn.put(records) else {
                        turn.log(Level::Error, "a draw of the screen was not made: its records were not put");
                        continue;
                    };
                    turn.to(renderer).ask(&CreateInstances { layout, capacity, records }, &context)?;
                }
                Order::Place(place, context) => {
                    turn.to(renderer).ask(&place, &context)?;
                }
                Order::Text(text, context) => {
                    turn.to(renderer).ask(&text, &context)?;
                }
                Order::PlaceText(place, context) => {
                    turn.to(renderer).ask(&place, &context)?;
                }
                Order::WriteInstances { instances, first, records } => {
                    let Ok(records) = turn.put(records) else {
                        turn.log(Level::Error, "a draw of the screen was not written: its records were not put");
                        continue;
                    };
                    turn.to(renderer).send(&WriteInstances { instances, first, records })?;
                }
                Order::WriteUniforms { uniforms, bytes } => {
                    let Ok(bytes) = turn.put(bytes) else {
                        turn.log(Level::Error, "the screen's frame size was not written: its bytes were not put");
                        continue;
                    };
                    turn.to(renderer).send(&WriteUniforms { uniforms, first: 0, bytes })?;
                }
                Order::WriteText(write) => {
                    turn.to(renderer).send(&write)?;
                }
                Order::MoveText(moved) => {
                    turn.to(renderer).send(&moved)?;
                }
                Order::Release(release) => {
                    turn.to(renderer).send(&release)?;
                }
                Order::List(entries) => {
                    turn.to(renderer).send(&DrawList { entries })?;
                }
                Order::AllowIme(allow) => {
                    turn.to(path::<Windows>(())).ask(&allow, &ImeAsked)?;
                }
                Order::Resize { child, resize } => {
                    turn.to(child).send(&resize)?;
                }
                Order::Sleep { delay, context } => {
                    let until = turn.now() + delay;
                    turn.run(&Sleep { until }, &context)?;
                }
                Order::Log(level, line) => turn.log(level, line),
            }
        }

        Ok(())
    }
}

/// The texture of `pixels`, its bytes put as `bytes`: sRGB, as a decoded image's colours are.
fn texture_of(pixels: &Pixels, bytes: aethereal_actor::Bytes) -> CreateTexture {
    let (width, height) = pixels.size();

    CreateTexture { width, height, format: TextureFormat::Rgba8Srgb, pixels: bytes }
}

/// The program `asked` names, from `wgsl`, made against `shape`: it reads the unit square and a record for each quad,
/// binds the frame's size, and for the library's program a texture and a sampler too, and lays what it draws over
/// what is drawn by its alpha, testing no depth.
fn program(wgsl: aethereal_actor::Bytes, shape: Shape, asked: Asked) -> CreateProgram {
    let frame = Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: FRAME_BYTES } };
    let (records, bindings) = match asked {
        Asked::Quads => {
            let texture = Binding { group: 0, binding: 1, kind: BindingKind::Texture };
            let sampler = Binding { group: 0, binding: 2, kind: BindingKind::Sampler };
            (record_layout(), vec![frame, texture, sampler])
        }
        Asked::Fills { .. } => (fill_layout(), vec![frame]),
    };

    CreateProgram {
        wgsl,
        vertex: String::from(PLACE),
        fragment: String::from(SHADE),
        layout: corner_layout(),
        instances: PerInstance::Records(records),
        bindings,
        reads: Vec::new(),
        state: Fixed { blend: Blend::Alpha, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
        shape,
    }
}
