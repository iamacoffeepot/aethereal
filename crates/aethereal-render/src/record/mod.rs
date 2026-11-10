//! The renderer's tool `record_clip` (ADR-0045): a window's frames, one held cycle each, written as PNGs into the
//! program's saves, so a clip plays in exact real time however slowly its frames were drawn.

#[cfg(test)]
mod tests;

use crate::Renderer;
use crate::capture::encode::{Encode, Encoded};
use crate::capture::{Crop, image, is_shown, unread};
use crate::readback::{Pixels, ReadBack, Readable};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, FilePath, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_files::{FileWritten, Files, Namespace, WriteFile};
use aethereal_pace::{Advance, Advanced, Pace, PaceSet, Pacer, SetPace, Unadvanced};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};
use aethereal_window::WindowId;
use core::mem;

/// The most bytes reading an input may take: a window, counts, a crop, a size, and a name.
const INPUT_BYTES: usize = 2048;

/// The most frames one clip holds: a minute at sixty a second.
pub const MOST_FRAMES: u32 = 3600;

/// The most frames a second a clip is recorded at.
const MOST_FRAMES_PER_SECOND: u32 = 240;

/// The most frames encoded or being written at once: drawing waits for one to finish past it, so a slow disk bounds
/// what the tool holds.
const MOST_IN_FLIGHT: u32 = 4;

/// A second, in nanoseconds.
const SECOND_NANOS: u64 = 1_000_000_000;

/// What the agent asks `record_clip` for: `frames` frames drawn onto `window` at `frames_per_second`, each cut by
/// `crop` and its longer side at most `longest` pixels, written into the program's saves under `clips/<name>/`.
#[derive(Debug, Clone, Plain)]
#[kind(name = "aether.render.record")]
pub struct Record {
    /// The window, by its number.
    #[flag(position)]
    pub window: WindowId,
    /// How many frames to record.
    #[flag(short = 'n')]
    pub frames: u32,
    /// How many frames a second are recorded.
    #[flag(long = "fps", short = 'r')]
    pub frames_per_second: u32,
    /// What of each frame is recorded: `whole`.
    #[flag(short = 'c')]
    pub crop: Crop,
    /// The most pixels of each image's longer side.
    #[flag(short = 'l')]
    pub longest: u32,
    /// The clip's name: it is written under `clips/<name>/` in the program's saves.
    #[flag(position)]
    pub name: FilePath,
}

/// What a clip recorded is: the folder its frames are in, within the program's saves, named `00000.png` on; how many
/// there are and at what rate; and the size of each, as its first was written.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.recorded")]
pub struct Recorded {
    pub folder: FilePath,
    pub frames: u32,
    pub frames_per_second: u32,
    pub width: u32,
    pub height: u32,
}

/// What the tool keeps for its asks of the pacer to hold or set back the clock: which clip it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.record_pacing")]
pub struct Pacing {
    clip: u64,
}

/// What the tool keeps for an advance of one frame: which clip it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.record_stepping")]
pub struct Stepping {
    clip: u64,
}

/// What the tool keeps for a frame read back, encoded, or written: which clip, and which frame of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.record_frame")]
pub struct Framing {
    clip: u64,
    frame: u32,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.record_tool_config")]
pub struct RecordToolConfig;

/// The tool `record_clip`: holds the program's clock through its [`Pacer`], then for each frame asks the [`Renderer`]
/// for the next frame drawn onto the window and advances the clock one cycle of the frame's delta, encodes what is
/// read back as `capture_frame` does, and asks [`Files`] to write it. Once every frame is written, it sets the clock
/// back as it found it and answers what it recorded. It records one clip at a time.
#[derive(Bound)]
#[kind(name = "aether.render.record_tool")]
pub struct RecordTool {
    clip: Clipping,
    /// The number of the next clip.
    next: u64,
}

/// Whether a clip is being recorded.
#[derive(Bound)]
enum Clipping {
    Idle,
    Recording(Clip),
}

/// A clip being recorded: the call it answers, what it asked, its number, and where it stands.
#[derive(Bound)]
struct Clip {
    held: Held<Called>,
    asked: Record,
    number: u64,
    stage: Stage,
}

/// Where a clip stands.
#[derive(Bound)]
enum Stage {
    /// The pacer has been asked to hold the clock, and has not said it has.
    Holding,
    /// The clock is held, and frames are drawn: `was` is how it moved before.
    Drawing { was: Pace, frames: Frames },
    /// The pacer has been asked to set the clock back, and the call is answered with this once it has.
    Restoring(Answering),
}

/// How far a clip's frames have got: how many have been asked of the renderer, read back, and written; how many are
/// encoded or being written; and the size of each, once the first has been read.
#[derive(Bound)]
struct Frames {
    asked: u32,
    read: u32,
    written: u32,
    in_flight: u32,
    size: FrameSize,
}

/// The size a clip's frames are written at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum FrameSize {
    /// No frame has been read yet.
    Unread,
    Read {
        width: u32,
        height: u32,
    },
}

/// What a clip's call is answered with, once the clock is set back.
#[derive(Bound)]
enum Answering {
    Recorded(Recorded),
    Refused(String),
}

/// What every turn of the tool may send: an ask of the pacer, the renderer, or the files, and an answer to its caller.
type Sends = (
    Asks<Path<Pacer>, SetPace, Pacing>,
    Asks<Path<Pacer>, Advance, Stepping>,
    Asks<Path<Renderer>, ReadBack, Framing>,
    Asks<Path<Files>, WriteFile, Framing>,
    ToSender<Called>,
);

impl ChildOf for RecordTool {
    type Parent = Anywhere;
}

impl Actor for RecordTool {
    type Config = RecordToolConfig;
    type Key = ();
    type Handles = (
        Call,
        Answer<PaceSet, Pacing>,
        Answer<Advanced, Stepping>,
        Answer<Pixels, Framing>,
        Answer<Encoded, Framing>,
        Answer<FileWritten, Framing>,
    );
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: RecordToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { clip: Clipping::Idle, next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for RecordTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![ToolSheet::of::<Record>(
            "record_clip",
            "Records `frames` frames drawn onto a window, numbered from 0, at `frames_per_second`, as PNGs written \
             into the program's saves at clips/<name>/00000.png on. It holds the program's clock and advances it \
             one cycle per frame, each by exactly 1/fps, so the clip plays in exact real time however slowly its \
             frames draw, and two runs of a deterministic program match frame for frame. The clock is left as it \
             was found. `crop` and `longest` cut and shrink each frame as capture_frame's do. At most 3600 \
             frames and 240 a second. It answers the folder, within the save directory the engine was given, \
             and each frame's size; make a video of it with, for example, `ffmpeg -framerate <fps> -i \
             %05d.png`. The program must list the pacer and the file capability.",
            Output::Value(SchemaNodes::of(&Recorded::SHAPE)),
        )]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

/// Checks the call, holds it, and asks the pacer to hold the clock: frames are drawn once it has.
impl Handles<Call> for RecordTool {
    type Sends = Sends;

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Ok(asked) = from_bytes::<Record>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) else {
            turn.reply(&refused("that is no clip to record"))?;
            return Ok(());
        };
        if let Some(why) = unrecordable(asked.frames, asked.frames_per_second, asked.longest) {
            turn.reply(&refused(why))?;
            return Ok(());
        }
        if let Clipping::Recording(_) = self.clip {
            turn.reply(&refused("a clip is being recorded already: record one at a time"))?;
            return Ok(());
        }
        let held = turn.hold()?;
        let number = self.next;
        self.next += 1;
        turn.to(path::<Pacer>(())).ask(&SetPace { pace: Pace::Held }, &Pacing { clip: number })?;
        self.clip = Clipping::Recording(Clip { held, asked, number, stage: Stage::Holding });

        Ok(())
    }
}

/// The pacer held the clock, so the first frame is drawn; or it set the clock back, so the call is answered.
impl Handles<Answer<PaceSet, Pacing>> for RecordTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Pacing { clip }): (Answered<PaceSet>, Pacing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(recording) = self.recording(clip) else {
            return Ok(());
        };
        match (&recording.stage, answered) {
            (Stage::Holding, Answered::Yes(PaceSet { was })) => {
                let frames = Frames { asked: 0, read: 0, written: 0, in_flight: 0, size: FrameSize::Unread };
                recording.stage = Stage::Drawing { was, frames };
                recording.draw_next(turn)?;
            }
            (Stage::Holding, Answered::No(_)) => {
                let Clipping::Recording(done) = mem::replace(&mut self.clip, Clipping::Idle) else {
                    return Ok(());
                };
                turn.answer(done.held, &refused("the program lists no pacer, so its clock cannot be held"))?;
            }
            (Stage::Restoring(_), _) => {
                let Clipping::Recording(done) = mem::replace(&mut self.clip, Clipping::Idle) else {
                    return Ok(());
                };
                let Stage::Restoring(ending) = done.stage else {
                    return Ok(());
                };
                let called = match ending {
                    Answering::Recorded(recorded) => Called::output(turn, to_bytes(&recorded)?),
                    Answering::Refused(why) => Called::Refused(why),
                };
                turn.answer(done.held, &called)?;
            }
            (Stage::Drawing { .. }, _) => {}
        }

        Ok(())
    }
}

/// A frame's cycle has run: only a refusal matters, as the frame read back is what moves the clip on.
impl Handles<Answer<Advanced, Stepping>> for RecordTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Stepping { clip }): (Answered<Advanced>, Stepping),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(recording) = self.recording(clip) else {
            return Ok(());
        };
        let why = match answered {
            Answered::Yes(Advanced::Advanced { .. }) => return Ok(()),
            Answered::Yes(Advanced::Refused(why)) => unadvanced(why),
            Answered::No(_) => String::from("the pacer did not advance the clock"),
        };
        recording.fail(why, turn)
    }
}

/// A frame was read back: it is checked and encoded, and the next frame drawn when there is room for it.
impl Handles<Answer<Pixels, Framing>> for RecordTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Framing { clip, frame }): (Answered<Pixels>, Framing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(recording) = self.recording(clip) else {
            return Ok(());
        };
        let (width, height, format, pixels) = match answered {
            Answered::Yes(Pixels::Read { width, height, format, pixels }) => (width, height, format, pixels),
            Answered::Yes(Pixels::Refused(why)) => return recording.fail(at(frame, unread(why)), turn),
            Answered::No(why) => {
                return recording.fail(at(frame, &format!("the renderer did not answer: {why:?}")), turn);
            }
        };
        if !is_shown(format) {
            return recording.fail(at(frame, "the frame is of a format no image is written from"), turn);
        }
        let region = recording.asked.crop.region(width, height);
        if !region.within(width, height) {
            let outside = format!("the crop is not within the frame, which is {width}x{height}");
            return recording.fail(at(frame, &outside), turn);
        }
        let (sent_width, sent_height) = image::sent_size(region.width, region.height, recording.asked.longest);
        let Stage::Drawing { frames, .. } = &mut recording.stage else {
            return Ok(());
        };
        frames.read += 1;
        frames.in_flight += 1;
        if frames.size == FrameSize::Unread {
            frames.size = FrameSize::Read { width: sent_width, height: sent_height };
        }
        let encode = Encode { pixels, frame_width: width, region, width: sent_width, height: sent_height };
        turn.run(&encode, &Framing { clip, frame })?;

        recording.draw_next(turn)
    }
}

/// A frame was encoded: it is written into the clip's folder.
impl Handles<Answer<Encoded, Framing>> for RecordTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Framing { clip, frame }): (Answered<Encoded>, Framing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(recording) = self.recording(clip) else {
            return Ok(());
        };
        let Answered::Yes(Encoded { png }) = answered else {
            return recording.fail(at(frame, "the frame could not be written as a PNG"), turn);
        };
        let Ok(file) = frame_path(&recording.asked.name, frame) else {
            return recording.fail(at(frame, "the clip's name makes no file's path"), turn);
        };
        let write = WriteFile { namespace: Namespace::Save, path: file, bytes: png };
        turn.to(path::<Files>(())).ask(&write, &Framing { clip, frame })?;

        Ok(())
    }
}

/// A frame was written: the next frame is drawn when there is room for it, and the clock set back once the last is.
impl Handles<Answer<FileWritten, Framing>> for RecordTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Framing { clip, frame }): (Answered<FileWritten>, Framing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(recording) = self.recording(clip) else {
            return Ok(());
        };
        let why = match answered {
            Answered::Yes(FileWritten::Written) => None,
            Answered::Yes(FileWritten::ReadOnly) => Some("the program's saves are not written"),
            Answered::Yes(FileWritten::Unavailable) => Some("the program has nowhere to save"),
            Answered::Yes(FileWritten::Failed) => Some("the file could not be written"),
            Answered::No(_) => Some("the program lists no file capability, or it did not answer"),
        };
        if let Some(why) = why {
            return recording.fail(at(frame, why), turn);
        }
        let Stage::Drawing { frames, .. } = &mut recording.stage else {
            return Ok(());
        };
        frames.in_flight -= 1;
        frames.written += 1;
        let is_last = frames.written == recording.asked.frames;
        if is_last {
            return recording.finish(turn);
        }

        recording.draw_next(turn)
    }
}

impl RecordTool {
    /// The clip being recorded, when it is the clip numbered `clip`: an answer for any other is for one already
    /// answered, and nothing more is done with it.
    fn recording(&mut self, clip: u64) -> Option<&mut Clip> {
        match &mut self.clip {
            Clipping::Recording(recording) if recording.number == clip => Some(recording),
            Clipping::Recording(_) | Clipping::Idle => None,
        }
    }
}

impl Clip {
    /// Asks for the next frame, when no frame is being drawn, one is left to draw, and fewer than the most are
    /// encoded or being written: the renderer for the next frame drawn onto the window, then the pacer for one cycle
    /// of its delta, which draws it.
    fn draw_next(&mut self, turn: &mut Turn<'_, RecordTool, Sends>) -> Result<(), Fault> {
        let Stage::Drawing { frames, .. } = &mut self.stage else {
            return Ok(());
        };
        let is_drawing = frames.read < frames.asked;
        let is_left = frames.asked < self.asked.frames;
        let has_room = frames.in_flight < MOST_IN_FLIGHT;
        let draws = !is_drawing && is_left && has_room;
        if !draws {
            return Ok(());
        }
        let frame = frames.asked;
        frames.asked += 1;
        let read = ReadBack { of: Readable::Frame(self.asked.window) };
        turn.to(path::<Renderer>(())).ask(&read, &Framing { clip: self.number, frame })?;
        let advance = Advance { cycles: 1, delta_nanos: delta_nanos(frame, self.asked.frames_per_second) };
        turn.to(path::<Pacer>(())).ask(&advance, &Stepping { clip: self.number })?;

        Ok(())
    }

    /// Every frame is written: the clock is set back, and the call answered with the clip once it is.
    fn finish(&mut self, turn: &mut Turn<'_, RecordTool, Sends>) -> Result<(), Fault> {
        let Stage::Drawing { frames, .. } = &self.stage else {
            return Ok(());
        };
        let FrameSize::Read { width, height } = frames.size else {
            return Ok(());
        };
        let Ok(folder) = FilePath::parse(format!("clips/{}", self.asked.name.as_str())) else {
            return self.fail(String::from("the clip's name makes no folder's path"), turn);
        };
        let recorded = Recorded {
            folder,
            frames: self.asked.frames,
            frames_per_second: self.asked.frames_per_second,
            width,
            height,
        };

        self.restore(Answering::Recorded(recorded), turn)
    }

    /// The clip cannot go on, for `why`: the clock is set back, and the call refused once it is.
    fn fail(&mut self, why: String, turn: &mut Turn<'_, RecordTool, Sends>) -> Result<(), Fault> {
        self.restore(Answering::Refused(why), turn)
    }

    /// Asks the pacer to set the clock back as the clip found it, the call to be answered with `ending` once it has.
    /// A clip already setting it back keeps the ending it had.
    fn restore(&mut self, ending: Answering, turn: &mut Turn<'_, RecordTool, Sends>) -> Result<(), Fault> {
        let Stage::Drawing { was, .. } = self.stage else {
            return Ok(());
        };
        self.stage = Stage::Restoring(ending);
        turn.to(path::<Pacer>(())).ask(&SetPace { pace: was }, &Pacing { clip: self.number })?;

        Ok(())
    }
}

/// Why a clip of `frames` frames at `frames_per_second`, each at most `longest` across, cannot be recorded, when it
/// cannot.
const fn unrecordable(frames: u32, frames_per_second: u32, longest: u32) -> Option<&'static str> {
    if frames == 0 || frames > MOST_FRAMES {
        return Some("`frames` is 1 to 3600");
    }
    if frames_per_second == 0 || frames_per_second > MOST_FRAMES_PER_SECOND {
        return Some("`frames_per_second` is 1 to 240");
    }
    if longest == 0 {
        return Some("`longest` is at least 1");
    }

    None
}

/// The delta of frame `frame` of a clip at `frames_per_second`, in nanoseconds: the deltas of its first `n` frames
/// sum to `⌊n·10⁹ / fps⌋`, so the clip never drifts by a rounding a frame.
fn delta_nanos(frame: u32, frames_per_second: u32) -> u64 {
    let rate = u64::from(frames_per_second);
    let ends = (u64::from(frame) + 1) * SECOND_NANOS / rate;
    let begins = u64::from(frame) * SECOND_NANOS / rate;

    ends - begins
}

/// Where frame `frame` of the clip `name` is written within the program's saves: `clips/<name>/00000.png` on.
fn frame_path(name: &FilePath, frame: u32) -> Result<FilePath, aethereal_data::DecodeError> {
    FilePath::parse(format!("clips/{}/{frame:05}.png", name.as_str()))
}

/// What the agent reads of why frame `frame` went no further.
fn at(frame: u32, why: &str) -> String {
    format!("frame {frame}: {why}")
}

/// What the agent reads of why the pacer did not advance the clock.
fn unadvanced(why: Unadvanced) -> String {
    let said = match why {
        Unadvanced::Running => "the clock was set running while the clip was recorded",
        Unadvanced::NoCycles | Unadvanced::NoTime => "the pacer was asked for no time",
        Unadvanced::PastLimit { .. } => {
            "a frame's delta is past the engine's limit for one tick: record at more frames a second"
        }
        Unadvanced::Stopping => "the program is stopping",
    };

    String::from(said)
}

/// A refusal saying `why`.
fn refused(why: &str) -> Called {
    Called::Refused(String::from(why))
}
