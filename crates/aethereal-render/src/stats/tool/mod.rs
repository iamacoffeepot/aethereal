//! The renderer's tool `render_stats` (ADR-0053): the next frame's statistics, the GPU's time over its passes, and the
//! adapter, so an agent reads the overlay's meters without a screenshot.
//!
//! It subscribes only while a call waits, and lets go once every call is answered, so a program that lists it spends
//! nothing on frame statistics until it is called.

#[cfg(test)]
mod tests;

use super::{
    DescribeGpu, FrameStats, GpuDescribed, GpuTimes, PassTime, SubscribeFrameStats, SubscribeGpuTimes,
    UnsubscribeFrameStats, UnsubscribeGpuTimes,
};
use crate::Renderer;
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, Tells, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};

/// The most bytes reading an input may take: one switch.
const INPUT_BYTES: usize = 8;

/// What the agent asks `render_stats` for: the next frame's statistics, and its passes' times unless `gpu_times` is
/// false.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.read_render_stats")]
pub struct ReadRenderStats {
    /// Whether to wait for the GPU's time over the frame's passes too, which comes a frame or more later: yes when
    /// left out.
    #[flag(short = 'g')]
    pub gpu_times: Option<bool>,
}

/// What `render_stats` answers: the frame's statistics, the adapter the renderer draws on, and the GPU's time over the
/// passes of a frame drawn since the call.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.render_stats")]
pub struct RenderStats {
    pub frame: FrameStats,
    pub adapter: GpuDescribed,
    pub gpu_times: PassTimes,
}

/// The GPU's time over a frame's passes, as `render_stats` answers it.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.pass_times")]
pub enum PassTimes {
    /// Each pass of the frame of the cycle at `elapsed_nanos`, in the order they ran.
    Took { elapsed_nanos: u64, passes: Vec<PassTime> },
    /// The device has no timestamps.
    Unsupported,
    /// The call asked for none.
    Unasked,
}

/// What the tool keeps for its ask of the adapter: nothing, as it asks once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.describing")]
pub struct Describing;

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.stats_tool_config")]
pub struct StatsToolConfig;

/// The tool `render_stats`: asks the [`Renderer`] for its adapter once, as it begins; at a call, subscribes to the
/// renderer's frame statistics and pass times, and answers each call waiting with the first of each heard after it.
/// Under a held clock no frame is drawn, and a call waits until the next is.
#[derive(Bound)]
#[kind(name = "aether.render.stats_tool")]
pub struct StatsTool {
    adapter: Adapter,
    reading: Reading,
}

/// What the tool knows of the adapter.
#[derive(Bound)]
enum Adapter {
    /// Asked, as the tool began, and not yet answered.
    Asked,
    /// Answered.
    Told(GpuDescribed),
    /// The renderer did not answer, for this reason.
    Unanswered(String),
}

/// Whether calls wait on the next frame.
#[derive(Bound)]
enum Reading {
    /// None does, and the tool is subscribed to nothing.
    Idle,
    /// These do, and the tool is subscribed to the renderer's frame statistics and pass times: what it has heard of
    /// each since the first call.
    Waiting { calls: Vec<Waiting>, frame: FrameHeard, times: TimesHeard },
}

/// A call waiting: its answer held, and whether it waits on the pass times.
#[derive(Bound)]
struct Waiting {
    held: Held<Called>,
    times: bool,
}

/// A frame's statistics, heard since the first call waiting, or not yet.
#[derive(Bound)]
enum FrameHeard {
    Awaited,
    Heard(FrameStats),
}

/// A frame's pass times, heard since the first call waiting, or not yet.
#[derive(Bound)]
enum TimesHeard {
    Awaited,
    Heard(GpuTimes),
}

/// What a turn of the tool may send: its subscriptions to the renderer, the end of them, and an answer to its caller.
type Subscribing = (
    ToSender<Called>,
    Tells<Path<Renderer>, SubscribeFrameStats>,
    Tells<Path<Renderer>, SubscribeGpuTimes>,
    Tells<Path<Renderer>, UnsubscribeFrameStats>,
    Tells<Path<Renderer>, UnsubscribeGpuTimes>,
);

impl ChildOf for StatsTool {
    type Parent = Anywhere;
}

impl Actor for StatsTool {
    type Config = StatsToolConfig;
    type Key = ();
    type Handles = (Call, FrameStats, GpuTimes, Answer<GpuDescribed, Describing>);
    type BeginSends = (Asks<Path<Renderer>, DescribeGpu, Describing>,);
    type EndSends = ();

    fn begin(_: StatsToolConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Renderer>(())).ask(&DescribeGpu, &Describing)?;

        Ok(Self { adapter: Adapter::Asked, reading: Reading::Idle })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for StatsTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![ToolSheet::of::<ReadRenderStats>(
            "render_stats",
            "Reads the renderer's statistics of the next frame it draws, as the dev overlay's meters show them: the \
             cycle's elapsed_nanos, which lines it up with Tick; the frame budget, a Limit the frames are paced to \
             (capped:N, or fifo at the display's refresh) or a Reference they are drawn against (uncapped, at the \
             display's refresh); the draws, triangles, and bytes written to the GPU since the frame before; the GPU \
             memory of what the renderer made, by meshes, textures, render targets, buffers, and the glyph atlas; how \
             many meshes, textures, programs, placements, and texts are alive; how full the glyph atlas is; and the \
             adapter it draws on. Unless gpu_times is false, it also waits for the GPU's time over each pass of a \
             frame drawn since the call, a frame or more later, each beside its frame and its pass's name, a pass \
             named by its type's id; Unsupported on a device without timestamps. Nothing is answered while the clock \
             is held and no frame is drawn.",
            Output::Value(SchemaNodes::of(&RenderStats::SHAPE)),
        )]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// Holds the call, and subscribes to the renderer when it is the first to wait.
impl Handles<Call> for StatsTool {
    type Sends = Subscribing;

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Subscribing>) -> Result<(), Fault> {
        let Ok(asked) = from_bytes::<ReadRenderStats>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) else {
            turn.reply(&Called::Refused(String::from("that is no ask of the render stats")))?;
            return Ok(());
        };
        let waiting = Waiting { held: turn.hold()?, times: asked.gpu_times.unwrap_or(true) };
        match &mut self.reading {
            Reading::Waiting { calls, .. } => calls.push(waiting),
            Reading::Idle => {
                turn.to(path::<Renderer>(())).send(&SubscribeFrameStats)?;
                turn.to(path::<Renderer>(())).send(&SubscribeGpuTimes)?;
                self.reading =
                    Reading::Waiting { calls: vec![waiting], frame: FrameHeard::Awaited, times: TimesHeard::Awaited };
            }
        }

        Ok(())
    }
}

/// Keeps the first frame's statistics heard while calls wait, and answers what it can.
impl Handles<FrameStats> for StatsTool {
    type Sends = Subscribing;

    fn handle(&mut self, stats: FrameStats, turn: &mut Turn<'_, Self, Subscribing>) -> Result<(), Fault> {
        if let Reading::Waiting { frame: frame @ FrameHeard::Awaited, .. } = &mut self.reading {
            *frame = FrameHeard::Heard(stats);
        }

        self.answer(turn)
    }
}

/// Keeps the first pass times heard while calls wait, and answers what it can.
impl Handles<GpuTimes> for StatsTool {
    type Sends = Subscribing;

    fn handle(&mut self, heard: GpuTimes, turn: &mut Turn<'_, Self, Subscribing>) -> Result<(), Fault> {
        if let Reading::Waiting { times: times @ TimesHeard::Awaited, .. } = &mut self.reading {
            *times = TimesHeard::Heard(heard);
        }

        self.answer(turn)
    }
}

/// Keeps the adapter, or why it was not told, and answers what it can.
impl Handles<Answer<GpuDescribed, Describing>> for StatsTool {
    type Sends = Subscribing;

    fn handle(
        &mut self,
        (answered, Describing): (Answered<GpuDescribed>, Describing),
        turn: &mut Turn<'_, Self, Subscribing>,
    ) -> Result<(), Fault> {
        self.adapter = match answered {
            Answered::Yes(described) => Adapter::Told(described),
            Answered::No(why) => Adapter::Unanswered(format!("the renderer did not say its adapter: {why:?}")),
        };

        self.answer(turn)
    }
}

impl StatsTool {
    /// Answers each call waiting that what is heard answers, and once none waits, lets go of the renderer.
    fn answer(&mut self, turn: &mut Turn<'_, Self, Subscribing>) -> Result<(), Fault> {
        let Reading::Waiting { calls, frame: FrameHeard::Heard(stats), times } = &mut self.reading else {
            return Ok(());
        };
        let adapter = match &self.adapter {
            Adapter::Asked => return Ok(()),
            Adapter::Told(described) => Ok(described),
            Adapter::Unanswered(why) => Err(why),
        };
        let mut still = Vec::new();
        for waiting in calls.drain(..) {
            let Some(gpu_times) = answered(waiting.times, times) else {
                still.push(waiting);
                continue;
            };
            let called = match adapter {
                Ok(described) => {
                    let read = RenderStats { frame: *stats, adapter: described.clone(), gpu_times };
                    Called::output(turn, to_bytes(&read)?)
                }
                Err(why) => Called::Refused(why.clone()),
            };
            turn.answer(waiting.held, &called)?;
        }
        *calls = still;

        if calls.is_empty() {
            turn.to(path::<Renderer>(())).send(&UnsubscribeFrameStats)?;
            turn.to(path::<Renderer>(())).send(&UnsubscribeGpuTimes)?;
            self.reading = Reading::Idle;
        }

        Ok(())
    }
}

/// The pass times a call that `asked` for them or not is answered with, by what is heard of them: none yet, when it
/// asked and they are not heard, and it waits on.
fn answered(asked: bool, times: &TimesHeard) -> Option<PassTimes> {
    match (asked, times) {
        (false, _) => Some(PassTimes::Unasked),
        (true, TimesHeard::Heard(GpuTimes::Took { elapsed_nanos, passes })) => {
            Some(PassTimes::Took { elapsed_nanos: *elapsed_nanos, passes: passes.clone() })
        }
        (true, TimesHeard::Heard(GpuTimes::Unsupported)) => Some(PassTimes::Unsupported),
        (true, TimesHeard::Awaited) => None,
    }
}
