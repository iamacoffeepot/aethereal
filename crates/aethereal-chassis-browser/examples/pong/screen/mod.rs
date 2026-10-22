//! The screen: an actor beneath the game that draws the court the game sends it.
//!
//! It makes what it draws one ask of the renderer after another, each answer carrying what was made so far: a quad,
//! a program that draws it once for each record of its instances, scaled to the window by uniforms, and three
//! placements of it, each with instances of its own (ADR-0023). The court's instances are written once, as it never
//! moves; the board's when the score or its banner changes; and the play's, the paddles and the ball, for every court
//! the game sends. Depth orders them, as the order of a pass's placements is not promised. It keeps the court fitted
//! to its window as the window's surface changes size (ADR-0024).

mod glyphs;
mod layers;
mod records;

use crate::court::{Court, HALF_HEIGHT, HALF_WIDTH};
use crate::game::Pong;
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, ErasedActorRef, Fault, Handles, Level, On, Path, Start,
    Tells, Turn, path,
};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_render::Renderer;
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::instances::{CreateInstances, Instanced, InstancesId, WriteInstances};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Bind, Drawn, Join, Place, Placed, Resource};
use aethereal_render::program::{
    Binding, BindingKind, Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::shape::Shape;
use aethereal_render::uniforms::{CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_window::{self as window, Event, Surface, Window, WindowId};
use layers::{BOARD_CAPACITY, Board, COURT_CAPACITY, PLAY_CAPACITY};
use records::Records;

/// A program that places each corner of the quad at its record's centre, out by half its record's size, scaled from
/// the court's units to the window's by the uniform `view`, at its record's depth, and shades it its record's colour.
const SHADE: &str = "
@group(0) @binding(0) var<uniform> view: vec4<f32>;

struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) colour: vec4<f32>,
}

@vertex
fn place(
    @location(0) corner: vec2<f32>,
    @location(1) centre: vec2<f32>,
    @location(2) half: vec2<f32>,
    @location(3) depth: f32,
    @location(4) colour: vec4<f32>,
) -> Between {
    return Between(vec4((centre + corner * half) * view.xy, depth, 1.0), colour);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    return between.colour;
}
";

/// How far across the window shows, from the middle of the court, in court units: the court and a margin.
const SHOWN_ACROSS: f32 = HALF_WIDTH + 0.04;

/// How far up the window shows, from the middle of the court, in court units: the court, its walls, and a margin.
const SHOWN_UP: f32 = HALF_HEIGHT + 0.07;

/// The screen's config: the window it draws on.
#[derive(Plain)]
pub struct Shows {
    pub window: WindowId,
}

impl Kind for Shows {
    const NAME: KindName = KindName::new("aether.example.pong.shows");
}

/// How the court is scaled from its units to the window's, across and up, so it fits the window whole and square.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct View {
    across: f32,
    up: f32,
}

impl View {
    /// The view of a window `width` by `height`: the shown court as large as fits, its units as long across as up.
    #[expect(clippy::cast_possible_truncation, reason = "a scale needs no more than a float's precision")]
    fn of(width: u32, height: u32) -> Self {
        let aspect = f64::from(width.max(1)) / f64::from(height.max(1));
        let shown = f64::from(SHOWN_ACROSS) / f64::from(SHOWN_UP);
        let (across, up) = if aspect >= shown {
            let up = 1.0 / f64::from(SHOWN_UP);
            (up / aspect, up)
        } else {
            let across = 1.0 / f64::from(SHOWN_ACROSS);
            (across, across * aspect)
        };

        Self { across: across as f32, up: up as f32 }
    }

    /// The view of `surface`.
    fn fitting(surface: Surface) -> Self {
        Self::of(surface.width, surface.height)
    }

    /// The uniforms' bytes: the view, then two floats the program does not read.
    fn bytes(self) -> Vec<u8> {
        [self.across, self.up, 0.0, 0.0].into_iter().flat_map(f32::to_ne_bytes).collect()
    }
}

/// The screen: it subscribes to its window, makes what it draws one ask after another, then writes the board and the
/// play from each court the game sends.
#[derive(Bound)]
pub enum Screen {
    /// Its asks are under way, and nothing is placed on `window`: the court is to be drawn in `view`.
    Making { window: WindowId, view: View },
    /// Everything is placed: these are the uniforms and instances it writes, and the board as they show it.
    Drawing { uniforms: UniformsId, board: InstancesId, play: InstancesId, shown: Board },
}

impl Kind for Screen {
    const NAME: KindName = KindName::new("aether.example.pong.screen");
}

impl ChildOf for Screen {
    type Parent = Pong;
}

/// The context of an ask that needs none.
#[derive(Plain)]
pub struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.example.pong.nothing");
}

/// What was learned before the quad was asked for: the shape the program draws into.
#[derive(Plain)]
pub struct Learned {
    shape: Shape,
}

impl Kind for Learned {
    const NAME: KindName = KindName::new("aether.example.pong.learned");
}

/// What was made before the program was asked for.
#[derive(Plain)]
pub struct MadeMesh {
    shape: Shape,
    mesh: MeshId,
}

impl Kind for MadeMesh {
    const NAME: KindName = KindName::new("aether.example.pong.made_mesh");
}

/// What was made before the uniforms were asked for.
#[derive(Plain)]
pub struct MadeProgram {
    mesh: MeshId,
    program: ProgramId,
}

impl Kind for MadeProgram {
    const NAME: KindName = KindName::new("aether.example.pong.made_program");
}

/// What every layer is placed with.
#[derive(Clone, Copy, Plain)]
pub struct Kit {
    mesh: MeshId,
    program: ProgramId,
    uniforms: UniformsId,
}

/// A layer of what is drawn, a placement each, made court first, then the board, then the play.
#[derive(Clone, Copy, Plain)]
pub enum Layer {
    /// The court, which never moves.
    Court,
    /// The score and the banner.
    Board,
    /// The paddles and the ball, once the board's instances were made.
    Play { board: InstancesId },
}

impl Layer {
    /// The records the layer's instances are made with, and the most they hold.
    fn first(self) -> (Records, u32) {
        match self {
            Self::Court => (layers::court(), COURT_CAPACITY),
            Self::Board => (Board::OPENING.records(), BOARD_CAPACITY),
            Self::Play { .. } => (layers::play(&Court::OPENING), PLAY_CAPACITY),
        }
    }
}

/// What a layer's instances were asked for with.
#[derive(Plain)]
pub struct Laying {
    kit: Kit,
    layer: Layer,
}

impl Kind for Laying {
    const NAME: KindName = KindName::new("aether.example.pong.laying");
}

/// What a layer's placement was asked for with.
#[derive(Plain)]
pub struct Placing {
    kit: Kit,
    layer: Layer,
    instances: InstancesId,
}

impl Kind for Placing {
    const NAME: KindName = KindName::new("aether.example.pong.placing");
}

impl Actor for Screen {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        Event,
        Court,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Learned>,
        Answer<Programmed, MadeMesh>,
        Answer<Uniformed, MadeProgram>,
        Answer<Instanced, Laying>,
        Answer<Placed, Placing>,
    );
    type BeginSends = (Tells<Path<Window>, window::Subscribe>,);
    type EndSends = ();

    fn begin(shows: Shows, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.to(path::<Window>(shows.window)).send(&window::Subscribe)?;

        Ok(Self::Making { window: shows.window, view: View::of(4, 3) })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// How the quad's corners are laid out: a place of two floats.
fn corners() -> VertexLayout {
    let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };

    VertexLayout { stride: 8, attributes: vec![corner] }
}

/// The bytes of `values`, as a blob is put.
fn bytes(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_ne_bytes).collect()
}

impl Handles<On<Start>> for Screen {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Self::Making { window, .. } = *self else {
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(window)), &Nothing)?;

        Ok(())
    }
}

/// Keeps the court fitted to the window as its surface changes size.
impl Handles<Event> for Screen {
    type Sends = (Tells<Path<Renderer>, WriteUniforms>,);

    fn handle(&mut self, event: Event, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Event::Resized(surface) = event else {
            return Ok(());
        };
        let fitted = View::fitting(surface);
        match self {
            Self::Making { view, .. } => *view = fitted,
            Self::Drawing { uniforms, .. } => {
                let Ok(bytes) = turn.put(fitted.bytes()) else {
                    turn.log(Level::Warn, "the court was not fitted to the window: its view was not put");
                    return Ok(());
                };
                turn.to(path::<Renderer>(())).send(&WriteUniforms { uniforms: *uniforms, first: 0, bytes })?;
            }
        }

        Ok(())
    }
}

/// Writes the play from each court the game sends, and the board when it changed.
impl Handles<Court> for Screen {
    type Sends = (Tells<Path<Renderer>, WriteInstances>,);

    fn handle(&mut self, court: Court, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let game = turn.parent();
        let is_game = turn.sender() == ErasedActorRef::from(game);
        if !is_game {
            turn.log(Level::Warn, "a court from an actor other than the game was left: only the game's is drawn");
            return Ok(());
        }
        let Self::Drawing { board, play, shown, .. } = self else {
            return Ok(());
        };
        write(turn, *play, layers::play(&court))?;
        let now = Board::of(&court);
        if now != *shown {
            write(turn, *board, now.records())?;
            *shown = now;
        }

        Ok(())
    }
}

/// Writes `records` over all that `instances` held, cutting them to their length.
fn write(
    turn: &mut Turn<'_, Screen, (Tells<Path<Renderer>, WriteInstances>,)>,
    instances: InstancesId,
    records: Records,
) -> Result<(), Fault> {
    let length = records.count();
    let put = (turn.put(records.into_bytes()), turn.put(Vec::new()));
    let (Ok(records), Ok(nothing)) = put else {
        turn.log(Level::Warn, "a layer was not written: its records were not put");
        return Ok(());
    };
    // Written whole, then cut to their length: a write that ends short of what they held leaves the rest.
    turn.to(path::<Renderer>(())).send(&WriteInstances { instances, first: 0, records })?.send(&WriteInstances {
        instances,
        first: length,
        records: nothing,
    })?;

    Ok(())
}

impl Handles<Answer<Shaped, Nothing>> for Screen {
    type Sends = (Asks<Path<Renderer>, CreateMesh, Learned>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Shaped>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Shaped::Shape(shape)) = answered else {
            turn.log(Level::Error, format_args!("the window's main pass has no shape: {answered:?}"));
            return Ok(());
        };
        let quad = bytes([-1.0_f32, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0].map(f32::to_bits));
        let (Ok(vertices), Ok(indices)) = (turn.put(quad), turn.put(bytes([0, 1, 2, 0, 2, 3]))) else {
            turn.log(Level::Error, "no quad was asked for: its bytes were not put");
            return Ok(());
        };
        let quad = CreateMesh { layout: corners(), vertices, indices };
        turn.to(path::<Renderer>(())).ask(&quad, &Learned { shape })?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, Learned>> for Screen {
    type Sends = (Asks<Path<Renderer>, CreateProgram, MadeMesh>,);

    fn handle(
        &mut self,
        (answered, learned): (Answered<Meshed>, Learned),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            turn.log(Level::Error, format_args!("the quad was not made: {answered:?}"));
            return Ok(());
        };
        let Ok(wgsl) = turn.put(Vec::from(SHADE)) else {
            turn.log(Level::Error, "no program was asked for: its source was not put");
            return Ok(());
        };
        let shade = CreateProgram {
            wgsl,
            vertex: String::from("place"),
            fragment: String::from("shade"),
            layout: corners(),
            instances: PerInstance::Records(records::layout()),
            bindings: vec![Binding { group: 0, binding: 0, kind: BindingKind::Uniform { bytes: 16 } }],
            state: Fixed { blend: Blend::Opaque, depth: Depth::TestWrite, cull: Cull::None },
            shape: learned.shape,
        };
        turn.to(path::<Renderer>(())).ask(&shade, &MadeMesh { shape: learned.shape, mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, MadeMesh>> for Screen {
    type Sends = (Asks<Path<Renderer>, CreateUniforms, MadeProgram>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Programmed>, MadeMesh),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(program)) = answered else {
            turn.log(Level::Error, format_args!("the program was not made: {answered:?}"));
            return Ok(());
        };
        let Self::Making { view, .. } = *self else {
            return Ok(());
        };
        let Ok(bytes) = turn.put(view.bytes()) else {
            turn.log(Level::Error, "no uniforms were asked for: their bytes were not put");
            return Ok(());
        };
        turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &MadeProgram { mesh: made.mesh, program })?;

        Ok(())
    }
}

impl Handles<Answer<Uniformed, MadeProgram>> for Screen {
    type Sends = (Asks<Path<Renderer>, CreateInstances, Laying>,);

    fn handle(
        &mut self,
        (answered, made): (Answered<Uniformed>, MadeProgram),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Uniformed::Made(uniforms)) = answered else {
            turn.log(Level::Error, format_args!("the uniforms were not made: {answered:?}"));
            return Ok(());
        };
        let kit = Kit { mesh: made.mesh, program: made.program, uniforms };
        let (first, capacity) = Layer::Court.first();
        let Ok(records) = turn.put(first.into_bytes()) else {
            turn.log(Level::Error, "the court's instances were not asked for: their records were not put");
            return Ok(());
        };
        let court = CreateInstances { layout: records::layout(), capacity, records };
        turn.to(path::<Renderer>(())).ask(&court, &Laying { kit, layer: Layer::Court })?;

        Ok(())
    }
}

impl Handles<Answer<Instanced, Laying>> for Screen {
    type Sends = (Asks<Path<Renderer>, Place, Placing>,);

    fn handle(
        &mut self,
        (answered, laying): (Answered<Instanced>, Laying),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Instanced::Made(instances)) = answered else {
            turn.log(Level::Error, format_args!("a layer's instances were not made: {answered:?}"));
            return Ok(());
        };
        let Self::Making { window, .. } = *self else {
            return Ok(());
        };
        let Laying { kit, layer } = laying;
        let bindings = vec![Bind { group: 0, binding: 0, to: Resource::Uniforms(kit.uniforms) }];
        let place = Place {
            mesh: kit.mesh,
            instances: Drawn::Records(instances),
            joins: vec![Join::to::<Main>(Frame::Main(window), kit.program, bindings)],
        };
        turn.to(path::<Renderer>(())).ask(&place, &Placing { kit, layer, instances })?;

        Ok(())
    }
}

/// Asks for the next layer's instances once one is placed, and once the play is, draws from then on, fitting the
/// court to the window as it last heard it.
impl Handles<Answer<Placed, Placing>> for Screen {
    type Sends = (Asks<Path<Renderer>, CreateInstances, Laying>, Tells<Path<Renderer>, WriteUniforms>);

    fn handle(
        &mut self,
        (answered, placing): (Answered<Placed>, Placing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(_)) = answered else {
            turn.log(Level::Error, format_args!("a layer was not placed: {answered:?}"));
            return Ok(());
        };
        let Self::Making { view, .. } = *self else {
            return Ok(());
        };
        let Placing { kit, layer, instances } = placing;
        let next = match layer {
            Layer::Court => Layer::Board,
            Layer::Board => Layer::Play { board: instances },
            Layer::Play { board } => {
                let Ok(bytes) = turn.put(view.bytes()) else {
                    turn.log(Level::Error, "the court was not fitted to the window: its view was not put");
                    return Ok(());
                };
                turn.to(path::<Renderer>(())).send(&WriteUniforms { uniforms: kit.uniforms, first: 0, bytes })?;
                *self = Self::Drawing { uniforms: kit.uniforms, board, play: instances, shown: Board::OPENING };
                return Ok(());
            }
        };
        let (first, capacity) = next.first();
        let Ok(records) = turn.put(first.into_bytes()) else {
            turn.log(Level::Error, "a layer's instances were not asked for: their records were not put");
            return Ok(());
        };
        let create = CreateInstances { layout: records::layout(), capacity, records };
        turn.to(path::<Renderer>(())).ask(&create, &Laying { kit, layer: next })?;

        Ok(())
    }
}
