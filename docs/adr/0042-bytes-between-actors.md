# ADR-0042: Bytes between actors

- **Status:** Proposed
- **Date:** 2026-11-02

## Context

Bytes cross between actors as `Vec<u8>` fields of kinds. A send encodes the field into the worker's arena, which is reused every step (ADR-0011), and the receiver decodes it out again into a `Vec` of its own: ADR-0004 decided that "a value decoded from a large input is copied out of it". TCP writes went the same way on purpose: "the bytes are inline, `Vec<u8>`, not a blob", left for later "when copying is measured to cost something" (ADR-0033).

It has been measured. A `read_file` of a 250 KB file through the hub takes 1.15 ms at p50 (release, Linux, the `mcp_server` example over loopback). An allocator that logged every allocation of 64 KiB or more found nine buffers of 256 KB to 1 MB made and freed in each call, about 4.2 MB in all:

| Actor | Buffer | Why |
|---|---|---|
| Files' `Load` job | the file read | I/O |
| `ReadTool` | the text, as a `String` | built |
| `ReadTool` | `Called::Output`, the output encoded | built |
| `Mcp` | `Called::Output` decoded | copied out of mail |
| `Mcp` | `to_json`'s value | built |
| `Mcp` | the reply body | built |
| The exchange | `Response.body` decoded | copied out of mail |
| The exchange | the head and body joined (`written`) | built |
| `Tcp` | `Write.bytes` decoded | copied out of mail |

The copies themselves are cheap: an encode of 535 KB into the arena takes 17 to 20 µs, because the arena's memory is in use already. A new buffer is not. glibc takes every block of 128 KB or more from the kernel and returns it when it is freed, so each call faults all of these in again: 291 page faults a call, about half its time. With glibc told to keep freed memory, the same call takes 0.66 ms at 2 faults a call, and with mimalloc 0.69 ms at none.

Allocator tuning fixes this one machine's allocator, and only by keeping memory nobody owns. The owner asked for the engine to own it instead: a place that is allocated once and reused, which agents writing actors cannot get wrong because they never have to think about it, and which covers every transient buffer, including ones not yet written.

Most of the machinery exists. A blob is bytes the engine holds and nobody changes, reached by a reference that crosses between actors as a pin, and pins are summed at the fold with no atomic operation (ADR-0014: `BlobRef`, `pins::change`, `Blobs::sweep`). ADR-0004 named this as the answer for large payloads: "a handle that crosses actors without a copy". ADR-0014 also already says that a freed blob's buffer "is handed back to the worker that made it, which frees it or keeps it to reuse for a put of a like size", and that `put` takes a `Vec<u8>` so that a buffer the engine handed out can be the one put. That reuse was never built, and nothing makes a kind use a blob rather than a `Vec<u8>`.

## Decision

**Bytes that cross between actors are a blob, and its reference is named `Bytes`.** `BlobRef` is renamed `Bytes`. A blob is still the bytes the engine holds (ADR-0014); `Bytes` is what a kind holds of them.

```rust
#[kind(name = "aether.http.response")]
pub struct Response { pub status: Status, pub headers: Vec<Header>, pub body: Bytes }
```

**A byte vector does not cross `Actors`.** `Vec<u8>` crosses `Bare` and `Within`, so it may be in an actor's state, a setting read from argv, and a frame written to another engine, and it may not be in mail or a config (ADR-0004: a door states the boundary). A sequence's element type states the boundaries it is an element at, and `u8` states every one but `Actors`; so a kind with a `Vec<u8>` field has no codec for `Actors`, and a send, an answer, or a config of it does not compile. The error names `Bytes`. A fixed array, `[u8; N]`, still crosses, and so does `String`.

**Each actor keeps the buffers its blobs were made in, and the engine hands them back.**

```rust
impl Turn<'_> {
    /// A buffer with room for at least `room` bytes: the smallest this actor keeps that is big enough and at most
    /// twice `room`, else a new one.
    pub fn buffer(&mut self, room: usize) -> Vec<u8>;
    pub fn put(&mut self, bytes: Vec<u8>) -> Result<Bytes, Refused>;   // as ADR-0014, under its new name
    pub fn read(&self, bytes: &Bytes) -> &[u8];                         // as ADR-0014
}
```

- **The engine keeps an actor's buffers beside it,** as it keeps its resources and the requests it holds. A turn takes from them on the worker running the actor's part, and the fold's sweep hands buffers back on the thread that steps the engine, while it holds every part; so only one thread reaches them at a time, and nothing in them is locked or atomic.
- **A put notes the actor that made it.** When the fold frees a blob, its buffer goes back to that actor, emptied, instead of to the allocator, and the actor's next `turn.buffer` can hand it out again. A kept buffer is handed out only when it is at most twice the room asked, since the budget charges a buffer by its capacity and a small put should not take a large one. A buffer is allocated only when the actor keeps none that fits, and freed only when keeping it would take the actor past `most_kept_bytes`, or when the actor ends. This replaces ADR-0014's handing back to the worker.
- **`most_kept_bytes`** is an engine setting, 16 MiB by default: what each actor may keep. An actor that puts nothing keeps nothing.
- **A job's puts are made by the actor that ran the job,** so their buffers go back to it. A job type says whether it fills buffers, `const FILLS: Fills = Fills::Buffers`, and fills nothing unless it says so. One that fills buffers is lent its actor's largest kept buffer in the turn that runs it, and takes it with `job.buffer(room)` by the rule `turn.buffer` keeps; one it does not take goes back to its actor with its answer, or as it is dropped when it is cancelled before it is handed out. Files' `Load` reads into one: only the storage learns the file's size, so the job hands it what it was lent, and the storage takes a buffer from it by the same rule and gives back the rest. A guest's job is lent nothing yet: the turn that files it knows only its kind, and the type it runs with is found as it is handed out.
- **A guest puts by handing its bytes to its host** (ADR-0014, ADR-0015). The host copies them into a buffer it takes from the actor's, so a guest's buffers are reused as a native actor's are. A guest still reads by a copy into its own memory.

**Reading stays a turn's.** `turn.read(&bytes)` borrows from the store, a view no turn changes (ADR-0014). A helper that reads is handed the slice. `to_json` takes the bytes it reads, and the link writes a `Bytes` into a frame by reading it in its turn: a frame crosses to another engine at `Bare`, so its bytes are copied there, as a frame's are now. A turn that hands blob bytes to a resource it holds, as a connection writes them to its socket, reaches the resource with `turn.resource(&handle)`, a shared borrow of the turn as `turn.read` is, so the two are held together; `turn.open` borrows the turn whole and cannot be. A socket is written through a shared reference (`Write for &TcpStream`), so the bytes go from the store to the socket with no copy.

**TCP and HTTP move bytes as blobs** (ADR-0033: `Write`, `Frame`):

```rust
pub struct Write { pub parts: Vec<Bytes> }    // written with one gathered write, in order
pub struct Frame { pub bytes: Bytes }         // one frame read, in a buffer the connection kept
```

- **A write is a list of parts,** so the exchange writes a response's head and its body as two, and nothing joins them. The connection writes them with `writev`, or in turn where the host has none.
- **A connection reads into its own buffers,** and puts each frame as a blob. A read takes a buffer from the connection rather than allocating one for each read.
- **HTTP's `Request` and `Response` bodies are `Bytes`.** The exchange passes a handler's body to the connection without reading it.

**The kinds that change** are every kind that crosses `Actors` and holds a byte vector, and the compiler names each: today TCP's `Write` and `Frame`; HTTP's `Request` and `Response`; the tools' `Call` and `Called`; the link's `CallEngine`; Files' `WriteFile`, and its `Save` job, since a job crosses `Actors` (ADR-0018). The echo guest's `Parcel` holds `String` instead: it exists to be mail past the read limit (ADR-0017), and a blob would keep it small. What an actor keeps in its own state, such as the exchange's read buffer, the link's frames to another engine, and its settings keep their `Vec<u8>`.

## Consequences

- **The 250 KB read makes no new buffer once it has run once.** The file is read into a buffer Files keeps. `ReadTool`, `Mcp`, and the exchange each build in a buffer of their own, put it, and get it back. The three copies out of mail are gone, and so is joining the head and body; what is left are the copies that build something, each into memory already in use. That is an estimate until it is measured: the decision's first slice measures it.
- **No agent chooses this.** A kind cannot hold a byte vector in mail, so whoever writes one writes `Bytes`, and every `Bytes` came from a put, which takes a `Vec` the turn handed out or one made anywhere else. A `Vec` made elsewhere is put without a copy, and its buffer then joins the actor's.
- **Memory is kept by the actors that use it.** An actor that answers large files keeps buffers the size of its answers, up to `most_kept_bytes`, and gives them up when it ends. An idle actor that once put a large blob keeps its buffer until it ends.
- **Small bytes cost a put.** A 20-byte frame becomes a blob, an entry in the store taken in at the fold, and its pins, where it was an inline `Vec`. The link's frames and TCP's small writes are measured before and after.
- **Every reader needs the turn,** as every reader of a blob already does (ADR-0014). Code that read a `Vec` field reads `turn.read(&field)`.
- **ADR-0033's reason for inline writes is gone.** It priced a blob as "a put and a hash for each write"; ADR-0014 hashes only when asked, and with reuse a put moves a buffer the actor already has.
- **The rename touches every use of `BlobRef`,** in native code, the guest library, and the records and ADRs that name it. ADR-0014's text keeps "blob" for the bytes held.
- **The per-worker reuse ADR-0014 described is replaced,** and since it was never built, nothing is undone.

## Alternatives considered

- **Keep the allocator's freed memory** (glibc's `mallopt`, or mimalloc as the global allocator). Measured: 0.66 and 0.69 ms. No code changes and every buffer is covered, but it tunes one platform's allocator, keeps every copy, and leaves memory no actor owns.
- **Counts kept with atomic operations, as `Arc` keeps them,** so a `Bytes` reads without the turn. ADR-0014 rejected this: when a blob is freed would depend on how workers' turns interleave.
- **Bytes with one owner, moved as a resource is** (ADR-0010). No counting, and the owner reads directly. Every send encodes from a borrow, so each would have to give the bytes up first, and a buffer sent to many would be copied for each. ADR-0014 rejected it for buffers many actors read.
- **A borrowed form of each kind,** whose byte fields point into the arena for the turn. Two types for every kind and every type inside it, and a handler that picks the owned form copies with nothing to say so.
- **A field type that borrows the arena and pins it,** as Aether's mail ring keeps a region until its readers release it (Aether ADR-0087). The arena is emptied when its writer's turn to write it comes round again, so a field kept past its step must copy itself out.
- **A pool of owned buffers behind a `Bytes` type,** decoded into and given back on drop. The copies stay, and the pool is shared by every worker.
- **Buffers kept by each worker,** as ADR-0014 wrote. Bounded by the number of workers, but an actor's part runs on whichever worker the step gives it, so its buffers drift away from it.
- **A `Bytes` type beside `BlobRef`.** Two names for one thing.
- **Small bytes inline, large ones as a blob.** Two shapes for one field, and a choice of threshold every kind would carry.
