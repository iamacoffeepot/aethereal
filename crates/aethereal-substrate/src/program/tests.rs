//! A program bootstrapped on a host of the tests' own: its settings, each root's needs, its ends, and its refusals.

use super::*;
use crate::Bootstrap;
use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top};
use aethereal_data::config::{ConfigError, ConfigFault};
use aethereal_data::{ActorPath, JsonFault, Kind, PathSegment, Plain};
use core::error::Error;
use std::cell::RefCell;
use std::mem;

thread_local! {
    /// What the roots of this thread's test have said as they began, in order.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Notes that a root began.
fn say(line: String) {
    SAID.with_borrow_mut(|said| said.push(line));
}

/// What has been said since this was last asked.
fn said() -> Vec<String> {
    SAID.with_borrow_mut(mem::take)
}

/// A host that keeps the engine, the ends its roots serve it, and the settings it read, and runs nothing.
struct Bench {
    engine: Engine,
    served: Vec<Lit>,
    pace: u32,
    hops: u32,
}

/// The bench's own section.
#[derive(Plain, Debug, Default)]
#[kind(name = "test.bench.running")]
#[settings]
struct BenchRunning {
    pace: u32,
}

impl Host for Bench {
    type Running = BenchRunning;
    type Hosting = Vec<Lit>;
    type Exit = Self;

    fn run(settled: Settled<Self>) -> Result<Self, ProgramError> {
        Self::build(settled)
    }
}

impl Builds for Bench {
    fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
        let pace = settled.running().pace;
        let hops = settled.engine().hops;
        let mut served = Vec::new();
        let engine = settled.bootstrap(&mut served)?;

        Ok(Self { engine, served, pace, hops })
    }
}

/// A light, which the lamp provides, and gives the host as its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lit(u32);

impl Serves<Lit> for Bench {
    fn serve(hosting: &mut Vec<Lit>, end: Lit) {
        hosting.push(end);
    }
}

/// A count, which the counter provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Counted(u32);

/// A root's config: what it says as it begins, and whether it faults instead.
#[derive(Plain)]
#[kind(name = "test.program.line")]
struct Line {
    text: String,
    faults: bool,
}

/// The lamp's settings.
#[derive(Plain, Debug, Default)]
#[kind(name = "test.program.lamp_settings")]
#[settings]
struct LampSettings {
    brightness: u32,
}

/// The scene's settings, and its config.
#[derive(Plain, Debug, Default)]
#[kind(name = "test.program.scene_settings")]
#[settings]
struct SceneSettings {
    seconds: u32,
}

/// A root that says its config's text as it begins, or faults.
macro_rules! root {
    ($name:ident, $named:literal) => {
        #[derive(Plain)]
        #[kind(name = $named)]
        struct $name;

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Line;
            type Key = ();
            type Handles = ();
            type BeginSends = ();
            type EndSends = ();

            fn begin(line: Line, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
                if line.faults {
                    return Err(Fault::new(&line)?);
                }
                say(line.text);

                Ok(Self)
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }
    };
}

root!(Lamp, "test.program.lamp");
root!(Counter, "test.program.counter");
root!(Keeper, "test.program.keeper");
root!(Dimmer, "test.program.dimmer");

/// A root whose config is its settings, read from `scene`.
#[derive(Plain, Bootstrap)]
#[kind(name = "test.program.scene")]
struct Scene;

impl ChildOf for Scene {
    type Parent = Top;
}

impl Actor for Scene {
    type Config = SceneSettings;
    type Key = ();
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(settings: SceneSettings, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        say(format!("scene {}", settings.seconds));

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl<H: Host + Serves<Lit>> Bootstrap<H> for Lamp {
    const SECTION: &'static str = "lamp";
    type Settings = LampSettings;
    type Needs = ();
    type Provides = (Lit,);

    fn bootstrap(settings: LampSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(Lit,), Unbuilt> {
        let lit = Lit(settings.brightness);
        cx.begin(&(), Line { text: format!("lamp {}", lit.0), faults: false })?;
        cx.serve(lit);

        Ok((lit,))
    }
}

impl<H: Host> Bootstrap<H> for Counter {
    const SECTION: &'static str = "counter";
    type Settings = ();
    type Needs = ();
    type Provides = (Counted,);

    fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(Counted,), Unbuilt> {
        cx.begin(&(), Line { text: String::from("counter"), faults: false })?;

        Ok((Counted(2),))
    }
}

impl<H: Host> Bootstrap<H> for Keeper {
    // It names the lamp's section, but reads none, so the name is not repeated.
    const SECTION: &'static str = "lamp";
    type Settings = ();
    type Needs = (Counted, Lit);
    type Provides = ();

    fn bootstrap((): (), (counted, lit): (Counted, Lit), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        let faults = lit.0 == 0;
        cx.begin(&(), Line { text: format!("keeper {} {}", counted.0, lit.0), faults })
    }
}

impl<H: Host> Bootstrap<H> for Dimmer {
    const SECTION: &'static str = "lamp";
    type Settings = SceneSettings;
    type Needs = ();
    type Provides = ();

    fn bootstrap(_: SceneSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        cx.begin(&(), Line { text: String::from("dimmer"), faults: false })
    }
}

/// The roots of the program the tests run, the last listed first.
type Roots = (Scene, (Keeper, (Counter, (Lamp, ()))));

/// The program of the lamp, the counter, the keeper, which needs both, and the scene.
fn program() -> Program<Bench, Roots> {
    Bench::program().root::<Lamp>().root::<Counter>().root::<Keeper>().root::<Scene>()
}

/// A layer of the program `program`, setting each `--set` text.
fn layer<R>(program: &Program<Bench, R>, sets: &[&str]) -> Result<Overlay, Box<dyn Error>> {
    let parts = program.parts()?;
    let mut overlay = Overlay::new("file");
    for text in sets {
        overlay.set(&parts, text)?;
    }

    Ok(overlay)
}

type Outcome = Result<(), Box<dyn Error>>;

#[test]
fn each_root_bootstraps_in_order_from_its_settings_and_what_earlier_roots_provide() -> Outcome {
    // Catches a layer that does not reach a root's section, a need handed another root's value or a value of the
    // wrong type, roots bootstrapped out of their order, and an end that does not reach the host.
    let program = program();
    let code = layer(&program, &["lamp.brightness=7", "scene.seconds=3"])?;

    let bench = program.layer(code).build()?;

    assert_eq!(said(), ["lamp 7", "counter", "keeper 2 7", "scene 3"]);
    assert_eq!(bench.served, [Lit(7)]);
    let scene = ActorPath::from_root(&PathSegment::new(Scene::NAME, &()));
    assert!(bench.engine.table().resolve(&scene).is_some());
    Ok(())
}

#[test]
fn a_repeated_section_is_refused_naming_both_roots_before_any_begins() -> Outcome {
    // Catches two roots handed one section, a root that reads no section counted as reading its name, and a refusal
    // found only once roots have begun.
    let program = Bench::program().root::<Lamp>().root::<Counter>().root::<Keeper>().root::<Dimmer>();

    let refused = program.build();

    let Err(ProgramError::Repeated { name, readers }) = refused else {
        return Err("two sections named lamp are refused".into());
    };
    assert_eq!(name, "lamp");
    assert_eq!(readers, [Reader::Root(Lamp::NAME), Reader::Root(Dimmer::NAME)]);
    assert!(said().is_empty());
    Ok(())
}

#[test]
fn a_setting_that_does_not_exist_stops_the_program_before_any_root_begins() -> Outcome {
    // Catches a bad setting found only as the root that reads it is stood up, after the roots before it began.
    let program = program();
    let code = layer(&program, &["scene.bogus=1"])?;

    let refused = program.layer(code).build();

    let Err(ProgramError::Configuration(error)) = refused else {
        return Err("an unknown setting is refused".into());
    };
    let unknown = ConfigFault::Json(JsonFault::UnknownField);
    assert!(matches!(error, ConfigError::Layer { fault, .. } if fault == unknown));
    assert!(said().is_empty());
    Ok(())
}

#[test]
fn a_failed_bootstrap_names_its_root() -> Outcome {
    // Catches a failure blamed on another root, or one that lets the roots after it bootstrap.
    let refused = program().build();

    let Err(ProgramError::Root { root, why: Unbuilt::Refused(_) }) = refused else {
        return Err("the keeper is refused as it begins".into());
    };
    assert_eq!(root, Keeper::NAME);
    assert_eq!(said(), ["lamp 0", "counter"]);
    Ok(())
}

#[test]
fn the_host_reads_its_own_sections() -> Outcome {
    // Catches a host section left out of the composite, or read from another section's name.
    let program = Bench::program().root::<Scene>();
    let code = layer(&program, &["running.pace=5", "engine.hops=2"])?;

    let bench = program.layer(code).build()?;

    assert_eq!((bench.pace, bench.hops), (5, 2));
    Ok(())
}

#[test]
fn the_programs_defaults_reach_their_sections_beneath_every_layer() -> Outcome {
    // Catches the program's defaults for a root or the host put in another section than the one the root or the host
    // names, left out in favour of the kind's `Default`, or put above a layer rather than beneath it.
    let program = Bench::program()
        .with_engine(EngineSettings { hops: 2, ..Default::default() })
        .with_running(BenchRunning { pace: 5 })
        .root_with::<Lamp>(LampSettings { brightness: 7 })
        .root::<Counter>()
        .root::<Keeper>()
        .root_with::<Scene>(SceneSettings { seconds: 3 });
    let file = layer(&program, &["scene.seconds=4", "engine.hops=1"])?;

    let bench = program.layer(file).build()?;

    assert_eq!(said(), ["lamp 7", "counter", "keeper 2 7", "scene 4"]);
    assert_eq!((bench.pace, bench.hops), (5, 1));
    Ok(())
}
