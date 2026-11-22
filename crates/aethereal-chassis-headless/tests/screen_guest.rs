//! Spike: a screen written with `view!` (ADR-0056), built as a guest, run by the headless engine with wasmtime. A
//! stand-in for the renderer, of the renderer's own kind, answers what the screen asks of it with ids, as the renderer
//! would, and draws nothing; the window root is the real one. What the screen sent the renderer is read from the
//! engine's record of the stand-in's turns, and what the screen counted is asked of it by typed mail.
#![cfg(not(target_arch = "wasm32"))]
#![allow(clippy::print_stderr, reason = "spike: the measurements are printed")]

use aethereal_actor::{ActorRef, Answered, Fault, Held, Prepare, ProtocolRef, Render, Subscribers, Tick, Turn, actor};
use aethereal_chassis_headless::guest::Runtime;
use aethereal_data::{Bound, Budget, Kind, KindId, Plain, from_bytes};
use aethereal_guest_screen as screen;
use aethereal_render::dispatch::DispatchRan;
use aethereal_render::draw_list::{DrawList, Entry, Pointer};
use aethereal_render::font::Font as FontFile;
use aethereal_render::frame::{Frame, ShapeOf, Shaped};
use aethereal_render::instances::{CreateInstances, Instanced, WriteInstances};
use aethereal_render::mesh::{CreateMesh, Meshed};
use aethereal_render::placement::{Place, Placed};
use aethereal_render::program::{CreateProgram, Programmed};
use aethereal_render::readback::{Pixels, StorageContents, TargetPixels};
use aethereal_render::shape::{ColourFormat, Shape};
use aethereal_render::stats::{FrameStats, GpuTimes};
use aethereal_render::text::{
    CreateText, FontMetrics, MoveText, PlaceText, ReadMetrics, TextMade, TextPlaced, WriteText,
};
use aethereal_render::texture::{CreateTexture, Textured};
use aethereal_render::timing::Timed;
use aethereal_render::uniforms::{CreateUniforms, Uniformed, WriteUniforms};
use aethereal_render::{Release, Renderer};
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Handed, Harness, Turned};
use aethereal_window::{Button, Hearer, Opening, Over, Pointed, PointerEvent, WindowId, Windows};
use core::time::Duration;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;

/// The screen guest the build script built.
const SCREEN: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_SCREEN"));

/// The renderer's own monospace font, whose metrics the stand-in hands the screen.
const MONO: &[u8] = include_bytes!("../../aethereal-render/fonts/IBMPlexMono-Regular.ttf");

/// One frame's time.
const FRAME: Duration = Duration::from_millis(16);

/// A place in the stand-in's state where the renderer keeps a resource's handle: a `u64`, as a handle is, so the
/// stand-in's state has the renderer's layout, and so its kind.
#[derive(Debug, Clone, Copy, Default, Plain)]
struct Slot {
    count: u64,
}

/// What the renderer owes, laid out as its own is.
#[derive(Bound)]
struct Owed {
    texts: Vec<(u64, Held<TextMade>)>,
    frames: Vec<(WindowId, Held<Pixels>)>,
    timings: Vec<Held<Timed>>,
    reading: Vec<(u64, Held<Pixels>)>,
    targets: Vec<(u64, Held<TargetPixels>)>,
    storing: Vec<(u64, Held<StorageContents>)>,
    timing: Vec<(u64, Held<Timed>)>,
    dispatches: Vec<(u64, ProtocolRef<(DispatchRan,)>)>,
}

/// Who is sent each frame's statistics, laid out as the renderer's own is.
#[derive(Bound)]
enum Watch {
    Unwatched,
    #[allow(dead_code, reason = "a variant of the renderer's layout")]
    Watched {
        subscribers: Subscribers<(FrameStats,)>,
        uploaded_bytes: u64,
    },
}

/// A stand-in for the renderer: the renderer's kind and layout, so a screen's dependency on the renderer is met by it.
/// It answers each ask with a new id, counted from one, and draws nothing.
#[derive(Bound)]
#[kind(name = "aether.render")]
struct StandIn {
    made: Slot,
    lists: Slot,
    writes: Slot,
    released: Slot,
    spare_one: Slot,
    spare_two: Slot,
    spare_three: Slot,
    owed: Owed,
    watch: Watch,
    timed: Subscribers<(GpuTimes,)>,
}

/// What the stand-in is made from: nothing.
#[derive(Plain)]
#[kind(name = "aether.test.stand_in")]
struct StandInConfig;

impl StandIn {
    /// A new id of any of the renderer's id types: an id is its number.
    fn next<T: Plain>(&mut self) -> Result<T, Fault> {
        self.made.count += 1;
        let Ok(id) = from_bytes(&self.made.count.to_le_bytes(), &mut Budget::new(8)) else {
            return Err(Fault::new(&StandInConfig)?);
        };

        Ok(id)
    }
}

#[actor]
impl StandIn {
    fn begin(_: StandInConfig) -> Result<Self, Fault> {
        let owed = Owed {
            texts: Vec::new(),
            frames: Vec::new(),
            timings: Vec::new(),
            reading: Vec::new(),
            targets: Vec::new(),
            storing: Vec::new(),
            timing: Vec::new(),
            dispatches: Vec::new(),
        };

        Ok(Self {
            made: Slot::default(),
            lists: Slot::default(),
            writes: Slot::default(),
            released: Slot::default(),
            spare_one: Slot::default(),
            spare_two: Slot::default(),
            spare_three: Slot::default(),
            owed,
            watch: Watch::Unwatched,
            timed: Subscribers::new(),
        })
    }

    #[request]
    fn shape_of(&mut self, _: ShapeOf) -> Result<Shaped, Fault> {
        Ok(Shaped::Shape(Shape::flat(ColourFormat::Bgra8)))
    }

    #[request]
    fn create_mesh(&mut self, _: CreateMesh) -> Result<Meshed, Fault> {
        Ok(Meshed::Made(self.next()?))
    }

    #[request]
    fn create_program(&mut self, _: CreateProgram) -> Result<Programmed, Fault> {
        Ok(Programmed::Made(self.next()?))
    }

    #[request]
    fn create_uniforms(&mut self, _: CreateUniforms) -> Result<Uniformed, Fault> {
        Ok(Uniformed::Made(self.next()?))
    }

    #[request]
    fn create_texture(&mut self, _: CreateTexture) -> Result<Textured, Fault> {
        Ok(Textured::Made(self.next()?))
    }

    #[request]
    fn create_instances(&mut self, _: CreateInstances) -> Result<Instanced, Fault> {
        Ok(Instanced::Made(self.next()?))
    }

    #[request]
    fn place(&mut self, _: Place) -> Result<Placed, Fault> {
        Ok(Placed::Made(self.next()?))
    }

    #[request]
    fn create_text(&mut self, _: CreateText) -> Result<TextMade, Fault> {
        Ok(TextMade::Made(self.next()?))
    }

    #[request]
    fn place_text(&mut self, _: PlaceText) -> Result<TextPlaced, Fault> {
        Ok(TextPlaced::Made(self.next()?))
    }

    #[request]
    fn read_metrics(&mut self, _: ReadMetrics, turn: &mut Turn<'_, Self>) -> Result<FontMetrics, Fault> {
        let Ok(font) = FontFile::read(MONO) else {
            return Err(Fault::new(&StandInConfig)?);
        };
        let Ok(own) = turn.put(font.metrics().encode()) else {
            return Err(Fault::new(&StandInConfig)?);
        };

        Ok(FontMetrics::Read { own, fallbacks: Vec::new() })
    }

    #[tell]
    fn draw_list(&mut self, _: DrawList) -> Result<(), Fault> {
        self.lists.count += 1;
        Ok(())
    }

    #[tell]
    fn write_instances(&mut self, _: WriteInstances) -> Result<(), Fault> {
        self.writes.count += 1;
        Ok(())
    }

    #[tell]
    fn write_uniforms(&mut self, _: WriteUniforms) -> Result<(), Fault> {
        self.writes.count += 1;
        Ok(())
    }

    #[tell]
    fn write_text(&mut self, _: WriteText) -> Result<(), Fault> {
        self.writes.count += 1;
        Ok(())
    }

    #[tell]
    fn move_text(&mut self, _: MoveText) -> Result<(), Fault> {
        self.writes.count += 1;
        Ok(())
    }

    #[tell]
    fn release(&mut self, _: Release) -> Result<(), Fault> {
        self.released.count += 1;
        Ok(())
    }
}

/// The window the screen draws into: the first, as the window root numbers them.
fn window() -> WindowId {
    let Ok(window) = from_bytes(&0_u64.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a window's id is its number");
    };

    window
}

/// A program on two parts that runs guests with wasmtime and runs `Tick`, `Prepare`, and `Render`: the window root,
/// opening no window; the stand-in renderer; and the screen guest, made from its blob. Its harness, the stand-in, and
/// the screen, with how long putting the guest's code took (compiling it) and making the screen took (instantiating
/// it and running its `begin`).
fn program(module: &[u8]) -> (Harness<Bench>, ActorRef<StandIn>, ActorRef<screen::Counter>, Duration, Duration) {
    let Some(parts) = NonZeroUsize::new(2) else {
        panic!("two is not zero");
    };
    // The blobs' budget holds a debug build of the guest, about 40 MB, with room to spare.
    let mut setup = Engine::new(parts, 1024, 1 << 20, 1 << 27, 64)
        .with_guests(Arc::new(Runtime::default()))
        .with_cycle::<(Tick, Prepare, Render)>();
    let (config, _hosting) = Opening::new(setup.source(), setup.controller()).config();
    let mut engine = setup.ready();
    let Ok(_) = engine.add_root::<Windows>(&(), config) else {
        panic!("the window root begins");
    };
    let Ok(_) = engine.add_root::<StandIn>(&(), StandInConfig) else {
        panic!("the stand-in renderer begins");
    };

    let putting = Instant::now();
    let code = match engine.put_code(module.to_vec()) {
        Ok(code) => code,
        Err(refused) => panic!("the screen guest is fit to run: {refused:?}"),
    };
    let compiled = putting.elapsed();

    let making = Instant::now();
    let made =
        engine.add_root_from::<screen::Counter>(&code, &(), &screen::CounterConfig { frame: Frame::Main(window()) });
    let instantiated = making.elapsed();
    if let Err(refused) = made {
        panic!("the screen begins: {refused:?}");
    }

    let Ok(mut harness) = Harness::bare(engine) else {
        panic!("the harness seats its outside");
    };
    let stand_in = harness.root::<StandIn>();
    let counter = harness.root::<screen::Counter>();

    (harness, stand_in, counter, compiled, instantiated)
}

/// Runs a frame and every chain it starts, and answers with the turns that ran.
fn frame(harness: &mut Harness<Bench>) -> Vec<Turned> {
    harness.advance(FRAME);

    harness.settle_turns()
}

/// Each `K` `actor` was handed among `turned`, in order.
fn handed<A, K: Kind + aethereal_data::Decode<aethereal_actor::Actors>>(
    turned: &[Turned],
    actor: ActorRef<A>,
) -> Vec<K> {
    let of_actor = turned.iter().filter(|turn| turn.actor == actor.seated());
    let mail = of_actor.filter_map(|turn| match &turn.handed {
        Handed::Mail(arrival) => arrival.read::<K>().ok().flatten(),
        _ => None,
    });

    mail.collect()
}

/// How many clicks the screen says it has counted.
fn clicks(harness: &mut Harness<Bench>, counter: ActorRef<screen::Counter>) -> u32 {
    let Ok(asked) = harness.ask(counter, &screen::CountClicks) else {
        panic!("the count is asked");
    };
    let Ok(Answered::Yes(clicks)) = harness.answer(asked) else {
        panic!("the screen answers its count");
    };

    clicks.clicks
}

/// The entry of `list` the pointer reaches that blocks it, last in the list, and the middle of where it is: the
/// button's, as the label's text is drawn on it and lets the pointer through.
fn button(list: &DrawList) -> (u32, f32, f32) {
    let reached = list.entries.iter().enumerate().filter_map(|(at, entry)| match entry {
        Entry::Hit { rect, pointer: Pointer::Blocks(_), .. } => {
            let middle = rect.center();
            Some((at, middle.x, middle.y))
        }
        Entry::Draw { pointer: Pointer::Blocks(_), .. } => Some((at, 1.0, 1.0)),
        _ => None,
    });
    let Some((at, x, y)) = reached.last() else {
        panic!("the list holds an entry that blocks the pointer: {list:?}");
    };

    (u32::try_from(at).unwrap_or(u32::MAX), x, y)
}

#[test]
fn the_stand_in_has_the_renderer_s_kind() {
    // Tripwire: the stand-in meets a screen's dependency on the renderer only while its kind is the renderer's, by name
    // and layout (ADR-0006, ADR-0059).
    assert_eq!(KindId::of::<StandIn>(), KindId::of::<Renderer>());
}

#[test]
fn a_screen_guest_draws_and_counts_a_click() {
    let (mut harness, stand_in, counter, compiled, instantiated) = program(SCREEN);

    let mut turned = Vec::new();
    for _ in 0..6 {
        turned.extend(frame(&mut harness));
    }
    let prepared = turned.iter().filter(|turn| turn.actor == counter.seated()).count();
    let lists: Vec<DrawList> = handed(&turned, stand_in);
    let made: Vec<CreateText> = handed(&turned, stand_in);
    eprintln!("screen turns over six frames: {prepared}; lists sent: {}; texts made: {made:?}", lists.len());
    let Some(list) = lists.last() else {
        panic!("the screen sent the renderer a draw list");
    };
    eprintln!("last list: {list:#?}");
    assert_eq!(clicks(&mut harness, counter), 0);

    let (entry, x, y) = button(list);
    let over = Over::Entry { actor: counter.protocol::<Hearer>(), entry, x, y };
    let down = Pointed { window: window(), entry, event: PointerEvent::PointerDown { x, y, button: Button::Main } };
    let up = Pointed { window: window(), entry, event: PointerEvent::PointerUp { x, y, button: Button::Main, over } };
    let Ok(()) = harness.tell(counter, &down) else {
        panic!("the press is sent");
    };
    let Ok(()) = harness.tell(counter, &up) else {
        panic!("the release is sent");
    };
    let clicking = Instant::now();
    let clicked = harness.settle_turns();
    let click_took = clicking.elapsed();
    eprintln!("click turns: {}", clicked.len());
    assert_eq!(clicks(&mut harness, counter), 1);

    let relaying = Instant::now();
    let after = frame(&mut harness);
    let relaid_took = relaying.elapsed();
    let written: Vec<WriteText> = handed(&after, stand_in);
    let made_after: Vec<CreateText> = handed(&after, stand_in);
    eprintln!("after the click: written {written:?}, made {made_after:?}");
    let runs = written.iter().flat_map(|write| &write.runs).chain(made_after.iter().flat_map(|made| &made.runs));
    let texts: Vec<&str> = runs.map(|run| run.text.as_str()).collect();
    assert!(texts.contains(&"Clicks: 1"), "the label shows the click: {texts:?}");

    let unchanged = Instant::now();
    let idle = frame(&mut harness);
    let idle_took = unchanged.elapsed();
    let idle_sent = idle.iter().filter(|turn| turn.actor == stand_in.seated()).count();

    eprintln!(
        "module_bytes={} compile_micros={} instantiate_micros={} click_micros={} relayout_frame_micros={} \
         unchanged_frame_micros={} unchanged_frame_renderer_turns={idle_sent}",
        SCREEN.len(),
        compiled.as_micros(),
        instantiated.as_micros(),
        click_took.as_micros(),
        relaid_took.as_micros(),
        idle_took.as_micros(),
    );
}

#[test]
#[ignore = "spike: measures a module built elsewhere, named by SCREEN_GUEST_MODULE"]
fn a_module_named_by_the_environment_loads_and_draws() {
    #[allow(clippy::disallowed_methods, reason = "spike: the module to measure is named from outside")]
    let Some(path) = std::env::var_os("SCREEN_GUEST_MODULE") else {
        panic!("SCREEN_GUEST_MODULE names a module");
    };
    let Ok(module) = std::fs::read(&path) else {
        panic!("the module reads");
    };
    let (mut harness, stand_in, _, compiled, instantiated) = program(&module);
    let mut turned = Vec::new();
    for _ in 0..6 {
        turned.extend(frame(&mut harness));
    }
    let lists: Vec<DrawList> = handed(&turned, stand_in);
    assert!(!lists.is_empty(), "the screen sent the renderer a draw list");

    eprintln!(
        "module_bytes={} compile_micros={} instantiate_micros={}",
        module.len(),
        compiled.as_micros(),
        instantiated.as_micros()
    );
}
