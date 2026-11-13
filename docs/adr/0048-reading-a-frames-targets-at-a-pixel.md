# ADR-0048: Reading a frame's targets at a pixel

- **Status:** Accepted
- **Date:** 2026-11-03

## Context

When the owner marks a spot on a still, the agent has to work out what is there: the object under the mark, and the point in the world it hit. Today it guesses from the colour image, and it has guessed wrong, as when it "fixed" a stair's underside when the owner meant the landing. What would settle it is what the frame drew besides colour: its depth at that pixel, and an object's id where the program draws one.

What is already decided:

- **A frame is passes over its end and its targets,** each target listed by its place in `CreateFrame::targets` and named by it, `Target::Texture(n)`, as floats (`Rgba16Float`), a whole number (`R32Uint`), or depth (`Depth32Float`), at a scale of the end's size (ADR-0025: `TargetSpec`, `TargetKind`, `Scale`). Which target holds what is the program's to decide. An object's id, if any, is written by the program's own pass into a `Uint` target.
- **A window's main frame is the renderer's own until an actor makes one** (`CreateFrame::own`): a compute pass, then `Main`, drawing into the window, with depth in target 0, full size, one sample.
- **A frame's targets are made when the frame is, and again only when its end changes size** (`Walk::new`, `made`). Between `Render`s each holds what the last one drew into it.
- **Targets are made to be drawn into and read by later passes** (`RENDER_ATTACHMENT | TEXTURE_BINDING`), not copied from. A depth target of four samples is never read, and a float one of four is resolved into one sample, which is what is read.
- **The renderer reads back a window's next frame, or a texture as it is now** (ADR-0043: `ReadBack`, `Readable`, `Pixels`), always all of it. A full-size `Rgba16Float` target at 4K is 66 MB, past the engine's default 64 MB of blobs (`EngineSettings::blob_bytes`).
- **The renderer knows no camera.** A program's camera actor keeps the view and projection. So a world point is computed from depth by the program, never by the renderer.

## Decision

**The renderer reads back a region of a frame's target as the last `Render` left it. A tool, `read_target`, answers a window's target at a pixel, or a small patch around it, as numbers. Turning a pixel into a world point is the program's, with its camera, from the depth this reads.**

### Reading a target

```rust
/// The frame's target at its place in `targets`, within `region`, in the frame's end's pixels, as the last `Render`
/// left it.
#[kind(name = "aether.render.read_frame_target")]
pub struct ReadFrameTarget { pub frame: Frame, pub target: u32, pub region: Region }

pub struct Region { pub x: u32, pub y: u32, pub width: u32, pub height: u32 }

#[kind(name = "aether.render.target_pixels")]
pub enum TargetPixels {
    Read { region: Region, kind: TargetKind, pixels: Bytes },   // region in the target's pixels
    Refused(ReadRefused),
}
pub enum ReadRefused { /* as now */ Frame, Target, Multisampled, Outside }
```

- **It is a request of its own, whose answer names its layout by `TargetKind`,** not a `Readable` answered as `Pixels`. `Pixels` names its layout by `TextureFormat`, which is also what `CreateTexture` makes, and a depth format there would let any actor make depth textures.
- **Read now, not at the next frame,** as a texture is. A target holds the last `Render`'s drawing until the next one, so it is read under a held clock without advancing it (ADR-0045). The agent captures a still, then reads the same frame's depth.
- **The region is in the end's pixels,** the window's surface pixels for a main frame. The renderer cuts it to the end, then scales it to the target as the target's scale scales the end: each side divided, the origin rounded down and the far edge up, so a half-scale target is read at the pixel that covers the one asked. The answer's region is the target's pixels read.
- **Only the region reaches the blob.** A float or whole-number target copies only the region. WebGPU copies a depth texture only whole, so a depth target is copied whole into the GPU buffer, and the region is cut from it once mapped: 33 MB of GPU buffer at 4K, never a blob.
- **A read outside a frame is mapped under a held clock.** No frame comes to start its mapping, so the desktop loop, when the clock is held, starts every mapping asked for and waits for the GPU, as it does after a held frame (ADR-0045).
- **Every target that holds one sample is made copyable** (`COPY_SRC`), and so is a float target's resolved texture. A depth target of four samples is refused, `Multisampled`, as WebGPU copies no multisampled texture. A program that wants its depth read keeps it at one sample, as the renderer's own frame does.
- **Refused:** `Frame` when no frame is the window's or the id's; `Target` when the frame has no target at that place; `Outside` when none of the region is within the end.
- **Depth is answered as the GPU holds it:** an `f32` from 0 at the near plane to 1 at the far plane, 1 where nothing was drawn, as each pass that writes depth clears it to the far plane (`DepthUse::Write`).

### `read_target`

```rust
#[kind(name = "aether.render.read_target")]
pub struct ReadTarget { pub window: WindowId, pub target: u32, pub at: Pixel, pub around: u32 }
pub struct Pixel { pub x: u32, pub y: u32 }

#[kind(name = "aether.render.target_read")]
pub struct TargetRead {
    pub region: Region,   // in the target's pixels, cut to it
    pub values: Values,   // row by row from the top
}
pub enum Values { Float(Vec<[f32; 4]>), Uint(Vec<u32>), Depth(Vec<f32>) }
```

- **`TargetTool`, in `aethereal-render` behind its `tools` feature,** asks `ReadFrameTarget` of the window's main frame and answers the values as numbers, so the agent reads depth or an id without decoding bytes.
- **`around` is a radius in the window's pixels:** 0 reads the one pixel, and at most 16 reads a 33-pixel square, cut to the window, so the agent can see an edge near its mark. A float is answered from its 16-bit half.
- The usual use: `capture_frame` a still, the owner marks a pixel, `read_target { window: "0", target: 0, at, around: 2 }` reads the depth under it, and the program's own tool turns it into a world point.

### The world point is the program's

- **A program whose camera the agent needs unprojects** with its own view and projection: the clip point `(2x/w − 1, 1 − 2y/h, depth)` through the inverse of `projection × view`, which `aethereal-math`'s `Mat4::inverse` gives.
- **An object's id is the program's to draw** into a `Uint` target, with its own pass, and `read_target` reads it the same way. The renderer gives no ids of its own: its placements are not the program's objects.

## Consequences

- **The agent learns what is under a pixel instead of guessing:** the depth of any program on the renderer's own frame at once, and an object's id from any program that draws one.
- **Every program reads its depth, target 0, with no change of its own,** where its main frame is the renderer's own or keeps depth at one sample.
- **Targets made copyable cost nothing measurable,** a usage flag on textures that exist already.
- **A world point needs the program's camera,** so a program that wants agents to pick in its world offers a tool for it. Ichor's painter does that with its own camera and target layout.
- **A target can be read only on the main frame of a window,** through the tool. A frame that ends on a texture is read through `ReadFrameTarget` by an actor that holds its id, which no agent does.
- **Follow-on work this leaves open:** reading a target at the next frame rather than the last; multisampled depth, resolved by a pass of its own; a shared way for a program to name its targets, so an agent need not know target 0 is depth.

## Alternatives considered

- **The renderer turns a pixel into a world point itself.** It would need the camera, which is a program's actor's, and so would the renderer, which draws whatever it is handed.
- **Read the whole target, and pick the pixel in the tool.** Simple, but a float target at 4K is past the default blob budget, and a pixel's read would copy 66 MB.
- **Read at the next frame, as `capture_frame` does.** It would match the frame a following capture shows, but under a held clock no next frame comes until the agent advances. What the agent wants is the frame it just saw, which is the one the targets hold.
- **The renderer writes an id per placement into a target of its own.** Every program would pay for a pass and a target it may not want, and a placement is not what the program calls an object: ichor's splats are one placement for a whole tree.
