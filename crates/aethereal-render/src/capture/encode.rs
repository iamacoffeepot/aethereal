//! The job that makes a frame's image: made by `capture_frame` alone (ADR-0043).

use super::image::{self, Region};
use aethereal_actor::{Bytes, Fault, Fills, Job, Runner, Running};
use aethereal_data::{Bound, Plain};

/// Cuts `region` out of `pixels`, a frame's RGBA rows `frame_width` wide, shrinks it to `width` by `height`, and
/// writes it as a PNG, put as a blob. It fills buffers: the PNG is written into the largest buffer its actor keeps
/// when it fits (ADR-0042).
#[derive(Debug, Bound)]
#[kind(name = "aether.render.encode")]
pub struct Encode {
    pub pixels: Bytes,
    pub frame_width: u32,
    pub region: Region,
    pub width: u32,
    pub height: u32,
}

/// What the job answers with: the PNG.
#[derive(Debug, Bound)]
#[kind(name = "aether.render.encoded")]
pub struct Encoded {
    pub png: Bytes,
}

/// Why the job failed: the PNG could not be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.unencoded")]
pub struct Unencoded;

/// It computes, on threads for computing, so no step waits on a large frame.
impl Job for Encode {
    type Done = Encoded;
    const RUNNER: Runner = Runner::Compute;
    const FILLS: Fills = Fills::Buffers;

    async fn run(self, job: &mut Running) -> Result<Encoded, Fault> {
        let rgb = image::cut(job.read(&self.pixels), self.frame_width, self.region);
        let rgb = image::shrunk(&rgb, self.region.width, self.region.height, self.width, self.height);
        // A guess at the PNG's size, which it grows past when it must.
        let buffer = job.buffer(rgb.len() / 2);
        let Ok(png) = image::png(&rgb, self.width, self.height, buffer) else {
            return Err(Fault::new(&Unencoded)?);
        };

        Ok(Encoded { png: job.put(png) })
    }
}
