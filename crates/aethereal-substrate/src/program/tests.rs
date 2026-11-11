//! A program bootstrapped on a host of the tests' own: its settings, each root's needs, its ends, and its refusals.

use super::*;
use crate::Bootstrap;
use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Given, Handle, Publisher, Root, Seated, Top};
use aethereal_data::config::{ConfigError, ConfigFault};
use aethereal_data::{ActorPath, Bound, JsonFault, Kind, KindName, PathSegment, Plain};
use core::error::Error;
use core::pin::Pin;
use core::task::{Context, Poll, Waker};

/// What a bench's hosting holds of what its roots served it, which the test that lends it to a bootstrap reads once the
/// bootstrap has come to its end, whether it built or failed.
#[derive(Default)]
struct Served {
    lights: Vec<Lit>,
    began: Vec<Began>,
}

/// A bench's hosting: what it was served, lent for the bootstrap.
type Hosted<'host> = &'host mut Served;

/// A root that began, by its name, with the values its config carried, given the host as an end once it has.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Began {
    root: KindName,
    values: Vec<u32>,
}

/// The root `T` began with `values`.
fn began<T: Kind>(values: &[u32]) -> Began {
    Began { root: T::NAME, values: values.to_vec() }
}

/// A host that keeps the engine, the ends its roots serve it, and the settings it read, and runs nothing.
struct Bench {
    engine: Engine,
    served: Served,
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
    type Hosting<'host> = Hosted<'host>;
    type Exit = Self;
    type Error = ProgramError;

    fn run(settled: Settled<Self>) -> Result<Self, ProgramError> {
        Self::build(settled)
    }
}

impl Builds for Bench {
    fn build(settled: Settled<Self>) -> Result<Self, ProgramError> {
        let pace = settled.running().pace;
        let hops = settled.engine().hops;
        let mut served = Served::default();
        let (engine, _) = pollster::block_on(settled.bootstrap(&mut served))?;

        Ok(Self { engine, served, pace, hops })
    }
}

/// A light, which the lamp provides, and gives the host as its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lit(u32);

impl Lit {
    /// Whether it gives no light, which a keeper does not begin from.
    const fn is_dark(self) -> bool {
        self.0 == 0
    }
}

impl Serves<Lit> for Bench {
    fn serve(hosting: &mut Hosted<'_>, end: Lit) {
        hosting.lights.push(end);
    }
}

impl Serves<Began> for Bench {
    fn serve(hosting: &mut Hosted<'_>, end: Began) {
        hosting.began.push(end);
    }
}

/// A count, which the counter provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Counted(u32);

/// A root's config: what it keeps as it begins, and whether it faults instead.
#[derive(Plain)]
#[kind(name = "test.program.line")]
struct Line {
    values: Vec<u32>,
    faults: bool,
}

/// Begins the root `A` from `line`, and gives the host its name and values once it has.
fn begin<H, A>(cx: &mut Bootstrapping<'_, H, A>, line: Line) -> Result<(), Unbuilt>
where
    H: Host + Serves<Began>,
    A: Root<Config = Line, Key = ()> + Kind,
{
    let values = line.values.clone();
    cx.begin(&(), line)?;
    cx.serve(Began { root: A::NAME, values });

    Ok(())
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

/// A root that keeps its config's values as it begins, or faults.
macro_rules! root {
    ($name:ident, $named:literal) => {
        #[derive(Plain)]
        #[kind(name = $named)]
        struct $name {
            values: Vec<u32>,
        }

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

                Ok(Self { values: line.values })
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
root!(Waiter, "test.program.waiter");

/// A root whose config is its settings, read from `scene`.
#[derive(Plain, Bootstrap)]
#[kind(name = "test.program.scene")]
struct Scene {
    seconds: u32,
}

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
        Ok(Self { seconds: settings.seconds })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl<H: Host + Serves<Lit> + Serves<Began>> Bootstrap<H> for Lamp {
    const SECTION: &'static str = "lamp";
    type Settings = LampSettings;
    type Needs = ();
    type Provides = (Lit,);

    async fn bootstrap(settings: LampSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(Lit,), Unbuilt> {
        let lit = Lit(settings.brightness);
        begin(cx, Line { values: vec![lit.0], faults: false })?;
        cx.serve(lit);

        Ok((lit,))
    }
}

impl<H: Host + Serves<Began>> Bootstrap<H> for Counter {
    const SECTION: &'static str = "counter";
    type Settings = ();
    type Needs = ();
    type Provides = (Counted,);

    async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(Counted,), Unbuilt> {
        begin(cx, Line { values: Vec::new(), faults: false })?;

        Ok((Counted(2),))
    }
}

impl<H: Host + Serves<Began>> Bootstrap<H> for Keeper {
    // It names the lamp's section, but reads none, so the name is not repeated.
    const SECTION: &'static str = "lamp";
    type Settings = ();
    type Needs = (Counted, Lit);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (counted, lit): (Counted, Lit),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        let faults = lit.is_dark();
        begin(cx, Line { values: vec![counted.0, lit.0], faults })
    }
}

impl<H: Host + Serves<Began>> Bootstrap<H> for Dimmer {
    const SECTION: &'static str = "lamp";
    type Settings = SceneSettings;
    type Needs = ();
    type Provides = ();

    async fn bootstrap(_: SceneSettings, (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        begin(cx, Line { values: Vec::new(), faults: false })
    }
}

/// What a bootstrap that awaits the host's setup waits on: pending as it is first polled, as a GPU opening is, and
/// ready after.
struct Opening {
    polled: bool,
}

impl Future for Opening {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.polled {
            return Poll::Ready(());
        }
        self.polled = true;
        cx.waker().wake_by_ref();

        Poll::Pending
    }
}

impl<H: Host + Serves<Began>> Bootstrap<H> for Waiter {
    const SECTION: &'static str = "waiter";
    type Settings = ();
    type Needs = (Lit,);
    type Provides = ();

    async fn bootstrap((): (), (lit,): (Lit,), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
        Opening { polled: false }.await;
        begin(cx, Line { values: vec![lit.0], faults: false })
    }
}

/// What a publishing root is made from: the program's one publisher.
#[derive(Bound)]
#[kind(name = "test.program.publishes")]
struct Publishes {
    publisher: Given<Publisher>,
}

/// A root that asks for the program's one publisher as it bootstraps and takes it from its config as it begins, as the
/// root that holds a program's code does (ADR-0054).
macro_rules! publishing {
    ($name:ident, $named:literal) => {
        #[derive(Bound)]
        #[kind(name = $named)]
        struct $name {
            publisher: Handle<Publisher>,
        }

        impl ChildOf for $name {
            type Parent = Top;
        }

        impl Actor for $name {
            type Config = Publishes;
            type Key = ();
            type Handles = ();
            type BeginSends = ();
            type EndSends = ();

            fn begin(config: Publishes, turn: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
                Ok(Self { publisher: turn.take(config.publisher)? })
            }

            fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
                Ok(())
            }
        }

        impl<H: Host + Serves<Began>> Bootstrap<H> for $name {
            const SECTION: &'static str = $named;
            type Settings = ();
            type Needs = ();
            type Provides = ();

            async fn bootstrap((): (), (): (), cx: &mut Bootstrapping<'_, H, Self>) -> Result<(), Unbuilt> {
                let publisher = cx.publisher()?;
                cx.begin(&(), Publishes { publisher })?;
                cx.serve(Began { root: Self::NAME, values: Vec::new() });

                Ok(())
            }
        }
    };
}

publishing!(Shelf, "test.program.shelf");
publishing!(Press, "test.program.press");

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

/// The state of the root `T` of `engine`, a singleton.
fn root<T: Actor + Kind>(engine: &Engine) -> Option<&T> {
    let id = crate::path_id(&ActorPath::from_root(&PathSegment::new(T::NAME, &())));

    engine.state(Seated { id, seat: engine.table().seat(id)? })
}

type Outcome = Result<(), Box<dyn Error>>;

#[test]
fn each_root_bootstraps_in_order_from_its_settings_and_what_earlier_roots_provide() -> Outcome {
    // Catches a layer that does not reach a root's section, a need handed another root's value or a value of the
    // wrong type, roots bootstrapped out of their order, and an end that does not reach the host.
    let program = program();
    let code = layer(&program, &["lamp.brightness=7", "scene.seconds=3"])?;

    let bench = program.layer(code).build()?;

    let expected = [began::<Lamp>(&[7]), began::<Counter>(&[]), began::<Keeper>(&[2, 7])];
    assert_eq!(bench.served.began, expected);
    assert_eq!(root::<Lamp>(&bench.engine).map(|lamp| lamp.values.as_slice()), Some(&[7][..]));
    assert_eq!(root::<Keeper>(&bench.engine).map(|keeper| keeper.values.as_slice()), Some(&[2, 7][..]));
    assert_eq!(root::<Scene>(&bench.engine).map(|scene| scene.seconds), Some(3));
    assert_eq!(bench.served.lights, [Lit(7)]);
    let scene = ActorPath::from_root(&PathSegment::new(Scene::NAME, &()));
    assert!(bench.engine.table().resolve(&scene).is_some());
    Ok(())
}

#[test]
fn a_repeated_section_is_refused_naming_both_roots_before_any_begins() -> Outcome {
    // Catches two roots handed one section, a root that reads no section counted as reading its name, and a refusal
    // found only once roots have begun.
    let program = Bench::program().root::<Lamp>().root::<Counter>().root::<Keeper>().root::<Dimmer>();

    // Refused as it settles, before there is an engine for a root to begin on.
    let refused = program.settle();

    let Err(ProgramError::Repeated { name, readers }) = refused else {
        return Err("two sections named lamp are refused".into());
    };
    assert_eq!(name, "lamp");
    assert_eq!(readers, [Reader::Root(Lamp::NAME), Reader::Root(Dimmer::NAME)]);
    Ok(())
}

#[test]
fn a_setting_that_does_not_exist_stops_the_program_before_any_root_begins() -> Outcome {
    // Catches a bad setting found only as the root that reads it is stood up, after the roots before it began.
    let program = program();
    let code = layer(&program, &["scene.bogus=1"])?;

    // Refused as it settles, before there is an engine for a root to begin on.
    let refused = program.layer(code).settle();

    let Err(ProgramError::Configuration(error)) = refused else {
        return Err("an unknown setting is refused".into());
    };
    let unknown = ConfigFault::Json(JsonFault::UnknownField);
    assert!(matches!(error, ConfigError::Layer { fault, .. } if fault == unknown));
    Ok(())
}

#[test]
fn a_failed_bootstrap_names_its_root() -> Outcome {
    // Catches a failure blamed on another root, or one that lets the roots after it bootstrap.
    // The waiter comes after the keeper, and serves what it began as the derived scene cannot.
    let mut hosted = Served::default();

    let refused = pollster::block_on(program().root::<Waiter>().settle()?.bootstrap(&mut hosted));

    let Err(ProgramError::Root { root, why: Unbuilt::Refused(_) }) = refused else {
        return Err("the keeper is refused as it begins".into());
    };
    assert_eq!(root, Keeper::NAME);
    assert_eq!(hosted.began, [began::<Lamp>(&[0]), began::<Counter>(&[])]);
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
fn a_host_that_waits_bootstraps_each_root_after_the_one_it_awaits() -> Outcome {
    // Catches roots after a waiting one bootstrapped before it is done, or an end given before the wait lost after it.
    // The counter comes after the waiter, and serves what it began as the derived scene cannot. Each root is served
    // what it began as its bootstrap is done, so a root bootstrapped before the waiter was done is served before it.
    let settled = Bench::program().root::<Lamp>().root::<Waiter>().root::<Counter>().root::<Scene>().settle()?;
    let mut hosted = Served::default();
    let mut booting = Box::pin(settled.bootstrap(&mut hosted));
    let mut cx = Context::from_waker(Waker::noop());

    let first = booting.as_mut().poll(&mut cx);
    assert!(first.is_pending());
    let Poll::Ready(made) = booting.as_mut().poll(&mut cx) else {
        return Err("the bootstrap is done once what it awaited is".into());
    };

    let (engine, served) = made?;
    assert_eq!(served.began, [began::<Lamp>(&[0]), began::<Waiter>(&[0]), began::<Counter>(&[])]);
    assert_eq!(root::<Scene>(&engine).map(|scene| scene.seconds), Some(0));
    assert_eq!(served.lights, [Lit(0)]);
    let scene = ActorPath::from_root(&PathSegment::new(Scene::NAME, &()));
    assert!(engine.table().resolve(&scene).is_some());
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

    let expected = [began::<Lamp>(&[7]), began::<Counter>(&[]), began::<Keeper>(&[2, 7])];
    assert_eq!(bench.served.began, expected);
    assert_eq!(root::<Scene>(&bench.engine).map(|scene| scene.seconds), Some(4));
    assert_eq!((bench.pace, bench.hops), (5, 1));
    Ok(())
}

#[test]
fn a_second_root_that_asks_for_the_publisher_fails_the_bootstrap() -> Outcome {
    // Catches a program that hands out a second publisher, so two roots could each say which blob a kind is, and a
    // first one handed to no root's config, which its root could then not take.
    let mut hosted = Served::default();

    let refused = pollster::block_on(Bench::program().root::<Shelf>().root::<Press>().settle()?.bootstrap(&mut hosted));

    let Err(ProgramError::Root { root, why: Unbuilt::SecondPublisher }) = refused else {
        return Err("the press is refused the publisher the shelf was handed".into());
    };
    assert_eq!(root, Press::NAME);
    assert_eq!(hosted.began, [began::<Shelf>(&[])]);
    Ok(())
}
