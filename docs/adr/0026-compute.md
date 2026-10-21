# ADR-0026: Compute

- **Status:** Proposed
- **Date:** 2026-10-21

## Context

A frame is an ordered list of passes that draw (ADR-0025: `CreateFrame`, `Does::Draw`, `Does::Effect`), and the storage they read is written only by its maker, from the CPU (ADR-0023: `WriteStorage`, `BindingKind::Storage`). A field of particles moved by its own rules, a cloth, a crowd, the culling of what is not seen, or a histogram of a frame's light, is written fastest by the GPU itself, many invocations of one program at once, each writing its part of a buffer.

What the earlier engine learned (its ADR-0170, ADR-0246: `PassStage::Compute`):

- **A compute pass costs about what a draw pass does.** It measured a compute pass and the indirect draw it fed at about 90 µs, against about 35 µs for a draw pass.
- **A grid fixed when a program was made was too rigid.** A compute pass's workgroups were fixed at registration, so what a pass covered changed only with a new program, and values that changed every frame rode the uniforms.
- **Compute and the draws it feeds belong in one frame.** It made a compute pass a stage of a program beside its draws, so a draw could read in the same frame what compute wrote.

WebGPU runs a compute pass's dispatches one after another, with what one wrote visible to the next, and orders passes as they are submitted. It never binds a storage buffer for writing in a vertex stage.

## Decision

**A compute pass is a kind of pass in a frame, and actors join it with dispatches, as placements join a draw pass.**

```rust
struct Simulate;  impl Pass for Simulate {}

let mut frame = FrameBuilder::on(Ends::Window(window));
frame.compute(Simulate);                                    // a pass with no target: it writes storage
let (scene, depth) = frame.draw(Surfaces, Float, depth::Write);
let create = frame.finish(Glaze, Effect::new(&glaze, (scene,)))?;
```

- `Does` gains `Compute`: a pass that runs every dispatch that joins it, and draws into nothing. A frame's last pass still writes its end, so a compute pass is never last.
- **Order is the frame's pass order.** A draw in a pass after a compute pass reads what it wrote this `Render`; one in a pass before it reads what it wrote at the last.
- **The render root's own main frame is two passes:** `Compute`, then `Main`, each a pass the render root names (ADR-0025: `Main`). An actor that simulates joins `Compute` and draws what it wrote in `Main`, without the main frame's owner doing anything for it. **This departs from ADR-0025, whose render root's own main frame was the one pass `Main`**, and ADR-0025 is edited to match.

**A compute program is made by asking for its WGSL, its entry point, and what it binds,** as a program is (ADR-0021):

```rust
CreateCompute { wgsl: String, entry: String, bindings: Vec<Binding> }  -> Computed::Made(ComputeId) | Computed::Refused(ProgramRefused)
```

- Its workgroup size is read from the shader's `@workgroup_size`, which must be constant.
- It is checked as a program is: its WGSL valid, `entry` a compute stage, each binding it uses listed, each listed one bound as the shader declares it, and no more than the device allows: invocations in a workgroup, workgroup memory, and storage buffers a stage binds.
- It is released as everything the render root makes is, and kept by the dispatches made with it until they are released (ADR-0023: a placement keeps what it draws).

**Storage is bound for reading or for writing.** `BindingKind::Storage` gains an access:

```rust
BindingKind::Storage { bytes: u32, access: Access }
enum Access { Read, Write }   // var<storage, read>, var<storage, read_write>
```

- A compute program binds storage either way. A draw program or an effect binds it only to read, and one whose shader writes storage is refused, as ADR-0023 refused it.
- A binding listed `Write` that the shader only reads is refused, as is one listed `Read` that it writes: what a program says it writes is what it writes.

**A dispatch is one compute program, run in one compute pass, with what it binds, over a count of invocations:**

```rust
CreateDispatch {
    join: Dispatch,           // Dispatch::to::<Simulate>(frame, compute, bindings): the pass by its type's id
    size: Invocations,
}                                                                  -> Dispatched::Made(DispatchId) | Dispatched::Refused(DispatchRefused)
enum Invocations {
    PerElement(StorageId),    // one invocation for each element of the array a bound storage ends in
    Workgroups([u32; 3]),     // a grid of workgroups, each of the shader's size
}
```

- **`PerElement` runs one invocation for each element of storage the dispatch binds,** in its first dimension: the storage's bytes past the fixed part its binding reads, divided by the stride of the array it ends in (ADR-0023: `array<T>`, read with `arrayLength`). The render root works out the workgroups from the shader's size, and the last workgroup's invocations past the end read `arrayLength` and stop. A field of particles made at its capacity is dispatched for its capacity, and a live count that changes every frame rides a uniform the shader tests, as a field of splats is drawn for a count of instances.
- `Workgroups` is a grid for work that is not one invocation per element: a frame's tiles, a grid of cells.
- A grid past what the device dispatches in one dimension, `max_compute_workgroups_per_dimension`, is refused (`Size`), and a `PerElement` whose storage the dispatch does not bind, or binds at a binding whose shader type ends in no array, is refused (`Elements`). **This departs from refusing both as `Size`**: a grid too large carries the grid and the device's most, and storage with no elements to count has neither.
- The fixed part and the stride are read from the compute program's shader when it is made, for each storage buffer it uses whose type ends in an array whose length is its buffer's.
- A grid of workgroups the GPU wrote is read from storage, `Invocations::Indirect`, as a draw's count is (**Counts a storage holds**, below).

**A dispatch is checked when it is made, as a placement is (ADR-0023), and nothing when it runs:** each id names a resource of its sort that is kept, the pass is a compute pass of the frame, and the bindings name each place the compute program binds, once, each with a resource of its kind and size. Storage bound where the program writes is bound nowhere else in the dispatch (`Aliased`), as WebGPU refuses one buffer bound to write and bound again in one dispatch. Its bind groups are made then, once. A dispatch joins a window's main frame by the window, and follows it when the frame is replaced, as a placement does (ADR-0025: `Frame::Main`); one whose pass the new frame has not is not run, and is logged, until a frame has that pass again. A dispatch does not change once made, and only its maker releases it.

**Only a storage's maker binds it for writing.** A dispatch that binds another actor's storage to write is refused (`NotOwner`), as only a maker writes its storage from the CPU (ADR-0023). Anyone handed its id may bind it to read. A storage another actor writes is later work, decided when it is needed.

**A storage may have several writers on the GPU, when passes or frames order them.** Two dispatches that write one storage in one pass would leave it as whichever the render root ran last, and dispatches of one pass run in no order an actor may rely on, so the second is refused (`Written`). For the same reason a dispatch that reads a storage another dispatch of its pass writes, or writes one another of its pass reads, is refused (`Written`): it would read the storage before or after it was written, as the render root ran them. **This departs from refusing only two writers**, which left a reader and a writer of one pass to the order they ran in. Two that write it in different passes of one frame are ordered by the frame's passes, and two in different frames by the frames' order, so both are made. Two in frames that neither reads the other, whose order is nobody's, are refused (`Written`). A window's main frame runs after every frame that ends on a texture (ADR-0025), so a writer in each is ordered. Writers made while a placement ordered their frames stay made when that placement is released, but neither is run, and each is logged, until their frames are ordered again: run in the order of the frames' ids, the storage would hang on which frame was made first.

**Frames are ordered by the storage they write and read, as by the textures they end on** (ADR-0025: `JoinRefused::Round`). A frame that writes a storage, by a dispatch that joins it, runs before a frame that reads it, by a placement, an effect, or a dispatch that binds it to read. A dispatch or placement that would have frames read each other round is refused (`Round`), and the order is worked out again whenever a frame, a placement, or a dispatch comes or goes. **A frame that ends on a texture reads storage a window's main frame writes as it was written at the last `Render`**, as every such frame runs before the main frame (ADR-0025), so that read orders nothing and is never round; this record first left it unsaid.

**A storage's maker writes it from the CPU as well.** A `WriteStorage` handled before a `Render` lands before that `Render`'s frames run, so an actor seeds a field from the CPU and moves it on the GPU, and what the GPU writes later overwrites it.

**Dispatches of one pass run in no order an actor may rely on.** The render root runs them grouped by program, each program's pipeline set once, as it draws a pass's placements (ADR-0025). A dispatch that needs another's output is in a later pass. **Order is declared where drawing is described, and nowhere else**: the frame's passes, which their author writes, and, within one dispatch, the order its own shader gives.

**Storage can be read back,** as a texture is (ADR-0023: `ReadBack`), by an ask of its own:

```rust
ReadStorage { storage: StorageId }  -> Bytes::Read { bytes: BlobRef } | Bytes::Refused(ReadRefused)
```

- It is read as it is when the ask is handled, after every `Render` before it, and answered with its bytes, held and answered as a texture's pixels are. Any actor handed the storage may read it. Storage not kept is refused (`Storage`), and storage larger than the store lets one worker put (`Unstored`).
- **It is an ask of its own, where this record first gave `ReadBack` a `Readable::Storage`.** An ask is answered with one kind (ADR-0017: `Carries`, `Answers = (A,)`), so a `ReadBack` answered with pixels or bytes could not be asked, and every actor that reads a frame back would handle bytes. ADR-0023 is edited to match.

**Counts a storage holds.** A draw's counts or a dispatch's workgroups may be read from storage at each frame, written by compute or by the storage's maker, so what the GPU culled or spawned is drawn without the CPU learning how many. It is decided here, with the storage compute writes, rather than in a record of its own:

```rust
enum Drawn {
    Count(u32),
    Records(InstancesId),
    Indirect { storage: StorageId, at: u32 },   // five u32 words from the byte `at`
}
enum Invocations {
    PerElement(StorageId),
    Workgroups([u32; 3]),
    Indirect { storage: StorageId, at: u32 },   // three u32 words from the byte `at`
}
```

- A draw's five words are its index count, instance count, first index, base vertex, and first instance, as WebGPU's `drawIndexedIndirect` reads them: a mesh is always drawn by its indices (ADR-0021). A dispatch's three are its workgroups along each dimension, as `dispatchWorkgroupsIndirect` reads them.
- Every storage buffer is made to be read as arguments as well, so any storage may hold them. Anyone handed a storage may draw or dispatch by it: reading arguments is a read, not a write.
- They are checked when the placement or the dispatch is made: the storage is kept (`Storage`), and the words lie inside its bytes from a whole word: `at` a multiple of four, and `at` and 20 bytes for a draw, or 12 for a dispatch, at most its bytes (`Arguments { at, bytes }`, with what was asked and the storage's bytes).
- A placement drawn `Indirect` reads nothing per instance, as one drawn for a `Count` does, and its program is checked as for a count (`Drawn`). Its shader reads what each instance needs from storage by `@builtin(instance_index)`, as a field of splats is read (ADR-0023).
- **What the words hold is checked on the GPU, not when the draw is made.** WebGPU checks them as it draws: a draw whose indices or instances pass what is bound, or a dispatch past the most workgroups, is skipped. A draw whose first instance is not zero is skipped too, without the optional `indirect-first-instance` feature, which the chassis does not ask for. **This departs from checking everything when it is made** (ADR-0023), as the words are written on the GPU after it is.
- **Reading arguments is a read of the storage** in the placement's or the dispatch's frame and pass. It joins the order of frames, the same-pass `Written` rule, and `Round`, as storage bound to read does. A dispatch that reads its workgroups from storage it binds to write is refused (`Aliased`), as WebGPU refuses one buffer used both ways in one dispatch.
- A placement or a dispatch keeps the storage it reads its arguments from, as it keeps what it binds (ADR-0023: a placement keeps what it draws).
- At `Render` the render root draws with `draw_indexed_indirect` and dispatches with `dispatch_workgroups_indirect`, from the storage's buffer at `at`.
- A count compute adds to is reset by its writer each frame: the storage's maker writes it with `WriteStorage` at each `Tick`, or a dispatch in an earlier compute pass sets it.

**`TimeFrame` times a compute pass as it times a draw pass,** its nanoseconds from beginning to end, in the order of the frame's passes.

## Consequences

- A particle system is one storage, one compute program, one dispatch, one draw program, and one placement, each made once. Nothing is sent each frame but the uniforms that change.
- A dispatch made by one actor runs in a frame another owns. The frame's owner decides where compute runs among its passes; an actor that dispatches decides what runs there.
- A frame replaced by one that keeps its compute passes keeps its dispatches running, and nothing compiles again.
- The render root handles two more kinds, `CreateCompute` and `CreateDispatch`, within an actor's 64 (ADR-0013: `HandledList`).
- What is decided separately, each when it is needed:
  - **Compute that reads a frame's targets**, a compute pass with `reads`, as an effect reads: culling against the depth of an earlier pass, post-processing by compute.
  - **Compute that writes textures**, storage textures.
  - **Many draws from one storage, and a count of draws the GPU writes**: multi-draw indirect, which WebGPU does not have (ADR-0021), and how many draws a storage holds.
  - **Storage another actor writes.**
  - **Workgroup memory and subgroups beyond what WGSL gives every device.**

## Alternatives considered

- **Dispatches outside frames, all run before them, ordered by what they read and write.** Simpler, and frames would not change, but no compute could run after a draw of the same `Render`, which culling against depth and compute post-processing need, and ordering by data would be a second order beside the frame's passes.
- **A compute pass as a stage of a program, with its grid fixed when the program is made**, as the earlier engine did. Its grid changed only with a new program, and an actor could not add compute to a frame another owns.
- **One writer for each storage, whatever orders them.** Deterministic, but it refuses writers that passes already order, such as two passes of one simulation that each write the field.
- **Writers of one storage in one pass, run in the order they were made.** Order would hang on when mail came, which ADR-0025 refused for draws.
- **Any actor handed a storage may write it.** Who writes what would be no single actor's, and nothing yet needs it.
- **A typed `Arguments` resource for a draw's or a dispatch's counts**, a second kind of resource for words in a buffer. Compute would have to bind it as storage to write it anyway, so it would be storage under another name; it can be added on top of storage later.
