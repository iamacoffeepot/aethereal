# ADR-0001: Pieces enter in their final shape, and every piece builds for two hosts

- **Status:** Proposed
- **Date:** 2026-10-10

## Context

This repository succeeds an earlier engine repository, which stays as a lab and archive. That engine grew to about 400k lines of Rust while its design was still being learned, and much of its core carries the shape of that learning. Two things follow from reading it.

The first is that porting the core and then rebuilding it costs the rebuild twice. The data layer, the math, and the codec are close to final; the runtime, the actor SDK, and the component host are not, and everything above them is written against their current surface.

The second is that the earlier engine cannot be hosted in a browser without reaching into most of its core. Four assumptions block it, each spread across many files:

- The guest runtime is named directly. The runtime's engine and linker types appear in the parameters of the code that hosts a component, and each host function reads guest memory through that runtime's caller type.
- The scheduler is a work-stealing thread pool. Workers spin and park, blocking work runs on offload threads, and a caller waits for a mail chain to settle by blocking.
- `target_arch = "wasm32"` means "this code is a guest". The gates enable the guest's FFI shims and disable the host's registries. In a browser the host itself is wasm32, so the same gates would turn the host into a guest.
- The core reads the clock and blocks on futures: `Instant::now()` across the runtime, and `block_on` around GPU adapter and device requests.

None of these is hard to avoid in code that is written with a browser host in mind. All of them are expensive to remove afterwards.

## Decision

**Entry.** The repository starts empty. A piece enters lowest layer first, and only in its final shape: the shape it is expected to keep, with its reasons recorded. Code is not carried in to be reworked here. A piece taken from the earlier repository is re-read against these rules and enters as a new change, not as a copy.

A piece is one concept that the pieces already present are enough to build and test. Each piece states, in the pull request that adds it:

1. what it is and which pieces it stands on;
2. the ADR that records its design, when the design is load-bearing;
3. that it meets the host rules below.

Reasons live in ADR text in this repository. A reference to an issue or decision record in another repository is not a reason here.

**Two hosts.** The engine has a native host and a browser host from the first piece. Every crate builds for `wasm32-unknown-unknown` as a host unless it is a native host crate that says so, and CI checks it. Four rules keep that true:

1. **Guest is not an architecture.** Code that exists only inside a guest is selected by a cargo feature or a custom cfg, never by `target_arch = "wasm32"`. `target_arch` is used only for what it means: a difference in the machine.
2. **One seam for the guest runtime.** The engine runs guests through one interface. A native host implements it with a wasm runtime; a browser host implements it another way. Nothing outside the implementations names a runtime's types.
3. **Pumped, single-threaded, non-blocking.** The scheduler has a mode in which one thread runs it by calling a pump, and that mode is complete: nothing in the engine blocks a thread, sleeps, or waits on another thread to make progress. Threads are an optimisation a native host adds on top of the same scheduler.
4. **Time and devices are handed in.** The engine does not read a clock or drive a future to completion. The host passes time in, and passes in devices it has already set up (a GPU device, an audio output), however it had to wait for them.

Rules 3 and 4 cover calls that compile for wasm32 and fail only when run there (`Instant::now`, `thread::spawn`, `thread::sleep`), so the build alone does not catch them. `clippy.toml` lists them as `disallowed-methods`; a host crate that owns the real clock or real threads allows the call with a stated reason.

## Consequences

- The first pieces are small and have no runtime: the data layer, then what stands directly on it. An engine that runs anything arrives later than a port would deliver it.
- The core is designed here. Open design questions about the core are settled as ADRs before the piece they govern enters, since "final shape" leaves no room to settle them afterwards.
- A capability that needs a thread, a socket, or a file on native needs a stated browser answer before it enters: an alternative implementation behind the same mail surface, or a declaration that it is a native-only host crate.
- The engine's control surface cannot assume the engine listens on a socket. A browser engine dials out.
- Which implementation each host uses behind the guest-runtime seam is ADR-0015's: wasmtime on native, and the browser's own `WebAssembly` in a browser.
- The lab repository keeps moving. A fix made there does not arrive here by itself.

## Alternatives considered

- **Port the earlier engine, then rework it in place.** Fastest to a running engine, and the rework then happens under the weight of everything already ported against the old surface.
- **Add a browser host to the earlier engine.** The four assumptions above are spread through its core; removing them there is the same rewrite without the clean entry rule.
- **Native first, browser later.** This is how the earlier engine came to assume threads, a clock, and one runtime. Each assumption was reasonable when made.
