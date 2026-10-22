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
use aethereal_data::{ActorId, Kind, KindName, Plain};
use aethereal_window::WindowId;

/// A frame, as a placement joins its passes: a window's main frame, whichever frame it is at each `Render`, or a frame
/// that ends on a texture, by its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub enum Frame {
    Main(WindowId),
    Texture(FrameId),
}

impl Kind for Frame {
    const NAME: KindName = KindName::new("aether.render.frame");
}

/// Asks the renderer for a frame: its passes in the order they run, over its end and `targets`, each the size of its
/// end. One that ends on a window is the window's main frame from the next `Render`, in place of the one before.
/// Answered with [`Framed`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct CreateFrame {
    pub ends: Ends,
    pub targets: Vec<TargetSpec>,
    pub passes: Vec<PassSpec>,
}

impl Kind for CreateFrame {
    const NAME: KindName = KindName::new("aether.render.create_frame");
}

/// Where a frame ends: shown on a window, as its main frame, or drawn into a texture of a size and a format, which any
/// actor handed its id binds or reads back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Ends {
    Window(WindowId),
    Texture { size: Size, format: TextureFormat },
}

impl Kind for Ends {
    const NAME: KindName = KindName::new("aether.render.ends");
}

/// The size of a texture a frame ends on, in its pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Kind for Size {
    const NAME: KindName = KindName::new("aether.render.size");
}

/// A target of a frame besides its end, each the size of the end: floats, whole numbers, or depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum TargetSpec {
    /// Four 16-bit floats a pixel.
    Float,
    /// One 32-bit whole number a pixel.
    Uint,
    /// Depth, of the format every program tests against.
    Depth,
}

impl Kind for TargetSpec {
    const NAME: KindName = KindName::new("aether.render.target_spec");
}

impl TargetSpec {
    /// The colour format a pass draws into it in: none for depth.
    #[must_use]
    pub const fn colour(self) -> Option<ColourFormat> {
        match self {
            Self::Float => Some(ColourFormat::Rgba16Float),
            Self::Uint => Some(ColourFormat::R32Uint),
            Self::Depth => None,
        }
    }

    /// How an effect reads it.
    #[must_use]
    pub const fn read(self) -> Read {
        match self {
            Self::Float => Read::Float,
            Self::Uint => Read::Uint,
            Self::Depth => Read::Depth,
        }
    }
}

/// One pass of a frame: its name, which placements join it by, and what it does.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct PassSpec {
    pub name: PassName,
    pub does: Does,
}

impl Kind for PassSpec {
    const NAME: KindName = KindName::new("aether.render.pass_spec");
}

/// What a pass does.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Does {
    /// The placements that join it draw into `into`, with `depth`, beginning from what `load` says.
    Draw { into: Target, depth: DepthUse, load: Load },
    /// The effect `program` draws once over the whole of `into`, reading the targets `reads` names, in order, and
    /// binding what `bindings` names in the groups after the first. It begins from what `into` holds when an earlier
    /// pass wrote it, and from black when none did.
    Effect { program: ProgramId, reads: Vec<Target>, into: Target, bindings: Vec<Bind> },
    /// The dispatches that join it run, and it draws into nothing: what they write is read by the passes after it in
    /// the same `Render`, and by those before it as written at the last (ADR-0026). It is never a frame's last pass,
    /// as the last writes the frame's end.
    Compute,
}

impl Kind for Does {
    const NAME: KindName = KindName::new("aether.render.does");
}

/// What a pass draws into or reads: the frame's end, or one of its targets, by its place in `targets`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Target {
    End,
    Texture(u32),
}

impl Kind for Target {
    const NAME: KindName = KindName::new("aether.render.target");
}

/// How a draw pass uses depth: none, or the depth target at a place in `targets`, written from the far plane, or
/// tested as an earlier pass left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum DepthUse {
    Off,
    Write(u32),
    Test(u32),
}

impl Kind for DepthUse {
    const NAME: KindName = KindName::new("aether.render.depth_use");
}

/// What a draw pass begins from in what it draws into: black, or what an earlier pass of the frame drew there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Load {
    Clear,
    Keep,
}

impl Kind for Load {
    const NAME: KindName = KindName::new("aether.render.load");
}

/// A frame the renderer made, which only its maker may release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub struct FrameId {
    pub(crate) id: u64,
}

impl Kind for FrameId {
    const NAME: KindName = KindName::new("aether.render.frame_id");
}

/// What the renderer answers [`CreateFrame`] with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
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

impl Kind for Framed {
    const NAME: KindName = KindName::new("aether.render.framed");
}

/// Why a frame was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
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

impl Kind for FrameRefused {
    const NAME: KindName = KindName::new("aether.render.frame_refused");
}

/// Why one pass of a frame was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum PassRefused {
    /// It names a target past the frame's last.
    Target,
    /// It draws or writes colour into a depth target.
    Colour,
    /// It uses a colour target as depth.
    Depth,
    /// It reads, keeps, or tests a target no earlier pass wrote.
    Unwritten,
    /// It reads the frame's end, which nothing reads.
    End,
    /// It reads the target it writes.
    Itself,
    /// No effect is kept under the id: never made, released, or a program that is not an effect.
    Effect,
    /// It reads targets of other kinds, or another count of them, than the effect was made to read.
    Reads,
    /// The effect is made for another shape than the target it writes.
    Shape,
    /// What it binds besides is not what the effect binds.
    Bindings { why: JoinRefused },
}

impl Kind for PassRefused {
    const NAME: KindName = KindName::new("aether.render.pass_refused");
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
            does: Does::Draw { into: Target::End, depth: DepthUse::Write(0), load: Load::Clear },
        };

        Self { ends: Ends::Window(window), targets: vec![TargetSpec::Depth], passes: vec![compute, main] }
    }

    /// The shape of a pass that draws into `into`, using depth as `depth` says, in a frame whose end is of `end`, as
    /// its check let through. An effect uses no depth.
    pub(crate) fn shape(&self, into: Target, depth: DepthUse, end: ColourFormat) -> Shape {
        let depth = match depth {
            DepthUse::Off => DepthFormat::None,
            DepthUse::Write(_) | DepthUse::Test(_) => DepthFormat::Depth32Float,
        };

        Shape { colour: self.colour(into, end).unwrap_or(end), samples: Samples::One, depth }
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
/// testing only what an earlier pass wrote, never what it writes, each writing colour into colour and depth into
/// depth, each effect reading and binding what it was made to and made for the shape it writes, and that its last
/// pass writes its end.
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
    match does {
        Does::Draw { depth, load, .. } => {
            let keeps = *load == Load::Keep;
            if keeps && !written.wrote(into) {
                return Err(PassRefused::Unwritten);
            }
            match depth {
                DepthUse::Off => {}
                DepthUse::Write(at) => {
                    depth_of(create, *at)?;
                    written.write(Target::Texture(*at));
                }
                DepthUse::Test(at) => {
                    depth_of(create, *at)?;
                    if !written.wrote(Target::Texture(*at)) {
                        return Err(PassRefused::Unwritten);
                    }
                }
            }
        }
        Does::Effect { program, reads, bindings, .. } => {
            let effect = kept.effect(*program).ok_or(PassRefused::Effect)?;
            let mut read = Vec::with_capacity(reads.len());
            for target in reads {
                let Target::Texture(at) = *target else {
                    return Err(PassRefused::End);
                };
                let spec = *create.targets.get(at as usize).ok_or(PassRefused::Target)?;
                if *target == into {
                    return Err(PassRefused::Itself);
                }
                if !written.wrote(*target) {
                    return Err(PassRefused::Unwritten);
                }
                read.push(spec.read());
            }
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

/// The colour format `target` is drawn in, the end being of `end`: refused when it is past the last target or a depth
/// target.
fn colour_of(create: &CreateFrame, target: Target, end: ColourFormat) -> Result<ColourFormat, PassRefused> {
    let Target::Texture(at) = target else {
        return Ok(end);
    };

    create.targets.get(at as usize).ok_or(PassRefused::Target)?.colour().ok_or(PassRefused::Colour)
}

/// Checks that the target at `at` is one of depth.
fn depth_of(create: &CreateFrame, at: u32) -> Result<(), PassRefused> {
    let spec = create.targets.get(at as usize).ok_or(PassRefused::Target)?;
    if *spec != TargetSpec::Depth {
        return Err(PassRefused::Depth);
    }

    Ok(())
}

/// Asks the renderer the shape of the pass `pass` of `frame` as it stands: what a program that joins it is made
/// against. Answered with [`Shaped`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct ShapeOf {
    pub frame: Frame,
    pub pass: PassName,
}

impl Kind for ShapeOf {
    const NAME: KindName = KindName::new("aether.render.shape_of");
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
pub enum Shaped {
    Shape(Shape),
    Refused(JoinRefused),
}

impl Kind for Shaped {
    const NAME: KindName = KindName::new("aether.render.shaped");
}
