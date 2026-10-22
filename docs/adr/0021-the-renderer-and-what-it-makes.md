# ADR-0021: The renderer and what it makes

- **Status:** Proposed
- **Date:** 2026-10-20

## Context

The browser chassis opens WebGPU on the page's canvas before the engine is made and hands the device, its queue, and the canvas's surface to a root as one resource, `Gpu` (ADR-0010: `Engine::hand`). Its example root records its own commands with them. That is enough to clear a canvas and too little for a program: every actor that draws would need the device, and a resource has one owner. A guest could never hold it, since a wgpu value is not data.

What makes drawing cheap is known, from the earlier engine and from WebGPU itself:

- **What a frame draws is already on the GPU.** Meshes, textures, and compiled pipelines are made once and kept, and a frame writes a few uniforms and records its passes. The earlier engine drew each object as its own dispatch and pass, and measured a pass at about 35 µs; a draw inside one pass cost about 0.1 to 0.4 µs (its ADR-0246).
- **A draw that can fail at frame time is a frame that can fail.** The earlier engine moved every check to where a resource is made, where its sender is answered, so a draw cannot fail once recorded (its ADR-0246).
- **A closed set of shaders is outgrown.** The earlier engine began with a few shaders of its own (its ADR-0140), and within months let actors register their own WGSL with the passes it runs, validated once (its ADR-0170, ADR-0171).
- **Uploading at first draw costs that frame.** The earlier engine moved uploads to the frames before a resource is drawn (its ADR-0251).
- **WebGPU has no multi-draw-indirect and no bindless binding.** A draw names one pipeline and one mesh, and textures are bound in groups. It has instancing, storage buffers a vertex stage reads, compute, and indirect draws.
- **In a browser one thread uses the device.** A `GPUDevice` cannot be shared between Web Workers. Natively wgpu's device is `Send` and `Sync`, and resources can be made from several threads.

This record decides how actors make what is drawn: the first piece of the renderer. How they say what is drawn each frame is left to a record of its own (see Consequences).

## Decision

**One capability, the render root, owns the GPU.** It is the root the chassis hands `Gpu` to, the device and its queue, with each window it draws to and the format of that window's surface, and a controller and a source for the work it waits on the host for (ADR-0023), and the only actor that touches the device or its queue (ADR-0019: a capability fronts one host system). Every other actor, native or a guest, reaches the GPU by asking the render root.

**The chassis owns each window's surface, and hands the render root each window's frame through one front.** A frame of a surface is bound to its host, as ADR-0019's values that stay with the chassis are: it is presented by value, and natively it is acquired by waiting for the display. So the chassis keeps each window's surface and each frame it acquires, `WindowSurface`, and the render root holds from boot one front for every window's frame, a way in, and a controller for presenting each, a way out:

```rust
pub struct Front { .. }                   // a resource: a slot for each window, keyed by its id, each holding at most one frame's texture
pub struct Present { window: WindowId }   // asked of the chassis once the window's frame is drawn and submitted
pub struct WindowFormat { pub window: WindowId, pub format: ColourFormat }

RendererConfig { gpu: Given<Gpu>, front: Given<Front>, presenting: Given<Controller<Present>>, windows: Vec<WindowFormat>, mapping: .., mapped: .. }

let mut front = Front::new();
let surface = WindowSurface::new(surface, &gpu, window, config, &mut front)?;   // configured here, in its own format
let windows = surfaces.iter().map(WindowSurface::drawn).collect();               // each window, and its format
let (presenting, presents) = setup.controller();

// each cycle, on the chassis's thread
surface.resized(width, height);    // for each window: the size it pushed as the window's last `Resized`
surface.acquire();                 // each shown window in turn: configures if the size changed, acquires, puts the frame's texture in its slot
engine.cycle(time);                // then steps: `Render` takes the textures, draws into each, submits, pushes `Present` for each
let asked = presents.take();
surface.present(&asked);           // each window: presents the frame it kept if `Present` was asked for that window
```

- **The front is one resource with a lock-free slot for each window, keyed by the window's id, each holding at most one frame**, a queue of one: the chassis puts in the texture of each window's frame it acquired, a clone of the frame's own, and the render root takes every window's at `Render`. A frame put while one is there takes its place. The host's thread and a worker's turn share each slot, so it is no lock: what the host brings in never waits on a turn.
- **It is one front for every window, not a front for each.** A resource the render root keeps leaves it only when the render root ends (ADR-0010), and windows will come and go while the program runs, so a front handed for each window would stay kept in the render root after its window closed. The render root learns each window and the format of its surface through its config, from each `WindowSurface`, and the GPU names no window.
- **A window whose slot is empty is not drawn this cycle**: its surface gave no texture, or it is hidden or of no size. Its main frame is not drawn and its frame not asked to be presented. A cycle with every slot empty draws nothing and asks for nothing, and the next frame drawn is the one read and timed (ADR-0023); a frame read back is the first window's drawn that cycle, in the order the chassis opened them.
- **The chassis presents each window after the cycle**, the frame it kept, when `Present` was asked for that window. A kept frame that was not presented is dropped before the next acquire, and its texture taken back from its slot.
- **One cycle draws every window shown, so a window on a slower display paces every window to its rate.** The chassis acquires each shown window's frame in turn before the cycle, and with `Fifo` each acquire waits on its own display. Presentation modes chosen per window are the way out of that (#246).
- **The chassis configures the surface**, when it is made and before an acquire whenever its window was resized since, at the size it pushed as `Resized` (ADR-0024). A window of no size is not acquired, and nothing is handed. A frame that no longer matches the surface, `Suboptimal`, is handed, and the surface is configured again before the next acquire. One that is `Outdated` or `Lost` configures the surface again and hands nothing.
- **Why: a native acquire blocks.** With `Fifo`, which is vsync, it waits until the display frees an image. A turn that waited would hold its worker, and the engine would time about one refresh as the render root's cost every frame. The chassis waits on its own thread instead, and that wait paces the cycles natively as `requestAnimationFrame` does in a browser, where the acquire returns at once. Both hosts take the one path, so the render root has no host's branch.
- **No resource is made each frame, and nothing is added to the actor API.** The front and the controller are resources the render root owns from boot (ADR-0010, ADR-0019), and the size of each window's main frame follows the texture each of its frames is drawn into (ADR-0025).

**An actor makes a mesh, a texture, or a program by asking for it.** Each is answered with the resource's id or a refusal that says why (ADR-0017: `turn.to(target).ask`, `Answer`):

```rust
CreateMesh    { layout: VertexLayout, vertices: BlobRef, indices: BlobRef }           -> Made<MeshId> | Refused
CreateTexture { width: u32, height: u32, format: TextureFormat, pixels: BlobRef }     -> Made<TextureId> | Refused
CreateProgram {
    wgsl: BlobRef,                      // the WGSL, as UTF-8
    vertex: String,                     // the vertex stage's entry point
    fragment: String,                   // the fragment stage's entry point
    layout: VertexLayout,               // the vertex buffer it reads: stride, and each attribute's location, format, offset
    instances: PerInstance,             // what it reads once per instance: nothing, or a record laid out so (ADR-0023)
    bindings: Vec<Binding>,             // what it binds, by group and binding: a uniform block or read-only storage of a size, a texture, a sampler
    reads: Vec<Read>,                   // the targets of its frame it reads at group 0, most often none (ADR-0025)
    state: Fixed,                       // blending (opaque, alpha, additive, multiply), depth (off, test, test and write), culling, coverage
}                                                                                     -> Made<ProgramId> | Refused
Release { id }                                                                        // no answer
```

- **A program is the compiled code and its fixed state:** one render pipeline. It is the costly thing to make and what draws are batched by.
- **A material, a program with the values it is drawn with, is not made on its own.** It is one bind group over a program, the cheap thing there are many of. A placement names what its program binds and its bind groups are made with it (ADR-0023), so each placement carries a material of its own; one that many placements share is added when sharing first needs it.
- **The bytes travel as blobs** (ADR-0014), so a mesh is not copied through mail. The render root reads them in its turn (`Turn::read`) and uploads them in the same turn, with `Queue::write_buffer` and `Queue::write_texture`. In a browser those calls hand the bytes to the GPU process and return.
- **A program's WGSL travels as a blob too.** A shader is kilobytes, and mail is read within the engine's limit, which a page sets to one kilobyte; a shader past it would fail the render root's turn as its request is read. Bytes that are not UTF-8 are refused, `ProgramRefused::Utf8`, naming how many of them are.

**Everything is checked when it is made, and nothing when it is drawn.** A resource that is made can be drawn:

- A mesh's indices are each less than its vertex count, its vertex bytes are a whole number of its layout's stride, and every attribute lies inside the stride.
- A texture's bytes are its width times its height times its format's size.
- A program's WGSL is parsed and validated with naga, in the render root's turn, with no capability beyond what every WebGPU device has. Its entry points exist and are a vertex and a fragment stage. Each input of the vertex stage is an attribute of `layout`, at its location and of its kind of number (`f32` or `u32`), of any count of components, as WebGPU fills what a format lacks. Each binding the shader uses is in `bindings`, at its group and binding and of its kind, and a uniform block's or a storage buffer's bytes are at least what the shader reads of it. A storage buffer is one the shader only reads (`var<storage, read>`), from either stage, as WebGPU lets a vertex stage read storage it does not write; one it writes is refused (ADR-0023). Then the pipeline is made. **This departs from those checks alone**, as each of these is one WebGPU makes as the pipeline is made, where natively wgpu stops the program and a browser makes an invalid pipeline:
  - the fragment stage reads each of its locations as the vertex stage writes it, of the same type and interpolation;
  - the fragment stage writes a `vec4<f32>` at location 0, the colour drawn, or a `u32` or a vector of them for a shape of whole numbers, which takes no blend (ADR-0025);
  - a program tests or writes depth only for a shape that holds it;
  - the module declares no constant a pipeline overrides, as a program sets none;
  - `bindings` lists each place once, in a group the GPU has, uniform blocks and storage buffers of a size it binds, and no more of each kind than a stage binds, by the device's limits;
  - a vertex layout reads at most sixteen locations, as many as WebGPU lets every device read; a mesh's layout is checked the same.
- **A program is made against the shape of what it draws into: its format, its samples, and its depth** (ADR-0025). A program fits every pass whose targets have its shape, in any frame, so a frame replaced keeps its programs. Depth is `Depth32Float`, tested as less than what is drawn, and a program that does not test depth compares `Always` and writes nothing. Samples are one or four (ADR-0025).
- **A program on a shape of four samples may turn its alpha into coverage**, `Fixed { .., coverage: Coverage::Alpha }`: the share of a pixel's samples it covers is its alpha, so a soft edge is drawn in one pass, in any order, with no blend. `Coverage::Off` is every program's until it asks; one asking on a shape of one sample is refused (`ProgramRefused::Coverage`), as there is nothing to cover.

**The id is plain data.** The render root numbers ids from one count it never takes back, so an id once released is refused, not taken for another resource. An actor may hand an id to another, so two actors can draw one mesh.

**Whoever made a resource owns it.** Only its maker may release it. The render root watches each actor the first time it makes something (ADR-0009: `watch`), and drops every resource that actor made when it ends. Nothing has to remember to release, and `Release` frees one early.

**The render root runs on one thread.** In a browser that is the page's. A native chassis may later make resources on several threads behind the same asks, as wgpu allows, without the asks changing.

## Consequences

- An actor that draws needs no GPU value: a guest makes resources as native code does, by asking.
- A program cannot fail at draw time, and neither can its mesh or texture, which is what lets a frame record its draws without a fallible path.
- naga is linked into the render root's build. In a browser that makes the module larger: the make example's module grew from 636 KB (199 KB gzipped) to 2.06 MB (638 KB gzipped) with its WGSL front end and validator. Without it the browser checks WGSL itself, but reports through a promise, after the turn, and cannot check a shader against the layout and bindings it was made with before a pipeline uses them.
- Uploads happen in the turn that makes a resource, so a turn that makes much uploads much. Spreading them over the frames before first use (the earlier engine's ADR-0251) is follow-on work, once content streams in.
- What is decided separately, each when it is needed:
  - **How actors say what is drawn each frame:** placements, instances, and uniforms, kept on the render root and changed by mail (ADR-0023).
  - **The frame's passes and effects:** a scene pass into a texture, effect passes over it, the last onto the canvas (the earlier engine's ADR-0170 pass graph).
  - **Compute programs, and storage they write,** for content made on the GPU. Storage a program only reads is decided (ADR-0023).
  - **Materials** shared by many placements, as above.
  - **A lost device**, through the capability's controller (ADR-0019).

## Alternatives considered

- **Each drawing actor owns GPU values and records its own commands.** A cookbook module would move into an actor almost unchanged. A guest could never draw, every frame would move a target and command buffers between actors as resources, and how draws are batched would be up to each actor.
- **A closed set of shaders the engine writes.** What the earlier engine began with and outgrew.
- **A material that is the program and its values together.** Two materials over one shader would compile it twice and batch apart.
- **The browser's own WGSL checks in place of naga.** The answer to `CreateProgram` would come only after a promise, and would not cover the layout and bindings.
- **The render root acquiring its frame in `Render`**, as the earlier engine's render actor did. The surface would be one actor's alone. That was harmless there, as its render ran last and alone on the main thread; here a native acquire blocks a turn on a worker, timed as the render root's cost.
- **`Mailbox` presenting, with cycles the host times.** No acquire would wait. Metal offers only `Fifo`, and a timer drifts from the display's refresh.
- **A new resource handed each frame**, through the source. Nothing would be held from boot. A resource the render root keeps leaves it only when it ends (ADR-0010), so each frame would be kept until then, and presenting needs the frame by value.
