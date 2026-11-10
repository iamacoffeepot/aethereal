//! How the overlay makes what it draws, once its window has said its size (ADR-0021, ADR-0023, ADR-0052): its one
//! placement, one ask after another, each answer carrying what was made so far — the shape of the window's main pass,
//! the unit square, the program, room for the records, the uniform of the surface's size, and the placement, joined
//! to the pass listed — and beside it the text of the chart's figures, made and placed listed.

use super::{Chart, Overlay, Readout};
use crate::chart::{self, MOST_RECORDS, SHADE};
use crate::frames::Figures;
use crate::layout::Layout;
use aethereal_actor::{Allows, Answer, Answered, Asks, Fault, Handles, Level, Path, Tells, Turn, path};
use aethereal_data::Plain;
use aethereal_render::Renderer;
use aethereal_render::draw_list::DrawList;
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::instances::{CreateInstances, Instanced, InstancesId, WriteInstances};
use aethereal_render::mesh::{CreateMesh, MeshId, Meshed};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Bind, Drawn, Join, Place, Placed, Resource};
use aethereal_render::program::{
    Binding, BindingKind, Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::shape::Shape;
use aethereal_render::text::{
    Align, Clip, CreateText, Font, Layer, MoveText, Outline, PlaceText, TextId, TextJoin, TextMade, TextPlaced, Wrap,
};
use aethereal_render::uniforms::{CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use core::fmt::Debug;

/// The shape of the window's main pass was asked for, to make the program against.
#[derive(Plain)]
#[kind(name = "aether.overlay.chart_asked")]
pub struct ChartAsked;

/// The square was asked for, with the shape its program is made against.
#[derive(Plain)]
#[kind(name = "aether.overlay.chart_shaped")]
pub struct ChartShaped {
    shape: Shape,
}

/// The program was asked for, with the square.
#[derive(Plain)]
#[kind(name = "aether.overlay.chart_meshed")]
pub struct ChartMeshed {
    mesh: MeshId,
}

/// The instances were asked for, with the square and the program.
#[derive(Plain)]
#[kind(name = "aether.overlay.chart_programmed")]
pub struct ChartProgrammed {
    mesh: MeshId,
    program: ProgramId,
}

/// The uniform was asked for, with what the placement draws.
#[derive(Plain)]
#[kind(name = "aether.overlay.chart_instanced")]
pub struct ChartInstanced {
    mesh: MeshId,
    program: ProgramId,
    instances: InstancesId,
}

/// The placement was asked for, with what the overlay writes once it is placed.
#[derive(Plain)]
#[kind(name = "aether.overlay.chart_made")]
pub struct ChartMade {
    instances: InstancesId,
    uniforms: UniformsId,
}

/// The figures' text was asked for.
#[derive(Plain)]
#[kind(name = "aether.overlay.readout_asked")]
pub struct ReadoutAsked;

/// The figures' text was placed, with the text.
#[derive(Plain)]
#[kind(name = "aether.overlay.readout_made")]
pub struct ReadoutMade {
    text: TextId,
}

impl Overlay {
    /// Asks for the first of what the overlay draws, its placement's pass's shape and the figures' text, unless they
    /// were asked for already: once the window has said its size, which the text is sized by.
    pub(super) fn make<S, I, J>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Asks<Path<Renderer>, ShapeOf, ChartAsked>, I>
            + Allows<Asks<Path<Renderer>, CreateText, ReadoutAsked>, J>,
    {
        if matches!(self.chart, Chart::Unasked) {
            let shape = ShapeOf::pass::<Main>(Frame::Main(self.window));
            turn.to(path::<Renderer>(())).ask(&shape, &ChartAsked)?;
            self.chart = Chart::Making;
        }
        if matches!(self.readout, Readout::Unasked) {
            let text = CreateText {
                font: Font::Mono,
                size: Layout::of(self.surface).text_pixels(),
                runs: Figures::default().runs(),
                wrap: Wrap::None,
                align: Align::Left,
                outline: Outline::None,
            };
            turn.to(path::<Renderer>(())).ask(&text, &ReadoutAsked)?;
            self.readout = Readout::Making;
        }

        Ok(())
    }

    /// Logs that `what` was not made, as `answered` says, and leaves the overlay's piece of it unmade.
    fn unmade<S>(turn: &mut Turn<'_, Self, S>, what: &str, answered: &impl Debug) {
        turn.log(Level::Error, format_args!("the overlay's {what} was not made: {answered:?}"));
    }
}

impl Handles<Answer<Shaped, ChartAsked>> for Overlay {
    type Sends = (Asks<Path<Renderer>, CreateMesh, ChartShaped>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Shaped>, ChartAsked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Shaped::Shape(shape)) = answered else {
            Self::unmade(turn, "chart: the window's main pass has no shape", &answered);
            return Ok(());
        };
        let (Ok(vertices), Ok(indices)) = (turn.put(chart::corners()), turn.put(chart::triangles())) else {
            Self::unmade(turn, "chart", &"its square's bytes were not put");
            return Ok(());
        };
        let square = CreateMesh { layout: chart::corner_layout(), vertices, indices };
        turn.to(path::<Renderer>(())).ask(&square, &ChartShaped { shape })?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, ChartShaped>> for Overlay {
    type Sends = (Asks<Path<Renderer>, CreateProgram, ChartMeshed>,);

    fn handle(
        &mut self,
        (answered, shaped): (Answered<Meshed>, ChartShaped),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            Self::unmade(turn, "chart's square", &answered);
            return Ok(());
        };
        let Ok(wgsl) = turn.put(Vec::from(SHADE)) else {
            Self::unmade(turn, "chart", &"its program's text was not put");
            return Ok(());
        };
        // Laid over what is drawn by its alpha, testing no depth, as the band is over everything in the pass.
        let program = CreateProgram {
            wgsl,
            vertex: String::from("place"),
            fragment: String::from("shade"),
            layout: chart::corner_layout(),
            instances: PerInstance::Records(chart::record_layout()),
            bindings: vec![Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: 16 } }],
            reads: Vec::new(),
            state: Fixed { blend: Blend::Alpha, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
            shape: shaped.shape,
        };
        turn.to(path::<Renderer>(())).ask(&program, &ChartMeshed { mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, ChartMeshed>> for Overlay {
    type Sends = (Asks<Path<Renderer>, CreateInstances, ChartProgrammed>,);

    fn handle(
        &mut self,
        (answered, meshed): (Answered<Programmed>, ChartMeshed),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(program)) = answered else {
            Self::unmade(turn, "chart's program", &answered);
            return Ok(());
        };
        let Ok(records) = turn.put(Vec::new()) else {
            Self::unmade(turn, "chart", &"its first records were not put");
            return Ok(());
        };
        let instances = CreateInstances { layout: chart::record_layout(), capacity: MOST_RECORDS, records };
        turn.to(path::<Renderer>(())).ask(&instances, &ChartProgrammed { mesh: meshed.mesh, program })?;

        Ok(())
    }
}

impl Handles<Answer<Instanced, ChartProgrammed>> for Overlay {
    type Sends = (Asks<Path<Renderer>, CreateUniforms, ChartInstanced>,);

    fn handle(
        &mut self,
        (answered, programmed): (Answered<Instanced>, ChartProgrammed),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Instanced::Made(instances)) = answered else {
            Self::unmade(turn, "chart's records", &answered);
            return Ok(());
        };
        let Ok(bytes) = turn.put(chart::surface(Layout::of(self.surface))) else {
            Self::unmade(turn, "chart", &"its uniform's bytes were not put");
            return Ok(());
        };
        let made = ChartInstanced { mesh: programmed.mesh, program: programmed.program, instances };
        turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &made)?;

        Ok(())
    }
}

impl Handles<Answer<Uniformed, ChartInstanced>> for Overlay {
    type Sends = (Asks<Path<Renderer>, Place, ChartMade>,);

    fn handle(
        &mut self,
        (answered, instanced): (Answered<Uniformed>, ChartInstanced),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Uniformed::Made(uniforms)) = answered else {
            Self::unmade(turn, "chart's uniform", &answered);
            return Ok(());
        };
        let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Uniforms(uniforms) }];
        let join = Join::listed::<Main>(Frame::Main(self.window), instanced.program, bindings);
        let place = Place { mesh: instanced.mesh, instances: Drawn::Records(instanced.instances), joins: vec![join] };
        turn.to(path::<Renderer>(())).ask(&place, &ChartMade { instances: instanced.instances, uniforms })?;

        Ok(())
    }
}

/// The placement is drawn from here. The surface's size is written again, as the window may have been resized while
/// it was made, and when the overlay is shown its records are written and its list sent.
impl Handles<Answer<Placed, ChartMade>> for Overlay {
    type Sends =
        (Tells<Path<Renderer>, WriteUniforms>, Tells<Path<Renderer>, WriteInstances>, Tells<Path<Renderer>, DrawList>);

    fn handle(
        &mut self,
        (answered, made): (Answered<Placed>, ChartMade),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(placement)) = answered else {
            Self::unmade(turn, "chart's placement", &answered);
            return Ok(());
        };
        self.chart = Chart::Made { instances: made.instances, written: 0, uniforms: made.uniforms, placement };
        self.write_surface(turn)?;
        if self.stop.draws() {
            self.write_chart(turn)?;
            self.send_list(turn)?;
        }

        Ok(())
    }
}

impl Handles<Answer<TextMade, ReadoutAsked>> for Overlay {
    type Sends = (Asks<Path<Renderer>, PlaceText, ReadoutMade>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<TextMade>, ReadoutAsked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(TextMade::Made(text)) = answered else {
            Self::unmade(turn, "figures' text", &answered);
            return Ok(());
        };
        let place = PlaceText {
            text,
            at: Layout::of(self.surface).figures_at(),
            layer: Layer::Over,
            opacity: 1.0,
            clip: Clip::None,
            joins: vec![TextJoin::listed::<Main>(Frame::Main(self.window))],
        };
        turn.to(path::<Renderer>(())).ask(&place, &ReadoutMade { text })?;

        Ok(())
    }
}

/// The text is drawn from here. It is moved to where the layout puts it now, as the window may have been resized
/// while it was placed, and when the overlay is shown its list is sent.
impl Handles<Answer<TextPlaced, ReadoutMade>> for Overlay {
    type Sends = (Tells<Path<Renderer>, MoveText>, Tells<Path<Renderer>, DrawList>);

    fn handle(
        &mut self,
        (answered, made): (Answered<TextPlaced>, ReadoutMade),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(TextPlaced::Made(placement)) = answered else {
            Self::unmade(turn, "figures' placement", &answered);
            return Ok(());
        };
        self.readout = Readout::Made { text: made.text, placement, shown: Figures::default() };
        self.move_readout(turn)?;
        if self.stop.draws() {
            self.send_list(turn)?;
        }

        Ok(())
    }
}
