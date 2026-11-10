//! A browser's engine has no H.264 encoder: `OpenH264` is built natively only, so a clip is refused before it is drawn.

use super::Unencodable;

/// Whether this engine encodes video: a browser's does not.
pub const ENCODES: bool = false;

/// No encoder: [`Self::new`] refuses, so none is ever made.
pub struct H264(());

impl H264 {
    /// Refused, as a browser's engine has no encoder.
    pub const fn new(_frames_per_second: u32, _width: u32, _height: u32) -> Result<Self, Unencodable> {
        Err(Unencodable::NoEncoder)
    }

    /// Refused, as the native encoder's would be: never called, as none is made.
    #[allow(
        clippy::unused_self,
        reason = "it keeps the native encoder's signature, which its one caller is written to"
    )]
    pub const fn encode(&self, _rgb: &[u8]) -> Result<&[u8], Unencodable> {
        Err(Unencodable::NoEncoder)
    }
}
