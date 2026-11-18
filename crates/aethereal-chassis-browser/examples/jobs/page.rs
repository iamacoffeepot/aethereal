//! The page's entry points, a job that sums, and an actor that runs it and three sleeps on the page's event loop, one
//! of them given up on, and places a square as each is answered, listing each, the root list's one root.

use crate::said::{Program, say};
use aethereal_actor::{
    Answered, Beginning, Fault, Job, JobId, Render, Runner, Running, Sleep, Slept, Start, Tick, Time, Turn, actor, path,
};
use aethereal_chassis_browser::Browser;
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::draw_list::{DrawList, Entry, Pointer};
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::mesh::{Attribute, CreateMesh, MeshId, Meshed, VertexFormat, VertexLayout};
use aethereal_render::pass::Main;
use aethereal_render::placement::{Drawn, Join, Place, Placed, PlacementId};
use aethereal_render::program::{
    Blend, Coverage, CreateProgram, Cull, Depth, Fixed, PerInstance, ProgramId, Programmed,
};
use aethereal_render::shape::Shape;
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{InitialWindows, WindowId, Windows};
use core::time::Duration;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;

/// Bootstraps the program on the page, its jobs run on the page's event loop, with the window root, which opens the
/// page's canvas as the one window, the renderer, which opens WebGPU on it, and the prober, the root list.
///
/// # Errors
///
/// When the canvas or WebGPU on it will not open, the page will not be listened to, or a root does not begin.
#[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
pub async fn start() -> Result<Program, JsError> {
    let program = Browser::program()
        .with_cycle::<(Tick, Render)>()
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Prober>()
        .draws::<(Prober,)>();

    Ok(Program::from(program.run()?.await?))
}

/// The prober draws on the one window the window root opened.
impl<H: Host> Bootstrap<H> for Prober {
    const SECTION: &'static str = "prober";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), Shows { window: windows.first().id() })?;

        Ok(())
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
/// places a square as each is answered, and lists it.
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
    /// Its jobs are running: it places `square` with one of `programs` as each is answered, and lists each placed, in
    /// the order placed.
    Running { square: MeshId, programs: Programs, given_up: GivenUp, placed: Vec<PlacementId> },
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

#[actor(depends(Renderer))]
impl Prober {
    fn begin(shows: Shows, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Tick>();

        Ok(Self { window: shows.window, frames: 0, stage: Stage::Making })
    }

    #[on]
    fn start(&mut self, _: Start, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.to(path::<Renderer>(())).ask(&ShapeOf::pass::<Main>(Frame::Main(self.window)), &Nothing)?;

        Ok(())
    }

    #[on]
    fn tick(&mut self, _: Tick) -> Result<(), Fault> {
        self.frames += 1;

        Ok(())
    }

    #[answer]
    fn shaped(&mut self, (answered, _): (Answered<Shaped>, Nothing), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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

    #[answer]
    fn meshed(&mut self, (answered, shape): (Answered<Meshed>, Shape), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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

    #[answer]
    fn programmed(
        &mut self,
        (answered, making): (Answered<Programmed>, Making),
        turn: &mut Turn<'_, Self>,
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
                self.stage = Stage::Running { square, programs, given_up: GivenUp::Unanswered, placed: Vec::new() };
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

    #[answer]
    fn summed(
        &mut self,
        (answered, summing): (Answered<Summed>, Summing),
        turn: &mut Turn<'_, Self>,
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

    #[answer]
    fn slept(
        &mut self,
        (answered, sleeping): (Answered<Slept>, Sleeping),
        turn: &mut Turn<'_, Self>,
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

    /// A square placed is listed after those placed before it.
    #[answer]
    fn placed(&mut self, (answered, _): (Answered<Placed>, Nothing), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Answered::Yes(Placed::Made(square)) = answered else {
            say(format!("unplaced square={answered:?}"));
            return Ok(());
        };
        let Stage::Running { placed, .. } = &mut self.stage else {
            return Ok(());
        };
        placed.push(square);
        let entries = placed.iter().map(|placement| Entry::Draw { placement: *placement, pointer: Pointer::Ignores });
        turn.to(path::<Renderer>(())).send(&DrawList { entries: entries.collect() })?;

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
fn program(turn: &mut Turn<'_, Prober>, wgsl: String, shape: Shape) -> Result<CreateProgram, String> {
    let wgsl = turn.put(Vec::from(wgsl)).map_err(|why| format!("no program asked: {why}"))?;
    let program = CreateProgram {
        wgsl,
        vertex: String::from("place"),
        fragment: String::from("shade"),
        layout: corners(),
        instances: PerInstance::None,
        bindings: Vec::new(),
        reads: Vec::new(),
        state: Fixed { blend: Blend::Opaque, depth: Depth::Off, cull: Cull::None, coverage: Coverage::Off },
        shape,
    };

    Ok(program)
}

/// How long the shown sleep sleeps, and when the check is run, after the jobs are.
const SHOWN: Duration = Duration::from_secs(1);
const CHECKED: Duration = Duration::from_secs(3);

/// How long the sleep given up on would sleep: past the sum's answer, which gives up on it.
const GIVEN_UP: Duration = Duration::from_secs(2);

impl Prober {
    /// Runs its jobs at the time of the turn `turn`: the sleep it gives up on, the sum that gives up on it, the sleep
    /// it shows, and the one it checks by.
    fn run_jobs(&self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
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
    fn place(&self, turn: &mut Turn<'_, Self>, program: fn(&Programs) -> ProgramId) -> Result<(), Fault> {
        let Stage::Running { square, programs, .. } = &self.stage else {
            return Ok(());
        };
        let join = Join::to::<Main>(Frame::Main(self.window), program(programs), Vec::new());
        let place = Place { mesh: *square, instances: Drawn::Count(1), joins: vec![join] };
        turn.to(path::<Renderer>(())).ask(&place, &Nothing)?;

        Ok(())
    }
}

/// The milliseconds of `time`, from when the page opened.
const fn millis(time: Time) -> u64 {
    time.nanos() / 1_000_000
}
