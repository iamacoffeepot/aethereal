//! The H.264 encoder: Cisco's `OpenH264`, compiled into the engine from source (ADR-0045).

use super::Unencodable;
use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, QpRange, RateControlMode, SpsPpsStrategy, UsageType,
    VuiConfig,
};
use openh264::formats::{RgbSliceU8, YUVBuffer};
use std::num::NonZero;
use std::thread::available_parallelism;

/// Whether this engine encodes video: a native one does.
pub const ENCODES: bool = true;

/// The bits a second a clip of 1080p at 60 frames a second is encoded at, which keeps a window's text and lines sharp
/// where little moves. A clip of another size or rate keeps the same bits a pixel: 800 × 600 at 60 is encoded at
/// about 2.3 Mb/s.
const FULL_HD_BITS_PER_SECOND: u64 = 10_000_000;

/// The pixels a second of 1080p at 60 frames a second.
const FULL_HD_PIXELS_PER_SECOND: u64 = 1920 * 1080 * 60;

/// The fewest bits a second a clip is encoded at, however small.
const LEAST_BITS_PER_SECOND: u64 = 500_000;

/// The coarsest quantizer a frame is encoded at, whatever the rate asks: past it, text blurs into its ground, so a busy
/// frame takes more bits than the rate rather than losing its edges. The finest is well below what any frame needs.
const COARSEST_QP: u8 = 36;
const FINEST_QP: u8 = 10;

/// How many seconds apart frames a player may start at are.
const KEYFRAME_SECONDS: u32 = 2;

/// The most threads a frame is encoded on. `OpenH264` uses no more (its encoder's `MAX_THREADS_NUM`), and a timing of
/// 120 1080p frames on one machine took 35 to 50% off each frame's encode at four threads against one, with or without
/// its assembly, where two gained little on a scene of text. The encode is a job on the engine's compute pool, beside other work, and these are
/// threads of the encoder's own, so a machine of fewer cores is given no more than it has.
const MOST_THREADS: u16 = 4;

/// The most bytes a slice of a frame holds. `OpenH264` encodes a frame on several threads only when it is cut into
/// slices, a part of the frame for each thread, and its bindings cut a frame only by a most size of slice: each part
/// is then one slice unless it passes this. It is far above what a frame takes at the clip's rate, so a frame is cut
/// into as many slices as threads, and only an unusually heavy one into more. Each slice costs a little: about 1% more
/// bytes at four, where a frame takes more than its rate.
const MOST_SLICE_BYTES: u32 = 250_000;

/// An encoder of frames of one size, and the YUV 4:2:0 picture each is converted into, and the bytes each is written
/// to, both kept from frame to frame.
pub struct H264 {
    encoder: Encoder,
    yuv: YUVBuffer,
    stream: Vec<u8>,
    width: usize,
    height: usize,
}

impl H264 {
    /// An encoder of frames `width` by `height`, both even, at `frames_per_second`: constant bit rate at the bits a
    /// pixel [`FULL_HD_BITS_PER_SECOND`] is at, and never skipping a frame, so each frame drawn is a frame of the clip;
    /// a frame a player may start at each [`KEYFRAME_SECONDS`]; colour as BT.601 at video range, as `OpenH264` converts
    /// it, and said so in the stream, so a player reads it as it was written; each frame cut into slices of at most
    /// [`MOST_SLICE_BYTES`], encoded on as many threads as the machine has cores, up to [`MOST_THREADS`].
    pub fn new(frames_per_second: u32, width: u32, height: u32) -> Result<Self, Unencodable> {
        let pixels_per_second = u64::from(width) * u64::from(height) * u64::from(frames_per_second);
        let bits_per_second =
            (FULL_HD_BITS_PER_SECOND * pixels_per_second / FULL_HD_PIXELS_PER_SECOND).max(LEAST_BITS_PER_SECOND);
        let bits_per_second = u32::try_from(bits_per_second).unwrap_or(u32::MAX);
        let rate = f32::from(u16::try_from(frames_per_second).unwrap_or(u16::MAX));
        let cores = available_parallelism().map_or(1, NonZero::get);
        let threads = u16::try_from(cores).unwrap_or(u16::MAX).min(MOST_THREADS);
        let config = EncoderConfig::new()
            .usage_type(UsageType::CameraVideoRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bits_per_second))
            .max_frame_rate(FrameRate::from_hz(rate))
            .skip_frames(false)
            .qp(QpRange::new(FINEST_QP, COARSEST_QP))
            .intra_frame_period(IntraFramePeriod::from_num_frames(frames_per_second * KEYFRAME_SECONDS))
            .sps_pps_strategy(SpsPpsStrategy::ConstantId)
            .max_slice_len(MOST_SLICE_BYTES)
            .num_threads(threads)
            .vui(VuiConfig::bt601());
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config).map_err(|_| Unencodable::Failed)?;
        let (width, height) = (width as usize, height as usize);

        Ok(Self { encoder, yuv: YUVBuffer::new(width, height), stream: Vec::new(), width, height })
    }

    /// Encodes `rgb`, a frame of the encoder's size row by row, and answers its NAL units as an Annex B stream.
    pub fn encode(&mut self, rgb: &[u8]) -> Result<&[u8], Unencodable> {
        let is_whole = rgb.len() == self.width * self.height * 3;
        if !is_whole {
            return Err(Unencodable::Failed);
        }
        self.yuv.read_rgb8(RgbSliceU8::new(rgb, (self.width, self.height)));
        let stream = self.encoder.encode(&self.yuv).map_err(|_| Unencodable::Failed)?;
        self.stream.clear();
        stream.write_vec(&mut self.stream);

        Ok(&self.stream)
    }
}
