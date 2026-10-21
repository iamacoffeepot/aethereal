# ADR-0023: What is drawn

- **Status:** Proposed
- **Date:** 2026-10-20

## Context

The render root makes meshes, textures, and programs for any actor that asks, and checks each as it is made, so a draw with them cannot fail (ADR-0021: `CreateMesh`, `CreateTexture`, `CreateProgram`). At `Render` it clears the canvas and draws nothing. ADR-0021 left how actors say what is drawn each frame to this record.

What the earlier engine measured decides most of the shape (its ADR-0246):

- **A draw inside one pass is cheap, a pass is not.** About 0.07 µs of CPU for a draw with no change of state, 0.22 to 0.40 µs with one, and about 35 µs a pass.
- **A list kept on the renderer beats a list sent each frame.** Carrying 30,000 draws in every frame was a 960 KB mail and 0.15 to 0.9 ms of encoding and checking. Editing a kept list was cheap: 11 µs for a hundred entries, 48 µs to rewrite 10,000 instance records.
- **What varies between many objects of one mesh is a record each, read per instance.** One mesh drawn for every placement of it, each placement's position and colour a record in a buffer the vertex stage steps once per instance.

The work is general: a 3D scene, a 2D game's sprites, a tilemap, particles, and the rectangles and glyphs of a UI are each a mesh drawn once for each of many records. The engine never reads what a record holds.

A frame's stages arrive at `Render` in order (ADR-0020: `Tick`, `Render`, `Present`): every turn `Tick` set off has run, however far its mail went, before `Render` is sent.

## Decision

**What is drawn is kept on the render root, and an actor changes it by mail.** An actor makes what it draws once and writes what changes, and the render root draws everything it keeps at each `Render`. Nothing is sent each frame that did not change. Mail sent in `Tick` has been handled before `Render` begins, so a frame draws what its `Tick` wrote.

**Instances hold the records a mesh is drawn once for each of.** They are made, and written, by one actor:

```rust
CreateInstances { layout: VertexLayout, capacity: u32, records: BlobRef }  -> Made<InstancesId> | Refused
WriteInstances  { instances: InstancesId, first: u32, records: BlobRef }   // no answer
```

- A record is laid out by a `VertexLayout`, as a vertex is, and read by the vertex stage once per instance.
- `capacity` counts records and is fixed when the instances are made. Growing past it is making new instances.
- The instances hold as many records as were last written, at most their capacity. A write lays its records over theirs from `first`; one that passes the end extends them, so adding is a write at their length; an empty write cuts them to their first `first` records. `first` is at most their length.
- A write's bytes are a whole number of records, within the capacity, or the write is refused: logged, and the records left as they were. A write is not answered, as a mover writes every frame and an answer each frame would be mail nobody reads.

**Uniforms hold the bytes a program's uniform block is bound to.** A camera makes one and writes its view each `Tick`:

```rust
CreateUniforms { bytes: BlobRef }                                    -> Made<UniformsId> | Refused
WriteUniforms  { uniforms: UniformsId, first: u32, bytes: BlobRef }  // no answer
```

Their size is fixed when they are made, a write lies inside it or is refused and logged, and nothing else of them changes.

**Storage holds the bytes a program's read-only storage buffer is bound to,** as uniforms hold a uniform block's: a field of splats, particles, or a skeleton's bones, read by index where a record per instance is read in order. It is made and written as uniforms are:

```rust
CreateStorage { bytes: BlobRef }                                    -> Made<StorageId> | Refused
WriteStorage  { storage: StorageId, first: u32, bytes: BlobRef }    // no answer
```

- Its bytes are a whole number of words, and at most what the GPU binds as one storage buffer (`max_storage_buffer_binding_size`). Its size is fixed when it is made, a write lies inside it on whole words or is refused and logged, and only its maker writes or releases it.
- A program binds it as `BindingKind::Storage { bytes }`: a storage buffer its shader only reads, `var<storage, read>`, which both stages see. A shader that writes it is refused: storage is written on the GPU only by compute (ADR-0026: `Access::Write`).
- `bytes` is the least the program binds: what the shader reads of the buffer, with an array whose length is the buffer's (`array<T>`, the usual last member) counted as holding one element. That is the fixed part before the array, padded to its alignment, and one element, as WebGPU counts a binding's least size. Storage bound there is at least `bytes`, and the array then holds as many elements as fit in the rest, which the shader reads with `arrayLength`. A field of splats is made at its capacity and drawn for a count of instances, as the shader reads one splat per instance.
- No more storage buffers are listed than a stage binds (`max_storage_buffers_per_shader_stage`), counted apart from uniform blocks.

**A placement is one thing drawn: a mesh, with a program, for each of a count of instances, with what the program binds.**

```rust
Place {
    program: ProgramId,
    mesh: MeshId,
    instances: Drawn,       // Drawn::Count(n): n instances, the program reading none; Drawn::Records(InstancesId): one for each record
    bindings: Vec<Bind>,    // for each place the program binds: Bind { group, binding, to: Resource }
}                                                                    -> Made<PlacementId> | Refused

enum Resource { Uniforms(UniformsId), Storage(StorageId), Texture(TextureId), Sampler(Sampling) }
struct Sampling { filter: Filter, wrap: Wrap }   // Filter::{Linear, Nearest}, Wrap::{Clamp, Repeat}
```

- A placement drawn from `Records` draws one instance for each record its instances hold at that frame, so a swarm whose count changes writes its records and leaves the placement as it is.
- A placement does not change once made. Another mesh, program, or binding is another placement, and the old one is released.
- **A placement joins a frame's passes by their type, each with a program and bindings of its own** (ADR-0025): `program` and `bindings` above are one join, of the render root's own main frame's one pass, `Main`. Each join is checked as this record checks a placement.
- A sampler is not made: a placement names how it samples, and the render root keeps one sampler for each way.

**A program says what it reads per instance.** `CreateProgram` gains `instances: PerInstance`, where `PerInstance::None` reads nothing per instance and `PerInstance::Records(VertexLayout)` reads a record laid out so, as a second vertex buffer stepped once per instance. Its attributes are checked as the vertex layout's are, the two layouts read no location in common, and together they read at most sixteen.

**Everything is checked when a placement is made, and nothing when it is drawn:**

- each id names a resource of its sort that is kept;
- the mesh's layout is the program's vertex layout, and the instances' layout is its instance layout, or the placement is a `Count` for a program that reads none;
- the bindings name each place the program binds, once, and nothing else, each with a resource of the kind bound there: uniforms for a uniform block, at least as many bytes as the block; storage for a storage buffer, at least as many bytes as the program binds there; a texture for a texture; a sampling for a sampler.

The placement's bind groups are made then, once, so a frame binds what is made and builds nothing.

**A placement keeps what it draws.** Releasing a mesh, a program, instances, uniforms, storage, or a texture, or its maker ending, takes back the id, so nothing more is placed with it, but the placements already made keep drawing it until they are released themselves. A placement made by one actor, of a mesh another made, is never dropped because the other ended. Writes to instances, uniforms, and storage still come only from their maker, and stop when it ends.

**Who may change what.** Only the maker of a resource may write it or release it (ADR-0021), and that includes a placement. Its ids are plain data and may be handed on, so others may place with them.

**Until an actor makes a window's main frame, the render root's own draws every placement in one pass onto the canvas at each `Render`** (ADR-0025), with a depth texture of `Depth32Float` cleared to the far plane, as every program is made to test against (ADR-0021). It sets each program's pipeline once, and draws its placements together.

**Draw order within a pass is not promised.** Order across passes is a frame's (ADR-0025). Placements of one program are drawn together, and programs and placements in no order an actor may rely on. Depth orders what is drawn where order would show: a 3D scene, and a 2D game whose layers are depths. Placements whose programs blend by `Multiply`, their colour times what is drawn, keeping the alpha drawn already, give the same frame in any order among themselves, as `Additive` ones do. The records of one placement are drawn in order, as they are one actor's. What depth cannot order, overlapping things that blend, is ordered by putting them in passes of their own (ADR-0025).

**What is drawn can be read back, and a frame timed.** Each is an ask the render root holds and answers once the GPU has done the work, a frame or more later (ADR-0017: `turn.hold`, `turn.answer`):

```rust
ReadBack { of: Readable }  -> Pixels::Read { width: u32, height: u32, format: TextureFormat, pixels: BlobRef } | Pixels::Refused
enum Readable { Frame, Texture(TextureId), Storage(StorageId) }   // storage is answered with its bytes (ADR-0026)
TimeFrame                  -> Timed::Took { passes: Vec<u64> } | Timed::Refused
```

- `Readable::Frame` reads the next frame drawn onto the canvas, as its pass leaves it, and a texture is read as it is when the ask is handled. Pixels are answered row by row from the top, each row packed tight, as a blob (ADR-0014). The render root copies them into a buffer whose rows are padded to the 256 bytes WebGPU copies rows on, and cuts the padding off as it reads them.
- A frame is answered in `Rgba8`, or `Rgba8Srgb` for a canvas that encodes its colours, red first: a canvas laid out blue first, as browsers often prefer, has each pixel's red and blue swapped. A canvas of any other format is refused (`Format`), and so is one the chassis did not open to be copied from (`Uncopied`). A frame larger than the store lets one worker put is refused (`Unstored`).
- `TimeFrame` times the next frame drawn: the nanoseconds from the beginning of each of its passes to its end, in the order it runs them: each pass of each frame that ends on a texture, in the order the frames run, then each of the window's main frame (ADR-0025). The GPU writes a timestamp as each pass begins and ends, which WebGPU offers only on a device with the optional `timestamp-query` feature. The chassis asks for the feature when the adapter offers it. An ask on a device without it is refused at once (`Unsupported`), not when the engine is built, so a program that times nothing never needs it.
- Every ask before one frame is answered from that frame's one copy and one set of timestamps.

**The GPU's work is waited on by the host, never by a turn (ADR-0019).** The render root records the copy in the turn that draws the frame, or that handles the ask for a texture, and submits it. Reading the copy needs its buffer mapped, which the GPU does after that turn: in a browser through a promise, natively once the device is polled. So the render root holds the ask, and pushes the buffer to map onto a controller, and the chassis brings word of each map back through a source:

```rust
RendererConfig { gpu: Given<Gpu>, mapping: Given<Controller<Mapping>>, mapped: Given<Source<Mapped>> }
enum Mapped { Ready { read: u64 }, Failed { read: u64 } }  // brought from the source, the render root to itself
```

- The chassis takes each `Mapping` between steps and starts it. A browser's chassis starts them at the end of each frame, once its turns have submitted their copies, and the page's promise for each map pushes `Mapped` onto the source, which a later frame brings in (ADR-0010: `bring`). A native chassis polls its device between steps without waiting.
- The render root reads the mapped bytes in the turn that handles `Mapped`, unmaps the buffer, and answers every ask the read holds. A map that failed is answered `Unmapped`.
- `Mapped` is read only when it comes from the render root itself, as what its own source brings does. Reading a buffer that is not mapped stops the program, so word of a map from any other actor is logged and left.

**This departs from ADR-0021 in three places,** and ADR-0021 is edited to match:

- `CreateProgram` gains `instances`.
- The render root is made from a controller and a source beside the GPU, for the maps it waits on. Nothing new is added to what a controller or a source carries (ADR-0019).
- ADR-0021 had a program's values come only through its uniforms, with materials made later. A placement's bindings are those values, bound once when it is made: a material, kept with the one placement that uses it. A material that many placements share is still later work.

## Consequences

- A mover sends one mail a frame for all it moves: 10,000 boids are one `WriteInstances`, not 10,000 pieces of mail. A thing that does not move sends nothing after it is placed.
- Nothing about a frame can fail as it is drawn, which ADR-0021 asked of what is made, and now holds for what is drawn.
- A read or a timing is answered a frame or more after it is asked, as the GPU maps its copy after the frame's turns. Nothing blocks on the GPU, in a turn or in the chassis.
- The canvas is opened to be copied from, whether or not anything reads it back. WebGPU lets every canvas be copied from; wgpu names only drawing among a canvas's capabilities, as they are fixed rather than asked of the browser, so the browser's chassis asks for copying without asking wgpu.
- The render root handles each write in a turn of its own, one after another, on one thread. Writes are batched by their writers, so this stays a few turns a frame for most programs; if the render root's turns become a frame's cost, the alternative below is where it goes.
- A resource released while placed is kept on the GPU until its last placement goes: an actor that releases a mesh it still has placed frees no memory until it releases the placements too.
- Layers by depth asks a 2D program to write a depth per instance and test it. Programs that blend, a UI's translucent panels, particles, glass, draw in an order nothing promises until a frame's passes are decided.
- What is decided separately, each when it is needed:
  - **The frame's passes**: decided by ADR-0025.
  - **Materials shared by many placements**, and texture arrays.
  - **Compute programs, and storage they write**: decided by ADR-0026. A range of a mesh's indices per placement.
  - **Spreading uploads over the frames before first use** (ADR-0021).

## Alternatives considered

- **Each actor sends what it draws every frame.** No list is kept, and a frame's mail grows with the scene: the earlier engine's 960 KB a frame for 30,000 draws.
- **A view folded from each actor's draw effects, read by the render root at `Render`** (ADR-0011). The writes would be folded side by side in parts rather than handled as turns of one actor. It would be the first view outside the engine, it needs the interface for one, and with writes batched it saves a few turns a frame.
- **An order key on each placement.** Unrelated actors would have to agree on numbers that nothing checks, order kept by a convention between actors. Order belongs to the frame, decided in one place, with its passes.
- **Releasing a resource drops the placements that use it.** A placement another actor made would vanish without being released, and its maker could not tell.
- **Writes answered.** A write every frame would be answered every frame, by mail its writer ignores.
