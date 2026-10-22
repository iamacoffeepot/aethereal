//! The disc: the root that owns the stars. Asked to move them in a frame, it holds the ask while it seeds their
//! storage, makes the step's uniforms and the compute program, and dispatches it in the frame's compute pass, then
//! answers with the stars it moves. At each `Tick` it writes the step: the time it covers, how far the arms have
//! turned, and where the pointer, while it is held, pulls.

mod pointer;
mod seed;

pub use seed::STARS;

use crate::camera::Camera;
use crate::mail::{Canvas, Move, Moving};
use crate::page::say;
use crate::passes::Simulate;
use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, Carries, ChildOf, Ending, Fault, Handles, Held, Level, On, Path, Tells,
    Tick, ToSender, Top, Turn, path,
};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_render::Renderer;
use aethereal_render::compute::{ComputeId, Computed, CreateCompute};
use aethereal_render::dispatch::{CreateDispatch, Dispatch, Dispatched, Invocations};
use aethereal_render::frame::{Frame, FrameId};
use aethereal_render::placement::{Bind, Resource};
use aethereal_render::program::{Access, Binding, BindingKind};
use aethereal_render::storage::{CreateStorage, StorageId, Stored};
use aethereal_render::uniforms::{CreateUniforms, Uniformed, UniformsId, WriteUniforms};
use aethereal_window::{self as window, Event, Window};
use core::f32::consts::TAU;
use core::mem;
use core::time::Duration;
use pointer::Pointer;

/// The compute program that moves the stars.
const SIMULATE: &str = include_str!("../shaders/simulate.wgsl");

/// The bytes of a star in storage, and of the step's uniforms.
const STAR_BYTES: u32 = 32;
const STEP_BYTES: u32 = 32;

/// How fast the arms turn, in radians a second.
const TURNING: f32 = 0.09;

/// The longest step the stars are moved by, in nanoseconds: a frame that took longer, as when the page was hidden,
/// moves them no further, so nothing is flung.
const LONGEST_STEP_NANOS: u64 = 300_000_000;

/// The disc: the window it follows, the pointer as it last heard it, and how far it has come in moving the stars.
#[derive(Bound)]
pub struct Disc {
    canvas: Canvas,
    pointer: Pointer,
    run: Run,
}

impl Kind for Disc {
    const NAME: KindName = KindName::new("aether.example.galaxy.disc");
}

impl Carries<Move> for Disc {
    type Answers = (Moving,);
}

impl ChildOf for Disc {
    type Parent = Top;
}

/// How far the disc has come in moving the stars.
#[derive(Bound)]
pub enum Run {
    /// It waits to be asked to move them in a frame.
    Waiting,
    /// Its asks are under way, and it holds the ask to move them, to answer once they move.
    Making { asked: Held<Moving> },
    /// They move, stepped by the uniforms `step`, the arms turned by `turned` radians as of the last step.
    Running { step: UniformsId, turned: f32 },
}

/// What was made before the step's uniforms were asked for.
#[derive(Plain)]
pub struct Seeded {
    frame: FrameId,
    stars: StorageId,
}

impl Kind for Seeded {
    const NAME: KindName = KindName::new("aether.example.galaxy.seeded");
}

/// What was made before the compute program was asked for.
#[derive(Plain)]
pub struct Stepped {
    frame: FrameId,
    stars: StorageId,
    step: UniformsId,
}

impl Kind for Stepped {
    const NAME: KindName = KindName::new("aether.example.galaxy.stepped");
}

/// What the dispatch was asked for with.
#[derive(Plain)]
pub struct Dispatching {
    stars: StorageId,
    step: UniformsId,
}

impl Kind for Dispatching {
    const NAME: KindName = KindName::new("aether.example.galaxy.dispatching");
}

impl Actor for Disc {
    type Config = Canvas;
    type Key = ();
    type Handles = (
        On<Tick>,
        Event,
        Move,
        Answer<Stored, FrameId>,
        Answer<Uniformed, Seeded>,
        Answer<Computed, Stepped>,
        Answer<Dispatched, Dispatching>,
    );
    type BeginSends = (Tells<Path<Window>, window::Subscribe>,);
    type EndSends = ();

    fn begin(canvas: Canvas, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(canvas.window)).send(&window::Subscribe)?;
        turn.subscribe::<Tick>();

        Ok(Self { canvas, pointer: Pointer::Up, run: Run::Waiting })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Disc {
    /// How the disc is seen on its window's surface as it now is.
    fn camera(&self) -> Camera {
        Camera::of(self.canvas.width, self.canvas.height)
    }
}

impl Handles<Event> for Disc {
    type Sends = ();

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        if let Event::Resized(surface) = event {
            self.canvas = Canvas { width: surface.width, height: surface.height, ..self.canvas };
        }
        self.pointer = self.pointer.heard(&event);

        Ok(())
    }
}

impl Handles<Move> for Disc {
    type Sends = (ToSender<Moving>, Asks<Path<Renderer>, CreateStorage, FrameId>);

    fn handle(&mut self, asked: Move, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Run::Waiting = self.run else {
            return Ok(());
        };
        let bytes = match turn.put(seed::stars()) {
            Ok(bytes) => bytes,
            Err(why) => {
                say(format!("no stars asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&CreateStorage { bytes }, &asked.frame)?;
        self.run = Run::Making { asked: turn.hold()? };

        Ok(())
    }
}

impl Handles<Answer<Stored, FrameId>> for Disc {
    type Sends = (Asks<Path<Renderer>, CreateUniforms, Seeded>,);

    fn handle(
        &mut self,
        (answered, frame): (Answered<Stored>, FrameId),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Stored::Made(stars)) = answered else {
            say(format!("the stars were not stored: {answered:?}"));
            return Ok(());
        };
        let bytes = match turn.put(Pointer::Up.step(self.camera(), 0.0, 0.0, TURNING)) {
            Ok(bytes) => bytes,
            Err(why) => {
                say(format!("no step asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&CreateUniforms { bytes }, &Seeded { frame, stars })?;

        Ok(())
    }
}

/// What the compute program binds: the stars to write, and the step's uniforms.
fn simulated() -> Vec<Binding> {
    vec![
        Binding { group: 0, binding: 0, kind: BindingKind::Storage { bytes: STAR_BYTES, access: Access::Write } },
        Binding { group: 0, binding: 1, kind: BindingKind::Uniform { bytes: STEP_BYTES } },
    ]
}

impl Handles<Answer<Uniformed, Seeded>> for Disc {
    type Sends = (Asks<Path<Renderer>, CreateCompute, Stepped>,);

    fn handle(
        &mut self,
        (answered, seeded): (Answered<Uniformed>, Seeded),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Uniformed::Made(step)) = answered else {
            say(format!("the step was not made: {answered:?}"));
            return Ok(());
        };
        let wgsl = match turn.put(Vec::from(SIMULATE)) {
            Ok(wgsl) => wgsl,
            Err(why) => {
                say(format!("no simulation asked: {why}"));
                return Ok(());
            }
        };
        let simulate = CreateCompute { wgsl, entry: String::from("simulate"), bindings: simulated() };
        let stepped = Stepped { frame: seeded.frame, stars: seeded.stars, step };
        turn.to(path::<Renderer>(())).ask(&simulate, &stepped)?;

        Ok(())
    }
}

impl Handles<Answer<Computed, Stepped>> for Disc {
    type Sends = (Asks<Path<Renderer>, CreateDispatch, Dispatching>,);

    fn handle(
        &mut self,
        (answered, stepped): (Answered<Computed>, Stepped),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Computed::Made(simulate)) = answered else {
            say(format!("the simulation was not made: {answered:?}"));
            return Ok(());
        };
        let dispatch = dispatched(stepped.frame, simulate, stepped.stars, stepped.step);
        turn.to(path::<Renderer>(())).ask(&dispatch, &Dispatching { stars: stepped.stars, step: stepped.step })?;

        Ok(())
    }
}

/// The dispatch of `simulate` in the compute pass of `frame`, moving each of `stars` by the uniforms `step`.
fn dispatched(frame: FrameId, simulate: ComputeId, stars: StorageId, step: UniformsId) -> CreateDispatch {
    let bindings = vec![
        Bind { group: 0, binding: 0, to: Resource::Storage(stars) },
        Bind { group: 0, binding: 1, to: Resource::Uniforms(step) },
    ];

    CreateDispatch {
        join: Dispatch::to::<Simulate>(Frame::Texture(frame), simulate, bindings),
        size: Invocations::PerElement(stars),
    }
}

impl Handles<Answer<Dispatched, Dispatching>> for Disc {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, dispatching): (Answered<Dispatched>, Dispatching),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Dispatched::Made(_)) = answered else {
            say(format!("the simulation was not dispatched: {answered:?}"));
            return Ok(());
        };
        let moving = Run::Running { step: dispatching.step, turned: 0.0 };
        let Run::Making { asked } = mem::replace(&mut self.run, moving) else {
            return Ok(());
        };
        turn.answer(asked, &Moving { stars: dispatching.stars })?;

        Ok(())
    }
}

impl Handles<On<Tick>> for Disc {
    type Sends = (Tells<Path<Renderer>, WriteUniforms>,);

    fn handle(&mut self, tick: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Run::Running { step, turned } = self.run else {
            return Ok(());
        };
        let delta = Duration::from_nanos(tick.delta_nanos.min(LONGEST_STEP_NANOS)).as_secs_f32();
        let turned = TURNING.mul_add(delta, turned) % TAU;
        let Ok(bytes) = turn.put(self.pointer.step(self.camera(), delta, turned, TURNING)) else {
            turn.log(Level::Warn, "the step was not written: its bytes were not put");
            return Ok(());
        };
        turn.to(path::<Renderer>(())).send(&WriteUniforms { uniforms: step, first: 0, bytes })?;
        self.run = Run::Running { step, turned };

        Ok(())
    }
}
