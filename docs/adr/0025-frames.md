# ADR-0025: Frames

- **Status:** Proposed
- **Date:** 2026-10-21

## Context

The render root draws every placement in one pass onto the canvas at each `Render` (ADR-0023). That pass is all a frame is: nothing drawn can be read, by a later draw or by a program, and the order between placements is the render root's.

A frame that does more is several passes. A pass draws into images fixed when it begins, and its draws run in the order they are recorded. Its images are finished when it ends, and only then can a later pass read them as textures; a pass never reads an image it writes. So:

- **Order across what different actors draw** is the order of passes: everything a pass draws is under everything a later pass draws.
- **Depth one pass writes** can be tested by a later pass, or read by it as a texture: a surface drawn first that later draws are clipped to.
- **A blend no fixed blend gives** is a pass that reads the finished image as a texture and writes what its program computes: multiplying through paper, darkening at edges, any post-processing.

What engines have learned about such frames:

- **What is costly is the pipeline, not the frame.** A program compiles in milliseconds or more and stalls the frame that waits for it, so engines make them at load and keep them (ADR-0021). A frame's passes are a list and some textures. Its shape changes rarely, at load, at a change of settings, at a change of mode; what changes every frame is the data the passes read.
- **The large render graphs rebuild the frame every frame and compile it** (Frostbite's FrameGraph, Unreal's RDG), mostly to place the barriers between passes, schedule queues, and alias memory. WebGPU places its own barriers and has one queue.
- **Passes cost little.** The earlier engine measured about 35 µs a pass, and a fullscreen pass at 1280×720 about 0.3 ms.
- **Many lights do not need a pass each.** Lights are read from a buffer in one pass, and shadows drawn into tiles of one depth texture in one pass, each light's draws in its own viewport.
- **Order taken from where actors stand is hard to follow.** The earlier engine painted each frame's draws in the order of their senders' places in its tree of actors. Where something drew depended on where its actor was made and when, and the renderer and the tree of actors, which need nothing of each other, each had to know the other.

## Decision

**A frame is an ordered list of passes over the textures it draws into, made once, checked whole, and run at every `Render`.** It is made by asking the render root, as everything drawn is (ADR-0021), and owned by its maker.

```rust
CreateFrame { ends: Ends, targets: Vec<TargetSpec>, passes: Vec<PassSpec> }   -> Made<FrameId> | Refused
enum Ends { Window(WindowId), Texture(Size) }   // shown on a window (ADR-0024), or drawn into a texture
struct Size { width: u32, height: u32 }         // the texture's, of `Rgba8`, each side at most the GPU's longest
enum TargetSpec { Float, Uint, Depth }          // Rgba16Float, R32Uint, Depth32Float

struct PassSpec { name: PassName, does: Does }   // a type's id, as a kind's is (ADR-0006)
enum Does {
    Draw   { into: Target, depth: DepthUse, load: Load },                    // placements that join it draw here
    Effect { program: ProgramId, reads: Vec<Target>, into: Target, bindings: Vec<Bind> },  // one draw over the whole target
}
enum Target { End, Texture(u32) }        // the frame's end, or one of `targets`
enum DepthUse { Off, Write(u32), Test(u32) }    // one of `targets`, of a depth format
enum Load { Clear, Keep }
```

- A target of floats is four 16-bit floats a pixel, `Rgba16Float`, which WebGPU filters and blends; one of whole numbers is one 32-bit whole number, `R32Uint`; depth is `Depth32Float`, as every program tests. The description's pass is `PassSpec`, as the builder's `Pass` is the trait a pass's type implements.
- **An effect binds what it reads and what it lists besides.** Its targets are bound in its first group, the first it reads at binding 0; its `bindings` are bound in the groups after, as a placement binds what its program lists (ADR-0023), so an effect that comes and goes is switched by its uniforms, and an effect reads a texture another frame ends on by binding it. **This departs from `Effect` having no bindings**, which left no way to bind those uniforms.
- An effect pass begins from what an earlier pass drew into its target, so an effect that blends lays its colour over it, and from black when no earlier pass wrote it.

- A pass reads only targets an earlier pass of the frame wrote, never the one it writes, and a depth it tests is one an earlier pass wrote.
- Every target is the size of the frame's end. Targets of other sizes are later work.
- A pass draws into one colour target. Several at once are later work.
- The last pass writes the frame's end.

**The frame is described whole, and replaced whole.** There is no frame half made, and no pass added to a frame or taken from it: a frame of another shape is a new frame, which takes the old one's place at the next `Render`. A frame made for a window is its main frame from the turn it is made in, to what is placed after it and to `ShapeOf`, and is drawn from the next `Render`, which is the first to run after it; the frame it replaces is taken back with its id. A frame's shape changes as a setting does, so it is described in one ask, and what changes every frame rides the data its passes read: placements, instances, uniforms, storage (ADR-0023). An effect that comes and goes is switched by its uniforms, and costs its pass while off.

**An actor builds the description with a typed builder,** so a frame that would be refused does not compile:

```rust
struct Surfaces;  impl Pass for Surfaces {}   // a pass is named by a type, which the frame's maker and the
struct Splats;    impl Pass for Splats {}     // actors that place things share

let mut frame = FrameBuilder::on(Ends::Window(window));
let (ids, depth) = frame.draw(Surfaces, Uint, depth::Write);             // -> (Target<Uint>, DepthTarget)
let splats = frame.draw(Splats, Float, depth::Test(&depth));             // -> Target<Float>
let create = frame.finish(Glaze, Effect::new(&glaze, (splats, ids)))?;   // -> CreateFrame, ending on the window
```

- **A pass's type and its target's kind are given as values**, `Surfaces` and `Uint`, where this record first wrote `draw::<Surfaces>(Colour::<Uint>, ..)`: a call that names one of its types and infers the rest is not one Rust has, and the kinds and the use of depth decide what a pass returns, so each is inferred from its value. A pass type is a unit struct, so its value is its name.
- `draw_over` draws a pass over a target an earlier pass returned (`Load::Keep`), `effect` draws an effect into a new target, and `finish` takes the last pass's type and either an `Effect` or a draw `Onto` the end.

- **A pass is named by a type, not a string.** A name mistyped, or one renamed by the frame's maker and not by those who place things, is a compile error, where a string would join nothing and be found only by what was not drawn. A guest names a pass by the type's id, as it names a kind: a pass type is a kind, `trait Pass: Kind`, given a name of its own, and its `PassName` is its `KindId`.

- A target is the output of the pass that draws it, so a pass reads only what an earlier pass returned, and never what it writes.
- `finish` takes the pass that writes the frame's end, and is the only way to a `CreateFrame`.
- An effect is made for what it reads and writes, `EffectId<(Float, Uint)>`, and `Effect::new` takes targets of those kinds and no others. Handles are typed by kind and not by frame, so one taken from another builder compiles, and is refused by the render root when it names no target of this frame that an earlier pass wrote.
- A target of whole numbers takes no blend: a program or an effect made for a shape of whole numbers is refused a blend, and writes `u32` where one of fractions or floats writes `vec4<f32>`.
- Two passes of one type in one frame are refused by `finish`, at once and before anything is sent.
- Each bound reads plainly when it fails (`#[diagnostic::on_unimplemented]`): which target, what the effect reads.

**An effect is made by asking for its fragment stage and what it reads,** as a program is (ADR-0021):

```rust
CreateEffect { wgsl: String, fragment: String, reads: Vec<Read>, bindings: Vec<Binding>, blend: Blend, shape: Shape }
    -> Effected::Made(MadeEffect { program: ProgramId, reads: Vec<Read> }) | Effected::Refused(ProgramRefused)
enum Read { Float, Uint, Depth }   // texture_2d<f32>, texture_2d<u32>, texture_depth_2d, at @group(0) @binding(i)
```

- **The render root writes the vertex stage**: one triangle over the whole target, passing its fragment stage where each pixel is across the target from the top left, `@location(0) uv: vec2<f32>`, beside `@builtin(position)`. Every target of a frame is the size of its end, so the pixel an effect shades is the pixel it reads with `textureLoad`. An effect's shape holds no depth, and it lists nothing in group 0.
- It is checked as a program is: its WGSL valid, its fragment stage reading only what the vertex stage writes and writing its shape's colour, each target it reads declared as `reads` says, each binding it uses listed, and no more textures than a stage binds, its reads counted with them. That is where an effect is held to the device's limits.
- **`EffectId<R>` is had only from the answer, and only when the answer reads what `R` says**: `EffectId::<(Float, Uint)>::of(&made)` is none for an effect made to read anything else. On the wire, and in an actor's state, it is the program's id alone, as a guest names it; what it reads is its type's, and the render root checks a pass's reads against what the effect was made to read, whatever its sender claims.
- An effect is a program, released as one; a placement cannot join with it, and an effect pass cannot draw a program that is not an effect.

**The render root checks the description again, whole,** as a guest's mail does not come through the builder, and a forged one must fail as the builder would have. Whatever the builder rules out is refused here too, and so is what no type sees: a window that is not there, a program released, an effect program made for another format than the target it writes, an effect's bindings not what the pass reads, more than the device allows. A frame that is made can be drawn: nothing in it fails at `Render`.

**A program is made against the shape of what it draws into: its format, its samples, and its depth.** A pass's targets have a shape, and a program fits every pass whose targets have its shape. So a frame replaced by one that keeps its passes' shapes keeps every program valid, and nothing compiles again. Samples are one and depth `Depth32Float`, or none for a pass that uses no depth and for an effect, until a target of another shape is needed; the shape is one type from the start, so that each addition widens it rather than changing every program's.

**A placement joins passes by their type, each with its own program and bindings.** A depth pass and the pass that shades draw the same things, with a program that writes only depth and one that shades:

```rust
Place {
    mesh: MeshId,
    instances: Drawn,
    joins: Vec<Join>,          // Join::to::<Splats>(frame, program, bindings): the pass by its type's id
}                                                                    -> Made<PlacementId> | Refused
```

- Each join is checked as ADR-0023 checks a placement: the program's shape is the pass's, and the mesh, the instances, and the bindings are what the program reads. A placement that joins nothing, or one pass of one frame twice, is refused, and a refused join is named by its place in `joins`.
- **A join names a window's main frame by the window, `Frame::Main(WindowId)`**, whichever frame is that window's main frame at each `Render`, so the placements that joined it follow it when it is replaced. A frame that ends on a texture is never replaced, as its texture is another texture once it is made again, and is joined by its id.
- **An actor learns the shape a join's program is made against by asking**, `ShapeOf { frame, pass } -> Shaped::Shape(Shape) | Shaped::Refused(JoinRefused)`, refused when there is no such frame or it has no such pass. A program written for a window's main frame cannot know the window's format, which is the host's: a browser's canvas prefers one laid out blue first. **This departs from the record, which said nothing of how a shape is learned.**
- A placement is drawn in every pass it joins, once in each.
- When a frame is replaced by one with a pass of the same type and shape, the placements that joined it draw in the new one. One whose pass the new frame has not, or has in another shape, is not drawn, and is logged, until a frame has that pass again.

**There are many frames, and each window shows one: its main frame.**

- A frame that ends on a texture is answered with that texture's `TextureId` beside its own id, `Framed::Texture { frame, texture }`. The texture is an ordinary one: an effect pass of another frame binds it, a placement binds it, and it is read back. A minimap, a screen in the world, a layer of interface, a reflection, each is a frame of its own that ends on a texture. **Its format is `Rgba8`**, as `Size` names none: a texture a placement binds is one of floats it filters, and one that is read back is one of bytes. A side of no pixels, or longer than the GPU's longest, is refused (`Size`).
- A frame that ends on a window is that window's main frame, and a window has one. Until an actor makes one, a window's main frame is the render root's own: a compute pass, `Compute`, which every dispatch joins (ADR-0026), then one pass onto the window, with depth, which every placement joins as `Main`, each a pass the render root names, as ADR-0023's one pass was.
- An actor that asks to make a frame on a window whose main frame another owns is refused, `FrameRefused::Owned { owner }`, naming the owner's `ActorId`.
- The chassis hands the render root the surface of a window with the window's id (ADR-0024: `WindowId`), so a frame that ends on a window names it as every other actor does. A browser's chassis opens the canvas as its one window and WebGPU on it in one call. Surfaces for several windows wait for a host that opens several.
- **The main frame of a window is owned as a resource is** (ADR-0010): the first actor to make a frame ending on a window owns that window's main frame, only it may replace it, and another actor's is refused, naming the owner. When the owner releases it, or ends, the window's main frame is the render root's own again. The render root tracks the owner as it tracks the maker of everything it holds (ADR-0021: `Made`, and its watch of each maker), and no token passes between them. **This departs from making the slot a resource the owner holds.** A resource is shown only by giving it, so each replacement would give it to the render root and back; what a held resource would add is handing the slot to another actor, and a token is added when that is needed.

**Frames meet only through textures, and their order follows from what reads what.** No frame writes into another's targets: it reads another's finished texture. A frame names another's texture only once that frame is made and has answered with it, so a frame reads only frames made before it, and no two frames read each other. **A frame reads what its effects bind and what the placements that join it bind**, and a placement made later can join an earlier frame and bind a later one's texture, so the render root refuses a placement whose joins would have frames read each other round, or a frame read its own texture (`JoinRefused::Round`), and the order is worked out again whenever a frame or a placement comes or goes, not at `Render`. Frames that do not read each other run in the order of their ids. At each `Render` the render root runs every frame that ends on a texture before the frames that read it, then each window's main frame, and then shows each window. A frame read in the same `Render` is read as drawn in that `Render`. Two frames that do not read each other run in no order an actor may rely on, as neither touches what the other draws.

**Order is declared where drawing is described, and nowhere else.** It is the order of a frame's passes, which their author writes, and the order of one placement's records, which one actor writes (ADR-0023). Within one pass, the placements of different actors are drawn in no order: grouped by program, each program's pipeline set once. Order is never taken from where actors stand, when they were made, or when their mail came. **This departs from the earlier engine's order of its tree of actors**, which made what was drawn on top hang on where and when actors were made, and joined the renderer to the tree of actors, which need nothing of each other.

**A frame keeps what it reads, as a placement does** (ADR-0023). A texture frame's maker ending, or its release, takes back its ids, and the frames and placements that read its texture read it as it was last drawn, until they are released themselves.

## Consequences

- A frame is checked when it is made and walked at `Render`: a pass is begun, its placements or its one effect draw recorded, and the next pass begun. Nothing is built, looked up, or refused at `Render`.
- A program that draws things and never mentions frames draws as it did: the render root's own main frame is ADR-0023's one pass.
- A change of settings is one `CreateFrame` and one release. The programs are kept, as they are made against shapes, so a frame is replaced without compiling anything.
- An actor written in Rust gets a frame's mistakes as compile errors that name them. A guest gets them as refusals from the render root's check, which is the one that decides.
- Every target lives as long as its frame, and every frame's targets are its own. At 3840×2160, eight targets of four 16-bit floats are about 530 MB. A frame is fixed when it is made, so which of its targets are never alive at once can be worked out then, and they can share memory; that is later work, inside the render root.
- An interface library, or anything drawn apart from the game, draws into a frame of its own and the main frame's owner reads its texture: one effect pass to lay it over the scene, decided by that owner.
- A frame cannot depth test against another frame's depth. A frame that needs to reads it as a texture, which the other frame ends on.
- ADR-0021 and ADR-0023 are edited to match: a program is made against a shape rather than the surface's format, a placement joins passes, and what ADR-0023 drew in one pass is the render root's own main frame.
- What is decided separately, each when it is needed:
  - **Compute passes**, and the storage they write: decided by ADR-0026. Then **draws whose count a buffer holds**.
  - **Targets of other sizes and several colour targets in one pass.**
  - **A viewport for each draw**, for shadows drawn into tiles of one texture.
  - **Shared memory for targets never alive at once.**
  - **A texture frame run less often than every `Render`**, or when asked.
  - **Places in a frame another actor's passes fill**, for a frame put together from parts that do not know each other.
  - **Handing a window's main frame to another actor.**

## Alternatives considered

- **One frame per window, and no other.** A minimap, a screen in the world, and a layer of interface would each be passes in the one frame its owner writes, and every library that draws would need that owner to write its passes.
- **A frame rebuilt every frame**, as the large render graphs are. Its shape could follow each frame's data, a pass for each of a number of views. It is a description sent each frame and checked each frame, and what it most buys, barriers, queues, and aliasing, WebGPU does itself or a fixed frame can do once.
- **A frame edited a pass at a time.** A frame between two edits is one nobody described, and every edit is checked against every other.
- **Passes named by their place in the list.** A pass put before another would change which pass every placement joined.
- **Passes named by strings.** A name mistyped, or renamed on one side, joins nothing, and nothing says so but what is not drawn.
- **Programs made against a pass.** A frame replaced would compile every program again.
- **Anyone may replace a window's main frame.** Two actors that each set it would replace each other's, and which frame was shown would hang on whose mail came last.
- **Placements drawn in the order of their makers' places in the tree of actors**, as the earlier engine drew. Order would hang on where and when actors were made rather than on what is described, and the renderer would read the topology.
- **An order key on each placement.** Refused already by ADR-0023: numbers unrelated actors must agree on, which nothing checks.
