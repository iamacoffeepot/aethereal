//! Effects: programs drawn once over a whole target in an effect pass, reading targets earlier passes of the frame
//! wrote (ADR-0025), or none, as a sky or a ray tracer that begins a frame reads none. An effect is a fragment stage
//! alone; the renderer draws one triangle over the target with a vertex stage of its own.

#[cfg(test)]
mod tests;

use crate::frame::builder::{Depth32, Float, Uint};
use crate::program::{
    Binding, Blend, ProgramId, ProgramRefused, Stage, check_bindings, check_fixed, check_output, check_reads_only,
    check_stages, check_uses, entry, parsed, used,
};
use crate::shape::{DepthFormat, Shape};
use aethereal_actor::BlobRef;
use aethereal_data::{Bare, Bound, Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Writer};
use core::fmt::{self, Debug, Formatter};
use core::marker::PhantomData;
use naga::{ImageClass, ScalarKind};

/// Asks the renderer for an effect: the fragment stage `fragment` of the WGSL the blob `wgsl` holds as UTF-8, drawn
/// over the whole of a target of the shape `shape`, which holds no depth, blending by `blend`. It reads the targets
/// `reads` names, the first at `@group(0) @binding(0)`, the next at binding 1, and binds what `bindings` lists in the
/// groups after. Group 0 is its reads' whatever their count: one that reads none binds nothing there, and what it
/// lists still begins at group 1. Answered with [`Effected`].
///
/// Its fragment stage may read the target's pixel, `@builtin(position)`, and where it is across the target from its
/// top left, `@location(0) uv: vec2<f32>`, from zero to one. Targets of a frame may be of other sizes (ADR-0025:
/// `Scale`), so `uv` is the same place across a target of any size, and a target read through a sampler at `uv` is
/// read at the place shaded whatever its size. A pixel of one is the same pixel of another only when both are of one
/// scale: an effect that reads a target of another scale with `textureLoad` scales the pixel itself, by
/// `textureDimensions`, as WebGPU answers a load past a texture's edge with zero or with a texel inside it, never an
/// error. The WGSL travels as a blob, as a program's does.
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
#[kind(name = "aether.render.create_effect")]
pub struct CreateEffect {
    pub wgsl: BlobRef,
    pub fragment: String,
    pub reads: Vec<Read>,
    pub bindings: Vec<Binding>,
    pub blend: Blend,
    pub shape: Shape,
}

/// What an effect or a draw program reads of a target, and how its shader declares it. A target of four samples is
/// read as its resolved one, and a depth of four samples is never read (ADR-0025).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read")]
pub enum Read {
    /// A target of floats, `texture_2d<f32>`.
    Float,
    /// A target of whole numbers, `texture_2d<u32>`.
    Uint,
    /// A depth target, `texture_depth_2d`.
    Depth,
}

impl Read {
    /// How a shader declares a texture read so, as naga names it.
    pub(crate) const fn class(self) -> ImageClass {
        match self {
            Self::Float => ImageClass::Sampled { kind: ScalarKind::Float, multi: false },
            Self::Uint => ImageClass::Sampled { kind: ScalarKind::Uint, multi: false },
            Self::Depth => ImageClass::Depth { multi: false },
        }
    }
}

/// An effect the renderer made: the program an effect pass names, and what it reads, in order.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.made_effect")]
pub struct MadeEffect {
    pub(crate) program: ProgramId,
    pub(crate) reads: Vec<Read>,
}

impl MadeEffect {
    /// The program an effect pass names.
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }

    /// What it reads, in order.
    #[must_use]
    pub fn reads(&self) -> &[Read] {
        &self.reads
    }
}

/// What the renderer answers [`CreateEffect`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.effected")]
pub enum Effected {
    Made(MadeEffect),
    Refused(ProgramRefused),
}

/// A kind of target an effect reads: [`Float`], [`Uint`], or [`Depth32`].
#[diagnostic::on_unimplemented(
    message = "an effect reads targets of `Float`, `Uint`, or `Depth32`, not `{Self}`",
    label = "not a kind of target"
)]
pub trait ReadKind {
    /// How it is read.
    const READ: Read;
}

impl ReadKind for Float {
    const READ: Read = Read::Float;
}

impl ReadKind for Uint {
    const READ: Read = Read::Uint;
}

impl ReadKind for Depth32 {
    const READ: Read = Read::Depth;
}

/// What an effect reads, in order, as a tuple of the kinds of its targets: `(Float, Uint)`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not what an effect reads",
    label = "an effect reads a tuple of up to four kinds of target",
    note = "write what it reads as a tuple of `Float`, `Uint`, and `Depth32`: `()`, `(Float,)`, `(Float, Uint)`"
)]
pub trait Reads: 'static {
    /// How each is read, in order.
    const READS: &'static [Read];
}

/// An effect that reads no target: one that begins a frame, as a sky or a ray tracer does.
impl Reads for () {
    const READS: &'static [Read] = &[];
}

impl<A: ReadKind + 'static> Reads for (A,) {
    const READS: &'static [Read] = &[A::READ];
}

impl<A: ReadKind + 'static, B: ReadKind + 'static> Reads for (A, B) {
    const READS: &'static [Read] = &[A::READ, B::READ];
}

impl<A: ReadKind + 'static, B: ReadKind + 'static, C: ReadKind + 'static> Reads for (A, B, C) {
    const READS: &'static [Read] = &[A::READ, B::READ, C::READ];
}

impl<A: ReadKind + 'static, B: ReadKind + 'static, C: ReadKind + 'static, D: ReadKind + 'static> Reads
    for (A, B, C, D)
{
    const READS: &'static [Read] = &[A::READ, B::READ, C::READ, D::READ];
}

/// An effect, typed by what it reads, `EffectId<(Float, Uint)>`: what a frame's builder takes targets of those kinds
/// for and no others. It is made only from the renderer's answer, when the answer reads what `R` says. On the wire it
/// is the program's id alone, as a guest names it; the renderer checks what an effect pass reads against what the
/// effect was made to read, whatever a sender claims.
pub struct EffectId<R> {
    program: ProgramId,
    reads: PhantomData<fn() -> R>,
}

impl<R: Reads> EffectId<R> {
    /// The effect `made`, when it reads what `R` says: none when it reads anything else.
    #[must_use]
    pub fn of(made: &MadeEffect) -> Option<Self> {
        let reads_as = made.reads == R::READS;

        reads_as.then_some(Self { program: made.program, reads: PhantomData })
    }
}

impl<R> EffectId<R> {
    /// The program an effect pass names.
    #[must_use]
    pub const fn program(&self) -> ProgramId {
        self.program
    }
}

impl<R> Clone for EffectId<R> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<R> Copy for EffectId<R> {}

impl<R> Debug for EffectId<R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("EffectId").field("program", &self.program).finish()
    }
}

impl<R> PartialEq for EffectId<R> {
    fn eq(&self, other: &Self) -> bool {
        self.program == other.program
    }
}

impl<R> Eq for EffectId<R> {}

/// Written as its program's id: what it reads is its type's, and the renderer's to check.
impl<R> Schema for EffectId<R> {
    const SHAPE: aethereal_data::Shape = ProgramId::SHAPE;
}

impl<R, B: Boundary> Encode<B> for EffectId<R> {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.program.encode(out)
    }
}

impl<R, B: Boundary> Decode<B> for EffectId<R> {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Ok(Self { program: ProgramId::decode(input)?, reads: PhantomData })
    }
}

impl<R> Plain for EffectId<R> where Self: Encode<Bare> + Decode<Bare> {}

/// The renderer's vertex stage of every effect: one triangle over the whole target, writing where each pixel is across
/// it from its top left at location 0.
pub(crate) const COVER: &str = "
struct Covered {
    @builtin(position) at: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn cover(@builtin(vertex_index) corner: u32) -> Covered {
    let uv = vec2(f32((corner << 1u) & 2u), f32(corner & 2u));
    return Covered(vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}
";

/// The entry point of [`COVER`].
pub(crate) const COVER_ENTRY: &str = "cover";

/// Checks an effect before it is made, with `wgsl` the WGSL its blob holds: that its shape holds no depth and takes
/// its blend, that it binds nothing in group 0 and what it binds after within `limits`, storage only to read, and that
/// the WGSL is a valid module whose fragment stage reads what the renderer's vertex stage writes, writes the colour
/// its shape draws, reads each target in group 0 as `reads` says, and binds only what `bindings` lists.
///
/// # Errors
///
/// Why it would not be made as asked.
pub fn check(create: &CreateEffect, wgsl: &str, limits: &wgpu::Limits) -> Result<(), ProgramRefused> {
    if create.shape.depth != DepthFormat::None {
        return Err(ProgramRefused::EffectDepth);
    }
    check_fixed(create.blend, create.shape)?;
    let reserved = create.bindings.iter().find(|listed| listed.group == 0);
    if let Some(listed) = reserved {
        return Err(ProgramRefused::Reserved { binding: listed.binding });
    }
    // An effect reads a handful of targets, far below what a `u32` counts.
    let reads = u32::try_from(create.reads.len()).unwrap_or(u32::MAX);
    check_bindings(&create.bindings, reads, limits)?;
    check_reads_only(&create.bindings)?;

    let (cover, _) = parsed(COVER)?;
    let (module, info) = parsed(wgsl)?;
    let vertex = entry(&cover, Stage::Vertex, COVER_ENTRY)?;
    let fragment = entry(&module, Stage::Fragment, &create.fragment)?;
    check_stages((&cover, vertex), (&module, fragment))?;
    check_output(&module, fragment, create.shape.colour)?;
    // Group 0 is its reads' whatever their count, and it lists nothing there, so one that reads none binds nothing
    // there.
    check_uses(&module, &info, fragment, |group, binding| used(&create.bindings, &create.reads, group, binding))
}
