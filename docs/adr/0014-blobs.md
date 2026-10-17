# ADR-0014: Blobs

- **Status:** Proposed
- **Date:** 2026-10-17

## Context

Mail is small. Its bytes are written into the arena of the worker that sent it, one slab for each parity of step, out of a quota the host shares among the writers (ADR-0011, `Arena`, `Payload`), and they are read in the step after and never again. That suits a command or an answer. It does not suit what an engine moves most by weight: a texture, a mesh's vertex data, a file read off a disk, a page read from an indexed database, a guest's code. Such a thing is large, it is often read by many actors, and it outlives the step it was made in. Copying it into every piece of mail that names it would write it again for every receiver, and a receiver that keeps it would have to copy it out again before its arena is emptied.

ADR-0009 already names one case: a guest's code is a blob, bytes the engine holds, named by their hash, and never changed (`Code`). It leaves open large payloads, as a further effect, and when the engine lets go of a blob.

A resource is not the answer (ADR-0010). A resource has one owner because a value two actors could both use would need a lock once turns run side by side, and nothing would say when to drop it. A large buffer that is never changed has neither problem: any number of readers on any number of workers can read it at once, and the last reader to let it go says when it goes.

## Decision

**A blob is a buffer of bytes the engine holds and nobody changes.** Any actor may put one. Whoever holds a reference to it may read it. Guest code is one kind of blob, and so is anything too large for mail.

```rust
pub struct BlobRef { .. }                       // a pin on one blob

impl Turn<'_> {
    pub fn put(&mut self, bytes: Vec<u8>) -> Result<BlobRef, Refused>;
    pub fn read(&self, blob: &BlobRef) -> &[u8];
}
```

**A blob lives while something references it, and the references are its pins.** A `BlobRef` is `Clone`, and each clone is one more pin. Dropping a reference takes its pin away. When a blob's last pin is gone, the blob is freed. Nothing else frees a blob, and nothing keeps one alive but a reference. A blob holds only bytes, never a reference of its own, so pins cannot form a cycle.

**Pins are counted as the step folds, not as turns run.** Each thread of the engine, every worker and the thread that steps it, keeps a buffer of pin changes of its own: a clone adds one, and a drop takes one away. Neither touches a count shared with another thread. When the step's turns are done, the changes are folded into the pins, a view kept under each blob (ADR-0011). Many actors change one blob's pins, and a sum is the same in whatever order its records arrive, so this is a key with many writers that the fold makes whole. A blob is freed at the fold that leaves it with no pin, so a blob dropped and referenced again within one step is never freed in between, and the step a blob is freed in is the same in every run.

- A turn that fails still hands in its pin changes. The values it held were dropped whether or not its effects were applied, as its lines are kept (ADR-0009).
- A reference dropped on a thread with no buffer of its own, such as the chassis's between steps, goes to a buffer kept for its engine, folded with the rest. A process may hold several engines, each numbered as it is made, and a blob's id names the engine that holds it as well as the blob, so one engine never sums another's changes, and a thread's buffer takes only the changes of the engine it runs a step for.

**A blob is read through the turn.** `turn.read` borrows the bytes from the engine's store for as long as the turn has them. The store is a view, and no view changes while a step's turns run (ADR-0011), so every reader in a step sees the same bytes without a lock.

**A put is made in its worker, and the store takes it in at the fold.** A put cannot add to the store while turns are reading it. A blob put in a turn goes into a list its worker keeps, and its reference names it there. The store takes in each worker's new blobs when the step folds. Mail is handled in a step after the one it was sent in, so whoever is mailed a reference finds the blob in the store. A hop fused in the same step runs on the same worker (ADR-0013), and a read looks in that worker's list before the store.

**A blob has a content hash only when something needs one.** Its hash is BLAKE3 over its bytes, computed the first time it is asked for and kept with the blob. A blob's code needs it, since a kind's head names the blob it points at (ADR-0009). So does a blob that crosses to another engine, and a put that asks for the same bytes to be one blob. A texture remade every frame is never hashed.

**A reference in mail moves its pin with it.** A kind may hold a `BlobRef`: it crosses `Actors` and `Within`, so it may be in mail, in a config, and in an actor's state, and not in anything written to a file or sent to another engine. A send encodes the kind from a borrow, so a reference in it is cloned, and that clone's pin is the mail's. The mail's bytes own nothing (ADR-0011), so its pins are kept in a list beside its writer's arena, each at where its reference begins in the arena's bytes. The receiver takes a pin of its own when it reads the reference out of its mail, and the mail's pins are dropped when its arena is emptied, whether or not the mail was delivered. Mail carried over at a slice (ADR-0011) carries pins of its own along with its bytes, and a hop fused in its step and carried into the next pins its blobs from the arena it was written in.

**The bytes of blobs come out of a budget the host gives.** The host gives the engine a budget for the bytes of all its blobs, and each step's plan gives each worker a share of what the budget has left as the step begins, as it shares the quota for mail. A put that would take its worker past its share is refused, and the turn that asked handles the refusal, as it handles a spawn refused (ADR-0009). The bytes in use change only at folds, so whether a put is refused depends on the step's turns and not on how other workers' turns happened to interleave. A blob freed in a step returns its bytes at that step's fold.

**The chassis may put blobs between steps, as it makes roots.** It puts the code a root of a guest is made from before that root, and anything a root is handed at birth in its config. Each put returns the reference the chassis hands on.

**A guest holds its references through its host.** A guest cannot reach the engine's store, so its host keeps a table of the pins each of the guest's actors holds, and the actor holds an index into it. A guest drops a reference by saying so in the effects its turn returns. When an actor ends, its host drops whatever pins its table still holds, so a guest that forgets a reference leaks nothing past its own end. A guest reads a blob by having a range of it copied into its own memory.

## Consequences

- Large things move between actors as a reference in a small piece of mail. A thousand actors handed one texture hold a thousand pins and one copy of its bytes.
- Pins cost no atomic operation in a turn: a clone and a drop are each a push onto a buffer the thread owns. A blob sent to many actors in one step costs that many pushes and one sum at the fold, and each reference that travels is two pins more: the mail's, and the receiver's.
- Mail that refers to no blob keeps no pins: a write files pins only when it made one.
- A blob is freed a step after its last reference is dropped, at the latest. Memory held by blobs is returned at folds, never in the middle of a step.
- A turn cannot know that it holds a blob's only pin, since the counts are folded. So a blob is never changed in place, even by the one actor that holds it. An actor that remakes a buffer every frame puts a new blob every frame.
- The bytes a turn puts are allocated on its worker and freed at the fold, which may run on another. ADR-0011 frees memory on the thread that made it. A freed blob's buffer is handed back to the worker that made it, which frees it or keeps it to reuse for a put of a like size. `put` takes a `Vec<u8>` so that a buffer the engine handed out can be the one put.
- A reference reaches a blob's bytes only through a turn, so a helper that reads a blob is handed what it reads, or a view the turn lends it, as a helper that logs is lent a `Logger` (ADR-0009).
- A guest that only passes a reference on never copies the blob's bytes.
- **Not decided here:** what a guest's code blob declares, and how a host runs it, which is the guest host's ADR; how a blob crosses to another engine; a budget for each actor or each part of the tree, beside the engine's; how an actor learns how much of the budget is left; and how a resource hands what it reads in as a blob, such as a file's contents (ADR-0010).

## Alternatives considered

- **Counts kept with atomic operations, as `Arc` keeps them.** No buffers and no fold. A blob is freed the moment its last reference goes, a reference reaches its bytes with no turn, any thread may drop one, and the one actor that holds a blob may change it in place. A blob sent to many actors at once makes every worker's clones contend for one count, and when a blob is freed, and so whether a put is refused, would depend on how the workers' turns interleaved, where ADR-0011 fixes what a step leaves so the same turns always leave the same result.
- **A count shared live across workers for the budget.** A put sees exactly what is in use. Whether it is refused then depends on what other workers put in the same instant.
- **Every blob hashed when it is put.** One name for every blob, and two puts of the same bytes are one blob. Every put reads every byte: measured on one core, BLAKE3 hashed about 4.5 GB a second, 0.22 ms a mebibyte, which a buffer remade every frame pays for a name it never uses.
- **A receiver that takes the mail's pin over, with no pin added.** Every reference that travels is counted once. A pin taken out of an arena's list must be marked as taken, so the arena does not drop it when it is emptied, and the mark has to reach the writer's list from the receiver's worker.
- **No budget.** Nothing refuses a put, and a process runs out of memory where an engine could have said no.
- **Blobs that last a fixed number of steps.** Cheap for what is remade each frame. Two kinds of lifetime, and a reference held past its steps names freed bytes.
- **A blob with one owner, moved as a resource is.** No counting. A buffer many actors read must then be copied for each.
- **Memory a guest and its host share.** A guest reads a blob with no copy. Every guest then needs atomics, and a browser serves such memory only to pages it isolates.
