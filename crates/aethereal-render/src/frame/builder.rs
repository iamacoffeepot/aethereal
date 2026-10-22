//! The typed builder of a frame's description: a frame that would be refused does not compile (ADR-0025).
//!
//! A target is the output of the pass that draws it, so a pass reads only what an earlier pass returned and never what
//! it writes; an effect takes targets of the kinds it was made to read and no others; and `finish` takes the pass that
//! writes the frame's end, and is the only way to a [`CreateFrame`].
//!
//! ```
//! use aethereal_data::Plain;
//! use aethereal_render::effect::EffectId;
//! use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, Uint, depth, scale};
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
//!     let (ids, depth) = frame.draw(Surfaces, Uint, scale::Full, depth::Write);
//!     let splats = frame.draw(Splats, Float, scale::Full, depth::Test(&depth));
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
//! # use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, Uint, depth, scale};
//! # use aethereal_render::frame::{CreateFrame, Ends, FrameRefused};
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.ids")] struct Ids;
//! # impl Pass for Ids {}
//! # #[derive(Plain)] #[kind(name = "aether.example.glaze")] struct Glaze;
//! # impl Pass for Glaze {}
//! fn frame(window: WindowId, glaze: &EffectId<(Float,)>) -> Result<CreateFrame, FrameRefused> {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let ids = frame.draw(Ids, Uint, scale::Full, depth::Off);
//!     frame.finish(Glaze, Effect::new(glaze, (ids,)))
//! }
//! ```
//!
//! A pass tests a depth target, and a colour target is not one:
//!
//! ```compile_fail
//! # use aethereal_data::Plain;
//! # use aethereal_render::frame::builder::{Float, FrameBuilder, depth, scale};
//! # use aethereal_render::frame::Ends;
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.surfaces")] struct Surfaces;
//! # impl Pass for Surfaces {}
//! # #[derive(Plain)] #[kind(name = "aether.example.splats")] struct Splats;
//! # impl Pass for Splats {}
//! fn frame(window: WindowId) {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let colour = frame.draw(Surfaces, Float, scale::Full, depth::Off);
//!     let splats = frame.draw(Splats, Float, scale::Full, depth::Test(&colour));
//! }
//! ```
//!
//! A target is of a size its scale makes of the end's, so a chain at reduced resolution, such as a bloom blurred at
//! half size and laid back over the scene, is passes of one frame. An effect reads targets of any scale:
//!
//! ```
//! # use aethereal_data::Plain;
//! # use aethereal_render::effect::EffectId;
//! # use aethereal_render::frame::builder::{Effect, Float, FrameBuilder, depth, scale};
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
//!     let (scene, _) = frame.draw(Scene, Float, scale::Full, depth::Write);
//!     let blurred = frame.effect(Blur, Effect::new(blur, (scene,)), Float, scale::Half);
//!     frame.finish(Bloom, Effect::new(bloom, (scene, blurred)))
//! }
//! ```
//!
//! A pass's targets are all of one size, so a pass tests a depth of the scale it draws at:
//!
//! ```compile_fail
//! # use aethereal_data::Plain;
//! # use aethereal_render::frame::builder::{Float, FrameBuilder, depth, scale};
//! # use aethereal_render::frame::Ends;
//! # use aethereal_render::pass::Pass;
//! # use aethereal_window::WindowId;
//! # #[derive(Plain)] #[kind(name = "aether.example.surfaces")] struct Surfaces;
//! # impl Pass for Surfaces {}
//! # #[derive(Plain)] #[kind(name = "aether.example.splats")] struct Splats;
//! # impl Pass for Splats {}
//! fn frame(window: WindowId) {
//!     let mut frame = FrameBuilder::on(Ends::Window(window));
//!     let (_, depth) = frame.draw(Surfaces, Float, scale::Full, depth::Write);
//!     let splats = frame.draw(Splats, Float, scale::Half, depth::Test(&depth));
//! }
//! ```

use super::{CreateFrame, DepthUse, Does, Ends, FrameRefused, Load, PassSpec, Scale, TargetKind, TargetSpec};
use crate::effect::{EffectId, ReadKind, Reads};
use crate::pass::{Pass, PassName};
use crate::placement::Bind;
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

/// A target of the frame being built, of the kind `K` and the scale `S`: the output of the pass that drew it, which a
/// later pass reads, tests, or draws over.
pub struct Target<K, S = scale::Full> {
    at: u32,
    kind: PhantomData<fn() -> (K, S)>,
}

/// A depth target of the scale `S`, written by the pass that returned it.
pub type DepthTarget<S = scale::Full> = Target<Depth32, S>;

impl<K, S> Clone for Target<K, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K, S> Copy for Target<K, S> {}

impl<K, S> Debug for Target<K, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Target").field(&self.at).finish()
    }
}

impl<K, S> Target<K, S> {
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
    use super::{DepthTarget, scale};

    /// It uses none.
    #[derive(Debug, Clone, Copy)]
    pub struct Off;

    /// It writes a depth target of its own, cleared to the far plane, which it returns beside its colour.
    #[derive(Debug, Clone, Copy)]
    pub struct Write;

    /// It tests the depth an earlier pass wrote, of the scale of the target it draws into.
    #[derive(Debug, Clone, Copy)]
    pub struct Test<'a, S = scale::Full>(pub &'a DepthTarget<S>);
}

/// Keeps [`DepthArg`] to the three uses this module gives, and [`Scaled`] to its four scales.
mod sealed {
    pub trait Sealed {}
}

impl sealed::Sealed for depth::Off {}
impl sealed::Sealed for depth::Write {}
impl<S> sealed::Sealed for depth::Test<'_, S> {}

/// How a draw pass into a target of the scale `S` uses depth, and what it returns: its colour target, and with
/// [`depth::Write`] the depth target it wrote, of the same scale, as a pass's targets are all of one size.
#[diagnostic::on_unimplemented(
    message = "a pass into a target of `{S}` uses depth as `depth::Off`, `depth::Write`, or `depth::Test(&depth)` of a \
               depth target of `{S}`, not `{Self}`",
    label = "not a use of depth at this target's scale"
)]
pub trait DepthArg<S: Scaled>: sealed::Sealed {
    /// What the pass returns of a colour target of the kind `K`.
    type Out<K>;

    /// The use, adding the depth target it writes to the frame's `targets`, and what the pass that drew `colour`
    /// returns.
    fn drawn<K>(self, colour: Target<K, S>, targets: &mut Vec<TargetSpec>) -> (DepthUse, Self::Out<K>);
}

impl<S: Scaled> DepthArg<S> for depth::Off {
    type Out<K> = Target<K, S>;

    fn drawn<K>(self, colour: Target<K, S>, _: &mut Vec<TargetSpec>) -> (DepthUse, Target<K, S>) {
        (DepthUse::Off, colour)
    }
}

impl<S: Scaled> DepthArg<S> for depth::Write {
    type Out<K> = (Target<K, S>, DepthTarget<S>);

    fn drawn<K>(self, colour: Target<K, S>, targets: &mut Vec<TargetSpec>) -> (DepthUse, Self::Out<K>) {
        let at = next(targets, TargetSpec { kind: TargetKind::Depth, scale: S::SCALE });

        (DepthUse::Write(at), (colour, Target::at(at)))
    }
}

impl<S: Scaled> DepthArg<S> for depth::Test<'_, S> {
    type Out<K> = Target<K, S>;

    fn drawn<K>(self, colour: Target<K, S>, _: &mut Vec<TargetSpec>) -> (DepthUse, Target<K, S>) {
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

impl ReadsAs<()> for () {
    fn named(self) -> Vec<super::Target> {
        Vec::new()
    }
}

impl<A, SA> ReadsAs<(A,)> for (Target<A, SA>,) {
    fn named(self) -> Vec<super::Target> {
        vec![self.0.named()]
    }
}

impl<A, SA, B, SB> ReadsAs<(A, B)> for (Target<A, SA>, Target<B, SB>) {
    fn named(self) -> Vec<super::Target> {
        vec![self.0.named(), self.1.named()]
    }
}

impl<A, SA, B, SB, C, SC> ReadsAs<(A, B, C)> for (Target<A, SA>, Target<B, SB>, Target<C, SC>) {
    fn named(self) -> Vec<super::Target> {
        vec![self.0.named(), self.1.named(), self.2.named()]
    }
}

impl<A, SA, B, SB, C, SC, D, SD> ReadsAs<(A, B, C, D)>
    for (Target<A, SA>, Target<B, SB>, Target<C, SC>, Target<D, SD>)
{
    fn named(self) -> Vec<super::Target> {
        vec![self.0.named(), self.1.named(), self.2.named(), self.3.named()]
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

/// A draw onto the frame's end, the last pass, using depth as `D` says: a depth of the end's own size.
#[derive(Debug, Clone, Copy)]
pub struct Onto<D>(pub D);

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

impl<D: DepthArg<scale::Full>> Last for Onto<D> {
    fn does(self, targets: &mut Vec<TargetSpec>) -> Does {
        // The end is no target of the frame's, and nothing reads what the last pass returns.
        let (depth, _) = self.0.drawn(Target::<Float, scale::Full>::at(u32::MAX), targets);

        Does::Draw { into: super::Target::End, depth, load: Load::Clear }
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

    /// A pass, `P`, the placements that join it drawing into a new target of `K` at the scale `S`, cleared, using
    /// depth as `depth` says. Answers the target, and with [`depth::Write`] the depth it wrote, of the same scale.
    pub fn draw<P: Pass, K: Colour, S: Scaled, D: DepthArg<S>>(&mut self, _: P, _: K, _: S, depth: D) -> D::Out<K> {
        let colour = Target::at(next(&mut self.targets, TargetSpec { kind: K::KIND, scale: S::SCALE }));
        let (used, out) = depth.drawn(colour, &mut self.targets);
        self.add::<P>(Does::Draw { into: colour.named(), depth: used, load: Load::Clear });

        out
    }

    /// A pass, `P`, the placements that join it drawing over what earlier passes drew into `target`, using depth as
    /// `depth` says. Answers the target as this pass leaves it, and with [`depth::Write`] the depth it wrote.
    pub fn draw_over<P: Pass, K: Colour, S: Scaled, D: DepthArg<S>>(
        &mut self,
        _: P,
        target: Target<K, S>,
        depth: D,
    ) -> D::Out<K> {
        let (used, out) = depth.drawn(target, &mut self.targets);
        self.add::<P>(Does::Draw { into: target.named(), depth: used, load: Load::Keep });

        out
    }

    /// A compute pass, `P`, which runs every dispatch that joins it and draws into nothing: what they write is read by
    /// the passes after it in the same `Render` (ADR-0026). It is never the frame's last, as [`finish`](Self::finish)
    /// takes the pass that writes the end.
    pub fn compute<P: Pass>(&mut self, _: P) {
        self.add::<P>(Does::Compute);
    }

    /// A pass, `P`, drawing `effect` over a new target of `K` at the scale `S`. Answers the target.
    pub fn effect<P: Pass, R: Reads, K: Colour, S: Scaled>(
        &mut self,
        _: P,
        effect: Effect<R>,
        _: K,
        _: S,
    ) -> Target<K, S> {
        let into = next(&mut self.targets, TargetSpec { kind: K::KIND, scale: S::SCALE });
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
