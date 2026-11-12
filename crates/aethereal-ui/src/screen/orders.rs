//! The mail a screen's library sends (ADR-0056): what each of its turns decided, in order, sent through the screen's
//! turn by [`Orders::send`]. What a send carries as a blob is put there, so the library's decisions are plain values.

use super::layers::{Instancing, Placing, TextPlacing, Texting};
use super::program::{Meshing, MetricsAsked, Programming, Uniforming, Whitening};
use crate::draw::{FRAME_BYTES, PLACE, PROGRAM, SHADE, corner_layout, corners, record_layout, triangles};
use aethereal_actor::{Allows, Asks, Fault, Level, Path, Tells, Turn, path};
use aethereal_render::draw_list::{DrawList, Entry};
use aethereal_render::instances::{CreateInstances, InstancesId, WriteInstances};
use aethereal_render::mesh::{CreateMesh, VertexLayout};
use aethereal_render::placement::Place;
use aethereal_render::program::{
    Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance,
};
use aethereal_render::shape::Shape;
use aethereal_render::text::{CreateText, Font, MoveText, PlaceText, ReadMetrics, WriteText};
use aethereal_render::texture::{CreateTexture, TextureFormat};
use aethereal_render::uniforms::{CreateUniforms, UniformsId, WriteUniforms};
use aethereal_render::{Release, Renderer};

/// Every send a screen's library makes, as a handler of its declares them: the asks that make what it draws and that
/// read a font's metrics, and the tells that write, move, release, and list them.
pub type Sends = (
    Asks<Path<Renderer>, CreateMesh, Meshing>,
    Asks<Path<Renderer>, CreateProgram, Programming>,
    Asks<Path<Renderer>, CreateUniforms, Uniforming>,
    Asks<Path<Renderer>, CreateTexture, Whitening>,
    Asks<Path<Renderer>, ReadMetrics, MetricsAsked>,
    Asks<Path<Renderer>, CreateInstances, Instancing>,
    Asks<Path<Renderer>, Place, Placing>,
    Asks<Path<Renderer>, CreateText, Texting>,
    Asks<Path<Renderer>, PlaceText, TextPlacing>,
    Tells<Path<Renderer>, WriteInstances>,
    Tells<Path<Renderer>, WriteUniforms>,
    Tells<Path<Renderer>, WriteText>,
    Tells<Path<Renderer>, MoveText>,
    Tells<Path<Renderer>, Release>,
    Tells<Path<Renderer>, DrawList>,
);

/// What one of a screen's turns decided to send, in order.
#[derive(Debug, Default)]
pub struct Orders {
    orders: Vec<Order>,
}

/// One send, or one line logged.
#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    Mesh(Meshing),
    Program(Programming, Shape),
    Uniforms(Uniforming, Vec<u8>),
    White(Whitening),
    Metrics(MetricsAsked),
    Instances { context: Instancing, layout: VertexLayout, capacity: u32, records: Vec<u8> },
    Place(Place, Placing),
    Text(CreateText, Texting),
    PlaceText(PlaceText, TextPlacing),
    WriteInstances { instances: InstancesId, first: u32, records: Vec<u8> },
    WriteUniforms { uniforms: UniformsId, bytes: Vec<u8> },
    WriteText(WriteText),
    MoveText(MoveText),
    Release(Release),
    List(Vec<Entry>),
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

    pub(super) fn ask_program(&mut self, context: Programming, shape: Shape) {
        self.orders.push(Order::Program(context, shape));
    }

    pub(super) fn ask_uniforms(&mut self, context: Uniforming, bytes: Vec<u8>) {
        self.orders.push(Order::Uniforms(context, bytes));
    }

    pub(super) fn ask_white(&mut self, context: Whitening) {
        self.orders.push(Order::White(context));
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

    pub(super) fn log(&mut self, level: Level, line: String) {
        self.orders.push(Order::Log(level, line));
    }

    /// Sends each, in order, through `turn`, putting what a send carries as a blob: one whose blob was not put is not
    /// sent, and is logged.
    ///
    /// # Errors
    ///
    /// A fault when a send was not written.
    pub fn send<A, S, I0, I1, I2, I3, I4, I5, I6, I7, I8, I9, I10, I11, I12, I13, I14>(
        self,
        turn: &mut Turn<'_, A, S>,
    ) -> Result<(), Fault>
    where
        S: Allows<Asks<Path<Renderer>, CreateMesh, Meshing>, I0>
            + Allows<Asks<Path<Renderer>, CreateProgram, Programming>, I1>
            + Allows<Asks<Path<Renderer>, CreateUniforms, Uniforming>, I2>
            + Allows<Asks<Path<Renderer>, CreateTexture, Whitening>, I3>
            + Allows<Asks<Path<Renderer>, ReadMetrics, MetricsAsked>, I4>
            + Allows<Asks<Path<Renderer>, CreateInstances, Instancing>, I5>
            + Allows<Asks<Path<Renderer>, Place, Placing>, I6>
            + Allows<Asks<Path<Renderer>, CreateText, Texting>, I7>
            + Allows<Asks<Path<Renderer>, PlaceText, TextPlacing>, I8>
            + Allows<Tells<Path<Renderer>, WriteInstances>, I9>
            + Allows<Tells<Path<Renderer>, WriteUniforms>, I10>
            + Allows<Tells<Path<Renderer>, WriteText>, I11>
            + Allows<Tells<Path<Renderer>, MoveText>, I12>
            + Allows<Tells<Path<Renderer>, Release>, I13>
            + Allows<Tells<Path<Renderer>, DrawList>, I14>,
    {
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
                Order::Program(context, shape) => {
                    let Ok(wgsl) = turn.put(Vec::from(PROGRAM)) else {
                        turn.log(Level::Error, "the screen's program was not made: its text was not put");
                        continue;
                    };
                    turn.to(renderer).ask(&program(wgsl, shape), &context)?;
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
                    turn.to(renderer).ask(&white, &context)?;
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
                Order::Log(level, line) => turn.log(level, line),
            }
        }

        Ok(())
    }
}

/// The library's program, from `wgsl`, made against `shape`: it reads the unit square and a record for each quad,
/// binds the frame's size, a texture, and a sampler, and lays what it draws over what is drawn by its alpha, testing no
/// depth.
fn program(wgsl: aethereal_actor::Bytes, shape: Shape) -> CreateProgram {
    CreateProgram {
        wgsl,
        vertex: String::from(PLACE),
        fragment: String::from(SHADE),
        layout: corner_layout(),
        instances: PerInstance::Records(record_layout()),
        bindings: vec![
            Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: FRAME_BYTES } },
            Binding { group: 0, binding: 1, kind: BindingKind::Texture },
            Binding { group: 0, binding: 2, kind: BindingKind::Sampler },
        ],
        reads: Vec::new(),
        state: Fixed { blend: Blend::Alpha, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
        shape,
    }
}
