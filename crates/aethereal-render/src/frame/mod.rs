//! Frames: an ordered list of passes over the textures they draw into, made once, checked whole, and run at every
//! `Render` (ADR-0025). Until an actor makes one, each window's main frame is the renderer's own: a compute pass,
//! [`Compute`](crate::pass::Compute), then one pass, [`Main`](crate::pass::Main), onto the window with depth.

pub mod builder;
mod order;
#[cfg(test)]
mod tests;

pub(crate) use order::{ordered, reaches};

pub use builder::FrameBuilder;

use crate::effect::Read;
use crate::pass::{Compute, Main, Pass, PassName};
use crate::placement::{self, Bind, Finds, JoinRefused};
use crate::program::{Binding, ProgramId};
use crate::shape::{ColourFormat, DepthFormat, Samples, Shape};
use crate::texture::{TextureFormat, TextureId};
use aethereal_data::{ActorId, Plain};
use aethereal_window::WindowId;

/// A frame, as a placement joins its passes: a window's main frame, whichever frame it is at each `Render`, or a frame
/// that ends on a texture, by its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.frame")]
pub enum Frame {
    Main(WindowId),
    Texture(FrameId),
}

/// Asks the renderer for a frame: its passes in the order they run, over its end and `targets`, each the size of its
/// end or a part of it that its scale says. One that ends on a window is the window's main frame from the next `Render`, in place of the one before.
/// Answered with [`Framed`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.create_frame")]
pub struct CreateFrame {
    pub ends: Ends,
    pub targets: Vec<TargetSpec>,
    pub passes: Vec<PassSpec>,
}

/// Where a frame ends: shown on a window, as its main frame, or drawn into a texture of a size and a format, which any
/// actor handed its id binds or reads back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.ends")]
pub enum Ends {
    Window(WindowId),
    Texture { size: Size, format: TextureFormat },
}

/// The size of a texture a frame ends on, in its pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.size")]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    /// One pixel, the size a frame that ends on a window is made at before its first `Render` fits it.
    pub const PIXEL: Self = Self { width: 1, height: 1 };

    /// The size `scale` makes of it: each side divided and rounded up, so an odd side keeps its last pixel, and a side
    /// of at least one pixel, as every end's is, keeps at least one.
    #[must_use]
    pub const fn scaled(self, scale: Scale) -> Self {
        let by = scale.divisor();

        Self { width: self.width.div_ceil(by), height: self.height.div_ceil(by) }
    }
}

/// A target of a frame besides its end: what it holds, its size, a part of the end's, and how many samples it holds of
/// each pixel. One of floats of four samples is two textures, the four a pass draws into and the one they are resolved
/// into as each pass that draws into it ends, which is what any later pass or frame reads; one of depth of four is
/// tested and written, and never read; one of whole numbers holds one (ADR-0025).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.target_spec")]
pub struct TargetSpec {
    pub kind: TargetKind,
    pub scale: Scale,
    pub samples: Samples,
}

/// What a target holds: floats, whole numbers, or depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.target_kind")]
pub enum TargetKind {
    /// Four 16-bit floats a pixel.
    Float,
    /// One 32-bit whole number a pixel.
    Uint,
    /// Depth, of the format every program tests against.
    Depth,
}

/// The size of a target, as a part of its frame's end: each side the end's divided by 1, 2, 4, or 8, and rounded up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.scale")]
pub enum Scale {
    Full,
    Half,
    Quarter,
    Eighth,
}

impl Scale {
    /// What each side of the end is divided by.
    #[must_use]
    pub const fn divisor(self) -> u32 {
        match self {
            Self::Full => 1,
            Self::Half => 2,
            Self::Quarter => 4,
            Self::Eighth => 8,
        }
    }
}

impl TargetSpec {
    /// The colour format a pass draws into it in: none for depth.
    #[must_use]
    pub const fn colour(self) -> Option<ColourFormat> {
        match self.kind {
            TargetKind::Float => Some(ColourFormat::Rgba16Float),
            TargetKind::Uint => Some(ColourFormat::R32Uint),
            TargetKind::Depth => None,
        }
    }

    /// How a pass reads it.
    #[must_use]
    pub const fn read(self) -> Read {
        match self.kind {
            TargetKind::Float => Read::Float,
            TargetKind::Uint => Read::Uint,
            TargetKind::Depth => Read::Depth,
        }
    }
}

/// One pass of a frame: its name, which placements join it by, and what it does.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.pass_spec")]
pub struct PassSpec {
    pub name: PassName,
    pub does: Does,
}

/// What a pass does.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.does")]
pub enum Does {
    /// The placements that join it draw into `into`, with `depth`, beginning from what `load` says, reading the
    /// targets `reads` names, in order, at group 0 of each program that joins it, as an effect reads its own.
    Draw { into: Target, depth: DepthUse, load: Load, reads: Vec<Target> },
    /// The effect `program` draws once over the whole of `into`, reading the targets `reads` names, in order, and
    /// binding what `bindings` names in the groups after the first. It begins from what `into` holds when an earlier
    /// pass wrote it, and from black when none did.
    Effect { program: ProgramId, reads: Vec<Target>, into: Target, bindings: Vec<Bind> },
    /// The dispatches that join it run, and it draws into nothing: what they write is read by the passes after it in
    /// the same `Render`, and by those before it as written at the last (ADR-0026). It is never a frame's last pass,
    /// as the last writes the frame's end.
    Compute,
}

/// What a pass draws into or reads: the frame's end, or one of its targets, by its place in `targets`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.target")]
pub enum Target {
    End,
    Texture(u32),
}

/// How a draw pass uses depth: none, or the depth target at a place in `targets`, written from the far plane, or
/// tested as an earlier pass left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.depth_use")]
pub enum DepthUse {
    Off,
    Write(u32),
    Test(u32),
}

/// What a draw pass begins from in what it draws into: black, or what an earlier pass of the frame drew there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.load")]
pub enum Load {
    Clear,
    Keep,
}

/// A frame the renderer made, which only its maker may release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
#[kind(name = "aether.render.frame_id")]
pub struct FrameId {
    pub(crate) id: u64,
}

/// What the renderer answers [`CreateFrame`] with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.framed")]
pub enum Framed {
    /// A window's main frame.
    Made(FrameId),
    /// A frame that ends on a texture, and the texture: an ordinary one, which a placement or an effect binds and
    /// which is read back.
    Texture {
        frame: FrameId,
        texture: TextureId,
    },
    Refused(FrameRefused),
}

/// Why a frame was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.frame_refused")]
pub enum FrameRefused {
    /// It ends on a window the renderer does not draw to.
    Window,
    /// The window's main frame is another actor's, `owner`, which alone may replace it.
    Owned { owner: ActorId },
    /// It ends on a texture with a side of no pixels, or longer than the GPU's longest, `most`.
    Size { width: u32, height: u32, most: u32 },
    /// It has no passes.
    Empty,
    /// The pass at `pass`, counted from zero, has the name of an earlier pass.
    Repeated { pass: u32 },
    /// Its last pass does not write its end.
    Unended,
    /// The pass at `pass` was refused.
    Pass { pass: u32, why: PassRefused },
}

/// Why one pass of a frame was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.pass_refused")]
pub enum PassRefused {
    /// It names a target past the frame's last.
    Target,
    /// It draws or writes colour into a depth target.
    Colour,
    /// It uses a colour target as depth.
    Depth,
    /// Its depth target is of another scale than the target it draws into, and so of another size.
    DepthScale,
    /// It reads, keeps, or tests a target no earlier pass wrote.
    Unwritten,
    /// It reads the frame's end, which nothing reads.
    End,
    /// It reads the target it writes, or the depth it tests or writes.
    Itself,
    /// Its colour and its depth hold other counts of samples, as WebGPU draws a pass; it is an effect drawing into a
    /// target of four, as an effect draws one sample a pixel; or the frame has a target of whole numbers of four, which
    /// WebGPU does not multisample, refused at the first pass that names it, or at the first pass when none does.
    Samples,
    /// It reads a depth of four samples, which WebGPU resolves to none.
    ReadsSamples,
    /// No effect is kept under the id: never made, released, or a program that is not an effect.
    Effect,
    /// It reads targets of other kinds, or another count of them, than the effect was made to read.
    Reads,
    /// The effect is made for another shape than the target it writes.
    Shape,
    /// What it binds besides is not what the effect binds.
    Bindings { why: JoinRefused },
}

impl CreateFrame {
    /// The renderer's own main frame of `window`: a compute pass, [`Compute`], which every dispatch that joins it runs
    /// in, then one pass, [`Main`], onto the window, writing a depth it clears to the far plane, as ADR-0023's one
    /// pass was (ADR-0026).
    #[must_use]
    pub fn own(window: WindowId) -> Self {
        let compute = PassSpec { name: PassName::of::<Compute>(), does: Does::Compute };
        let main = PassSpec {
            name: PassName::of::<Main>(),
            does: Does::Draw { into: Target::End, depth: DepthUse::Write(0), load: Load::Clear, reads: Vec::new() },
        };

        let depth = TargetSpec { kind: TargetKind::Depth, scale: Scale::Full, samples: Samples::One };

        Self { ends: Ends::Window(window), targets: vec![depth], passes: vec![compute, main] }
    }

    /// The shape of a pass that draws into `into`, using depth as `depth` says, in a frame whose end is of `end`, as
    /// its check let through: of the samples its colour holds, as its depth holds the same. An effect uses no depth.
    pub(crate) fn shape(&self, into: Target, depth: DepthUse, end: ColourFormat) -> Shape {
        let depth = match depth {
            DepthUse::Off => DepthFormat::None,
            DepthUse::Write(_) | DepthUse::Test(_) => DepthFormat::Depth32Float,
        };

        Shape { colour: self.colour(into, end).unwrap_or(end), samples: self.samples(into), depth }
    }

    /// How many samples `target` holds of each pixel: the end holds one.
    fn samples(&self, target: Target) -> Samples {
        match target {
            Target::End => Samples::One,
            Target::Texture(at) => self.targets.get(at as usize).map_or(Samples::One, |spec| spec.samples),
        }
    }

    /// The colour format `target` is drawn in, the end being of `end`: none for a depth target, or one past the last.
    fn colour(&self, target: Target, end: ColourFormat) -> Option<ColourFormat> {
        match target {
            Target::End => Some(end),
            Target::Texture(at) => self.targets.get(at as usize)?.colour(),
        }
    }
}

/// What an effect pass is checked against of the effect it draws: what it reads, what it binds besides, and the shape
/// it was made for.
pub(crate) struct EffectReads<'a> {
    pub reads: &'a [Read],
    pub bindings: &'a [Binding],
    pub shape: Shape,
}

/// What the renderer keeps, as a frame's check looks it up, beside what a placement's check does.
pub(crate) trait Holds: Finds {
    /// The format of the surface of `window`: none when the renderer does not draw to it.
    fn window(&self, window: WindowId) -> Option<ColourFormat>;
    /// The longest side of a texture the GPU makes.
    fn longest(&self) -> u32;
    /// The effect kept under `id`: none when there is no program under it, or it is not an effect.
    fn effect(&self, id: ProgramId) -> Option<EffectReads<'_>>;
}

/// Checks that `by` may make a frame on a window whose main frame is owned by `owner`, or by none: the first to make one
/// owns it, and only it may replace it, until it releases it or ends (ADR-0025).
pub(crate) fn may_replace(owner: Option<ActorId>, by: ActorId) -> Result<(), FrameRefused> {
    match owner {
        Some(owner) if owner != by => Err(FrameRefused::Owned { owner }),
        Some(_) | None => Ok(()),
    }
}

/// Checks a frame before it is made, whole, as the builder would have built it and more: that it ends on a window
/// drawn to or on a texture of a size the GPU makes, that it has passes, each of a name of its own, each reading and
/// testing only what an earlier pass wrote, never what it writes or tests, and never a depth of four samples, each
/// writing colour into colour and depth into depth, of one count of samples, each effect drawing one sample a pixel,
/// reading and binding what it was made to and made for the shape it writes, no target of whole numbers holding four
/// samples, and that its last pass writes its end.
pub(crate) fn check(create: &CreateFrame, kept: &impl Holds) -> Result<(), FrameRefused> {
    let end = match create.ends {
        Ends::Window(window) => kept.window(window).ok_or(FrameRefused::Window)?,
        Ends::Texture { size: Size { width, height }, format } => {
            let most = kept.longest();
            let fits = (1..=most).contains(&width) && (1..=most).contains(&height);
            if !fits {
                return Err(FrameRefused::Size { width, height, most });
            }
            format.colour()
        }
    };
    if create.passes.is_empty() {
        return Err(FrameRefused::Empty);
    }

    let mut written = Written { end: false, targets: vec![false; create.targets.len()] };
    for (pass, at) in create.passes.iter().zip(0..) {
        let repeated = create.passes[..at as usize].iter().any(|before| before.name == pass.name);
        if repeated {
            return Err(FrameRefused::Repeated { pass: at });
        }
        check_pass(create, &pass.does, end, &mut written, kept).map_err(|why| FrameRefused::Pass { pass: at, why })?;
    }
    // One no pass named, as each pass that names one was refused it.
    let unsampled = create.targets.iter().any(|spec| spec.kind == TargetKind::Uint && spec.samples != Samples::One);
    if unsampled {
        return Err(FrameRefused::Pass { pass: 0, why: PassRefused::Samples });
    }
    let ends = create.passes.last().is_some_and(|last| last.does.writes() == Some(Target::End));
    if !ends {
        return Err(FrameRefused::Unended);
    }

    Ok(())
}

/// What the passes checked so far wrote: the end, and each target.
struct Written {
    end: bool,
    targets: Vec<bool>,
}

impl Written {
    /// Whether an earlier pass wrote `target`, which the check found in the frame.
    fn wrote(&self, target: Target) -> bool {
        match target {
            Target::End => self.end,
            Target::Texture(at) => self.targets[at as usize],
        }
    }

    /// Notes that a pass wrote `target`.
    fn write(&mut self, target: Target) {
        match target {
            Target::End => self.end = true,
            Target::Texture(at) => self.targets[at as usize] = true,
        }
    }
}

impl Does {
    /// What the pass draws into: none for a compute pass, which draws into nothing.
    #[must_use]
    pub const fn writes(&self) -> Option<Target> {
        match self {
            Self::Draw { into, .. } | Self::Effect { into, .. } => Some(*into),
            Self::Compute => None,
        }
    }

    /// What the pass binds: an effect's bindings, and nothing for a draw or a compute pass, whose placements and
    /// dispatches bind their own.
    #[must_use]
    pub fn bindings(&self) -> &[Bind] {
        match self {
            Self::Effect { bindings, .. } => bindings,
            Self::Draw { .. } | Self::Compute => &[],
        }
    }
}

/// Checks one pass of `create`, whose end is of `end`, after the passes that wrote what `written` says, and notes what
/// it writes.
fn check_pass(
    create: &CreateFrame,
    does: &Does,
    end: ColourFormat,
    written: &mut Written,
    kept: &impl Holds,
) -> Result<(), PassRefused> {
    let Some(into) = does.writes() else {
        // A compute pass reads and writes no target: its dispatches are checked as they join it.
        return Ok(());
    };
    let colour = colour_of(create, into, end)?;
    let samples = create.samples(into);
    let resolves = samples != Samples::One;
    if resolves && colour.is_whole() {
        return Err(PassRefused::Samples);
    }
    match does {
        Does::Draw { depth, load, reads, .. } => {
            let keeps = *load == Load::Keep;
            if keeps && !written.wrote(into) {
                return Err(PassRefused::Unwritten);
            }
            let drawn = scale_of(create, into);
            let own = match *depth {
                DepthUse::Off => vec![into],
                DepthUse::Write(at) | DepthUse::Test(at) => {
                    depth_of(create, at, drawn, samples)?;
                    vec![into, Target::Texture(at)]
                }
            };
            // What a draw reads is checked against each program that joins it, as the program is made to read it.
            reads_of(create, reads, &own, written)?;
            match *depth {
                DepthUse::Off => {}
                DepthUse::Write(at) => written.write(Target::Texture(at)),
                DepthUse::Test(at) => {
                    if !written.wrote(Target::Texture(at)) {
                        return Err(PassRefused::Unwritten);
                    }
                }
            }
        }
        Does::Effect { program, reads, bindings, .. } => {
            let effect = kept.effect(*program).ok_or(PassRefused::Effect)?;
            if resolves {
                return Err(PassRefused::Samples);
            }
            let read = reads_of(create, reads, &[into], written)?;
            if read != effect.reads {
                return Err(PassRefused::Reads);
            }
            if effect.shape != Shape::flat(colour) {
                return Err(PassRefused::Shape);
            }
            placement::check_bindings(bindings, effect.bindings, kept).map_err(|why| PassRefused::Bindings { why })?;
        }
        Does::Compute => {}
    }
    written.write(into);

    Ok(())
}

/// How a pass reads each target of `reads`, in order: refused when one is the end, past the last target, one of `own`,
/// the pass's own targets, one no earlier pass wrote, or a depth of four samples.
fn reads_of(
    create: &CreateFrame,
    reads: &[Target],
    own: &[Target],
    written: &Written,
) -> Result<Vec<Read>, PassRefused> {
    let mut read = Vec::with_capacity(reads.len());
    for target in reads {
        let Target::Texture(at) = *target else {
            return Err(PassRefused::End);
        };
        let spec = *create.targets.get(at as usize).ok_or(PassRefused::Target)?;
        if own.contains(target) {
            return Err(PassRefused::Itself);
        }
        if !written.wrote(*target) {
            return Err(PassRefused::Unwritten);
        }
        let resolves = spec.kind != TargetKind::Depth || spec.samples == Samples::One;
        if !resolves {
            return Err(PassRefused::ReadsSamples);
        }
        read.push(spec.read());
    }

    Ok(read)
}

/// The colour format `target` is drawn in, the end being of `end`: refused when it is past the last target or a depth
/// target.
fn colour_of(create: &CreateFrame, target: Target, end: ColourFormat) -> Result<ColourFormat, PassRefused> {
    let Target::Texture(at) = target else {
        return Ok(end);
    };

    create.targets.get(at as usize).ok_or(PassRefused::Target)?.colour().ok_or(PassRefused::Colour)
}

/// The scale of `target`, which the check found in the frame: the end is its own size.
fn scale_of(create: &CreateFrame, target: Target) -> Scale {
    match target {
        Target::End => Scale::Full,
        Target::Texture(at) => create.targets[at as usize].scale,
    }
}

/// Checks that the target at `at` is one of depth, of the scale `drawn` and the samples `samples` of the target its
/// pass draws into: a pass's targets are all of one size and one count of samples, as WebGPU draws it.
fn depth_of(create: &CreateFrame, at: u32, drawn: Scale, samples: Samples) -> Result<(), PassRefused> {
    let spec = create.targets.get(at as usize).ok_or(PassRefused::Target)?;
    if spec.kind != TargetKind::Depth {
        return Err(PassRefused::Depth);
    }
    if spec.scale != drawn {
        return Err(PassRefused::DepthScale);
    }
    if spec.samples != samples {
        return Err(PassRefused::Samples);
    }

    Ok(())
}

/// Asks the renderer the shape of the pass `pass` of `frame` as it stands: what a program that joins it is made
/// against. Answered with [`Shaped`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.shape_of")]
pub struct ShapeOf {
    pub frame: Frame,
    pub pass: PassName,
}

impl ShapeOf {
    /// The pass `P` of `frame`.
    #[must_use]
    pub const fn pass<P: Pass>(frame: Frame) -> Self {
        Self { frame, pass: PassName::of::<P>() }
    }
}

/// What the renderer answers [`ShapeOf`] with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.shaped")]
pub enum Shaped {
    Shape(Shape),
    Refused(JoinRefused),
}
