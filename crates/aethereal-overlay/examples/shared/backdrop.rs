//! The overlay example's scene: one triangle over the whole of the first window, shaded from a dark blue at its foot
//! to a warm brown at its top, with faint upright stripes, so the overlay's panels show what is behind them. It is
//! made one ask after another, as it starts, placed in the window's main pass, and listed, first in the program's root
//! list, beneath the overlay (ADR-0021, ADR-0023, ADR-0055).

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Level, On, Path, Start, Tells, Top,
    Turn, path,
};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::draw_list::{DrawList, Entry, Pointer};
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Drawn, Join, Place, Placed};
use aethereal_render::program::{Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, Programmed};
use aethereal_render::shape::Shape;
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{Cursor, InitialWindows, WindowId};

/// A program that draws its triangle where its corners are, in clip space, shaded by height with faint stripes.
const SHADE: &str = "
struct Between {
    @builtin(position) at: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn place(@location(0) corner: vec2<f32>) -> Between {
    return Between(vec4(corner, 0.0, 1.0), corner * 0.5 + 0.5);
}

@fragment
fn shade(between: Between) -> @location(0) vec4<f32> {
    let dusk = mix(vec3(0.004, 0.012, 0.03), vec3(0.16, 0.07, 0.025), between.uv.y);
    let stripe = step(0.5, fract(between.uv.x * 16.0)) * 0.02;
    return vec4(dusk + stripe, 1.0);
}
";

/// The backdrop draws on the first window the window root opened.
impl<H: Host> Bootstrap<H> for Backdrop {
    const SECTION: &'static str = "backdrop";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), Backed { window: windows.first() })
    }
}

/// The backdrop's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.backed")]
pub struct Backed {
    window: WindowId,
}

/// The backdrop: it asks for its triangle as the program starts, places it once made, and lists it.
#[derive(Bound)]
#[kind(name = "aether.example.backdrop")]
pub struct Backdrop {
    window: WindowId,
}

/// The shape of the window's main pass was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.backdrop_asked")]
pub struct BackdropAsked;

/// The triangle was asked for, with the shape its program is made against.
#[derive(Plain)]
#[kind(name = "aether.example.backdrop_shaped")]
pub struct BackdropShaped {
    shape: Shape,
}

/// The program was asked for, with the triangle.
#[derive(Plain)]
#[kind(name = "aether.example.backdrop_meshed")]
pub struct BackdropMeshed {
    mesh: MeshId,
}

/// The placement was asked for.
#[derive(Plain)]
#[kind(name = "aether.example.backdrop_placing")]
pub struct BackdropPlacing;

impl ChildOf for Backdrop {
    type Parent = Top;
}

impl Actor for Backdrop {
    type Config = Backed;
    type Key = ();
    type Handles = (
        On<Start>,
        Answer<Shaped, BackdropAsked>,
        Answer<Meshed, BackdropShaped>,
        Answer<Programmed, BackdropMeshed>,
        Answer<Placed, BackdropPlacing>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(backed: Backed, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();

        Ok(Self { window: backed.window })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// How the triangle's corners are laid out: a place of two floats, in clip space.
fn corner() -> VertexLayout {
    VertexLayout { stride: 8, attributes: vec![Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 }] }
}

impl Handles<On<Start>> for Backdrop {
    type Sends = (Asks<Path<Renderer>, ShapeOf, BackdropAsked>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(self.window)), &BackdropAsked)?;

        Ok(())
    }
}

impl Handles<Answer<Shaped, BackdropAsked>> for Backdrop {
    type Sends = (Asks<Path<Renderer>, CreateMesh, BackdropShaped>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Shaped>, BackdropAsked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Shaped::Shape(shape)) = answered else {
            turn.log(Level::Error, format_args!("the window's main pass has no shape: {answered:?}"));
            return Ok(());
        };
        // One triangle past every edge of clip space covers all of it.
        let corners: Vec<u8> = [-1.0_f32, -1.0, 3.0, -1.0, -1.0, 3.0].iter().flat_map(|at| at.to_ne_bytes()).collect();
        let indices: Vec<u8> = [0_u32, 1, 2].iter().flat_map(|index| index.to_ne_bytes()).collect();
        let (Ok(vertices), Ok(indices)) = (turn.put(corners), turn.put(indices)) else {
            turn.log(Level::Error, "the backdrop was not asked for: its bytes were not put");
            return Ok(());
        };
        let triangle = CreateMesh { layout: corner(), vertices, indices };
        turn.to(path::<Renderer>(())).ask(&triangle, &BackdropShaped { shape })?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, BackdropShaped>> for Backdrop {
    type Sends = (Asks<Path<Renderer>, CreateProgram, BackdropMeshed>,);

    fn handle(
        &mut self,
        (answered, shaped): (Answered<Meshed>, BackdropShaped),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(mesh)) = answered else {
            turn.log(Level::Error, format_args!("the backdrop's triangle was not made: {answered:?}"));
            return Ok(());
        };
        let Ok(wgsl) = turn.put(Vec::from(SHADE)) else {
            turn.log(Level::Error, "the backdrop's program was not asked for: its text was not put");
            return Ok(());
        };
        let program = CreateProgram {
            wgsl,
            vertex: String::from("place"),
            fragment: String::from("shade"),
            layout: corner(),
            instances: PerInstance::None,
            bindings: Vec::new(),
            reads: Vec::new(),
            state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
            shape: shaped.shape,
        };
        turn.to(path::<Renderer>(())).ask(&program, &BackdropMeshed { mesh })?;

        Ok(())
    }
}

impl Handles<Answer<Programmed, BackdropMeshed>> for Backdrop {
    type Sends = (Asks<Path<Renderer>, Place, BackdropPlacing>,);

    fn handle(
        &mut self,
        (answered, meshed): (Answered<Programmed>, BackdropMeshed),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(program)) = answered else {
            turn.log(Level::Error, format_args!("the backdrop's program was not made: {answered:?}"));
            return Ok(());
        };
        let join = Join::to::<Main>(Frame::Main(self.window), program, Vec::new());
        let place = Place { mesh: meshed.mesh, instances: Drawn::Count(1), joins: vec![join] };
        turn.to(path::<Renderer>(())).ask(&place, &BackdropPlacing)?;

        Ok(())
    }
}

impl Handles<Answer<Placed, BackdropPlacing>> for Backdrop {
    type Sends = (Tells<Path<Renderer>, DrawList>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Placed>, BackdropPlacing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(placement)) = answered else {
            turn.log(Level::Error, format_args!("the backdrop was not placed: {answered:?}"));
            return Ok(());
        };
        let drawn = Entry::Draw { placement, pointer: Pointer::Ignores, cursor: Cursor::Default };
        turn.to(path::<Renderer>(())).send(&DrawList { entries: vec![drawn] })?;

        Ok(())
    }
}
