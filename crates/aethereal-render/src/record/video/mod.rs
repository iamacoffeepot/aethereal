//! What `record_clip` encodes a clip with (ADR-0045): an H.264 encoder and the MP4 it is written into, held in memory
//! as one resource, which goes to the job encoding each frame and comes back with its answer (ADR-0018).

#[cfg_attr(target_arch = "wasm32", path = "none.rs")]
mod h264;
pub mod mp4;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

use aethereal_actor::Resource;
use aethereal_data::Plain;
use h264::H264;
use mp4::Mp4;

pub use h264::ENCODES;

/// The most bytes a clip's MP4 takes as it is encoded, held in memory until it is written. At the bit rate a clip is
/// encoded at, a minute at 1080p takes about 75 MB; the file is also put as one blob, so the engine's budget for blobs
/// must hold it too.
pub const MOST_CLIP_BYTES: usize = 1 << 29;

/// A clip's encoder: H.264 at the clip's rate and size, writing each frame into the clip's MP4, in order, from frame 0.
pub struct ClipEncoder {
    h264: H264,
    mp4: Mp4,
    /// The frame it encodes next.
    next: u32,
}

/// It goes from the tool to the job encoding a frame, and back, so one frame is encoded at a time, in order.
impl Resource for ClipEncoder {}

/// Why a frame was not encoded, or a clip not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.unencodable")]
pub enum Unencodable {
    /// The engine has no video encoder, as a browser's has none.
    NoEncoder,
    /// A frame came out of order: `frame` when the encoder was at `next`.
    OutOfOrder { frame: u32, next: u32 },
    /// The clip's MP4 would pass [`MOST_CLIP_BYTES`].
    PastMostBytes,
    /// The encoder failed: `OpenH264` says little more.
    Failed,
    /// A frame was encoded as no picture.
    NoPicture,
    /// No frame brought the parameter sets a player needs to decode any.
    NoParameterSets,
}

impl ClipEncoder {
    /// An encoder of a clip at `frames_per_second`, each frame `width` by `height`, both even.
    ///
    /// # Errors
    ///
    /// [`Unencodable::NoEncoder`] in a browser's engine, and [`Unencodable::Failed`] when `OpenH264` refuses.
    pub fn new(frames_per_second: u32, width: u32, height: u32) -> Result<Self, Unencodable> {
        let h264 = H264::new(frames_per_second, width, height)?;
        let mp4 = Mp4::new(frames_per_second, width, height);

        Ok(Self { h264, mp4, next: 0 })
    }

    /// Encodes frame `frame`, `rgb` of the clip's size row by row, as the clip's next.
    ///
    /// # Errors
    ///
    /// [`Unencodable::OutOfOrder`] when it is not the next frame, [`Unencodable::PastMostBytes`] when the clip would
    /// pass [`MOST_CLIP_BYTES`], and why the encoder failed.
    pub fn encode(&mut self, frame: u32, rgb: &[u8]) -> Result<(), Unencodable> {
        if frame != self.next {
            return Err(Unencodable::OutOfOrder { frame, next: self.next });
        }
        let stream = self.h264.encode(rgb)?;
        self.mp4.frame(stream)?;
        if self.mp4.len() > MOST_CLIP_BYTES {
            return Err(Unencodable::PastMostBytes);
        }
        self.next += 1;

        Ok(())
    }

    /// The clip's MP4, whole, of every frame encoded.
    ///
    /// # Errors
    ///
    /// [`Unencodable::NoParameterSets`] when no frame was encoded.
    pub fn finish(self) -> Result<Vec<u8>, Unencodable> {
        self.mp4.finish()
    }
}

/// What the agent reads of why a clip went no further than its encoder.
pub fn said(why: Unencodable) -> String {
    match why {
        Unencodable::NoEncoder => String::from("this engine has no video encoder, as a browser's has none"),
        Unencodable::OutOfOrder { frame, next } => {
            format!("frame {frame} came to the encoder before frame {next}")
        }
        Unencodable::PastMostBytes => format!(
            "the clip's MP4 would pass {} MB: record fewer frames, or a smaller `longest`",
            MOST_CLIP_BYTES >> 20
        ),
        Unencodable::Failed => String::from("the H.264 encoder failed"),
        Unencodable::NoPicture => String::from("a frame was encoded as no picture"),
        Unencodable::NoParameterSets => String::from("the encoder wrote no parameter sets"),
    }
}
