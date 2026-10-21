//! A frame's check, and its builder: what WebGPU would refuse as the frame is drawn is refused as it is made, and the
//! builder builds what the check lets through.

use super::builder::{Effect, Float, Onto, Uint, depth};
use super::*;
use crate::effect::{EffectId, MadeEffect};
use crate::instances::InstancesId;
use crate::mesh::{MeshId, VertexLayout};
use crate::placement::{Reads, Resource};
use crate::program::BindingKind;
use crate::storage::StorageId;
use crate::texture::TextureId;
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
/// Uniforms of 16 bytes.
const TINT: UniformsId = UniformsId { id: 4 };
/// The longest side of a texture the test's GPU makes.
const LONGEST: u32 = 2048;

/// What a test keeps: the window [`window`] `0`, of `Bgra8`; [`GLAZE`]; [`BLUR`]; and [`TINT`].
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
    fn pass(&self, _: Frame, _: PassName) -> Result<Shape, JoinRefused> {
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
            _ => return None,
        };

        Some(EffectReads { reads, bindings, shape: Shape::flat(colour) })
    }
}

/// Passes named by types, as actors name them.
macro_rules! passes {
    ($($name:ident $kind:literal),*) => {$(
        #[derive(Plain)]
        struct $name;

        impl Kind for $name {
            const NAME: KindName = KindName::new($kind);
        }

        impl Pass for $name {}
    )*};
}

passes!(Surfaces "aether.test.surfaces", Splats "aether.test.splats", Glaze "aether.test.glaze", Blur "aether.test.blur", Ids "aether.test.ids");

/// [`GLAZE`], as the builder takes it.
fn glaze() -> EffectId<(Float, Uint)> {
    let made = MadeEffect { program: GLAZE, reads: vec![Read::Float, Read::Uint] };
    let Some(glaze) = EffectId::of(&made) else {
        panic!("the glaze reads floats and whole numbers");
    };

    glaze
}

/// The glaze bound to [`TINT`].
fn tinted() -> Vec<Bind> {
    vec![Bind { group: 1, binding: 0, to: Resource::Uniforms(TINT) }]
}

/// A frame onto the window: surfaces drawn into whole numbers, writing depth; splats drawn into floats, tested against
/// it; and the glaze of both onto the window.
fn frame() -> CreateFrame {
    let mut frame = FrameBuilder::on(Ends::Window(window(0)));
    let (ids, depth) = frame.draw(Surfaces, Uint, depth::Write);
    let splats = frame.draw(Splats, Float, depth::Test(&depth));
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
        targets: vec![TargetSpec::Uint, TargetSpec::Depth, TargetSpec::Float],
        passes: vec![
            named(
                PassName::of::<Surfaces>(),
                Does::Draw { into: Target::Texture(0), depth: DepthUse::Write(1), load: Load::Clear },
            ),
            named(
                PassName::of::<Splats>(),
                Does::Draw { into: Target::Texture(2), depth: DepthUse::Test(1), load: Load::Clear },
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
    let (ids, depth) = frame.draw(Surfaces, Uint, depth::Write);
    let splats = frame.draw(Surfaces, Float, depth::Test(&depth));

    assert_eq!(frame.finish(Glaze, Effect::new(&glaze(), (splats, ids))), Err(FrameRefused::Repeated { pass: 1 }));
}

#[test]
fn a_frame_drawing_over_kept_targets_keeps_what_its_draws_left() {
    // Catches a draw over a target described as cleared, which would draw on black what the passes before drew.
    let mut frame = FrameBuilder::on(Ends::Window(window(0)));
    let (scene, depth) = frame.draw(Surfaces, Float, depth::Write);
    let scene = frame.draw_over(Splats, scene, depth::Test(&depth));
    let blur = MadeEffect { program: BLUR, reads: vec![Read::Float] };
    let Some(blur) = EffectId::<(Float,)>::of(&blur) else {
        panic!("the blur reads floats");
    };
    let blurred = frame.effect(Blur, Effect::new(&blur, (scene,)), Float);
    let ids = frame.draw(Ids, Uint, depth::Off);
    let Ok(create) = frame.finish(Glaze, Effect::new(&glaze(), (blurred, ids)).bound(tinted())) else {
        panic!("the frame's passes are of types of their own");
    };

    let over = Does::Draw { into: Target::Texture(0), depth: DepthUse::Test(1), load: Load::Keep };
    assert_eq!(create.passes[1].does, over);
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
        frame.passes[0].does = Does::Draw { into: Target::Texture(9), depth: DepthUse::Off, load: Load::Clear }
    });
    let into_depth = changed(|frame| {
        frame.passes[0].does = Does::Draw { into: Target::Texture(1), depth: DepthUse::Off, load: Load::Clear }
    });
    let colour_depth = changed(|frame| {
        frame.passes[0].does = Does::Draw { into: Target::Texture(0), depth: DepthUse::Write(2), load: Load::Clear };
    });
    let depth_past = changed(|frame| {
        frame.passes[0].does = Does::Draw { into: Target::Texture(0), depth: DepthUse::Write(9), load: Load::Clear };
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
        frame.passes[0].does = Does::Draw { into: Target::Texture(0), depth: DepthUse::Write(1), load: Load::Keep };
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
    let shapes = frame().shapes(ColourFormat::Bgra8);

    assert_eq!(
        shapes,
        [
            Shape::with_depth(ColourFormat::R32Uint),
            Shape::with_depth(ColourFormat::Rgba16Float),
            Shape::flat(ColourFormat::Bgra8)
        ]
    );
}

/// A frame onto a texture `width` by `height`: one pass drawing into it.
fn onto_texture(width: u32, height: u32) -> CreateFrame {
    let frame = FrameBuilder::on(Ends::Texture(Size { width, height }));
    let Ok(create) = frame.finish(Surfaces, Onto(depth::Off)) else {
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

#[test]
fn only_the_owner_of_a_window_s_main_frame_replaces_it() {
    // Catches a window's main frame replaced by whoever asks last, which would show whichever actor's mail came
    // last, or its owner refused its own replacement.
    let (owner, other) = (ActorId::new(1), ActorId::new(2));

    assert_eq!(may_replace(None, other), Ok(()));
    assert_eq!(may_replace(Some(owner), owner), Ok(()));
    assert_eq!(may_replace(Some(owner), other), Err(FrameRefused::Owned { owner }));
}
