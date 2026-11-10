//! The renderer's tool `read_target` (ADR-0048): what a window's main frame drew into one of its targets at a pixel,
//! or a small square around it, answered as numbers: depth, a program's ids, or floats.

#[cfg(test)]
mod tests;

use crate::Renderer;
use crate::frame::{Frame, TargetKind};
use crate::readback::{ReadFrameTarget, ReadRefused, Region, TargetPixels};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};
use aethereal_window::WindowId;
use std::collections::BTreeMap;

/// The most bytes reading an input may take: a window, a target, a pixel, and a radius.
const INPUT_BYTES: usize = 64;

/// The most pixels `around` reaches from the one asked.
pub const MOST_AROUND: u32 = 16;

/// What the agent asks `read_target` for: what `window`'s main frame drew into its target at `target`, its place in
/// the frame's targets, at the pixel `at` and those within `around` of it, in the window's surface pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read_target")]
pub struct ReadTarget {
    /// The window, by its number.
    #[flag(position)]
    pub window: WindowId,
    /// The target's place in the frame's targets: 0 for depth on the renderer's own frame.
    #[flag(position)]
    pub target: u32,
    /// The pixel, in the window's surface pixels from the top left: `{"x":0,"y":0}`.
    #[flag(position)]
    pub at: Pixel,
    /// How many pixels around it are read too, 0 to 16: none for the pixel alone.
    #[flag(short = 'a')]
    pub around: Option<u32>,
}

/// A pixel, from the top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Pixel {
    pub x: u32,
    pub y: u32,
}

/// What a target held: the region read, in the target's pixels, and its values, row by row from the top.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.render.target_read")]
pub struct TargetRead {
    pub region: Region,
    pub values: Values,
}

/// A target's values, as its kind holds them: four floats a pixel, a whole number, or depth, from 0 at the near plane
/// to 1 at the far, where nothing was drawn.
#[derive(Debug, Clone, PartialEq, Plain)]
pub enum Values {
    Float(Vec<[f32; 4]>),
    Uint(Vec<u32>),
    Depth(Vec<f32>),
}

/// What the tool keeps for its ask of the renderer: which call it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.target_reading")]
pub struct Targeting {
    call: u64,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.target_tool_config")]
pub struct TargetToolConfig;

/// The tool `read_target`: asks the [`Renderer`] for the region of the window's main frame's target around the pixel,
/// as the last `Render` left it, and answers its values as numbers.
#[derive(Bound)]
#[kind(name = "aether.render.target_tool")]
pub struct TargetTool {
    /// The calls asked of the renderer and not yet answered, by number.
    calls: BTreeMap<u64, Held<Called>>,
    /// The number of the next call.
    next: u64,
}

/// What every turn of the tool may send: an ask of the renderer, and an answer to its caller.
type Sends = (Asks<Path<Renderer>, ReadFrameTarget, Targeting>, ToSender<Called>);

impl ChildOf for TargetTool {
    type Parent = Anywhere;
}

impl Actor for TargetTool {
    type Config = TargetToolConfig;
    type Key = ();
    type Handles = (Call, Answer<TargetPixels, Targeting>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: TargetToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { calls: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for TargetTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![ToolSheet::of::<ReadTarget>(
            "read_target",
            "Reads what a window's main frame drew into one of its targets at the pixel `at`, in the window's surface \
             pixels from the top left, and the pixels within `around` of it (0 to 16, 0 when left out), as the last \
             frame drawn left it: no frame is waited for, so it reads under a held clock the frame a capture just \
             showed. `target` is the target's place in the frame's targets; a program on the renderer's own frame \
             keeps depth at 0. Depth is from 0 at the near plane to 1 at the far, 1 where nothing was drawn; a program \
             that draws ids into a whole-number target is read the same way. The region is answered in the target's \
             pixels, cut to it, row by row. Turning depth into a point in the world needs the program's camera, and is \
             the program's own tool's.",
            Output::Value(SchemaNodes::of(&TargetRead::SHAPE)),
        )]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// Holds the call, and asks the renderer for the region around the pixel.
impl Handles<Call> for TargetTool {
    type Sends = Sends;

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Ok(asked) = from_bytes::<ReadTarget>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) else {
            turn.reply(&Called::Refused(String::from("that is no target to read")))?;
            return Ok(());
        };
        let reach = asked.around.unwrap_or(0);
        if reach > MOST_AROUND {
            turn.reply(&Called::Refused(format!("`around` is at most {MOST_AROUND}")))?;
            return Ok(());
        }
        let targeting = Targeting { call: self.next };
        let read =
            ReadFrameTarget { frame: Frame::Main(asked.window), target: asked.target, region: around(asked.at, reach) };
        turn.to(path::<Renderer>(())).ask(&read, &targeting)?;
        self.next += 1;
        self.calls.insert(targeting.call, turn.hold()?);

        Ok(())
    }
}

/// The renderer read the region, or would not: the call is answered with its values, or why.
impl Handles<Answer<TargetPixels, Targeting>> for TargetTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Targeting { call }): (Answered<TargetPixels>, Targeting),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(held) = self.calls.remove(&call) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(TargetPixels::Read { region, kind, pixels }) => {
                let values = values(kind, turn.read(&pixels));
                Called::output(turn, to_bytes(&TargetRead { region, values })?)
            }
            Answered::Yes(TargetPixels::Refused(why)) => Called::Refused(String::from(unread(why))),
            Answered::No(why) => Called::Refused(format!("the renderer did not answer: {why:?}")),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

/// The region read around `at`, in the window's pixels: the pixel and those within `around` of it, cut at the top
/// and left; the renderer cuts it at the bottom and right.
const fn around(at: Pixel, around: u32) -> Region {
    let x = at.x.saturating_sub(around);
    let y = at.y.saturating_sub(around);

    Region { x, y, width: at.x - x + around + 1, height: at.y - y + around + 1 }
}

/// The values `bytes` hold, laid out as a target of `kind` holds them.
fn values(kind: TargetKind, bytes: &[u8]) -> Values {
    let words = || bytes.chunks_exact(4).map(|word| [word[0], word[1], word[2], word[3]]);
    match kind {
        TargetKind::Float => Values::Float(
            bytes
                .chunks_exact(8)
                .map(|pixel| {
                    let half = |at: usize| half_to_float(u16::from_le_bytes([pixel[at], pixel[at + 1]]));
                    [half(0), half(2), half(4), half(6)]
                })
                .collect(),
        ),
        TargetKind::Uint => Values::Uint(words().map(u32::from_le_bytes).collect()),
        TargetKind::Depth => Values::Depth(words().map(f32::from_le_bytes).collect()),
    }
}

/// The `f32` an IEEE 754 half, `bits`, holds: exactly, as every half is an `f32`.
fn half_to_float(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exponent = u32::from((bits >> 10) & 0x1f);
    let fraction = u32::from(bits & 0x3ff);
    let magnitude = match (exponent, fraction) {
        (0, 0) => 0,
        // A subnormal half is a normal float: shifted until its leading one is the implicit bit.
        (0, _) => {
            let shift = fraction.leading_zeros() - 21;
            ((127 - 15 + 1 - shift) << 23) | (((fraction << shift) & 0x3ff) << 13)
        }
        (0x1f, _) => 0x7f80_0000 | (fraction << 13),
        _ => ((exponent + 127 - 15) << 23) | (fraction << 13),
    };

    f32::from_bits(sign | magnitude)
}

/// What the agent reads of why the renderer did not read the target.
const fn unread(why: ReadRefused) -> &'static str {
    match why {
        ReadRefused::Frame => "no window of that number is drawn to",
        ReadRefused::Target => "the window's main frame has no target at that place",
        ReadRefused::Multisampled => "the target holds four samples of depth, which cannot be copied",
        ReadRefused::Outside => "the pixel is not within the window's frame",
        ReadRefused::Unmapped => "the GPU did not hand the target back",
        ReadRefused::Unstored => "the region is larger than the engine may hold",
        ReadRefused::Texture
        | ReadRefused::Storage
        | ReadRefused::Undrawn
        | ReadRefused::Unshown
        | ReadRefused::Window
        | ReadRefused::Format
        | ReadRefused::Uncopied => "the renderer read back something other than a target",
    }
}
