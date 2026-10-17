# ADR-0015: The guest host

- **Status:** Proposed
- **Date:** 2026-10-17

## Context

A guest is an actor whose code is a WebAssembly module: a blob the engine holds (ADR-0009, ADR-0014). ADR-0001 gives the engine one seam for the guest runtime, with a native implementation and a browser one, and leaves which each uses to its own ADR. ADR-0009 says a guest's turn is one call, its mail's bytes in and its effects out, and a spawn one call out to its host. ADR-0013 says a guest's module carries its declaration as data and that a false declaration by a guest fails its turn and does not stop the engine.

A spike measured what crossing into a guest costs, with a guest of a few kilobytes written in Rust, on one core of a 16-core desktop:

- With wasmtime, which compiles a module ahead of running it, an empty call took about 18.5 ns. A call that was handed 16 bytes of mail and read them took about 28 ns, and one that also called out to its host once to send them took about 37. A call that ran 16 items one after another cost about 5.4 ns an item.
- With wasmi, an interpreter written in Rust, the same calls took 54, 170, and 118 ns, and 100 ns an item: 3 to 20 times as long as wasmtime wherever the guest did any work.
- Making an instance took 8.5 to 14 µs, whether it had a store of its own or shared one, and whichever allocator wasmtime used. An instance held 4 to 10 KiB of resident memory, as a guest's memory is mapped as its pages are touched. With 10,000 instances, a call spread across them took 143 to 245 ns where a call into one took about 29, as each touched memory of its own.

The engine runs a step's parts on its workers, and a part is run by one worker at a time (ADR-0011). An instance is used by one thread at a time.

## Decision

**The engine runs guests through one seam, and each host implements it with the WebAssembly it has.** The native host uses wasmtime. In a browser the engine is itself a WebAssembly module, and its host makes guest instances through the browser's own `WebAssembly`, so it carries no runtime of its own. Nothing outside the two implementations names a runtime's types (ADR-0001). The engine, the blob store, and `Code::Blob` see only the seam.

**A blob's actors share an instance in each part.** An instance of a blob is made in a part the first time one of its actors is seated there, and every actor of that blob seated in that part lives in it: its state in the instance's memory, beside its siblings'. The instance belongs to its part and moves with it, so whichever worker runs the part runs the instance, and only it. A part with no actor of a blob left drops that blob's instance.

- An actor's spawn into an instance that exists makes no instance: the guest makes room for the actor's state in its own memory.
- An instance's memory has no walls inside it. Actors of one blob in one part trust one another, as one program's parts do: one can write over another's state only through code that is not safe Rust, or code in a language that does not check its writes. The instance's sandbox keeps everything outside it, the engine, other blobs, and native actors, out of its reach.

**A trap ends every actor in its instance.** A trap can leave a guest's memory half changed, its allocator among it, so no actor of that blob in that part runs on it again. Each of them ends as a failed turn ends its actor (ADR-0009): its watchers are shown its ending, and the trap is in its log. The instance is dropped, and the next actor of the blob seated in the part is made a new one. The guest's library turns a Rust panic into a fault it returns, which fails only the turn that panicked, as a native fault does, so a trap is left to what cannot be caught.

**A guest's effects are written into effect buffers in its own memory, and its host reads them where they lie.** A guest's turn fills the same three effect buffers a native turn does (ADR-0009): its mail, its lines, and its effects on the lives of actors. Each entry of a buffer is a record of one fixed width in ADR-0002's layout, and the bytes of mail and the text of lines lie in a byte area of the guest's memory that the records point into. When the call returns, the host reads each record from the guest's memory as it stands, with no decode, and routes it as it routes a native turn's (ADR-0011). A guest cannot see the host's memory, so the host copies what crosses: the bytes of each piece of mail and each line are copied into the worker's arena as they are routed, and from then on they are handled as a native turn's are; and the bytes of each item handed to a guest are copied into an inbox in the guest's memory before the call.

**An actor's items go into its guest in one call, a run of turns.** The host writes as many of an actor's items as its inbox holds and calls the guest's one export for turns once. The guest runs a turn for each, in order, and marks in its buffers where each turn's entries end. It stops after the first turn that fails, and answers with how many turns ran and whether the last failed. The host routes the buffers turn by turn: a failed turn's lines are kept and the rest of it dropped, and the turns after it never ran, as a native actor's later items are not handed to it once a turn fails (ADR-0011).

**What a turn must have answered within the turn is a call out to its host.** A spawn is one, since its child's `begin` runs inside the turn and the parent learns there whether it began (ADR-0009). Reading the engine is the other: resolving a path, and what the table says of an actor. Nothing else is a call out.

**A blob says what it makes in a section of its module.** The guest's library writes a custom section, `aethereal.declared`, when the guest is built. It holds, in plain data (ADR-0004), what `Declared` holds for each actor type the blob makes: its name, its kind's id, its key, its config's kind, what it handles, and what each handler may send (ADR-0013). Each host reads a module's custom sections without making an instance. When a blob that is code enters the store, its host reads the section, and checks that the module exports each function the section promises, with the signature it should have. A blob whose section is missing, does not decode, or promises an export the module lacks is refused there, so a broken blob is found when it is put and not by a trap in a turn. The engine then keeps a `Declared` for each kind the blob makes for as long as it holds the blob.

**A guest's declaration is a claim its host holds it to.** Nothing reads a guest's code to see that it does what its section says. The host hands an actor only items its declaration lists, and checks each send when it reads the turn's mail, against the declared sends of the handler that ran, as a native send is checked (ADR-0013). A send like the one before it, from the same declared send, is checked once, as natively. A guest that breaks its declaration fails its turn with a fault and does not stop the engine (ADR-0013).

## Consequences

- A guest's turn costs about what a native one does to cross into: a call is in the tens of nanoseconds, and an item in a run a few. What a guest does in its turn runs at about the speed wasmtime compiles it to.
- Ten thousand actors of one blob in one part are one instance, made once. A guest actor's spawn is the guest's own allocation, and a call reaches its state in memory its siblings keep warm.
- One instance's actors are as isolated from one another as one native actor's fields are. A blob that is not written in safe Rust puts its siblings in a part at the mercy of its own writes, and a trap in it ends all of them.
- A guest's mail costs one copy each way: into the guest's inbox, and out into the arena. If those copies come to matter, the guest's byte area can be kept in two halves by parity of step, as an arena is (ADR-0011), and a payload could then name bytes in a guest's memory, with no change to what a guest writes.
- A host that hands a run to a guest must hand it whole or say where it stopped, so the slice of ADR-0011, which cuts an actor between turns, cuts a guest between calls.
- The browser host compiles a code blob when it is put, which a browser does without blocking only in the background, so a code blob is ready before any actor of it is seated. Whether a browser lets it make an instance from a compiled module without waiting where the engine runs, and what crossing between the engine's module and a guest's costs there, are to be measured before it enters.
- **Not decided here:** how a guest's library keeps its parent's state untouched while a child of the same blob begins in the same instance, inside the parent's turn; how mail larger than a guest's inbox is handed in; how the engine takes a guest actor's state out, to replace its code or to save its siblings from a trap; and what a guest's effects record of the blobs it pins (ADR-0014).

## Alternatives considered

- **wasmi on native.** Small, written in Rust, and it runs where nothing may be compiled at run time. Three to twenty times as long as wasmtime wherever a guest does work, and 70 to 460 µs to make an instance. It can be a second native implementation of the seam if a target forbids compiling.
- **Both wasmtime and wasmi behind the seam from the start.** Two native implementations to keep in step before anything needs the second.
- **An instance for each actor.** Each actor walled off from its siblings, and a trap ends only its own. Each spawn makes an instance, about 9 µs, and calls spread across thousands of instances cost 2 to 8 times as much, as each reaches memory of its own. wasmtime's memory protection keys pack many instances' memories closer and keep them apart; they were not measured, on a processor without them. It can be offered later for blobs the engine does not trust.
- **An instance for each blob on each worker.** An instance is never shared between threads. A part moves between workers and an actor's state would be in the instance of the worker it last ran on, so either an actor is kept to a worker or its state is copied between instances when its part moves.
- **A blob's actors seated in a fixed set of parts.** Fewer instances. Placement would follow blobs where the plan and the seats follow the work (ADR-0011, ADR-0013).
- **A trap that ends only the actor whose turn trapped.** Its siblings go on. They go on in memory the trap may have left half changed.
- **A call out to the host for each send, checked as it is made.** A send is refused at the moment it is written. Every send crosses the boundary on its own, about 9 ns each in the spike, and a guest's turn is no longer one call.
- **Memory a guest and its host share**, so neither copies. Every guest then needs atomics, and a browser serves such memory only to pages it isolates.
- **An export the host calls to learn what a blob makes.** Simpler to write. The engine would run a blob's code before it had checked anything about it.
- **A manifest kept beside the blob.** Nothing ties it to the code, and the blob's hash would not cover what the engine trusts.
