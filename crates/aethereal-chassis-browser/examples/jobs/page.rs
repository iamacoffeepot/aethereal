//! The page's entry points, a job that sums, and an actor that runs it and three sleeps on the page's event loop, one
//! of them given up on, and places a square as each is answered.

use aethereal_actor::{
    Actor, Answer, Answered, Asks, Beginning, ChildOf, Ending, Fault, Handles, Job, JobId, JobType, On, Path, Render,
    Runner, Running, Sleep, Slept, Start, Tick, Time, Top, Turn, path,
};
use aethereal_chassis_browser::canvas::{self, Density};
use aethereal_chassis_browser::{Browser, EventLoop, Hosted, Listening, Performance, open, renderer};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Drawn, Join, Place, Placed};
use aethereal_render::program::{Blend, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed};
use aethereal_render::shape::Shape;
use aethereal_substrate::Engine;
use aethereal_window::{Heard, Opening, WindowId, Windowing, Windows};
use core::cell::RefCell;
use core::mem;
use core::num::NonZeroUsize;
use core::time::Duration;
use std::sync::Arc;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::HtmlCanvasElement;

thread_local! {
    /// What the prober was answered, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes what the prober was answered.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// The program the page runs, for its `requestAnimationFrame` to call, and the page's listeners, kept while it runs.
#[wasm_bindgen]
pub struct Program {
    browser: Browser<Performance>,
    _canvas: Listening,
}

/// Opens `canvas` as the one window and WebGPU on it, then makes the engine, its jobs run on the page's event loop,
/// with the window root, the renderer, handed the GPU, and the prober as its roots, and listens to the page for the
/// canvas's size.
///
/// # Errors
///
/// When WebGPU will not open on the canvas, a root does not begin, or the page will not be listened to.
#[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
pub async fn start(canvas: HtmlCanvasElement) -> Result<Program, JsError> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 20, 0)
        .with_jobs(Arc::new(EventLoop::new(vec![JobType::of::<Sum>()])))
        .with_cycle::<(Tick, Render)>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let (gpu, [shown]) = open([canvas.clone()], &mut opening).await?;
    let (windows, hosting) = opening.config();
    let mut setup = setup
        .add_root::<Windows>(&(), windows)
        .map_err(|refused| JsError::new(&format!("the window root did not begin: {refused:?}")))?;
    let (rendering, mappings) = renderer(&mut setup, gpu);
    let engine = setup
        .add_root::<Renderer>(&(), rendering)
        .map_err(|refused| JsError::new(&format!("the renderer did not begin: {refused:?}")))?
        .add_root::<Prober>(&(), Shows { window: shown.window() })
        .map_err(|refused| JsError::new(&format!("the prober did not begin: {refused:?}")))?
        .ready();

    // The canvas is drawn at every device pixel of the size the page lays it out at, followed as it is resized.
    let listening = canvas::listen(&canvas, &shown, Density::SCREEN)?;

    Ok(Program { browser: Browser::new(engine, Performance, mappings, Hosted::Windows(hosting)), _canvas: listening })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.browser.frame(timestamp_millis);
    }

    /// What the prober was answered since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// A job for computing: the sum of the squares of the numbers up to `up_to`.
#[derive(Plain)]
#[kind(name = "aether.example.sum")]
struct Sum {
    up_to: u64,
}

/// What a sum answers with.
#[derive(Plain)]
#[kind(name = "aether.example.summed")]
struct Summed {
    total: u64,
}

impl Job for Sum {
    type Done = Summed;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, _: &mut Running) -> Result<Summed, Fault> {
        let total = (1..=self.up_to).map(|number| number * number).sum();

        Ok(Summed { total })
    }
}

/// The context of an ask that needs none.
#[derive(Plain)]
#[kind(name = "aether.example.nothing")]
struct Nothing;

/// The prober's config: the window it draws on.
#[derive(Plain)]
#[kind(name = "aether.example.shows")]
struct Shows {
    window: WindowId,
}

/// The prober: it makes a square and a program for each of its jobs, one ask after another, then runs its jobs, and
/// places a square as each is answered.
#[derive(Bound)]
#[kind(name = "aether.example.prober")]
struct Prober {
    window: WindowId,
    /// The frames that have begun.
    frames: u64,
    stage: Stage,
}

/// Where the prober stands.
#[derive(Bound)]
#[kind(name = "aether.example.stage")]
enum Stage {
    /// Its asks are under way.
    Making,
    /// Its jobs are running: it places `square` with one of `programs` as each is answered.
    Running { square: MeshId, programs: Programs, given_up: GivenUp },
}

/// Whether the sleep the prober gave up on was answered.
#[derive(Plain)]
#[kind(name = "aether.example.given_up")]
enum GivenUp {
    /// Never, as a job given up on never is.
    Unanswered,
    /// It was.
    Answered,
}

/// The programs that draw a square for each job: the sum's on the left, the sleep's in the middle, and the check's on
/// the right.
#[derive(Plain)]
#[kind(name = "aether.example.programs")]
struct Programs {
    summed: ProgramId,
    slept: ProgramId,
    checked: ProgramId,
}

/// Which program was asked for, with what was made before it.
#[derive(Plain)]
#[kind(name = "aether.example.making")]
enum Making {
    /// The sum's, once the square was made.
    Summed { shape: Shape, square: MeshId },
    /// The sleep's, once the sum's was.
    Slept { shape: Shape, square: MeshId, summed: ProgramId },
    /// The check's, once the sleep's was.
    Checked { square: MeshId, summed: ProgramId, slept: ProgramId },
}

/// What the prober keeps of its sum: the frame it ran in, and the sleep it gives up on once it is answered.
#[derive(Bound)]
#[kind(name = "aether.example.summing")]
struct Summing {
    frame: u64,
    give_up: JobId,
}

/// Which of the prober's sleeps was answered.
#[derive(Plain)]
#[kind(name = "aether.example.sleeping")]
enum Sleeping {
    /// The sleep it shows, run at `ran` until `until`.
    Shown { ran: Time, until: Time },
    /// The sleep it gave up on.
    GivenUp,
    /// The sleep it checks by that the one given up on was never answered.
    Check,
}

impl ChildOf for Prober {
    type Parent = Top;
}

impl Actor for Prober {
    type Config = Shows;
    type Key = ();
    type Handles = (
        On<Start>,
        On<Tick>,
        Answer<Shaped, Nothing>,
        Answer<Meshed, Shape>,
        Answer<Programmed, Making>,
        Answer<Summed, Summing>,
        Answer<Slept, Sleeping>,
        Answer<Placed, Nothing>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(shows: Shows, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Tick>();

        Ok(Self { window: shows.window, frames: 0, stage: Stage::Making })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// The bytes of `values`, as a blob is put.
fn bytes(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_ne_bytes).collect()
}

/// How the square's corners are laid out: a place of two floats.
fn corners() -> VertexLayout {
    let corner = Attribute { location: 0, format: VertexFormat::Float32x2, offset: 0 };

    VertexLayout { stride: 8, attributes: vec![corner] }
}

/// The WGSL of a program that draws the square a fifth of the canvas wide about `across` on it, from -1 at its left
/// to 1 at its right, in the colour `colour`.
fn square_at(across: &str, colour: &str) -> String {
    format!(
        "
@vertex
fn place(@location(0) corner: vec2<f32>) -> @builtin(position) vec4<f32> {{
    return vec4(corner * 0.1 + vec2({across}, 0.0), 0.0, 1.0);
}}

@fragment
fn shade() -> @location(0) vec4<f32> {{
    return vec4({colour}, 1.0);
}}
"
    )
}

/// The program of the WGSL `wgsl` over the square, drawn into `shape`.
fn program<S>(turn: &mut Turn<'_, Prober, S>, wgsl: String, shape: Shape) -> Result<CreateProgram, String> {
    let wgsl = turn.put(Vec::from(wgsl)).map_err(|why| format!("no program asked: {why}"))?;
    let program = CreateProgram {
        wgsl,
        vertex: String::from("place"),
        fragment: String::from("shade"),
        layout: corners(),
        instances: PerInstance::None,
        bindings: Vec::new(),
        state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None },
        shape,
    };

    Ok(program)
}

impl Handles<On<Start>> for Prober {
    type Sends = (Asks<Path<Renderer>, ShapeOf, Nothing>,);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(self.window)), &Nothing)?;

        Ok(())
    }
}

impl Handles<On<Tick>> for Prober {
    type Sends = ();

    fn handle(&mut self, _: Tick, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.frames += 1;

        Ok(())
    }
}

impl Handles<Answer<Shaped, Nothing>> for Prober {
    type Sends = (Asks<Path<Renderer>, CreateMesh, Shape>,);

    fn handle(
        &mut self,
        (answered, _): (Answered<Shaped>, Nothing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Shaped::Shape(shape)) = answered else {
            say(format!("the window's main pass has no shape: {answered:?}"));
            return Ok(());
        };
        let square = bytes([-1.0_f32, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0].map(f32::to_bits));
        let (vertices, indices) = match (turn.put(square), turn.put(bytes([0, 1, 2, 0, 2, 3]))) {
            (Ok(vertices), Ok(indices)) => (vertices, indices),
            (Err(why), _) | (_, Err(why)) => {
                say(format!("nothing asked: {why}"));
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&CreateMesh { layout: corners(), vertices, indices }, &shape)?;

        Ok(())
    }
}

impl Handles<Answer<Meshed, Shape>> for Prober {
    type Sends = (Asks<Path<Renderer>, CreateProgram, Making>,);

    fn handle(
        &mut self,
        (answered, shape): (Answered<Meshed>, Shape),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Meshed::Made(square)) = answered else {
            say(format!("the square was not made: {answered:?}"));
            return Ok(());
        };
        let summed = match program(turn, square_at("-0.5", "0.2, 0.8, 0.3"), shape) {
            Ok(summed) => summed,
            Err(why) => {
                say(why);
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&summed, &Making::Summed { shape, square })?;

        Ok(())
    }
}

/// How long the shown sleep sleeps, and when the check is run, after the jobs are.
const SHOWN: Duration = Duration::from_secs(1);
const CHECKED: Duration = Duration::from_secs(3);

/// How long the sleep given up on would sleep: past the sum's answer, which gives up on it.
const GIVEN_UP: Duration = Duration::from_secs(2);

impl Handles<Answer<Programmed, Making>> for Prober {
    type Sends = (Asks<Path<Renderer>, CreateProgram, Making>,);

    fn handle(
        &mut self,
        (answered, making): (Answered<Programmed>, Making),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let Answered::Yes(Programmed::Made(made)) = answered else {
            say(format!("a program was not made: {answered:?}"));
            return Ok(());
        };
        let (next, shape, asked) = match making {
            Making::Summed { shape, square } => {
                (square_at("0.0", "0.3, 0.5, 1.0"), shape, Making::Slept { shape, square, summed: made })
            }
            Making::Slept { shape, square, summed } => {
                (square_at("0.5", "1.0, 1.0, 1.0"), shape, Making::Checked { square, summed, slept: made })
            }
            Making::Checked { square, summed, slept } => {
                let programs = Programs { summed, slept, checked: made };
                self.stage = Stage::Running { square, programs, given_up: GivenUp::Unanswered };
                return self.run_jobs(turn);
            }
        };
        let program = match program(turn, next, shape) {
            Ok(program) => program,
            Err(why) => {
                say(why);
                return Ok(());
            }
        };
        turn.to(path::<Renderer>(())).ask(&program, &asked)?;

        Ok(())
    }
}

impl Prober {
    /// Runs its jobs at the time of the turn `turn`: the sleep it gives up on, the sum that gives up on it, the sleep it
    /// shows, and the one it checks by.
    fn run_jobs<S>(&self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault> {
        let ran = turn.now();
        let give_up = turn.run(&Sleep { until: ran + GIVEN_UP }, &Sleeping::GivenUp)?;
        turn.run(&Sum { up_to: 1000 }, &Summing { frame: self.frames, give_up })?;
        let until = ran + SHOWN;
        turn.run(&Sleep { until }, &Sleeping::Shown { ran, until })?;
        turn.run(&Sleep { until: ran + CHECKED }, &Sleeping::Check)?;
        say(format!("ran frame={} at_millis={}", self.frames, millis(ran)));

        Ok(())
    }

    /// Places the square with `program`: one of its programs, once its jobs are running.
    fn place(&self, turn: &mut Turn<'_, Self, Places>, program: fn(&Programs) -> ProgramId) -> Result<(), Fault> {
        let Stage::Running { square, programs, .. } = &self.stage else {
            return Ok(());
        };
        let join = Join::to::<Main>(Frame::Main(self.window), program(programs), Vec::new());
        let place = Place { mesh: *square, instances: Drawn::Count(1), joins: vec![join] };
        turn.to(path::<Renderer>(())).ask(&place, &Nothing)?;

        Ok(())
    }
}

/// What a turn that places a square sends.
type Places = (Asks<Path<Renderer>, Place, Nothing>,);

/// The milliseconds of `time`, from when the page opened.
const fn millis(time: Time) -> u64 {
    time.nanos() / 1_000_000
}

impl Handles<Answer<Summed, Summing>> for Prober {
    type Sends = Places;

    fn handle(
        &mut self,
        (answered, summing): (Answered<Summed>, Summing),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        let total = match answered {
            Answered::Yes(Summed { total }) => total,
            Answered::No(why) => {
                say(format!("unanswered sum={why:?}"));
                return Ok(());
            }
        };
        let frames_after = self.frames - summing.frame;
        say(format!("summed total={total} frames_after={frames_after} at_millis={}", millis(turn.now())));
        turn.cancel(summing.give_up);

        self.place(turn, |programs| programs.summed)
    }
}

impl Handles<Answer<Slept, Sleeping>> for Prober {
    type Sends = Places;

    fn handle(
        &mut self,
        (answered, sleeping): (Answered<Slept>, Sleeping),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        if let Answered::No(why) = answered {
            say(format!("unanswered sleep={why:?}"));
            return Ok(());
        }
        match sleeping {
            Sleeping::Shown { ran, until } => {
                let now = turn.now();
                say(format!(
                    "slept asked_millis={} after_millis={} at_millis={}",
                    millis(until) - millis(ran),
                    millis(now) - millis(ran),
                    millis(now)
                ));
                self.place(turn, |programs| programs.slept)
            }
            Sleeping::GivenUp => {
                say(String::from("answered given_up=true"));
                if let Stage::Running { given_up, .. } = &mut self.stage {
                    *given_up = GivenUp::Answered;
                }
                Ok(())
            }
            Sleeping::Check => {
                let Stage::Running { given_up: GivenUp::Unanswered, .. } = self.stage else {
                    say(String::from("checked given_up_answered=true"));
                    return Ok(());
                };
                say(String::from("checked given_up_answered=false"));
                self.place(turn, |programs| programs.checked)
            }
        }
    }
}

impl Handles<Answer<Placed, Nothing>> for Prober {
    type Sends = ();

    fn handle(&mut self, (answered, _): (Answered<Placed>, Nothing), _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(_)) = answered else {
            say(format!("unplaced square={answered:?}"));
            return Ok(());
        };

        Ok(())
    }
}
