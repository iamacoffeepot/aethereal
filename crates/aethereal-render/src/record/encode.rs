//! The jobs that encode a clip: each frame into the clip's encoder, given to the job and handed back, and the encoder's
//! MP4 finished as one blob once the last frame is in (ADR-0018, ADR-0045).

use super::video::{ClipEncoder, Unencodable};
use crate::capture::image::{self, Region};
use aethereal_actor::{Bytes, Fault, Given, Job, Runner, Running};
use aethereal_data::Bound;

/// Cuts `region` out of `pixels`, a frame's RGBA rows `frame_width` wide, shrinks it to `width` by `height`, and
/// encodes it with `encoder` as the clip's frame `frame`.
#[derive(Bound)]
#[kind(name = "aether.render.encode_frame")]
pub struct EncodeFrame {
    pub encoder: Given<ClipEncoder>,
    pub frame: u32,
    pub pixels: Bytes,
    pub frame_width: u32,
    pub region: Region,
    pub width: u32,
    pub height: u32,
}

/// What the job answers with: the encoder handed back, the frame in it; or why the frame was not encoded, the encoder
/// dropped with the job, as the clip goes no further.
#[derive(Bound)]
#[kind(name = "aether.render.frame_encoded")]
pub enum FrameEncoded {
    Encoded { encoder: Given<ClipEncoder> },
    Refused(Unencodable),
}

/// It computes, on threads for computing, so no step waits on a frame's encoding: one at a time, as the clip has one
/// encoder.
impl Job for EncodeFrame {
    type Done = FrameEncoded;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<FrameEncoded, Fault> {
        let mut encoder = job.take(self.encoder)?;
        let rgb = image::cut(job.read(&self.pixels), self.frame_width, self.region);
        let rgb = image::shrunk(&rgb, self.region.width, self.region.height, self.width, self.height);
        let done = match encoder.encode(self.frame, &rgb) {
            Ok(()) => FrameEncoded::Encoded { encoder: job.hand(encoder) },
            Err(why) => FrameEncoded::Refused(why),
        };

        Ok(done)
    }
}

/// Finishes the MP4 `encoder` holds, every frame in it, and puts it as one blob.
#[derive(Bound)]
#[kind(name = "aether.render.finish_clip")]
pub struct FinishClip {
    pub encoder: Given<ClipEncoder>,
}

/// What the job answers with: the clip's MP4, whole; or why it could not be finished. A file too large for the
/// engine's budget for blobs answers the job `Faulted`.
#[derive(Bound)]
#[kind(name = "aether.render.clip_finished")]
pub enum ClipFinished {
    Finished { mp4: Bytes },
    Refused(Unencodable),
}

impl Job for FinishClip {
    type Done = ClipFinished;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<ClipFinished, Fault> {
        let finished = match job.take(self.encoder)?.finish() {
            Ok(mp4) => ClipFinished::Finished { mp4: job.put(mp4) },
            Err(why) => ClipFinished::Refused(why),
        };

        Ok(finished)
    }
}
