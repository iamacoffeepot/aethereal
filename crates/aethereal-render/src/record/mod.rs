//! The renderer's tool `record_clip` (ADR-0045): a window's frames, one held cycle each, encoded as H.264 into one MP4
//! in the program's saves, so a clip plays in exact real time however slowly its frames were drawn.

mod encode;
#[cfg(test)]
mod tests;
mod video;

use crate::Renderer;
use crate::capture::image::{self, Region};
use crate::capture::{Crop, crop_or_whole, is_shown, longest_or_frame, unread};
use crate::readback::{Pixels, ReadBack, Readable};
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, Bytes, ChildOf, Ending, Fault, Handle, Handles, Held,
    Path, ProtocolRef, ToSender, Turn, Unanswered, path,
};
use aethereal_data::{Bound, Budget, DecodeError, FilePath, Plain, Schema, SchemaNodes, from_bytes, to_bytes};
use aethereal_files::{FileWritten, Files, Namespace, WriteFile};
use aethereal_pace::{Advance, Advanced, Pace, PaceSet, Pacer, SetPace, Unadvanced};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};
use aethereal_window::WindowId;
use core::mem;
use encode::{ClipFinished, EncodeFrame, FinishClip, FrameEncoded};
use video::{ClipEncoder, Unencodable, said};

/// The most bytes reading an input may take: a window, counts, a crop, a size, and a name.
const INPUT_BYTES: usize = 2048;

/// The most frames one clip holds: a minute at sixty a second.
pub const MOST_FRAMES: u32 = 3600;

/// The most frames a second a clip is recorded at.
const MOST_FRAMES_PER_SECOND: u32 = 240;

/// The most frames read back and not yet encoded: drawing waits for one to be encoded past it, so a slow encoder
/// bounds the frames the tool holds.
const MOST_IN_FLIGHT: u32 = 4;

/// The frames a second a clip that names none is recorded at.
const FRAMES_PER_SECOND: u32 = 60;

/// A second, in nanoseconds.
const SECOND_NANOS: u64 = 1_000_000_000;

/// What the agent asks `record_clip` for: `frames` frames drawn onto `window` at `frames_per_second`, each cut by
/// `crop` and its longer side at most `longest` pixels, encoded into the program's saves as `clips/<name>.mp4`. The
/// tool reads a field left out as its fallback, which the field's doc names.
#[derive(Debug, Clone, Plain)]
#[kind(name = "aether.render.record")]
pub struct Record {
    /// The window, by its number.
    #[flag(position)]
    pub window: WindowId,
    /// How many frames to record.
    #[flag(short = 'n')]
    pub frames: u32,
    /// How many frames a second are recorded: none for 60.
    #[flag(long = "fps", short = 'r')]
    pub frames_per_second: Option<u32>,
    /// What of each frame is recorded: none for `whole`.
    #[flag(short = 'c')]
    pub crop: Option<Crop>,
    /// The most pixels of each frame's longer side: none for the frame's own size. Each side is then rounded down to
    /// even.
    #[flag(short = 'l')]
    pub longest: Option<u32>,
    /// The clip's name: it is written as `clips/<name>.mp4` in the program's saves.
    #[flag(position)]
    pub name: FilePath,
}

impl Record {
    /// How many frames a second the clip is recorded at, 60 when it names none.
    const fn rate(&self) -> u32 {
        match self.frames_per_second {
            Some(rate) => rate,
            None => FRAMES_PER_SECOND,
        }
    }
}

/// What a clip recorded is: its MP4 file, within the program's saves; how many frames it holds and at what rate; and
/// the size of each, both sides even.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.recorded")]
pub struct Recorded {
    pub file: FilePath,
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

/// What the tool keeps for a frame read back or encoded: which clip, and which frame of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.record_frame")]
pub struct Framing {
    clip: u64,
    frame: u32,
}

/// What the tool keeps for a clip's MP4 finished or written: which clip it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.record_filing")]
pub struct Filing {
    clip: u64,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.record_tool_config")]
pub struct RecordToolConfig;

/// The tool `record_clip`: holds the program's clock through its [`Pacer`], then for each frame asks the [`Renderer`]
/// for the next frame drawn onto the window and advances the clock one cycle of the frame's delta. Each frame read
/// back is encoded by a compute job, given the clip's one [`ClipEncoder`] and handing it back, one frame at a time and
/// in order, while the next frame draws. Once every frame is encoded, a last job finishes the MP4, and [`Files`]
/// writes it. Then it sets the clock back as it found it and answers what it recorded. It records one clip at a time.
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
    Recording(Box<Clip>),
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
    /// The clock is held, and the clip is under way: `was` is how it moved before.
    Held { was: Pace, step: Step },
    /// The pacer has been asked to set the clock back, and the call is answered with this once it has.
    Restoring(Answering),
}

/// How far a clip under a held clock has got.
#[derive(Bound)]
enum Step {
    /// Its frames are drawn and encoded.
    Drawing(Frames),
    /// Every frame is encoded, and the job finishing its MP4 holds the encoder: the clip's frames are of this size.
    Finishing(ClipSize),
    /// Its MP4 is being written, and the call is answered with this once it is.
    Writing(Recorded),
}

/// How far a clip's frames have got: how many have been asked of the renderer, read back, and encoded; the frames
/// read back that wait for the encoder, in order; and the encoder.
#[derive(Bound)]
struct Frames {
    asked: u32,
    read: u32,
    encoded: u32,
    waiting: Vec<Waiting>,
    encoder: Encoder,
}

/// A frame read back that waits for the encoder: its number, its pixels, RGBA rows `frame_width` wide, and the region
/// of them it is cut to.
#[derive(Bound)]
struct Waiting {
    frame: u32,
    pixels: Bytes,
    frame_width: u32,
    region: Region,
}

/// A clip's encoder.
#[derive(Bound)]
enum Encoder {
    /// No frame has been read yet: the encoder is made at the first frame's size.
    Unmade,
    /// Made for frames of `size`, which every frame of the clip must be.
    Made { size: ClipSize, at: EncoderAt },
}

/// Where a clip's encoder is.
#[derive(Bound)]
enum EncoderAt {
    /// The tool holds it, and no frame is being encoded.
    Here(Handle<ClipEncoder>),
    /// The job encoding a frame holds it.
    Away,
}

/// The size a clip's frames are encoded at, each side even.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
struct ClipSize {
    width: u32,
    height: u32,
}

/// What of a frame a clip's image is: the region cut from it, and the size that is encoded at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cut {
    region: Region,
    size: ClipSize,
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
    Asks<Path<Files>, WriteFile, Filing>,
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
        Answer<FrameEncoded, Framing>,
        Answer<ClipFinished, Filing>,
        Answer<FileWritten, Filing>,
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
            "Records frames drawn onto a window as an MP4. It records `frames` frames of the window at \
             `frames_per_second` (60 when left out), encoded as H.264 and written into the program's saves as \
             clips/<name>.mp4. It holds the program's clock and advances it one cycle per frame, each by exactly \
             1/fps, so the clip plays in exact real time however slowly its frames draw, and two runs of a \
             deterministic program match frame for frame. The clock is left as it was found. `crop` and `longest` \
             cut and shrink each frame as capture_frame's do; each side is then rounded down to even, as H.264 \
             stores colour two by two pixels, so a frame not shrunk loses its last column or row. At most 3600 \
             frames and 240 a second, and the file must fit the engine's budget for blobs. It answers the file, \
             within the save directory the engine was given, and its frames, rate and size. The program must list \
             the pacer and the file capability; a browser's engine has no encoder.",
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
        if let Some(why) = unrecordable(asked.frames, asked.rate(), longest_or_frame(asked.longest)) {
            turn.reply(&refused(why))?;
            return Ok(());
        }
        if !video::ENCODES {
            turn.reply(&Called::Refused(said(Unencodable::NoEncoder)))?;
            return Ok(());
        }
        let is_named = clip_path(&asked.name).is_ok();
        if !is_named {
            turn.reply(&refused("the clip's name makes no file's path"))?;
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
        self.clip = Clipping::Recording(Box::new(Clip { held, asked, number, stage: Stage::Holding }));

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
                let frames = Frames { asked: 0, read: 0, encoded: 0, waiting: Vec::new(), encoder: Encoder::Unmade };
                recording.stage = Stage::Held { was, step: Step::Drawing(frames) };
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
                let Clip { held, stage, .. } = *done;
                let Stage::Restoring(ending) = stage else {
                    return Ok(());
                };
                let called = match ending {
                    Answering::Recorded(recorded) => Called::output(turn, to_bytes(&recorded)?),
                    Answering::Refused(why) => Called::Refused(why),
                };
                turn.answer(held, &called)?;
            }
            (Stage::Held { .. }, _) => {}
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

/// A frame was read back: it is checked and waits for the encoder, made at the first frame's size; it is encoded once
/// the encoder is free, and the next frame drawn when there is room for it.
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
        let region = crop_or_whole(recording.asked.crop).region(width, height);
        if !region.within(width, height) {
            let outside = format!("the crop is not within the frame, which is {width}x{height}");
            return recording.fail(at(frame, &outside), turn);
        }
        let cut = match clip_cut(region, longest_or_frame(recording.asked.longest)) {
            Ok(cut) => cut,
            Err(why) => return recording.fail(at(frame, why), turn),
        };
        let rate = recording.asked.rate();
        let Stage::Held { step: Step::Drawing(frames), .. } = &mut recording.stage else {
            return Ok(());
        };

        match &frames.encoder {
            Encoder::Unmade => {
                let encoder = match ClipEncoder::new(rate, cut.size.width, cut.size.height) {
                    Ok(encoder) => encoder,
                    Err(why) => return recording.fail(at(frame, &said(why)), turn),
                };
                frames.encoder = Encoder::Made { size: cut.size, at: EncoderAt::Here(turn.keep(encoder)?) };
            }
            Encoder::Made { size, .. } if *size == cut.size => {}
            Encoder::Made { size, .. } => {
                let ClipSize { width, height } = *size;
                let ClipSize { width: now_width, height: now_height } = cut.size;
                let resized = format!(
                    "the window changed size during the clip: its frames are {width}x{height}, and this one \
                     {now_width}x{now_height}"
                );
                return recording.fail(at(frame, &resized), turn);
            }
        }

        frames.read += 1;
        frames.waiting.push(Waiting { frame, pixels, frame_width: width, region: cut.region });
        frames.encode_next(turn, clip)?;

        recording.draw_next(turn)
    }
}

/// A frame was encoded, and the encoder handed back: the next frame waiting is encoded, the next drawn when there is
/// room for it, and the clip's MP4 finished once the last is in.
impl Handles<Answer<FrameEncoded, Framing>> for RecordTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Framing { clip, frame }): (Answered<FrameEncoded>, Framing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(recording) = self.recording(clip) else {
            return Ok(());
        };
        let encoder = match answered {
            Answered::Yes(FrameEncoded::Encoded { encoder }) => encoder,
            Answered::Yes(FrameEncoded::Refused(why)) => return recording.fail(at(frame, &said(why)), turn),
            Answered::No(why) => {
                return recording.fail(at(frame, &format!("the frame was not encoded: {why:?}")), turn);
            }
        };
        // A clip that has failed takes no encoder back: it is dropped with the answer, and the tool's log says so.
        let Stage::Held { step: Step::Drawing(frames), .. } = &mut recording.stage else {
            return Ok(());
        };
        let Encoder::Made { at, .. } = &mut frames.encoder else {
            return Ok(());
        };
        *at = EncoderAt::Here(turn.take(encoder)?);
        frames.encoded += 1;
        let is_last = frames.encoded == recording.asked.frames;
        if is_last {
            return recording.finish(turn);
        }
        frames.encode_next(turn, clip)?;

        recording.draw_next(turn)
    }
}

/// The clip's MP4 is finished: it is written into the program's saves.
impl Handles<Answer<ClipFinished, Filing>> for RecordTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Filing { clip }): (Answered<ClipFinished>, Filing),
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(recording) = self.recording(clip) else {
            return Ok(());
        };
        let mp4 = match answered {
            Answered::Yes(ClipFinished::Finished { mp4 }) => mp4,
            Answered::Yes(ClipFinished::Refused(why)) => return recording.fail(said(why), turn),
            Answered::No(Unanswered::Faulted) => return recording.fail(String::from(UNPUT), turn),
            Answered::No(why) => return recording.fail(format!("the clip's MP4 was not finished: {why:?}"), turn),
        };
        let Ok(file) = clip_path(&recording.asked.name) else {
            return recording.fail(String::from("the clip's name makes no file's path"), turn);
        };
        let Stage::Held { step, .. } = &mut recording.stage else {
            return Ok(());
        };
        let Step::Finishing(ClipSize { width, height }) = *step else {
            return Ok(());
        };
        let (frames, frames_per_second) = (recording.asked.frames, recording.asked.rate());
        *step = Step::Writing(Recorded { file: file.clone(), frames, frames_per_second, width, height });
        let write = WriteFile { namespace: Namespace::Save, path: file, bytes: mp4 };
        turn.to(path::<Files>(())).ask(&write, &Filing { clip })?;

        Ok(())
    }
}

/// The clip's MP4 is written: the clock is set back, and the call answered with the clip once it is.
impl Handles<Answer<FileWritten, Filing>> for RecordTool {
    type Sends = Sends;

    fn handle(
        &mut self,
        (answered, Filing { clip }): (Answered<FileWritten>, Filing),
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
            return recording.fail(String::from(why), turn);
        }
        let Stage::Held { step: Step::Writing(recorded), .. } = &recording.stage else {
            return Ok(());
        };
        let recorded = recorded.clone();

        recording.restore(Answering::Recorded(recorded), turn)
    }
}

/// What the agent reads when the clip's MP4 could not be put as a blob.
const UNPUT: &str = "the clip's MP4 could not be put as one blob: it is likely past the engine's budget for blobs \
                     (`blob_bytes`); record fewer frames, a smaller `longest`, or give the engine a larger budget";

impl RecordTool {
    /// The clip being recorded, when it is the clip numbered `clip`: an answer for any other is for one already
    /// answered, and nothing more is done with it.
    fn recording(&mut self, clip: u64) -> Option<&mut Clip> {
        match &mut self.clip {
            Clipping::Recording(recording) if recording.number == clip => Some(recording.as_mut()),
            Clipping::Recording(_) | Clipping::Idle => None,
        }
    }
}

impl Frames {
    /// Gives the encoder to a job encoding the first frame waiting, when the tool holds it and one waits: frames are
    /// encoded one at a time, in the order they were read.
    fn encode_next(&mut self, turn: &mut Turn<'_, RecordTool, Sends>, clip: u64) -> Result<(), Fault> {
        let Encoder::Made { size, at } = &mut self.encoder else {
            return Ok(());
        };
        let is_here = matches!(at, EncoderAt::Here(_));
        let is_waiting = !self.waiting.is_empty();
        let starts = is_here && is_waiting;
        if !starts {
            return Ok(());
        }
        let EncoderAt::Here(encoder) = mem::replace(at, EncoderAt::Away) else {
            return Ok(());
        };
        let Waiting { frame, pixels, frame_width, region } = self.waiting.remove(0);
        let encoder = turn.give(encoder)?;
        let ClipSize { width, height } = *size;
        let job = EncodeFrame { encoder, frame, pixels, frame_width, region, width, height };
        turn.run(&job, &Framing { clip, frame })?;

        Ok(())
    }
}

impl Clip {
    /// Asks for the next frame, when no frame is being drawn, one is left to draw, and fewer than the most are read
    /// back and not yet encoded: the renderer for the next frame drawn onto the window, then the pacer for one cycle
    /// of its delta, which draws it.
    fn draw_next(&mut self, turn: &mut Turn<'_, RecordTool, Sends>) -> Result<(), Fault> {
        let Stage::Held { step: Step::Drawing(frames), .. } = &mut self.stage else {
            return Ok(());
        };
        let is_drawing = frames.read < frames.asked;
        let is_left = frames.asked < self.asked.frames;
        let has_room = frames.read - frames.encoded < MOST_IN_FLIGHT;
        let draws = !is_drawing && is_left && has_room;
        if !draws {
            return Ok(());
        }
        let frame = frames.asked;
        frames.asked += 1;
        let read = ReadBack { of: Readable::Frame(self.asked.window) };
        turn.to(path::<Renderer>(())).ask(&read, &Framing { clip: self.number, frame })?;
        let advance = Advance { cycles: 1, delta_nanos: Some(delta_nanos(frame, self.asked.rate())) };
        turn.to(path::<Pacer>(())).ask(&advance, &Stepping { clip: self.number })?;

        Ok(())
    }

    /// Every frame is encoded, and the tool holds the encoder: it is given to a job that finishes the clip's MP4.
    fn finish(&mut self, turn: &mut Turn<'_, RecordTool, Sends>) -> Result<(), Fault> {
        let Stage::Held { step, .. } = &mut self.stage else {
            return Ok(());
        };
        let Step::Drawing(Frames { encoder: Encoder::Made { size, at }, .. }) = step else {
            return Ok(());
        };
        let size = *size;
        let EncoderAt::Here(encoder) = mem::replace(at, EncoderAt::Away) else {
            return Ok(());
        };
        *step = Step::Finishing(size);
        let finish = FinishClip { encoder: turn.give(encoder)? };
        turn.run(&finish, &Filing { clip: self.number })?;

        Ok(())
    }

    /// The clip cannot go on, for `why`: the clock is set back, and the call refused once it is.
    fn fail(&mut self, why: String, turn: &mut Turn<'_, RecordTool, Sends>) -> Result<(), Fault> {
        self.restore(Answering::Refused(why), turn)
    }

    /// Asks the pacer to set the clock back as the clip found it, the call to be answered with `ending` once it has,
    /// and drops the encoder when the tool holds it. A clip already setting it back keeps the ending it had.
    fn restore(&mut self, ending: Answering, turn: &mut Turn<'_, RecordTool, Sends>) -> Result<(), Fault> {
        let Stage::Held { was, .. } = self.stage else {
            return Ok(());
        };
        let left = mem::replace(&mut self.stage, Stage::Restoring(ending));
        release(left, turn)?;
        turn.to(path::<Pacer>(())).ask(&SetPace { pace: was }, &Pacing { clip: self.number })?;

        Ok(())
    }
}

/// Drops the encoder `left`, a clip's stage it has left, holds, when the tool holds it: given and carried by no mail,
/// it is dropped as the turn ends, and the tool's log records it (ADR-0010). One a job holds is dropped with the job's
/// answer, which the clip no longer takes.
fn release(left: Stage, turn: &mut Turn<'_, RecordTool, Sends>) -> Result<(), Fault> {
    let Stage::Held { step: Step::Drawing(frames), .. } = left else {
        return Ok(());
    };
    let Encoder::Made { at: EncoderAt::Here(encoder), .. } = frames.encoder else {
        return Ok(());
    };
    let _dropped = turn.give(encoder)?;

    Ok(())
}

/// What of a frame's `region` a clip's image is, its longer side at most `longest`: shrunk as `capture_frame`'s, then
/// each side rounded down to even, as H.264 at 4:2:0 stores one colour for each two by two pixels. A region not shrunk
/// loses its last column or row rather than being resampled for one pixel, so the frame stays sharp; a shrunk one is
/// shrunk to the even size.
fn clip_cut(region: Region, longest: u32) -> Result<Cut, &'static str> {
    let (sent_width, sent_height) = image::sent_size(region.width, region.height, longest);
    let size = ClipSize { width: sent_width & !1, height: sent_height & !1 };
    let is_empty = size.width == 0 || size.height == 0;
    if is_empty {
        return Err("a clip's frame is at least 2x2 pixels: crop it larger, or shrink it less");
    }
    let is_shrunk = sent_width < region.width;
    let region = if is_shrunk {
        region
    } else {
        Region { width: size.width, height: size.height, ..region }
    };

    Ok(Cut { region, size })
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
    if longest < 2 {
        return Some("`longest` is at least 2");
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

/// Where the clip `name` is written within the program's saves: `clips/<name>.mp4`.
fn clip_path(name: &FilePath) -> Result<FilePath, DecodeError> {
    FilePath::parse(format!("clips/{}.mp4", name.as_str()))
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
