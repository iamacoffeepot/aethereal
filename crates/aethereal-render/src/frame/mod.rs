//! Frames: an ordered list of passes over the textures they draw into, run at every `Render` (ADR-0025). Until an
//! actor makes one, each window's main frame is the renderer's own: one pass, [`Main`](crate::pass::Main), onto the
//! window with depth.

use crate::pass::{Main, Pass, PassName};
use crate::placement::JoinRefused;
use crate::shape::{ColourFormat, DepthFormat, Samples, Shape};
use aethereal_data::{Kind, KindName, Plain};
use aethereal_window::WindowId;

/// A frame, as a placement joins its passes: a window's main frame, whichever frame it is at each `Render`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Plain)]
pub enum Frame {
    Main(WindowId),
}

impl Kind for Frame {
    const NAME: KindName = KindName::new("aether.render.frame");
}

/// One pass of a frame: its name, and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PassSpec {
    pub name: PassName,
    pub does: Does,
}

/// What a pass does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Does {
    /// The placements that join it draw into `into`, with `depth`, beginning from what `load` says.
    Draw { into: Target, depth: DepthUse, load: Load },
}

/// What a pass draws into: the frame's end, or one of its targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    End,
}

/// How a pass uses depth: it writes the depth target at the index, cleared to the far plane as it begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DepthUse {
    Write(u32),
}

/// What a pass begins from in what it draws into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Load {
    /// Black, and nothing drawn.
    Clear,
}

/// A target of a frame besides its end: each is the size of the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetSpec {
    /// Depth, of the format every program tests against.
    Depth,
}

/// A frame, described: its targets, and its passes in the order they run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Description {
    pub targets: Vec<TargetSpec>,
    pub passes: Vec<PassSpec>,
}

impl Description {
    /// The renderer's own main frame of a window: one pass, [`Main`], onto the window, writing a depth it clears to the
    /// far plane, as ADR-0023's one pass was.
    pub fn own() -> Self {
        let main = PassSpec {
            name: PassName::of::<Main>(),
            does: Does::Draw { into: Target::End, depth: DepthUse::Write(0), load: Load::Clear },
        };

        Self { targets: vec![TargetSpec::Depth], passes: vec![main] }
    }

    /// The shape of each pass, in order, for a frame whose end is of `end`.
    pub fn shapes(&self, end: ColourFormat) -> Vec<Shape> {
        self.passes
            .iter()
            .map(|pass| match pass.does {
                Does::Draw { into: Target::End, depth: DepthUse::Write(_), .. } => {
                    Shape { colour: end, samples: Samples::One, depth: DepthFormat::Depth32Float }
                }
            })
            .collect()
    }
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
