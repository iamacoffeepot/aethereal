//! The renderer's tool, `capture_frame` (ADR-0037, ADR-0043): a window's next frame, cut and shrunk as the agent asks,
//! answered as an image.

pub mod encode;
pub mod image;
#[cfg(test)]
mod tests;

use crate::Renderer;
use crate::readback::{Pixels, ReadBack, ReadRefused, Readable};
use crate::texture::TextureFormat;
use aethereal_actor::{
    Actor, ActorRef, Answer, Answered, Anywhere, Asks, Beginning, ChildOf, Ending, Fault, Handles, Held, Path,
    ProtocolRef, ToSender, Turn, path,
};
use aethereal_data::{Bound, Budget, Plain, Schema, SchemaNodes, from_bytes};
use aethereal_tools::{Call, Called, Output, Tool, ToolSheet, Tooling};
use aethereal_window::WindowId;
use encode::{Encode, Encoded};
use image::Region;
use std::collections::BTreeMap;

/// The most bytes reading an input may take: a window, a crop, and a size.
const INPUT_BYTES: usize = 1024;

/// What the agent asks `capture_frame` for: the next frame drawn onto `window`, cut by `crop`, its longer side at most
/// `longest` pixels.
#[derive(Debug, Clone, Copy, Plain)]
#[kind(name = "aether.render.capture")]
pub struct Capture {
    pub window: WindowId,
    pub crop: Crop,
    pub longest: u32,
}

/// What of a frame is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.crop")]
pub enum Crop {
    /// All of it.
    Whole,
    /// The part `width` by `height` whose top left is at `x`, `y`, in the frame's pixels from its top left.
    Region { x: u32, y: u32, width: u32, height: u32 },
}

impl Crop {
    /// The region of a frame `width` by `height` this cuts.
    #[must_use]
    pub const fn region(self, frame_width: u32, frame_height: u32) -> Region {
        match self {
            Self::Whole => Region::whole(frame_width, frame_height),
            Self::Region { x, y, width, height } => Region { x, y, width, height },
        }
    }
}

/// What the tool keeps for the frame it asked the renderer for: which call it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.capture_reading")]
pub struct Reading {
    number: u64,
}

/// What the tool keeps for the image a job makes: which call it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.capture_encoding")]
pub struct Encoding {
    number: u64,
}

/// What the tool is made from: nothing.
#[derive(Debug, Default, PartialEq, Eq, Plain)]
#[kind(name = "aether.render.capture_tool_config")]
pub struct CaptureToolConfig;

/// A call waiting on its frame: to cut by `crop` and send at most `longest` across.
#[derive(Bound)]
struct Asked {
    held: Held<Called>,
    crop: Crop,
    longest: u32,
}

/// A call waiting on its image, and the text the agent reads beside it.
#[derive(Bound)]
struct Made {
    held: Held<Called>,
    text: String,
}

/// The tool `capture_frame`: asks the [`Renderer`] for the next frame drawn onto a window, then runs a job that cuts
/// it, shrinks it in linear light, and writes it as a PNG, and answers the image with a line of its sizes.
#[derive(Bound)]
#[kind(name = "aether.render.capture_tool")]
pub struct CaptureTool {
    /// The calls waiting on their frame, each by its number.
    reading: BTreeMap<u64, Asked>,
    /// The calls waiting on their image, each by its number.
    encoding: BTreeMap<u64, Made>,
    next: u64,
}

impl ChildOf for CaptureTool {
    type Parent = Anywhere;
}

impl Actor for CaptureTool {
    type Config = CaptureToolConfig;
    type Key = ();
    type Handles = (Call, Answer<Pixels, Reading>, Answer<Encoded, Encoding>);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: CaptureToolConfig, _: &mut Beginning<'_, Self, ()>) -> Result<Self, Fault> {
        Ok(Self { reading: BTreeMap::new(), encoding: BTreeMap::new(), next: 0 })
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Tool for CaptureTool {
    fn sheets() -> Vec<ToolSheet> {
        vec![ToolSheet {
            name: String::from("capture_frame"),
            description: String::from(
                "Captures the next frame drawn onto a window, numbered from 0, as a PNG image, with a line giving the \
                 frame's size, the crop, and the image's size. `longest` bounds the image's longer side: a larger \
                 image is shrunk to keep its shape, and a smaller one is never enlarged. An image costs about width \
                 × height / 750 tokens, so a whole frame at 1024 is about a thousand. `crop` cuts a region of the \
                 frame, in its pixels from the top left, before shrinking: crop to look closely at part of a frame.",
            ),
            input: SchemaNodes::of(&Capture::SHAPE),
            output: Output::Image,
        }]
    }

    fn worker(me: ActorRef<Self>) -> ProtocolRef<Tooling> {
        me.protocol()
    }
}

impl Handles<Call> for CaptureTool {
    type Sends = (Asks<Path<Renderer>, ReadBack, Reading>, ToSender<Called>);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let Ok(capture) = from_bytes::<Capture>(turn.read(&call.input), &mut Budget::new(INPUT_BYTES)) else {
            turn.reply(&refused("that is no capture"))?;
            return Ok(());
        };
        if capture.longest == 0 {
            turn.reply(&refused("`longest` is at least 1"))?;
            return Ok(());
        }
        let held = turn.hold()?;
        let number = self.next;
        self.next += 1;
        turn.to(path::<Renderer>(())).ask(&ReadBack { of: Readable::Frame(capture.window) }, &Reading { number })?;
        self.reading.insert(number, Asked { held, crop: capture.crop, longest: capture.longest });

        Ok(())
    }
}

impl Handles<Answer<Pixels, Reading>> for CaptureTool {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Reading { number }): (Answered<Pixels>, Reading),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(Asked { held, crop, longest }) = self.reading.remove(&number) else {
            return Ok(());
        };
        let (width, height, format, pixels) = match answered {
            Answered::Yes(Pixels::Read { width, height, format, pixels }) => (width, height, format, pixels),
            Answered::Yes(Pixels::Refused(why)) => {
                turn.answer(held, &refused(unread(why)))?;
                return Ok(());
            }
            Answered::No(why) => {
                turn.answer(held, &Called::Refused(format!("the renderer did not answer: {why:?}")))?;
                return Ok(());
            }
        };
        if !is_shown(format) {
            turn.answer(held, &refused("the frame is of a format no image is written from"))?;
            return Ok(());
        }
        let region = crop.region(width, height);
        if !region.within(width, height) {
            let outside = format!("the crop is not within the frame, which is {width}x{height}");
            turn.answer(held, &Called::Refused(outside))?;
            return Ok(());
        }

        let (sent_width, sent_height) = image::sent_size(region.width, region.height, longest);
        let encode = Encode { pixels, frame_width: width, region, width: sent_width, height: sent_height };
        turn.run(&encode, &Encoding { number })?;
        let text = said(width, height, crop, sent_width, sent_height);
        self.encoding.insert(number, Made { held, text });

        Ok(())
    }
}

impl Handles<Answer<Encoded, Encoding>> for CaptureTool {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Encoding { number }): (Answered<Encoded>, Encoding),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let Some(Made { held, text }) = self.encoding.remove(&number) else {
            return Ok(());
        };
        let called = match answered {
            Answered::Yes(Encoded { png }) => Called::Image { png, text },
            Answered::No(_) => refused("the frame could not be written as a PNG"),
        };
        turn.answer(held, &called)?;

        Ok(())
    }
}

/// A refusal saying `why`.
fn refused(why: &str) -> Called {
    Called::Refused(String::from(why))
}

/// Whether a frame of `format` is as its window shows it, a byte a channel, which is what an image is written from.
pub const fn is_shown(format: TextureFormat) -> bool {
    match format {
        TextureFormat::Rgba8 | TextureFormat::Rgba8Srgb => true,
        TextureFormat::R8 | TextureFormat::Rgba16Float => false,
    }
}

/// What the agent reads of why the renderer did not read the frame back.
pub const fn unread(why: ReadRefused) -> &'static str {
    match why {
        ReadRefused::Window => "no window of that number is drawn to",
        ReadRefused::Format => "the window's surface is of a format no image is written from",
        ReadRefused::Uncopied => "the window's surface cannot be copied from",
        ReadRefused::Unmapped => "the GPU did not hand the frame back",
        ReadRefused::Unstored => "the frame is larger than the engine may hold",
        ReadRefused::Texture
        | ReadRefused::Storage
        | ReadRefused::Frame
        | ReadRefused::Target
        | ReadRefused::Multisampled
        | ReadRefused::Outside => "the renderer read back something other than a frame",
    }
}

/// The line the agent reads beside an image `sent_width` by `sent_height` of a frame `frame_width` by `frame_height`
/// cut by `crop`: what it cannot read off the pixels, to map what it sees back to the frame's for the next crop.
fn said(frame_width: u32, frame_height: u32, crop: Crop, sent_width: u32, sent_height: u32) -> String {
    let cut = match crop {
        Crop::Whole => String::from("whole"),
        Crop::Region { x, y, width, height } => format!("{width}x{height} at {x},{y}"),
    };

    format!("frame {frame_width}x{frame_height}, crop {cut}, image {sent_width}x{sent_height}")
}
