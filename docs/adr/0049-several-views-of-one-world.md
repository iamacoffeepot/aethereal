# ADR-0049: Several views of one world

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

The owner judges a look in pairs, side by side: one change against the scene without it, or two cameras on one place. Today each side of a pair is its own bake and its own process, so the two sides can differ in more than the one thing being compared, each costs a full bake, and the agent captures two windows and lays the stills together itself. What a pair wants is one engine and one copy of the world, drawn twice into one window, each side from its own camera or with its own settings, so one `capture_frame` shows the pair.

What is already decided:

- **One placement draws in several frames,** joining a pass of each with its own program and bindings (ADR-0025: `Place::joins`, `Join`). So one mesh and one set of instances on the GPU are drawn into two frames, each join bound to its own camera's uniforms. Pass names are unique within a frame, not across frames, so two frames may each have a pass `Splats`.
- **A window shows one frame, its main frame, and every pass draws over the whole of its target** (ADR-0025: `Windowed::main`; WebGPU begins each pass with its viewport the size of its target). There is no viewport, so no pass draws into part of a window. A pass that cleared would clear the whole target in any case: WebGPU clears an attachment whole whatever the viewport.
- **A frame may end on a texture,** which any frame's effect or placement binds (ADR-0025: `Ends::Texture`, `Framed::Texture`). Its size is fixed when it is made, and frames that read it run after it.
- **A main frame follows its window's size:** at the first `Render` after a resize its targets are made again at the new size, with the bind groups its own passes read them through (`Kept::fit`, `Walk::fit`). A texture frame never is.
- **Frames are ordered by what they read,** the textures they end on and the storage their dispatches write (ADR-0025, ADR-0026: `ordered`). Two frames that neither reads are in no order, and a storage written in both is refused (`Written`).
- **A program's camera and its settings are the program's.** The renderer knows no camera (ADR-0048).

## Decision

**A view is a frame that ends on a texture whose size follows a part of a window's, made again when the window is resized. A window shows several views as its main frame's owner lays their textures out, with an effect. The world is shared by placements that join each view's passes, each join bound to its view's camera.**

### A texture that follows a window

```rust
pub enum Ends {
    Window(WindowId),
    Texture { size: Size, format: TextureFormat },
    /// A texture of `window`'s size divided as `width` and `height` say, made again at each size the window is drawn at.
    Follows { window: WindowId, width: Scale, height: Scale, format: TextureFormat },
}
```

- **Its size is the window's, each side divided by its own scale** and rounded up, as a target's is (ADR-0025: `Scale`, `Size::scaled`): two views side by side are each `Half` wide and `Full` high, four in a grid `Half` and `Half`.
- **It is answered as any texture frame is,** `Framed::Texture { frame, texture }`, and its texture is bound, joined, and read back as any other.
- **It is made at one pixel and fitted at the first `Render` that draws its window,** as a main frame is, and **fitted again with the window's main frame at each `Render` after a resize**: its end texture is made again at the new size, then its targets, then every bind group that binds the texture, whoever made it: the placements' and dispatches' that bind it, and the effects' of other frames. Each of those keeps the bindings it was made from beside its groups, so it is bound again as it was made. A window not drawn in a `Render` fits none of its views, which keep their size until it is drawn.
- **The texture keeps its id when it is made again,** so what names it by its id names it still (ADR-0025). A frame that binds it reads the new one from the `Render` it is fitted in, and the frames that read it run after it, as before.
- **It is refused when the renderer does not draw to the window** (`FrameRefused::Window`). Its window closing leaves it at its last size, drawn until its maker releases it, as a frame that reads a released texture reads it as last drawn.

### Laying views out

- **The main frame's owner lays the views' textures on the window** with an effect that binds them and a sampler, as any effect binds a texture (ADR-0025: `Does::Effect::bindings`). Side by side is a few lines of WGSL:

```wgsl
@group(1) @binding(0) var left: texture_2d<f32>;
@group(1) @binding(1) var right: texture_2d<f32>;
@group(1) @binding(2) var linear: sampler;

@fragment fn lay(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let at = vec2(fract(uv.x * 2.0), uv.y);
    let a = textureSample(left, linear, at);
    let b = textureSample(right, linear, at);
    return select(b, a, uv.x < 0.5);
}
```

- **Each view is a whole frame,** so the two sides can differ in anything a frame describes: their passes, their effects and bindings, their targets' scales and samples. A pair that compares a pass with and without an effect is two views of two shapes over the same placements.
- **The pair is one image:** `capture_frame` and `record_clip` of the window capture both sides at once.

### One world, a camera for each view

- **A thing drawn in both views is one placement that joins each view's pass,** `Join::to::<Splats>(Frame::Texture(left), ..)` and `Join::to::<Splats>(Frame::Texture(right), ..)`, each join bound to its own view's camera uniforms. Its mesh, its instances, and its storage are on the GPU once.
- **Compute the views share runs in one of them.** A dispatch that writes storage both views read joins one view's compute pass, so that view runs before the other (ADR-0026: frames ordered by the storage they write and read). Compute each view does for itself, such as culling to its own camera, joins each view's own pass and writes storage of its own.
- **A camera actor for each view, and the settings each view keeps, are the program's,** as a camera is. Input reaches the window as a whole, so a program with a view on each side of one window sends a click to the camera of the side it lands in.

## Consequences

- **A pair is drawn by one engine over one world,** and the two sides differ only in what the program gives each view, so a pair compares one thing.
- **The owner sees the pair in one window,** and the agent captures and records it in one call, with no layout of its own.
- **Several windows still work as they did,** each with its own main frame. A view is a texture frame that follows a window, so a program can show its views in separate windows instead, each window's main frame showing one.
- **Two views cost what one window at full size does** in targets, as each is half the window, and each pass runs once a view.
- **A frame that follows a window can serve more than a split:** an interface drawn at half the window's size, or a reflection that tracks it, is one too.
- **A view's targets are not read by `read_target`,** which reads a window's main frame. A pixel the owner marks on a pair is read by the program's own tool, which knows which view it falls in, until the tool can name a view.
- **Follow-on work this leaves open:**
  - a stock effect in `aethereal-render` that lays out two or four views, so a program need write no WGSL to split a window;
  - scales of a third, for three views side by side;
  - `read_target` naming a view;
  - a viewport for each pass, if a frame ever needs to draw into part of a target it shares.

## Alternatives considered

- **A viewport for each pass, so one frame draws both views into halves of the window.** It needs no new kind of end, but every pass and every target is then shared by the two sides: a pass that clears clears both, as WebGPU clears a target whole, and each effect reads a target the other side drew into too. The sides could not differ in their passes, only in their bindings.
- **The renderer lays views on the window itself,** each view a frame that ends on a part of a window. No program would write the effect, but a window would show several frames instead of one, two actors could each lay a view over the same part, and the renderer would decide a layout its owner writes everywhere else.
- **Two windows.** They work today, but on eve's Wayland desktop the compositor places them, the owner sees the pair wherever they land, and the agent captures two stills.
- **Texture frames of a fixed size, made again by the program on each resize.** It needs nothing of the renderer, but every frame and placement that binds the texture would be made again by its maker, and the program would hear the resize before the renderer had drawn at it.
