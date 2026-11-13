# ADR-0043: Capturing a frame

- **Status:** Proposed
- **Date:** 2026-11-03

## Context

An agent drives a program through the hub (ADR-0037, ADR-0038), and its tools read and write files, keep binaries, and spawn and stop engines. None of them shows the agent what a program draws, so visual work on the desktop chassis is done blind.

The renderer can already read a frame back. `ReadBack { of: Readable::Frame }` is held until the next frame is drawn, and answered `Pixels::Read { width, height, format, pixels: Bytes }`: `Rgba8` or `Rgba8Srgb`, red first, each row packed tight (ADR-0023: `Renderer`'s `Handles<ReadBack>`, `copy_frame`, `Copied::unpad`). It reads the first window drawn that `Render`, so a program of several windows cannot say which.

What is missing is a way to hand the pixels to the agent. A tool answers `Called::Output`, the bytes of a value of its output schema, and `Mcp` writes it as JSON in a text block and in `structuredContent` (ADR-0037: `reply::output`). Bytes have a hexadecimal JSON form, so a frame would reach the agent as text at two characters a byte: a 1280 × 800 frame is about 8 MB of hex.

MCP has a content block for this: `{"type": "image", "data": <base64>, "mimeType": "image/png"}`. The client hands it to the model as an image, which it sees in the same turn. An image is charged by its pixels, about width × height / 750 tokens, whatever its encoding's size, and the model's API shrinks one that is large past a limit of its own before the model sees it. So what an image costs the agent, and what detail survives, is decided by its pixel size, not its bytes, and the cheap way to look closely at part of a frame is to crop it, not to send it larger.

A file path is the other way to hand an image over: the agent opens it with a tool of its own. That works only where the hub writes on the agent's machine, which a hub on another host does not. This record leaves files out.

## Decision

**A tool answers an image with `Called::Image`, and says so in its sheet.**

```rust
pub enum Called {
    Output(Bytes),                        // a value of the tool's output schema
    Image { png: Bytes, text: String },   // a PNG, and what the agent reads beside it
    Refused(String),
}

pub struct ToolSheet { pub name: String, pub description: String, pub input: SchemaNodes, pub output: Output }

pub enum Output {
    Value(SchemaNodes),          // answered `Called::Output`
    Image,                       // answered `Called::Image`
    ValueOrImage(SchemaNodes),   // either: the hub's `call_tool`, which answers what an engine's tool answered
}
```

- **The tooling protocol names what it carries.** `Mcp` writes what a tool answers by its case, and never tells an image from a value by the identity of the output's kind.
- **`Mcp` writes `Called::Image` as two blocks:** the image, its bytes in base64 with `mimeType` `image/png`, and a text block of its `text`. It writes no `structuredContent`. A `Called::Image` from a tool whose sheet says `Value`, or a `Called::Output` from one whose sheet says `Image`, is the result `isError` true, as an output that is no value of its schema is today.
- **`Mcp` lists an `outputSchema` only for `Value`.** MCP asks that `structuredContent` match a listed schema, and an image has none, so `Image` and `ValueOrImage` list none. A `ValueOrImage` tool's value is still written as text and `structuredContent`, which MCP allows with no schema listed.
- **An image carries its own text,** which says what the agent cannot read off the pixels: `capture_frame` writes the frame's size, the crop, and the size of the image sent, so the agent can map what it sees back to the frame's pixels for the next crop.
- **The link carries it.** `Outcome` gains `Image { png: Vec<u8>, text: String }`, and `Sheets` carries the sheet's `Output`. An older side would misread both, so `REVISION` is `3` (ADR-0038).
- **The hub's `call_tool` is `ValueOrImage`.** It answers a value of `EngineAnswer`, as it does today, when the engine's tool answers a value, and the engine's image and text as they are when it answers an image, so an engine's image reaches the agent as an image. It checks the answer against the engine's sheet as it does a value. The agent calls it as before; only its listing loses an `outputSchema`, which said that `output` is any JSON. `describe_engine` shows an image tool's output as `"image"`.

**`ReadBack` names the window.** `Readable::Frame` becomes `Frame(WindowId)`: the next frame drawn onto that window, as its pass leaves it. An ask for a window the renderer does not keep, one never opened or already closed, is refused at once (`ReadRefused::Window`), and one held for a window that closes before it is drawn is refused as it closes. A window the chassis hands no frame for in a cycle, as a minimised one may be, keeps its asks until it is drawn. ADR-0023 says so too.

**The tool `capture_frame`, `CaptureTool`, is the renderer's,** in `aethereal-render` behind its `tools` feature, as `read_file` is Files' (ADR-0037).

```rust
#[kind(name = "aether.render.capture")]
pub struct Capture { pub window: WindowId, pub crop: Crop, pub longest: u32 }

#[kind(name = "aether.render.crop")]
pub enum Crop {
    Whole,
    Region { x: u32, y: u32, width: u32, height: u32 },   // in the frame's pixels, from its top left
}
```

```
capture_frame { window, crop, longest }
  → CaptureTool ─ ReadBack { of: Frame(window) } ─▶ Renderer
  ◀─ Pixels::Read { width, height, format, pixels }
  → Encode { pixels, width, height, crop, longest }   a compute job
  ◀─ Encoded { png, width, height }
  → Called::Image { png, text: the sizes } ─▶ Mcp ─▶ [image block, text block]
```

- **It crops, then shrinks, then encodes,** in a compute job (ADR-0018: `Runner::Compute`), so no step waits on a large frame. The job reads the pixels blob and puts the PNG in a buffer it is lent (ADR-0042: `Fills::Buffers`).
- **`longest` is the most pixels of the image's longer side.** An image larger is shrunk to it, keeping its shape; one smaller is sent as it is, never enlarged. Each pixel of the shrunk image is the mean of the frame's pixels it covers, taken in linear light, so a thin bright line on a dark ground keeps its brightness. A `longest` of `0` is refused.
- **A crop is checked against the frame it is applied to:** a region that is empty or reaches past the frame's edge is refused, saying the frame's size, since the agent may not know it until the first capture.
- **The image is opaque.** A frame's alpha is how its pass left it, not what the window shows, so the PNG is RGB. A `Rgba8Srgb` frame's bytes are already encoded and written as they are, with the PNG's `sRGB` chunk; an `Rgba8` frame's bytes are written as they are, as the window shows them.
- **The PNG is written with the `png` crate,** at its fast compression, a dependency of `aethereal-render`'s `tools` feature only. Its size moves no tokens, only bytes over the hub's connection.
- **Every refusal is a result the agent reads:** the renderer's (`Window`, `Format`, `Uncopied`, `Unmapped`, `Unstored`), the crop's, and an ask the renderer did not answer, such as one whose window draws nothing before the call's deadline (ADR-0041).

## Consequences

- **The agent sees frames.** A program that lists `CaptureTool` in its toolbox is visible to an agent through the hub, from any engine linked to it.
- **What a look costs is the agent's choice.** A whole frame at `longest` 1024 is about a thousand tokens; a crop at full resolution costs what its pixels do. The tool's description says so.
- **A capture takes a frame or more.** The renderer answers after the next frame is drawn and its copy mapped, and the job then encodes: tens of milliseconds for a large frame, on a compute thread.
- **Nothing is saved to disk.** A frame the owner wants kept is a later tool's, and so are checks of a frame's pixels, comparing two frames, and reading back a texture.
- **Every tool sheet changes shape,** as `output` becomes `Output`. Each existing tool writes `Output::Value(..)`, and `call_tool` `Output::ValueOrImage(..)`.
- **The link's revision goes up,** so an engine and a hub must be built from the same revision to link, as at every change of `Up` or `Down`.
- **A program that reads a frame back names its window.** The browser examples that read `Readable::Frame` read window `0`.
- **The first check is by hand on the desktop chassis,** since only it draws to a window an agent can link to. The tests cover what needs no GPU: the crop, the shrink, `Mcp`'s image result, and the link's `Outcome`.

## Alternatives considered

- **An image output kind that `Mcp` recognises** by its name or schema. No new case in `Called`, but `Mcp` would judge content by a kind's identity, and every proxy of the tooling protocol would have to know the kind.
- **A path the PNG is written to, answered as text.** Costs the agent nothing until it looks, but it can look only where the hub writes on its machine, and it must make a second call to.
- **A hub tool of its own for images,** beside `call_tool`. No third case of `Output`, but the agent would choose between two tools by what the engine's tool answers.
- **The text written from the tool's description.** A description is fixed when the tool is offered, and the sizes are known only once the frame is read.
- **The image only, no text block.** The agent could not tell a shrunk image from a small frame, nor which pixels of the frame a crop covered.
- **One argument, a scale.** A scale says nothing of the result's size until the frame's size is known; `longest` bounds the cost however large the window is.
- **Shrinking on the GPU**, in a pass before the copy. Less to copy, but a pipeline and texture of its own in the renderer, for a cost the compute job already keeps off every step.
- **JPEG.** Smaller, and lossy where the agent judges pixels; tokens follow pixels, not bytes, so smaller saves nothing the agent pays for.
- **Encoding in the renderer's turn.** It would hold up every step behind a large frame.
- **A PNG encoder of our own.** A stored, uncompressed one is short, but four times the bytes over the connection for no saving, and a compressing one is the `png` crate again.
