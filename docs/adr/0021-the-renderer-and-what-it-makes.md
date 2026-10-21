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

**One capability, the render root, owns the GPU.** It is the root the chassis hands `Gpu` to, and the only actor that touches the device, its queue, or the surface (ADR-0019: a capability fronts one host system). Every other actor, native or a guest, reaches the GPU by asking the render root.

**An actor makes a mesh, a texture, or a program by asking for it.** Each is answered with the resource's id or a refusal that says why (ADR-0017: `turn.to(target).ask`, `Answer`):

```rust
CreateMesh    { layout: VertexLayout, vertices: BlobRef, indices: BlobRef }           -> Made<MeshId> | Refused
CreateTexture { width: u32, height: u32, format: TextureFormat, pixels: BlobRef }     -> Made<TextureId> | Refused
CreateProgram {
    wgsl: String,
    vertex: String,                     // the vertex stage's entry point
    fragment: String,                   // the fragment stage's entry point
    layout: VertexLayout,               // the vertex buffer it reads: stride, and each attribute's location, format, offset
    bindings: Vec<Binding>,             // what it binds, by group and binding: a uniform block of a size, a texture, a sampler
    state: Fixed,                       // blending (opaque, alpha, additive), depth (off, test, test and write), culling
}                                                                                     -> Made<ProgramId> | Refused
Release { id }                                                                        // no answer
```

- **A program is the compiled code and its fixed state:** one render pipeline. It is the costly thing to make and what draws are batched by.
- **A material, a program with the values it is drawn with, is not made yet.** It is one bind group over a program, the cheap thing there are many of, and is added when objects that share a program first need different textures. Until then a program's values come through its uniforms.
- **The bytes travel as blobs** (ADR-0014), so a mesh is not copied through mail. The render root reads them in its turn (`Turn::read`) and uploads them in the same turn, with `Queue::write_buffer` and `Queue::write_texture`. In a browser those calls hand the bytes to the GPU process and return.

**Everything is checked when it is made, and nothing when it is drawn.** A resource that is made can be drawn:

- A mesh's indices are each less than its vertex count, its vertex bytes are a whole number of its layout's stride, and every attribute lies inside the stride.
- A texture's bytes are its width times its height times its format's size.
- A program's WGSL is parsed and validated with naga, in the render root's turn, with no capability beyond what every WebGPU device has. Its entry points exist and are a vertex and a fragment stage. Each input of the vertex stage is an attribute of `layout`, at its location and of its kind of number (`f32` or `u32`), of any count of components, as WebGPU fills what a format lacks. Each binding the shader uses is in `bindings`, at its group and binding and of its kind, and a uniform block's bytes are at least what the shader reads of it. Then the pipeline is made. **This departs from those checks alone**, as each of these is one WebGPU makes as the pipeline is made, where natively wgpu stops the program and a browser makes an invalid pipeline:
  - the fragment stage reads each of its locations as the vertex stage writes it, of the same type and interpolation;
  - the fragment stage writes a `vec4<f32>` at location 0, the colour drawn;
  - the module declares no constant a pipeline overrides, as a program sets none;
  - `bindings` lists each place once, in a group the GPU has, uniform blocks of a size it binds, and no more of each kind than a stage binds, by the device's limits;
  - a vertex layout reads at most sixteen locations, as many as WebGPU lets every device read; a mesh's layout is checked the same.
- **A program draws to the surface's format, and tests depth in `Depth32Float`.** The pass a frame draws in holds a depth texture of that format, so every program is made against it, a program that does not test depth comparing `Always` and writing nothing. When a scene pass draws into a texture of its own (below), programs are made against that texture's format.

**The id is plain data.** The render root numbers ids from one count it never takes back, so an id once released is refused, not taken for another resource. An actor may hand an id to another, so two actors can draw one mesh.

**Whoever made a resource owns it.** Only its maker may release it. The render root watches each actor the first time it makes something (ADR-0009: `watch`), and drops every resource that actor made when it ends. Nothing has to remember to release, and `Release` frees one early.

**The render root runs on one thread.** In a browser that is the page's. A native chassis may later make resources on several threads behind the same asks, as wgpu allows, without the asks changing.

## Consequences

- An actor that draws needs no GPU value: a guest makes resources as native code does, by asking.
- A program cannot fail at draw time, and neither can its mesh or texture, which is what lets a frame record its draws without a fallible path.
- naga is linked into the render root's build. In a browser that makes the module larger: the make example's module grew from 636 KB (199 KB gzipped) to 2.06 MB (638 KB gzipped) with its WGSL front end and validator. Without it the browser checks WGSL itself, but reports through a promise, after the turn, and cannot check a shader against the layout and bindings it was made with before a pipeline uses them.
- Uploads happen in the turn that makes a resource, so a turn that makes much uploads much. Spreading them over the frames before first use (the earlier engine's ADR-0251) is follow-on work, once content streams in.
- What is decided separately, each when it is needed:
  - **How actors say what is drawn each frame.** One choice is mail to the render root for each change; another is an effect each actor leaves, folded into a view the render root reads at `Render` (ADR-0011), which would be the first view outside the engine's own. This is the next record.
  - **The frame's passes and effects:** a scene pass into a texture, effect passes over it, the last onto the canvas (the earlier engine's ADR-0170 pass graph).
  - **Storage buffers and compute programs,** for content made on the GPU.
  - **Materials,** as above.
  - **A lost device**, through the capability's controller (ADR-0019).

## Alternatives considered

- **Each drawing actor owns GPU values and records its own commands.** A cookbook module would move into an actor almost unchanged. A guest could never draw, every frame would move a target and command buffers between actors as resources, and how draws are batched would be up to each actor.
- **A closed set of shaders the engine writes.** What the earlier engine began with and outgrew.
- **A material that is the program and its values together.** Two materials over one shader would compile it twice and batch apart.
- **The browser's own WGSL checks in place of naga.** The answer to `CreateProgram` would come only after a promise, and would not cover the layout and bindings.
