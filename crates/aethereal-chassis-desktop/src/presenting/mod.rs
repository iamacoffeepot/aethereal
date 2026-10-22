//! How each window's frames are presented: in step with the display, as fast as they are made, or at a rate, and the
//! mode of a window's surface each is presented with (ADR-0021).

#[cfg(test)]
mod tests;

use aethereal_data::{
    Boundary, Decode, DecodeError, Encode, EncodeError, Integer, Plain, Reader, Schema, Shape, Writer,
};
use core::error::Error;
use core::fmt;

/// How each window's frames are presented, set as the window is opened (ADR-0021). A mode the window's surface lacks
/// is refused, never stood in for by one that behaves otherwise.
#[derive(Plain, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presenting {
    /// In step with the display, `fifo`: acquiring a frame waits for the display to free an image, which paces the
    /// frames, and none tears.
    #[default]
    Fifo,
    /// As fast as frames are made, `uncapped`: presented with `Immediate`, which may tear, or else `Mailbox`, where the
    /// newest frame replaces one waiting for the display. Neither waits, and each cycle follows the last at once.
    Uncapped,
    /// At most a rate, `capped:60`: presented as `Uncapped` is, with each frame due on a grid of the rate from the
    /// start, and one missed while the program was busy skipped, not queued.
    Capped {
        /// The frames a second.
        frames_per_second: FrameRate,
    },
}

impl Presenting {
    /// The surface's mode this is presented with, the first of those it asks for that the surface offers, `offered`.
    /// `Fifo` asks for `Fifo`, and the others for `Immediate`, then `Mailbox`; `FifoRelaxed`, `AutoVsync`, and
    /// `AutoNoVsync` are never chosen.
    ///
    /// # Errors
    ///
    /// When the surface offers none of them, naming the modes it does.
    pub(crate) fn mode(self, offered: &[wgpu::PresentMode]) -> Result<wgpu::PresentMode, Unoffered> {
        let asked: &[wgpu::PresentMode] = match self {
            Self::Fifo => &[wgpu::PresentMode::Fifo],
            Self::Uncapped | Self::Capped { .. } => &[wgpu::PresentMode::Immediate, wgpu::PresentMode::Mailbox],
        };

        asked
            .iter()
            .copied()
            .find(|mode| offered.contains(mode))
            .ok_or_else(|| Unoffered { presenting: self, offered: offered.to_vec() })
    }
}

impl fmt::Display for Presenting {
    /// As the command line writes it: `fifo`, `uncapped`, `capped:60`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fifo => f.write_str("fifo"),
            Self::Uncapped => f.write_str("uncapped"),
            Self::Capped { frames_per_second } => write!(f, "capped:{}", frames_per_second.get()),
        }
    }
}

/// A window's surface offers no mode a presentation asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unoffered {
    /// The presentation asked for.
    presenting: Presenting,
    /// The modes the surface offers.
    offered: Vec<wgpu::PresentMode>,
}

impl fmt::Display for Unoffered {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { presenting, offered } = self;

        write!(f, "the window's surface cannot present `{presenting}`: it offers {offered:?}")
    }
}

impl Error for Unoffered {}

/// A rate of frames a second, from 1 to 1000: a capped window's (ADR-0021).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRate(u16);

impl FrameRate {
    /// The fewest frames a second a rate holds.
    pub const LEAST: u16 = 1;
    /// The most frames a second a rate holds.
    pub const MOST: u16 = 1000;

    /// The rate of `frames_per_second`.
    ///
    /// # Errors
    ///
    /// When it is below [`LEAST`](Self::LEAST) or above [`MOST`](Self::MOST).
    pub const fn new(frames_per_second: u16) -> Result<Self, Unrated> {
        let enough = frames_per_second >= Self::LEAST;
        let within = frames_per_second <= Self::MOST;
        if enough && within {
            Ok(Self(frames_per_second))
        } else {
            Err(Unrated { frames_per_second })
        }
    }

    /// The frames a second.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    /// The nanoseconds from one frame to the next: a second over the rate, rounded down.
    pub(crate) fn period_nanos(self) -> u64 {
        1_000_000_000 / u64::from(self.0)
    }
}

/// A count of frames a second that is no [`FrameRate`]: below 1 or above 1000.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unrated {
    /// The count.
    pub frames_per_second: u16,
}

impl fmt::Display for Unrated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { frames_per_second } = self;
        let (least, most) = (FrameRate::LEAST, FrameRate::MOST);

        write!(f, "{frames_per_second} frames a second is outside {least} to {most}")
    }
}

impl Error for Unrated {}

impl Schema for FrameRate {
    const SHAPE: Shape = Shape::Integer(Integer::U16);
}

impl<B: Boundary> Encode<B> for FrameRate {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.0.encode(out)
    }
}

/// Refuses a count outside the rates, so a rate read from a configuration is one by the time it is held.
impl<B: Boundary> Decode<B> for FrameRate {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let frames_per_second = u16::decode(input)?;
        let (least, most) = (u64::from(Self::LEAST), u64::from(Self::MOST));

        Self::new(frames_per_second).map_err(|_| DecodeError::OutOfRange {
            value: frames_per_second.into(),
            least,
            most,
        })
    }
}

impl Plain for FrameRate {}
