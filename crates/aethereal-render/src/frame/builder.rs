//! The typed builder of a frame's description: a frame that would be refused does not compile (ADR-0025).
//!
//! A target is the output of the pass that draws it, so a pass reads only what an earlier pass returned and never what
//! it writes; an effect takes targets of the kinds it was made to read and no others; and `finish` takes the pass that
//! writes the frame's end, and is the only way to a [`CreateFrame`].
//!
//! ```
//! use aethereal_data::Plain;
//! use aethereal_render::effect::EffectId;
//! use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, Uint, depth, samples, scale};
//! use aethereal_render::frame::{CreateFrame, Ends, FrameRefused};
//! use aethereal_render::pass::Pass;
//! use aethereal_window::WindowId;
//!
//! #[derive(Plain)]
//! #[kind(name = "aether.example.surfaces")]
//! struct Surfaces;
//! impl Pass for Surfaces {}
//!
//! #[derive(Plain)]
//! #[kind(name = "aether.example.splats")]
//! struct Splats;
//! impl Pass for Splats {}
//!
//! #[derive(Plain)]
//! #[kind(name = "aether.example.glaze")]
//! struct Glaze;
//! impl Pass for Glaze {}
//!
//! fn frame(window: WindowId, glaze: &EffectId<(Float, Uint)>) -> Result<CreateFrame, FrameRefused> {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let (ids, depth) = frame.draw(Surfaces, Uint, scale::Full, samples::One, depth::Write, ());
//!     let splats = frame.draw(Splats, Float, scale::Full, samples::One, depth::Test(&depth), ());
//!     frame.finish(Glaze, Effect::new(glaze, (splats, ids)))
//! }
//! ```
//!
//! A target is had only from the pass that draws it, so nothing reads one before it is written:
//!
//! ```compile_fail
//! # use aethereal_render::frame::builder::{Float, Target};
//! let unwritten = Target::<Float> { at: 0, kind: core::marker::PhantomData };
//! ```
//!
//! An effect takes targets of the kinds it was made to read:
//!
//! ```compile_fail
//! # use aethereal_data::Plain;
//! # use aethereal_render::effect::EffectId;
//! # use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, Uint, depth, samples, scale};
//! # use aethereal_render::frame::{CreateFrame, Ends, FrameRefused};
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.ids")] struct Ids;
//! # impl Pass for Ids {}
//! # #[derive(Plain)] #[kind(name = "aether.example.glaze")] struct Glaze;
//! # impl Pass for Glaze {}
//! fn frame(window: WindowId, glaze: &EffectId<(Float,)>) -> Result<CreateFrame, FrameRefused> {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let ids = frame.draw(Ids, Uint, scale::Full, samples::One, depth::Off, ());
//!     frame.finish(Glaze, Effect::new(glaze, (ids,)))
//! }
//! ```
//!
//! A pass tests a depth target, and a colour target is not one:
//!
//! ```compile_fail
//! # use aethereal_data::Plain;
//! # use aethereal_render::frame::builder::{Float, FrameBuilder, depth, samples, scale};
//! # use aethereal_render::frame::Ends;
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.surfaces")] struct Surfaces;
//! # impl Pass for Surfaces {}
//! # #[derive(Plain)] #[kind(name = "aether.example.splats")] struct Splats;
//! # impl Pass for Splats {}
//! fn frame(window: WindowId) {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let colour = frame.draw(Surfaces, Float, scale::Full, samples::One, depth::Off, ());
//!     let splats = frame.draw(Splats, Float, scale::Full, samples::One, depth::Test(&colour), ());
//! }
//! ```
//!
//! A target is of a size its scale makes of the end's, so a chain at reduced resolution, such as a bloom blurred at
//! half size and laid back over the scene, is passes of one frame. An effect reads targets of any scale:
//!
//! ```
//! # use aethereal_data::Plain;
//! # use aethereal_render::effect::EffectId;
//! # use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, depth, samples, scale};
//! # use aethereal_render::frame::{CreateFrame, Ends, FrameRefused};
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.scene")] struct Scene;
//! # impl Pass for Scene {}
//! # #[derive(Plain)] #[kind(name = "aether.example.blur")] struct Blur;
//! # impl Pass for Blur {}
//! # #[derive(Plain)] #[kind(name = "aether.example.bloom")] struct Bloom;
//! # impl Pass for Bloom {}
//! fn frame(
//!     window: WindowId,
//!     (blur, bloom): (&EffectId<(Float,)>, &EffectId<(Float, Float)>),
//! ) -> Result<CreateFrame, FrameRefused> {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let (scene, _) = frame.draw(Scene, Float, scale::Full, samples::One, depth::Write, ());
//!     let blurred = frame.effect(Blur, Effect::new(blur, (scene,)), Float, scale::Half);
//!     frame.finish(Bloom, Effect::new(bloom, (scene, blurred)))
//! }
//! ```
//!
//! A pass's targets are all of one size, so a pass tests a depth of the scale it draws at:
//!
//! ```compile_fail
//! # use aethereal_data::Plain;
//! # use aethereal_render::frame::builder::{Float, FrameBuilder, depth, samples, scale};
//! # use aethereal_render::frame::Ends;
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.surfaces")] struct Surfaces;
//! # impl Pass for Surfaces {}
//! # #[derive(Plain)] #[kind(name = "aether.example.splats")] struct Splats;
//! # impl Pass for Splats {}
//! fn frame(window: WindowId) {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let (_, depth) = frame.draw(Surfaces, Float, scale::Full, samples::One, depth::Write, ());
//!     let splats = frame.draw(Splats, Float, scale::Half, samples::One, depth::Test(&depth), ());
//! }
//! ```
//!
//! A target of floats may hold four samples of each pixel, resolved to one as each pass that draws into it ends, and
//! a draw reads targets earlier passes wrote, as an effect does: rims drawn with their own depth into four samples
//! read the depth a floor wrote at one, and a last effect lays them on the end, reading what they were resolved to:
//!
//! ```
//! # use aethereal_data::Plain;
//! # use aethereal_render::effect::EffectId;
//! # use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, depth, samples, scale};
//! # use aethereal_render::frame::{CreateFrame, Ends, FrameRefused};
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.floor")] struct Floor;
//! # impl Pass for Floor {}
//! # #[derive(Plain)] #[kind(name = "aether.example.rims")] struct Rims;
//! # impl Pass for Rims {}
//! # #[derive(Plain)] #[kind(name = "aether.example.lay")] struct Lay;
//! # impl Pass for Lay {}
//! fn frame(window: WindowId, lay: &EffectId<(Float, Float)>) -> Result<CreateFrame, FrameRefused> {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let (floor, under) = frame.draw(Floor, Float, scale::Full, samples::One, depth::Write, ());
//!     let (rims, _) = frame.draw(Rims, Float, scale::Full, samples::Four, depth::Write, (under,));
//!     frame.finish(Lay, Effect::new(lay, (floor, rims)))
//! }
//! ```
//!
//! A depth of four samples is tested and written, and never read:
//!
//! ```compile_fail
//! # use aethereal_data::Plain;
//! # use aethereal_render::frame::builder::{Float, FrameBuilder, depth, samples, scale};
//! # use aethereal_render::frame::Ends;
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.rims")] struct Rims;
//! # impl Pass for Rims {}
//! # #[derive(Plain)] #[kind(name = "aether.example.over")] struct Over;
//! # impl Pass for Over {}
//! fn frame(window: WindowId) {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let (_, depth) = frame.draw(Rims, Float, scale::Full, samples::Four, depth::Write, ());
//!     let over = frame.draw(Over, Float, scale::Full, samples::One, depth::Off, (depth,));
//! }
//! ```
//!
//! A target of whole numbers holds one sample, as WebGPU multisamples none:
//!
//! ```compile_fail
//! # use aethereal_data::Plain;
//! # use aethereal_render::frame::builder::{FrameBuilder, Uint, depth, samples, scale};
//! # use aethereal_render::frame::Ends;
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.ids")] struct Ids;
//! # impl Pass for Ids {}
//! fn frame(window: WindowId) {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let ids = frame.draw(Ids, Uint, scale::Full, samples::Four, depth::Off, ());
//! }
//! ```
//!
//! A pass's colour and its depth hold one count of samples, so a pass of four tests no depth of one:
//!
//! ```compile_fail
//! # use aethereal_data::Plain;
//! # use aethereal_render::frame::builder::{Float, FrameBuilder, depth, samples, scale};
//! # use aethereal_render::frame::Ends;
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.floor")] struct Floor;
//! # impl Pass for Floor {}
//! # #[derive(Plain)] #[kind(name = "aether.example.rims")] struct Rims;
//! # impl Pass for Rims {}
//! fn frame(window: WindowId) {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let (_, depth) = frame.draw(Floor, Float, scale::Full, samples::One, depth::Write, ());
//!     let rims = frame.draw(Rims, Float, scale::Full, samples::Four, depth::Test(&depth), ());
//! }
//! ```

use super::{CreateFrame, DepthUse, Does, Ends, FrameRefused, Load, PassSpec, Scale, TargetKind, TargetSpec};
use crate::effect::{EffectId, ReadKind, Reads};
use crate::pass::{Pass, PassName};
use crate::placement::Bind;
use crate::shape::Samples;
use core::fmt::{self, Debug, Formatter};
use core::marker::PhantomData;

/// Targets of floats: four 16-bit floats a pixel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Float;

/// Targets of whole numbers: one 32-bit whole number a pixel, which nothing blends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uint;

/// Depth targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Depth32;

/// A kind of target a pass draws colour into: [`Float`] or [`Uint`].
#[diagnostic::on_unimplemented(
    message = "a pass draws colour into a target of `Float` or `Uint`, not `{Self}`",
    label = "not a kind of colour target"
)]
pub trait Colour: ReadKind {
    /// What the target holds.
    const KIND: TargetKind;
}

impl Colour for Float {
    const KIND: TargetKind = TargetKind::Float;
}

impl Colour for Uint {
    const KIND: TargetKind = TargetKind::Uint;
}

/// The size of a target as a part of its frame's end: [`scale::Full`], [`scale::Half`], [`scale::Quarter`], or
/// [`scale::Eighth`].
pub mod scale {
    /// The end's size.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Full;

    /// Half the end's size each way, rounded up.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Half;

    /// A quarter of the end's size each way, rounded up.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Quarter;

    /// An eighth of the end's size each way, rounded up.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Eighth;
}

impl sealed::Sealed for scale::Full {}
impl sealed::Sealed for scale::Half {}
impl sealed::Sealed for scale::Quarter {}
impl sealed::Sealed for scale::Eighth {}

/// A size of target, as the frame's description names it.
#[diagnostic::on_unimplemented(
    message = "a target is of `scale::Full`, `scale::Half`, `scale::Quarter`, or `scale::Eighth`, not `{Self}`",
    label = "not a scale of target"
)]
pub trait Scaled: sealed::Sealed {
    /// The scale it is.
    const SCALE: Scale;
}

impl Scaled for scale::Full {
    const SCALE: Scale = Scale::Full;
}

impl Scaled for scale::Half {
    const SCALE: Scale = Scale::Half;
}

impl Scaled for scale::Quarter {
    const SCALE: Scale = Scale::Quarter;
}

impl Scaled for scale::Eighth {
    const SCALE: Scale = Scale::Eighth;
}

/// How many samples a target holds of each pixel: [`samples::One`] or [`samples::Four`].
pub mod samples {
    /// One: nothing is multisampled.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct One;

    /// Four, resolved to one as each pass that draws into the target ends, which is what a later pass reads.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Four;
}

impl sealed::Sealed for samples::One {}
impl sealed::Sealed for samples::Four {}

/// A count of samples a target holds, as the frame's description names it.
#[diagnostic::on_unimplemented(
    message = "a target holds `samples::One` or `samples::Four`, not `{Self}`",
    label = "not a count of samples"
)]
pub trait Sampled: sealed::Sealed {
    /// The count it is.
    const SAMPLES: Samples;
}

impl Sampled for samples::One {
    const SAMPLES: Samples = Samples::One;
}

impl Sampled for samples::Four {
    const SAMPLES: Samples = Samples::Four;
}

/// A kind of colour target that holds `N` samples of each pixel: floats hold one or four, and whole numbers one, as
/// WebGPU multisamples none.
#[diagnostic::on_unimplemented(
    message = "a target of `{Self}` does not hold `{N}`",
    label = "not a count of samples this kind of target holds",
    note = "a target of whole numbers holds `samples::One`, as WebGPU multisamples none"
)]
pub trait HoldsSamples<N: Sampled>: Colour {}

impl<N: Sampled> HoldsSamples<N> for Float {}

impl HoldsSamples<samples::One> for Uint {}

/// A target of the frame being built, of the kind `K`, the scale `S`, and `N` samples of each pixel: the output of the
/// pass that drew it, which a later pass reads, tests, or draws over.
pub struct Target<K, S = scale::Full, N = samples::One> {
    at: u32,
    kind: Typed<K, S, N>,
}

/// What a target's handle is typed by, holding none of it.
type Typed<K, S, N> = PhantomData<fn() -> (K, S, N)>;

/// A depth target of the scale `S` and `N` samples, written by the pass that returned it.
pub type DepthTarget<S = scale::Full, N = samples::One> = Target<Depth32, S, N>;

impl<K, S, N> Clone for Target<K, S, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K, S, N> Copy for Target<K, S, N> {}

impl<K, S, N> Debug for Target<K, S, N> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Target").field(&self.at).finish()
    }
}

impl<K, S, N> Target<K, S, N> {
    /// The target at `at` of the frame's targets.
    const fn at(at: u32) -> Self {
        Self { at, kind: PhantomData }
    }

    /// The target, as a pass names it.
    const fn named(self) -> super::Target {
        super::Target::Texture(self.at)
    }
}

/// How a draw pass uses depth: [`depth::Off`], [`depth::Write`], or [`depth::Test`].
pub mod depth {
    use super::{DepthTarget, samples, scale};

    /// It uses none.
    #[derive(Debug, Clone, Copy)]
    pub struct Off;

    /// It writes a depth target of its own, cleared to the far plane, which it returns beside its colour.
    #[derive(Debug, Clone, Copy)]
    pub struct Write;

    /// It tests the depth an earlier pass wrote, of the scale and the samples of the target it draws into.
    #[derive(Debug, Clone, Copy)]
    pub struct Test<'a, S = scale::Full, N = samples::One>(pub &'a DepthTarget<S, N>);
}

/// Keeps [`DepthArg`] to the three uses this module gives, [`Scaled`] to its four scales, and [`Sampled`] to its two
/// counts of samples.
mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for depth::Off {}
impl sealed::Sealed for depth::Write {}
impl<S, N> sealed::Sealed for depth::Test<'_, S, N> {}

/// How a draw pass into a target of the scale `S` and `N` samples uses depth, and what it returns: its colour target,
/// and with [`depth::Write`] the depth target it wrote, of the same scale and samples, as a pass's targets are all of
/// one size and one count of samples.
#[diagnostic::on_unimplemented(
    message = "a pass into a target of `{S}` and `{N}` uses depth as `depth::Off`, `depth::Write`, or \
               `depth::Test(&depth)` of a depth target of `{S}` and `{N}`, not `{Self}`",
    label = "not a use of depth at this target's scale and samples"
)]
pub trait DepthArg<S: Scaled, N: Sampled>: sealed::Sealed {
    /// What the pass returns of a colour target of the kind `K`.
    type Out<K>;

    /// The use, adding the depth target it writes to the frame's `targets`, and what the pass that drew `colour`
    /// returns.
    fn drawn<K>(self, colour: Target<K, S, N>, targets: &mut Vec<TargetSpec>) -> (DepthUse, Self::Out<K>);
}

impl<S: Scaled, N: Sampled> DepthArg<S, N> for depth::Off {
    type Out<K> = Target<K, S, N>;

    fn drawn<K>(self, colour: Target<K, S, N>, _: &mut Vec<TargetSpec>) -> (DepthUse, Target<K, S, N>) {
        (DepthUse::Off, colour)
    }
}

impl<S: Scaled, N: Sampled> DepthArg<S, N> for depth::Write {
    type Out<K> = (Target<K, S, N>, DepthTarget<S, N>);

    fn drawn<K>(self, colour: Target<K, S, N>, targets: &mut Vec<TargetSpec>) -> (DepthUse, Self::Out<K>) {
        let at = next(targets, TargetSpec { kind: TargetKind::Depth, scale: S::SCALE, samples: N::SAMPLES });

        (DepthUse::Write(at), (colour, Target::at(at)))
    }
}

impl<S: Scaled, N: Sampled> DepthArg<S, N> for depth::Test<'_, S, N> {
    type Out<K> = Target<K, S, N>;

    fn drawn<K>(self, colour: Target<K, S, N>, _: &mut Vec<TargetSpec>) -> (DepthUse, Target<K, S, N>) {
        (DepthUse::Test(self.0.at), colour)
    }
}

/// Adds `spec` to `targets`, and answers its place.
fn next(targets: &mut Vec<TargetSpec>, spec: TargetSpec) -> u32 {
    // A frame has a handful of targets, far below what a `u32` counts.
    let at = u32::try_from(targets.len()).unwrap_or(u32::MAX);
    targets.push(spec);

    at
}

/// A target a pass may read: one of floats of any samples, read as the one they were resolved to, or one of whole
/// numbers or of depth of one sample, as a depth of four is never read.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a target a pass reads",
    label = "a depth of four samples is tested and written, and never read",
    note = "write the depth a pass reads at `samples::One`, in a pass of its own"
)]
pub trait Readable {
    /// The kind of target it is, as a pass reads it.
    type Kind;

    /// The target, as a pass names it.
    fn named(self) -> super::Target;
}

impl<S, N> Readable for Target<Float, S, N> {
    type Kind = Float;

    fn named(self) -> super::Target {
        Self::named(self)
    }
}

impl<S> Readable for Target<Uint, S, samples::One> {
    type Kind = Uint;

    fn named(self) -> super::Target {
        Self::named(self)
    }
}

impl<S> Readable for Target<Depth32, S, samples::One> {
    type Kind = Depth32;

    fn named(self) -> super::Target {
        Self::named(self)
    }
}

/// Targets a pass reads, in order, as a tuple of up to four, `()` for none: what earlier passes returned, so a pass
/// reads only what an earlier pass wrote.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not targets a pass reads",
    label = "a pass reads a tuple of up to four targets earlier passes returned",
    note = "write what it reads as a tuple: `()`, `(scene,)`, `(scene, depth)`"
)]
pub trait ReadTargets {
    /// The kinds of the targets, in order: `(Float, Depth32)`.
    type Kinds;

    /// The targets, as a pass names them.
    fn named(self) -> Vec<super::Target>;
}

impl ReadTargets for () {
    type Kinds = ();

    fn named(self) -> Vec<super::Target> {
        Vec::new()
    }
}

impl<A: Readable> ReadTargets for (A,) {
    type Kinds = (A::Kind,);

    fn named(self) -> Vec<super::Target> {
        vec![self.0.named()]
    }
}

impl<A: Readable, B: Readable> ReadTargets for (A, B) {
    type Kinds = (A::Kind, B::Kind);

    fn named(self) -> Vec<super::Target> {
        vec![self.0.named(), self.1.named()]
    }
}

impl<A: Readable, B: Readable, C: Readable> ReadTargets for (A, B, C) {
    type Kinds = (A::Kind, B::Kind, C::Kind);

    fn named(self) -> Vec<super::Target> {
        vec![self.0.named(), self.1.named(), self.2.named()]
    }
}

impl<A: Readable, B: Readable, C: Readable, D: Readable> ReadTargets for (A, B, C, D) {
    type Kinds = (A::Kind, B::Kind, C::Kind, D::Kind);

    fn named(self) -> Vec<super::Target> {
        vec![self.0.named(), self.1.named(), self.2.named(), self.3.named()]
    }
}

/// Targets of the kinds an effect reads, `R`, in its order: `(Target<Float>, Target<Uint>)` for `(Float, Uint)`.
#[diagnostic::on_unimplemented(
    message = "the effect reads `{R}`, and these targets are `{Self}`",
    label = "targets of other kinds than the effect reads",
    note = "an effect takes a tuple of targets of the kinds it was made to read, in the order it reads them"
)]
pub trait ReadsAs<R> {
    /// The targets, as a pass names them.
    fn named(self) -> Vec<super::Target>;
}

impl<T: ReadTargets> ReadsAs<T::Kinds> for T {
    fn named(self) -> Vec<super::Target> {
        ReadTargets::named(self)
    }
}

/// An effect drawn by a pass: the effect, the targets it reads, and what it binds besides.
#[derive(Debug, Clone)]
pub struct Effect<R> {
    drawn: EffectId<R>,
    reads: Vec<super::Target>,
    bindings: Vec<Bind>,
}

impl<R: Reads> Effect<R> {
    /// `effect`, reading `reads`, targets of the kinds it was made to read, in its order, and binding nothing besides.
    #[must_use]
    pub fn new(effect: &EffectId<R>, reads: impl ReadsAs<R>) -> Self {
        Self { drawn: *effect, reads: reads.named(), bindings: Vec::new() }
    }

    /// The effect, binding `bindings` in the groups after its first, as a placement binds what its program binds.
    #[must_use]
    pub fn bound(self, bindings: Vec<Bind>) -> Self {
        Self { bindings, ..self }
    }

    /// The pass, drawing into `into`.
    fn does(self, into: super::Target) -> Does {
        Does::Effect { program: self.drawn.program(), reads: self.reads, into, bindings: self.bindings }
    }
}

/// A draw onto the frame's end, the last pass, using depth as `D` says, a depth of the end's own size and of one
/// sample, as the end holds one, and reading the targets `T`, `()` for none.
#[derive(Debug, Clone, Copy)]
pub struct Onto<D, T = ()>(pub D, pub T);

/// What writes a frame's end, its last pass: an [`Effect`], or a draw, [`Onto`].
#[diagnostic::on_unimplemented(
    message = "a frame ends with an `Effect` or a draw `Onto` its end, not `{Self}`",
    label = "not a pass that writes the end"
)]
pub trait Last {
    /// The pass, adding what it needs to the frame's `targets`.
    fn does(self, targets: &mut Vec<TargetSpec>) -> Does;
}

impl<R: Reads> Last for Effect<R> {
    fn does(self, _: &mut Vec<TargetSpec>) -> Does {
        Self::does(self, super::Target::End)
    }
}

impl<D: DepthArg<scale::Full, samples::One>, T: ReadTargets> Last for Onto<D, T> {
    fn does(self, targets: &mut Vec<TargetSpec>) -> Does {
        // The end is no target of the frame's, and nothing reads what the last pass returns.
        let (depth, _) = self.0.drawn(Target::<Float, scale::Full, samples::One>::at(u32::MAX), targets);

        Does::Draw { into: super::Target::End, depth, load: Load::Clear, reads: self.1.named() }
    }
}

/// A frame's description, built a pass at a time, the last by [`FrameBuilder::finish`].
#[derive(Debug)]
pub struct FrameBuilder {
    ends: Ends,
    targets: Vec<TargetSpec>,
    passes: Vec<PassSpec>,
}

impl FrameBuilder {
    /// A frame that ends as `ends` says, with no passes yet.
    #[must_use]
    pub const fn on(ends: Ends) -> Self {
        Self { ends, targets: Vec::new(), passes: Vec::new() }
    }

    /// A pass, `P`, the placements that join it drawing into a new target of `K` at the scale `S` holding `N` samples
    /// of each pixel, cleared, using depth as `depth` says, and reading `reads`, targets earlier passes returned, in
    /// order. Answers the target, and with [`depth::Write`] the depth it wrote, of the same scale and samples.
    pub fn draw<P: Pass, K: HoldsSamples<N>, S: Scaled, N: Sampled, D: DepthArg<S, N>>(
        &mut self,
        _: P,
        _: K,
        _: S,
        _: N,
        depth: D,
        reads: impl ReadTargets,
    ) -> D::Out<K> {
        let spec = TargetSpec { kind: K::KIND, scale: S::SCALE, samples: N::SAMPLES };
        let colour = Target::at(next(&mut self.targets, spec));
        let (used, out) = depth.drawn(colour, &mut self.targets);
        self.add::<P>(Does::Draw { into: colour.named(), depth: used, load: Load::Clear, reads: reads.named() });

        out
    }

    /// A pass, `P`, the placements that join it drawing over what earlier passes drew into `target`, using depth as
    /// `depth` says, and reading `reads`. Answers the target as this pass leaves it, and with [`depth::Write`] the
    /// depth it wrote.
    pub fn draw_over<P: Pass, K: Colour, S: Scaled, N: Sampled, D: DepthArg<S, N>>(
        &mut self,
        _: P,
        target: Target<K, S, N>,
        depth: D,
        reads: impl ReadTargets,
    ) -> D::Out<K> {
        let (used, out) = depth.drawn(target, &mut self.targets);
        self.add::<P>(Does::Draw { into: target.named(), depth: used, load: Load::Keep, reads: reads.named() });

        out
    }

    /// A compute pass, `P`, which runs every dispatch that joins it and draws into nothing: what they write is read by
    /// the passes after it in the same `Render` (ADR-0026). It is never the frame's last, as [`finish`](Self::finish)
    /// takes the pass that writes the end.
    pub fn compute<P: Pass>(&mut self, _: P) {
        self.add::<P>(Does::Compute);
    }

    /// A pass, `P`, drawing `effect` over a new target of `K` at the scale `S`, of one sample, as an effect draws one
    /// sample a pixel. Answers the target.
    pub fn effect<P: Pass, R: Reads, K: Colour, S: Scaled>(
        &mut self,
        _: P,
        effect: Effect<R>,
        _: K,
        _: S,
    ) -> Target<K, S> {
        let into = next(&mut self.targets, TargetSpec { kind: K::KIND, scale: S::SCALE, samples: Samples::One });
        self.add::<P>(effect.does(super::Target::Texture(into)));

        Target::at(into)
    }

    /// The frame, its last pass, `P`, writing its end as `last` says: refused at once, before anything is sent, when
    /// two of its passes are of one type.
    ///
    /// # Errors
    ///
    /// [`FrameRefused::Repeated`], naming the later of two passes of one type.
    pub fn finish<P: Pass>(mut self, _: P, last: impl Last) -> Result<CreateFrame, FrameRefused> {
        let does = last.does(&mut self.targets);
        self.add::<P>(does);
        let names: Vec<PassName> = self.passes.iter().map(|pass| pass.name).collect();
        for (name, at) in names.iter().zip(0..) {
            let repeated = names[..at as usize].contains(name);
            if repeated {
                return Err(FrameRefused::Repeated { pass: at });
            }
        }

        Ok(CreateFrame { ends: self.ends, targets: self.targets, passes: self.passes })
    }

    /// Adds the pass `P`, doing as `does` says.
    fn add<P: Pass>(&mut self, does: Does) {
        self.passes.push(PassSpec { name: PassName::of::<P>(), does });
    }
}
