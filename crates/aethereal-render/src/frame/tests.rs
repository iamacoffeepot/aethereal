//! A frame's check, and its builder: what WebGPU would refuse as the frame is drawn is refused as it is made, and the
//! builder builds what the check lets through.

use super::builder::{Effect, Float, Onto, Uint, depth, samples, scale};
use super::*;
use crate::dispatch::Use;
use crate::effect::{EffectId, MadeEffect};
use crate::instances::InstancesId;
use crate::mesh::{MeshId, VertexLayout};
use crate::placement::{Fits, Reads, Resource};
use crate::program::BindingKind;
use crate::storage::StorageId;
use crate::texture::{TextureFormat, TextureId};
use crate::uniforms::UniformsId;
use aethereal_data::{Budget, from_bytes};

/// The window opened `nth`, as a host's window capability numbers it.
fn window(nth: u64) -> WindowId {
    let Ok(window) = from_bytes(&nth.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a window's id is its number");
    };

    window
}

/// The effect that glazes a target of floats with one of whole numbers onto the window, binding uniforms of 16 bytes.
const GLAZE: ProgramId = ProgramId { id: 7 };
/// The effect that blurs a target of floats into another.
const BLUR: ProgramId = ProgramId { id: 8 };
/// The effect that draws a sky into a target of floats, reading nothing.
const SKY: ProgramId = ProgramId { id: 9 };
/// The effect that lays one target of floats over another onto the window.
const LAY: ProgramId = ProgramId { id: 10 };
/// Uniforms of 16 bytes.
const TINT: UniformsId = UniformsId { id: 4 };
/// The longest side of a texture the test's GPU makes.
const LONGEST: u32 = 2048;

/// What a test keeps: the window [`window`] `0`, of `Bgra8`; [`GLAZE`]; [`BLUR`]; [`SKY`]; [`LAY`]; and [`TINT`].
struct Kept {
    glaze: (Vec<Read>, Vec<Binding>),
    blur: Vec<Read>,
}

impl Kept {
    fn new() -> Self {
        let tint = Binding { group: 1, binding: 0, kind: BindingKind::Uniform { bytes: 16 } };

        Self { glaze: (vec![Read::Float, Read::Uint], vec![tint]), blur: vec![Read::Float] }
    }
}

impl Finds for Kept {
    fn pass(&self, _: Frame, _: PassName) -> Result<Fits, JoinRefused> {
        Err(JoinRefused::Frame)
    }

    fn program(&self, _: ProgramId) -> Option<Reads<'_>> {
        None
    }

    fn mesh(&self, _: MeshId) -> Option<&VertexLayout> {
        None
    }

    fn instances(&self, _: InstancesId) -> Option<&VertexLayout> {
        None
    }

    fn uniforms(&self, id: UniformsId) -> Option<u32> {
        (id == TINT).then_some(16)
    }

    fn storage(&self, _: StorageId) -> Option<u32> {
        None
    }

    fn has_texture(&self, _: TextureId) -> bool {
        false
    }

    fn drawn_by(&self, _: TextureId) -> Option<u64> {
        None
    }

    fn reads(&self) -> Vec<(u64, u64)> {
        Vec::new()
    }

    fn uses(&self, _: StorageId) -> Vec<Use> {
        Vec::new()
    }
}

impl Holds for Kept {
    fn window(&self, window: WindowId) -> Option<ColourFormat> {
        (window == self::window(0)).then_some(ColourFormat::Bgra8)
    }

    fn longest(&self) -> u32 {
        LONGEST
    }

    fn effect(&self, id: ProgramId) -> Option<EffectReads<'_>> {
        let (reads, bindings, colour) = match id {
            GLAZE => (self.glaze.0.as_slice(), self.glaze.1.as_slice(), ColourFormat::Bgra8),
            BLUR => (self.blur.as_slice(), [].as_slice(), ColourFormat::Rgba16Float),
            SKY => ([].as_slice(), [].as_slice(), ColourFormat::Rgba16Float),
            LAY => ([Read::Float, Read::Float].as_slice(), [].as_slice(), ColourFormat::Bgra8),
            _ => return None,
        };

        Some(EffectReads { reads, bindings, shape: Shape::flat(colour) })
    }
}

/// Passes named by types, as actors name them.
macro_rules! passes {
    ($($name:ident $kind:literal),*) => {$(
        #[derive(Plain)]
        #[kind(name = $kind)]
        struct $name;

        impl Pass for $name {}
    )*};
}

passes!(Surfaces "aether.test.surfaces", Splats "aether.test.splats", Glaze "aether.test.glaze", Blur "aether.test.blur", Ids "aether.test.ids", Simulate "aether.test.simulate", Sky "aether.test.sky", Floor "aether.test.floor", Rims "aether.test.rims", Lay "aether.test.lay");

/// [`GLAZE`], as the builder takes it.
fn glaze() -> EffectId<(Float, Uint)> {
    let made = MadeEffect { program: GLAZE, reads: vec![Read::Float, Read::Uint] };
    let Some(glaze) = EffectId::of(&made) else {
        panic!("the glaze reads floats and whole numbers");
    };

    glaze
}

/// A target holding `kind`, of the end's size.
const fn full(kind: TargetKind) -> TargetSpec {
    TargetSpec { kind, scale: Scale::Full, samples: Samples::One }
}

/// The glaze bound to [`TINT`].
fn tinted() -> Vec<Bind> {
    vec![Bind { group: 1, binding: 0, to: Resource::Uniforms(TINT) }]
}

/// A frame onto the window: surfaces drawn into whole numbers, writing depth; splats drawn into floats, tested against
/// it; and the glaze of both onto the window.
fn frame() -> CreateFrame {
    let mut frame = FrameBuilder::on(Ends::Window(window(0)));
    let (ids, depth) = frame.draw(Surfaces, Uint, scale::Full, samples::One, depth::Write, ());
    let splats = frame.draw(Splats, Float, scale::Full, samples::One, depth::Test(&depth), ());
    let Ok(create) = frame.finish(Glaze, Effect::new(&glaze(), (splats, ids)).bound(tinted())) else {
        panic!("the frame's passes are of types of their own");
    };

    create
}

#[test]
fn the_builder_describes_each_pass_over_the_targets_it_returned() {
    // Catches a pass described over a target other than the one its handle names, a depth written where it is
    // tested, a load that keeps what was never drawn, and an effect reading its targets out of order.
    let named = |pass: PassName, does: Does| PassSpec { name: pass, does };
    let expected = CreateFrame {
        ends: Ends::Window(window(0)),
        targets: vec![full(TargetKind::Uint), full(TargetKind::Depth), full(TargetKind::Float)],
        passes: vec![
            named(
                PassName::of::<Surfaces>(),
                Does::Draw {
                    into: Target::Texture(0),
                    depth: DepthUse::Write(1),
                    load: Load::Clear,
                    reads: Vec::new(),
                },
            ),
            named(
                PassName::of::<Splats>(),
                Does::Draw { into: Target::Texture(2), depth: DepthUse::Test(1), load: Load::Clear, reads: Vec::new() },
            ),
            named(
                PassName::of::<Glaze>(),
                Does::Effect {
                    program: GLAZE,
                    reads: vec![Target::Texture(2), Target::Texture(0)],
                    into: Target::End,
                    bindings: tinted(),
                },
            ),
        ],
    };

    assert_eq!(frame(), expected);
    assert_eq!(check(&frame(), &Kept::new()), Ok(()));
}

#[test]
fn the_builder_refuses_two_passes_of_one_type_before_anything_is_sent() {
    // Catches a frame whose placements would join one of two passes of a name, whichever the renderer found first.
    let mut frame = FrameBuilder::on(Ends::Window(window(0)));
    let (ids, depth) = frame.draw(Surfaces, Uint, scale::Full, samples::One, depth::Write, ());
    let splats = frame.draw(Surfaces, Float, scale::Full, samples::One, depth::Test(&depth), ());

    assert_eq!(frame.finish(Glaze, Effect::new(&glaze(), (splats, ids))), Err(FrameRefused::Repeated { pass: 1 }));
}

#[test]
fn a_frame_drawing_over_kept_targets_keeps_what_its_draws_left() {
    // Catches a draw over a target described as cleared, which would draw on black what the passes before drew.
    let mut frame = FrameBuilder::on(Ends::Window(window(0)));
    let (scene, depth) = frame.draw(Surfaces, Float, scale::Full, samples::One, depth::Write, ());
    let scene = frame.draw_over(Splats, scene, depth::Test(&depth), ());
    let blur = MadeEffect { program: BLUR, reads: vec![Read::Float] };
    let Some(blur) = EffectId::<(Float,)>::of(&blur) else {
        panic!("the blur reads floats");
    };
    let blurred = frame.effect(Blur, Effect::new(&blur, (scene,)), Float, scale::Full);
    let ids = frame.draw(Ids, Uint, scale::Full, samples::One, depth::Off, ());
    let Ok(create) = frame.finish(Glaze, Effect::new(&glaze(), (blurred, ids)).bound(tinted())) else {
        panic!("the frame's passes are of types of their own");
    };

    let over = Does::Draw { into: Target::Texture(0), depth: DepthUse::Test(1), load: Load::Keep, reads: Vec::new() };
    assert_eq!(create.passes[1].does, over);
    assert_eq!(check(&create, &Kept::new()), Ok(()));
}

#[test]
fn a_frame_begins_with_an_effect_that_reads_nothing() {
    // Catches an effect reading nothing described as reading something, and the check refusing it as the first pass,
    // which no earlier pass wrote for: what it draws into is its own, begun from black.
    let mut frame = FrameBuilder::on(Ends::Window(window(0)));
    let sky = MadeEffect { program: SKY, reads: Vec::new() };
    let Some(sky) = EffectId::<()>::of(&sky) else {
        panic!("the sky reads nothing");
    };
    let sky = frame.effect(Sky, Effect::new(&sky, ()), Float, scale::Full);
    let scene = frame.draw_over(Splats, sky, depth::Off, ());
    let ids = frame.draw(Ids, Uint, scale::Full, samples::One, depth::Off, ());
    let Ok(create) = frame.finish(Glaze, Effect::new(&glaze(), (scene, ids)).bound(tinted())) else {
        panic!("the frame's passes are of types of their own");
    };

    let begun = Does::Effect { program: SKY, reads: Vec::new(), into: Target::Texture(0), bindings: Vec::new() };
    assert_eq!(create.passes[0].does, begun);
    assert_eq!(check(&create, &Kept::new()), Ok(()));
}

/// [`frame`], with `change` made to it.
fn changed(change: impl FnOnce(&mut CreateFrame)) -> CreateFrame {
    let mut frame = frame();
    change(&mut frame);

    frame
}

/// What the check answers `frame` of its pass `pass`, refused for `why`.
const fn refused(pass: u32, why: PassRefused) -> Result<(), FrameRefused> {
    Err(FrameRefused::Pass { pass, why })
}

#[test]
fn a_frame_on_no_window_without_passes_or_not_ending_on_its_end_is_refused() {
    // Catches a frame on a window nothing draws to, one that draws nothing, two passes a placement could not tell
    // apart, and a frame whose end nothing writes, which shows what the window last held.
    let kept = Kept::new();
    let elsewhere = changed(|frame| frame.ends = Ends::Window(window(1)));
    let empty = changed(|frame| frame.passes.clear());
    let repeated = changed(|frame| frame.passes[1].name = frame.passes[0].name);
    let unended = changed(|frame| {
        frame.passes.pop();
    });

    assert_eq!(check(&elsewhere, &kept), Err(FrameRefused::Window));
    assert_eq!(check(&empty, &kept), Err(FrameRefused::Empty));
    assert_eq!(check(&repeated, &kept), Err(FrameRefused::Repeated { pass: 1 }));
    assert_eq!(check(&unended, &kept), Err(FrameRefused::Unended));
}

#[test]
fn a_pass_using_a_target_the_frame_has_not_or_of_the_other_sort_is_refused() {
    // Catches a target past the last one, colour drawn into depth, and depth that is colour: each is a texture the
    // pass cannot be begun with.
    let kept = Kept::new();
    let past = changed(|frame| {
        frame.passes[0].does =
            Does::Draw { into: Target::Texture(9), depth: DepthUse::Off, load: Load::Clear, reads: Vec::new() }
    });
    let into_depth = changed(|frame| {
        frame.passes[0].does =
            Does::Draw { into: Target::Texture(1), depth: DepthUse::Off, load: Load::Clear, reads: Vec::new() }
    });
    let colour_depth = changed(|frame| {
        frame.passes[0].does =
            Does::Draw { into: Target::Texture(0), depth: DepthUse::Write(2), load: Load::Clear, reads: Vec::new() };
    });
    let depth_past = changed(|frame| {
        frame.passes[0].does =
            Does::Draw { into: Target::Texture(0), depth: DepthUse::Write(9), load: Load::Clear, reads: Vec::new() };
    });

    assert_eq!(check(&past, &kept), refused(0, PassRefused::Target));
    assert_eq!(check(&into_depth, &kept), refused(0, PassRefused::Colour));
    assert_eq!(check(&colour_depth, &kept), refused(0, PassRefused::Depth));
    assert_eq!(check(&depth_past, &kept), refused(0, PassRefused::Target));
}

#[test]
fn a_pass_reading_keeping_or_testing_what_no_earlier_pass_wrote_is_refused() {
    // Catches order unchecked: a pass reading a target a later pass writes would read what the frame before left, or
    // nothing.
    let kept = Kept::new();
    let tests_first = changed(|frame| frame.passes.swap(0, 1));
    let keeps_first = changed(|frame| {
        frame.passes[0].does =
            Does::Draw { into: Target::Texture(0), depth: DepthUse::Write(1), load: Load::Keep, reads: Vec::new() };
    });
    let reads_unwritten = changed(|frame| {
        frame.passes.remove(1);
    });

    assert_eq!(check(&tests_first, &kept), refused(0, PassRefused::Unwritten));
    assert_eq!(check(&keeps_first, &kept), refused(0, PassRefused::Unwritten));
    assert_eq!(check(&reads_unwritten, &kept), refused(1, PassRefused::Unwritten));
}

/// [`frame`] with its glaze's reads `reads`.
fn reading(reads: Vec<Target>) -> CreateFrame {
    changed(|frame| {
        if let Does::Effect { reads: read, .. } = &mut frame.passes[2].does {
            *read = reads;
        }
    })
}

#[test]
fn an_effect_reading_the_end_itself_or_other_kinds_than_it_was_made_for_is_refused() {
    // Catches an effect bound a texture it draws into, which WebGPU refuses as the pass is begun, the end read, which
    // nothing binds, and reads that are not the effect's, which its bind group would not fit.
    let kept = Kept::new();
    let end = reading(vec![Target::End, Target::Texture(0)]);
    let swapped = reading(vec![Target::Texture(0), Target::Texture(2)]);
    let fewer = reading(vec![Target::Texture(2)]);
    let none = reading(Vec::new());
    let sky_reading = changed(|frame| {
        frame.passes[2].does = Does::Effect {
            program: SKY,
            reads: vec![Target::Texture(0)],
            into: Target::Texture(2),
            bindings: Vec::new(),
        };
    });
    let itself = changed(|frame| {
        frame.passes[2].does = Does::Effect {
            program: BLUR,
            reads: vec![Target::Texture(2)],
            into: Target::Texture(2),
            bindings: Vec::new(),
        };
        frame.passes.push(PassSpec { name: PassName::of::<Blur>(), does: frame.passes[1].does.clone() });
    });

    assert_eq!(check(&end, &kept), refused(2, PassRefused::End));
    assert_eq!(check(&swapped, &kept), refused(2, PassRefused::Reads));
    assert_eq!(check(&fewer, &kept), refused(2, PassRefused::Reads));
    assert_eq!(check(&none, &kept), refused(2, PassRefused::Reads));
    assert_eq!(check(&sky_reading, &kept), refused(2, PassRefused::Reads));
    assert_eq!(check(&itself, &kept), refused(2, PassRefused::Itself));
}

#[test]
fn an_effect_released_made_for_another_shape_or_bound_otherwise_is_refused() {
    // Catches an effect drawn into a target of another format than its pipeline's, or with a bind group short of what
    // it binds: WebGPU refuses each as the pass draws it.
    let kept = Kept::new();
    let effect = |program: ProgramId, bindings: Vec<Bind>| {
        changed(move |frame| {
            if let Does::Effect { program: drawn, bindings: bound, .. } = &mut frame.passes[2].does {
                (*drawn, *bound) = (program, bindings);
            }
        })
    };
    let released = effect(ProgramId { id: 99 }, tinted());
    let unbound = effect(GLAZE, Vec::new());
    let other_shape = changed(|frame| {
        frame.passes[2].does =
            Does::Effect { program: BLUR, reads: vec![Target::Texture(2)], into: Target::End, bindings: Vec::new() };
    });

    assert_eq!(check(&released, &kept), refused(2, PassRefused::Effect));
    assert_eq!(
        check(&unbound, &kept),
        refused(2, PassRefused::Bindings { why: JoinRefused::Unbound { group: 1, binding: 0 } })
    );
    assert_eq!(check(&other_shape, &kept), refused(2, PassRefused::Shape));
}

#[test]
fn each_pass_is_shaped_by_what_it_draws_into() {
    // Catches a pass shaped by the window where it draws into a target, or with depth it does not hold: programs made
    // for it would not join it, or would be refused as it draws.
    let frame = frame();
    let shape = |into: Target, depth: DepthUse| frame.shape(into, depth, ColourFormat::Bgra8);

    assert_eq!(shape(Target::Texture(0), DepthUse::Write(1)), Shape::with_depth(ColourFormat::R32Uint));
    assert_eq!(shape(Target::Texture(2), DepthUse::Test(1)), Shape::with_depth(ColourFormat::Rgba16Float));
    assert_eq!(shape(Target::End, DepthUse::Off), Shape::flat(ColourFormat::Bgra8));
}

#[test]
fn a_compute_pass_draws_into_nothing_and_never_ends_a_frame() {
    // Catches a compute pass taken as writing a target, which would let a later pass keep or read what nothing drew,
    // or a frame let end on one, which would show what the window last held: a forged one must fail as the builder,
    // whose `finish` takes a pass that writes the end, would never build it.
    let mut built = FrameBuilder::on(Ends::Window(window(0)));
    built.compute(Simulate);
    let (ids, depth) = built.draw(Surfaces, Uint, scale::Full, samples::One, depth::Write, ());
    let splats = built.draw(Splats, Float, scale::Full, samples::One, depth::Test(&depth), ());
    let Ok(built) = built.finish(Glaze, Effect::new(&glaze(), (splats, ids)).bound(tinted())) else {
        panic!("the frame's passes are of types of their own");
    };
    let compute = PassSpec { name: PassName::of::<Simulate>(), does: Does::Compute };
    let last = changed(|frame| frame.passes.push(compute.clone()));
    let kept_over_compute = changed(|frame| {
        frame.passes.insert(0, compute.clone());
        frame.passes[1].does =
            Does::Draw { into: Target::Texture(0), depth: DepthUse::Write(1), load: Load::Keep, reads: Vec::new() };
    });
    let kept = Kept::new();

    assert_eq!(built.passes[0], compute);
    assert_eq!(check(&built, &kept), Ok(()));
    assert_eq!(check(&last, &kept), Err(FrameRefused::Unended));
    assert_eq!(check(&kept_over_compute, &kept), refused(1, PassRefused::Unwritten));
    assert_eq!(check(&CreateFrame::own(window(0)), &kept), Ok(()));
}

/// A frame onto a texture `width` by `height`: one pass drawing into it.
fn onto_texture(width: u32, height: u32) -> CreateFrame {
    let frame = FrameBuilder::on(Ends::Texture { size: Size { width, height }, format: TextureFormat::Rgba8 });
    let Ok(create) = frame.finish(Surfaces, Onto(depth::Off, ())) else {
        panic!("a frame of one pass is of types of its own");
    };

    create
}

#[test]
fn a_frame_onto_a_texture_of_a_size_the_gpu_makes_is_made() {
    // Catches a texture frame's size left unchecked, which WebGPU refuses as the texture is made, or checked against
    // a bound one side short.
    let kept = Kept::new();

    assert_eq!(check(&onto_texture(64, 64), &kept), Ok(()));
    assert_eq!(check(&onto_texture(LONGEST, 1), &kept), Ok(()));
    assert_eq!(check(&onto_texture(0, 64), &kept), Err(FrameRefused::Size { width: 0, height: 64, most: LONGEST }));
    assert_eq!(
        check(&onto_texture(64, LONGEST + 1), &kept),
        Err(FrameRefused::Size { width: 64, height: LONGEST + 1, most: LONGEST })
    );
}

/// A frame onto a texture of `format`: the sky, made for floats, drawn into it.
fn sky_onto(format: TextureFormat) -> CreateFrame {
    let frame = FrameBuilder::on(Ends::Texture { size: Size { width: 64, height: 64 }, format });
    let sky = MadeEffect { program: SKY, reads: Vec::new() };
    let Some(sky) = EffectId::<()>::of(&sky) else {
        panic!("the sky reads nothing");
    };
    let Ok(create) = frame.finish(Sky, Effect::new(&sky, ())) else {
        panic!("a frame of one pass is of types of their own");
    };

    create
}

#[test]
fn the_pass_that_writes_a_texture_end_is_shaped_by_the_end_s_format() {
    // Catches a texture end drawn in one format whatever format it was asked in: an effect made for floats would be
    // refused the float end it fits, or let draw into an end of bytes, which WebGPU refuses as it draws.
    let kept = Kept::new();

    assert_eq!(check(&sky_onto(TextureFormat::Rgba16Float), &kept), Ok(()));
    assert_eq!(check(&sky_onto(TextureFormat::Rgba8), &kept), refused(0, PassRefused::Shape));
}

#[test]
fn only_the_owner_of_a_window_s_main_frame_replaces_it() {
    // Catches a window's main frame replaced by whoever asks last, which would show whichever actor's mail came
    // last, or its owner refused its own replacement.
    let (owner, other) = (ActorId::new(1), ActorId::new(2));

    assert_eq!(may_replace(None, other), Ok(()));
    assert_eq!(may_replace(Some(owner), owner), Ok(()));
    assert_eq!(may_replace(Some(owner), other), Err(FrameRefused::Owned { owner }));
}

#[test]
fn a_scale_divides_each_side_of_the_end_rounding_up_to_at_least_a_pixel() {
    // Catches a side divided rounding down, which loses an odd end's last column and makes a target of no pixels
    // of an end narrower than its divisor, which WebGPU refuses as the target is made.
    let end = Size { width: 1281, height: 7 };

    assert_eq!(end.scaled(Scale::Full), end);
    assert_eq!(end.scaled(Scale::Half), Size { width: 641, height: 4 });
    assert_eq!(end.scaled(Scale::Quarter), Size { width: 321, height: 2 });
    assert_eq!(end.scaled(Scale::Eighth), Size { width: 161, height: 1 });
    assert_eq!(Size::PIXEL.scaled(Scale::Eighth), Size::PIXEL);
    assert_eq!(Size { width: 16, height: 8 }.scaled(Scale::Eighth), Size { width: 2, height: 1 });
}

/// A frame onto the window at reduced resolution: surfaces drawn at half size, writing a depth of that size, splats
/// over them tested against it, the blur of them into a quarter-size target, and the glaze of the blur and of ids
/// drawn at the end's size onto the window.
fn reduced() -> CreateFrame {
    let mut frame = FrameBuilder::on(Ends::Window(window(0)));
    let (scene, depth) = frame.draw(Surfaces, Float, scale::Half, samples::One, depth::Write, ());
    let scene = frame.draw_over(Splats, scene, depth::Test(&depth), ());
    let blur = MadeEffect { program: BLUR, reads: vec![Read::Float] };
    let Some(blur) = EffectId::<(Float,)>::of(&blur) else {
        panic!("the blur reads floats");
    };
    let blurred = frame.effect(Blur, Effect::new(&blur, (scene,)), Float, scale::Quarter);
    let ids = frame.draw(Ids, Uint, scale::Full, samples::One, depth::Off, ());
    let Ok(create) = frame.finish(Glaze, Effect::new(&glaze(), (blurred, ids)).bound(tinted())) else {
        panic!("the frame's passes are of types of their own");
    };

    create
}

#[test]
fn the_builder_gives_each_target_the_scale_of_the_pass_that_draws_it() {
    // Catches a target described at the end's size whatever scale it was drawn at, and a depth written at another
    // scale than the colour drawn beside it, which WebGPU refuses as the pass begins.
    let scales: Vec<(TargetKind, Scale)> = reduced().targets.iter().map(|target| (target.kind, target.scale)).collect();

    assert_eq!(
        scales,
        vec![
            (TargetKind::Float, Scale::Half),
            (TargetKind::Depth, Scale::Half),
            (TargetKind::Float, Scale::Quarter),
            (TargetKind::Uint, Scale::Full),
        ]
    );
    assert_eq!(check(&reduced(), &Kept::new()), Ok(()));
}

#[test]
fn a_pass_whose_depth_is_of_another_scale_than_its_colour_is_refused() {
    // Catches a forged frame whose pass draws into a target of one size with a depth of another, which WebGPU
    // refuses as the pass begins, at `Render`: a frame that is made can be drawn.
    let kept = Kept::new();
    let writes_full_depth = {
        let mut frame = reduced();
        frame.targets[1].scale = Scale::Full;
        frame
    };
    let tests_quarter_depth = {
        let mut frame = reduced();
        frame.targets.push(TargetSpec { kind: TargetKind::Depth, scale: Scale::Quarter, samples: Samples::One });
        frame.passes[1].does =
            Does::Draw { into: Target::Texture(0), depth: DepthUse::Test(4), load: Load::Keep, reads: Vec::new() };
        frame
    };
    let onto_end_at_half = {
        let mut frame = reduced();
        let onto = Does::Draw { into: Target::End, depth: DepthUse::Test(1), load: Load::Clear, reads: Vec::new() };
        frame.passes.push(PassSpec { name: PassName::of::<Sky>(), does: onto });
        frame
    };

    assert_eq!(check(&writes_full_depth, &kept), refused(0, PassRefused::DepthScale));
    assert_eq!(check(&tests_quarter_depth, &kept), refused(1, PassRefused::DepthScale));
    assert_eq!(check(&onto_end_at_half, &kept), refused(5, PassRefused::DepthScale));
}

/// A frame onto the window: a floor drawn into floats of one sample, writing depth; rims drawn into floats of four
/// samples with a depth of their own, reading the floor's; and the rims laid over the floor onto the window.
fn rims() -> CreateFrame {
    let made = MadeEffect { program: LAY, reads: vec![Read::Float, Read::Float] };
    let Some(lay) = EffectId::<(Float, Float)>::of(&made) else {
        panic!("the lay reads two targets of floats");
    };
    let mut frame = FrameBuilder::on(Ends::Window(window(0)));
    let (floor, under) = frame.draw(Floor, Float, scale::Full, samples::One, depth::Write, ());
    let (rims, _) = frame.draw(Rims, Float, scale::Full, samples::Four, depth::Write, (under,));
    let Ok(create) = frame.finish(Lay, Effect::new(&lay, (floor, rims))) else {
        panic!("the frame's passes are of types of their own");
    };

    create
}

/// [`rims`], with `change` made to it.
fn rims_changed(change: impl FnOnce(&mut CreateFrame)) -> CreateFrame {
    let mut frame = rims();
    change(&mut frame);

    frame
}

#[test]
fn the_builder_describes_a_draw_into_four_samples_reading_a_depth_an_earlier_pass_wrote() {
    // Catches a target's samples dropped from its description, a depth written at other samples than the colour
    // beside it, and a draw's reads described out of order or not at all.
    let four = |kind| TargetSpec { kind, scale: Scale::Full, samples: Samples::Four };
    let targets =
        vec![full(TargetKind::Float), full(TargetKind::Depth), four(TargetKind::Float), four(TargetKind::Depth)];
    let drawn = Does::Draw {
        into: Target::Texture(2),
        depth: DepthUse::Write(3),
        load: Load::Clear,
        reads: vec![Target::Texture(1)],
    };

    assert_eq!(rims().targets, targets);
    assert_eq!(rims().passes[1].does, drawn);
    assert_eq!(check(&rims(), &Kept::new()), Ok(()));
}

#[test]
fn a_pass_of_colour_and_depth_of_other_samples_or_whole_numbers_or_an_effect_of_four_is_refused() {
    // Catches what WebGPU refuses as a pass is begun or a texture made: a colour and a depth of other counts of
    // samples, whole numbers of four, drawn or named by no pass, and an effect's one triangle drawn into four samples.
    let kept = Kept::new();
    let depth_of_one = rims_changed(|frame| frame.targets[3].samples = Samples::One);
    let colour_of_one = rims_changed(|frame| frame.targets[2].samples = Samples::One);
    let whole = changed(|frame| frame.targets[0].samples = Samples::Four);
    let unnamed = changed(|frame| {
        frame.targets.push(TargetSpec { kind: TargetKind::Uint, scale: Scale::Full, samples: Samples::Four });
    });
    let effect = rims_changed(|frame| {
        let made = Does::Effect { program: SKY, reads: Vec::new(), into: Target::Texture(2), bindings: Vec::new() };
        frame.passes[1].does = made;
    });

    assert_eq!(check(&depth_of_one, &kept), refused(1, PassRefused::Samples));
    assert_eq!(check(&colour_of_one, &kept), refused(1, PassRefused::Samples));
    assert_eq!(check(&whole, &kept), refused(0, PassRefused::Samples));
    assert_eq!(check(&unnamed, &kept), refused(0, PassRefused::Samples));
    assert_eq!(check(&effect, &kept), refused(1, PassRefused::Samples));
}

#[test]
fn a_pass_reading_a_depth_of_four_samples_is_refused() {
    // Catches a depth of four samples bound as a texture to read, which WebGPU resolves to none and binds only as
    // another kind of texture: by a draw, or by an effect.
    let kept = Kept::new();
    let drawn = rims_changed(|frame| {
        let over = PassSpec {
            name: PassName::of::<Splats>(),
            does: Does::Draw {
                into: Target::Texture(0),
                depth: DepthUse::Off,
                load: Load::Keep,
                reads: vec![Target::Texture(3)],
            },
        };
        frame.passes.insert(2, over);
    });
    let effect = rims_changed(|frame| {
        let Does::Effect { reads, .. } = &mut frame.passes[2].does else {
            panic!("the last pass is the lay");
        };
        reads[1] = Target::Texture(3);
    });

    assert_eq!(check(&drawn, &kept), refused(2, PassRefused::ReadsSamples));
    assert_eq!(check(&effect, &kept), refused(2, PassRefused::ReadsSamples));
}

#[test]
fn a_draw_reading_its_own_targets_the_end_or_what_no_earlier_pass_wrote_is_refused() {
    // Catches a draw's reads checked less than an effect's: a target it draws into or a depth it tests or writes,
    // which WebGPU refuses bound while the pass draws it; the end, which nothing reads; one past the last; and one a
    // later pass writes, which would be read as the frame before left it.
    let kept = Kept::new();
    let reading = |reads: Vec<Target>| {
        rims_changed(|frame| {
            let Does::Draw { reads: drawn, .. } = &mut frame.passes[1].does else {
                panic!("the rims are drawn");
            };
            *drawn = reads;
        })
    };

    assert_eq!(check(&reading(vec![Target::Texture(2)]), &kept), refused(1, PassRefused::Itself));
    assert_eq!(check(&reading(vec![Target::Texture(3)]), &kept), refused(1, PassRefused::Itself));
    assert_eq!(check(&reading(vec![Target::End]), &kept), refused(1, PassRefused::End));
    assert_eq!(check(&reading(vec![Target::Texture(9)]), &kept), refused(1, PassRefused::Target));
    let early = rims_changed(|frame| {
        let Does::Draw { reads, .. } = &mut frame.passes[0].does else {
            panic!("the floor is drawn");
        };
        *reads = vec![Target::Texture(2)];
    });
    assert_eq!(check(&early, &kept), refused(0, PassRefused::Unwritten));
}
