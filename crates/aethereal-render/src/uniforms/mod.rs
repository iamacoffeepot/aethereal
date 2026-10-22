//! Uniforms: the bytes a program's uniform block is bound to, made and written by one actor, as a camera writes its
//! view each frame (ADR-0023).

#[cfg(test)]
mod tests;

use aethereal_actor::BlobRef;
use aethereal_data::{Bound, Plain};

/// Asks the renderer for uniforms holding `bytes`, whose size is fixed from then. Answered with [`Uniformed`].
#[derive(Bound)]
#[kind(name = "aether.render.create_uniforms")]
pub struct CreateUniforms {
    pub bytes: BlobRef,
}

/// Writes `bytes` over the bytes of `uniforms` from the byte `first`. Only their maker may write them, and nothing is
/// answered: a write that does not fit is logged, and the bytes are left as they were.
#[derive(Bound)]
#[kind(name = "aether.render.write_uniforms")]
pub struct WriteUniforms {
    pub uniforms: UniformsId,
    pub first: u32,
    pub bytes: BlobRef,
}

/// Uniforms the renderer made, which any actor handed them may bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.uniforms_id")]
pub struct UniformsId {
    pub(crate) id: u64,
}

/// What the renderer answers [`CreateUniforms`] with.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.uniformed")]
pub enum Uniformed {
    Made(UniformsId),
    Refused(UniformsRefused),
}

/// Why uniforms were not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.uniforms_refused")]
pub enum UniformsRefused {
    /// There are no bytes, or they are no whole number of four, or more than the `most` the GPU binds as a uniform
    /// block.
    Size { bytes: u64, most: u64 },
}

/// Checks uniforms before they are made: that their bytes are whole words, and as many as the GPU binds in one block,
/// the `most`. Answers their size.
///
/// # Errors
///
/// Why they would not be made as asked.
pub fn check(bytes: &[u8], most: u64) -> Result<u32, UniformsRefused> {
    let size = bytes.len() as u64;
    let bound = size > 0 && size.is_multiple_of(4) && size <= most;
    let (true, Ok(sized)) = (bound, u32::try_from(size)) else {
        return Err(UniformsRefused::Size { bytes: size, most });
    };

    Ok(sized)
}

/// Whether `bytes` written from the byte `first` lie inside uniforms of `size` bytes, on whole words, as the GPU
/// writes a buffer.
pub(crate) fn writes(size: u32, first: u32, bytes: u64) -> bool {
    let end = u64::from(first) + bytes;
    let inside = end <= u64::from(size);
    let aligned = first.is_multiple_of(4) && bytes.is_multiple_of(4);

    inside && aligned
}
