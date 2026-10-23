# ADR-0032: Files

- **Status:** Proposed
- **Date:** 2026-10-23

## Context

A program reads files: the assets it ships with, and later what it saves. An actor cannot read one itself. A turn does not block (ADR-0001: a pumped, single-threaded mode in which nothing waits; ADR-0009), and a file read takes as long as the disk or the network makes it. In a browser there is no file to open at all: what a page ships with is fetched from where the page was served, and what it saves goes to the browser's storage, each a promise.

Work that takes time is a job (ADR-0018: `Job`, `Turn::run`): bytes in, bytes out, run by the host outside the steps, and answered as a request is. ADR-0018 names a file read among the work a job is for. But a job a turn may run is work any actor may ask for, and a job that read any path it was given would hand every actor the whole of the host's disk. What an actor reads must be limited to what the program means it to read, and named the same way on every host, so that one actor runs unchanged on a desktop, a headless test, and a page.

The two hosts read differently. A native host reads a directory; a browser fetches a URL, and saves into the origin private file system (OPFS), the browser's storage of directories and files for each origin, writable from a page in every major browser since 2025. A job's code is linked unchanged into each build (ADR-0018), and code for one host does not belong in a crate both build, nor in a guest, which builds for the same machine as the browser (ADR-0001, rule 1).

The earlier engine reached files by mail to one actor that owned named roots (its ADR-0041). It was the right shape, and two of its parts are kept out here: a verb that read a raw host path with no check on who asked, and a check of paths that read only their text.

## Decision

**One capability, `Files`, reads files for every other actor, in named namespaces, by path.** It is a root actor (ADR-0019), in a crate of its own, `aethereal-files`. An actor asks it, and it answers once the read is done (ADR-0017: `turn.hold`, `turn.answer`):

```rust
turn.to(files).ask(&ReadFile { namespace: Namespace::Assets, path: FilePath::new("tiles/grass.png")? }, &Loading { tile })?;

impl Handles<Answer<FileRead, Loading>> for Tiles { .. }

pub enum FileRead {
    Read { bytes: BlobRef },     // the whole file, as a blob (ADR-0014)
    Missing,                     // no file at that path
    Refused,                     // a request the namespace does not allow
    Unavailable,                 // the host has nowhere to read it from
    Failed,                      // the host could not read it
}
```

- **A namespace is a name, never a place.** `Namespace` is an enum the engine fixes, `Assets` first: read-only, the files the program ships with. Its root is a setting of `Files` on each host (ADR-0028: `#[settings]`), and an actor never learns where a namespace's files are.
- **A path is checked when it is read.** `FilePath` is plain data whose decoder refuses a path that is not one or more segments joined by single `/`, each of `a`–`z`, `0`–`9`, `.`, `_`, and `-`, none `.` or `..`, at most 255 bytes a segment and 1024 a path (ADR-0004: a type's decoder holds the rules it adds to its shape). So no path climbs out of its namespace, and a path names one file on a host that ignores case and on one that does not.
- **The whole file comes back as a blob,** put by the job (ADR-0018: `Running::put`), so its bytes are written once, and a guest's code is a read away from `spawn_from`.

**The read is a job that only `Files` can make.** `Files` runs it on the asker's behalf, keeps the asker's held request until it is answered, and answers with what it read. The job's fields are private to `aethereal-files`, so no other actor can write one, and what an actor reads goes through the one actor that checks it.

```rust
pub struct Load { namespace: Namespace, path: FilePath }   // fields private: only `Files` makes one

impl Job for Load {
    type Done = FileRead;
    const RUNNER: Runner = Runner::Compute;

    async fn run(self, job: &mut Running) -> Result<FileRead, Fault> {
        job.storage().read(self.namespace, &self.path).await   // the host's storage, as its timer is
    }
}
```

- **It runs on `Compute`.** A native read blocks the thread it is on, and the `Io` thread is one thread every timer waits on (ADR-0018). In a browser every job runs on the page's event loop, whichever runner it names.

**The host's storage reaches a job as its timer does.** ADR-0018 hands every running job its host's timer (`Running::until`, `Timer`). The host hands it its storage beside it:

```rust
pub trait Storage: Send + Sync {
    fn read(&self, namespace: Namespace, path: &FilePath) -> Pin<Box<dyn Future<Output = Result<FileRead, Fault>> + '_>>;
}

impl Running {
    pub fn storage(&self) -> &dyn Storage;
}
```

- `Storage` and `Running::storage` are in `aethereal-actor`, beside `Timer`. `Namespace`, `FilePath`, and `FileRead` are plain data there too, since a storage answers with them.
- **A host with no storage hands `NoStorage`,** which answers every read `Unavailable`, as a host with no thread to wait on hands `Never` for its timer.

**Each host's crate bootstraps `Files`, and hands its storage to its runners.** ADR-0029 has a host's crate write the bootstrap of a root that needs the host (`Bootstrap<H>`). The headless and desktop hosts' storage reads a directory for each namespace; the browser's fetches each asset from a URL relative to the page. The bootstrap begins `Files` and serves the host its storage and the job type it runs (ADR-0029: `cx.serve`), and the host adds both to its runners before its first step:

```rust
// aethereal-chassis-headless, and the desktop's beside it
impl Bootstrap<Headless> for Files {
    const SECTION: &'static str = "files";
    type Settings = FileRoots;                       // assets = "assets", beside the program
    type Needs = ();
    type Provides = ();

    async fn bootstrap(roots: FileRoots, (): (), cx: &mut Bootstrapping<'_, Headless, Self>) -> Result<(), Unbuilt> {
        cx.serve(FileHosting::new(Directories::new(roots)));     // the storage, and `JobType::of::<Load>()`
        cx.begin(&(), ())
    }
}
```

- **A program lists `Files` once,** `.root::<Files>()`, and its job comes with it: the runners are handed it as they are handed `Sleep`, which every host runs. A program does not list `Load` among its jobs.
- **A host that does not bootstrap `Files` cannot list it** (ADR-0029), so a program that reads files builds only for the hosts that can read them.

**What a host cannot do is an answer, never a difference an actor must know of.** Each host answers every request in the same `FileRead`. A native host's storage answers `Missing` for a file that is not there and `Failed` for a read the disk refused; a browser's answers `Missing` for a fetch the server answered 404, and `Failed` for one it could not make.

**This record builds reading the files a program ships with.** What comes after, each its own piece:

- **Saving**, in the namespaces `Save` and `Config`, which are written: a directory natively, written whole by writing a sibling and renaming it over the file, and OPFS in a browser, whose writable stream replaces a file whole as it is closed. A browser that refuses its storage, as a private window or a sandboxed frame may, answers `Unavailable`.
- **Listing a directory.** A fetch cannot list, so a browser lists `Assets` from a manifest of the files the program ships, written when the page is built.
- **Reading a range of a file**, a plain ask whose job reads one range: a seek natively, a slice of an OPFS file, and a fetch with a `Range` header; and, over it, **a streamed read** (ADR-0017: `Stream`), for a file read as it is played or one larger than the engine's budget for blobs.

## Consequences

- An actor reads a file one way on every host, and what it may read is what the program's namespaces hold.
- A read costs a job and an ask: its bytes are copied once out of the host into the blob store, and the asker is answered a step or more after it asked.
- A file larger than the engine's budget for blobs cannot be read whole: its job's put does not fit, and its read is answered `Failed`, as the job is answered `Faulted` (ADR-0018). Reading such a file waits for ranged reads.
- `Running` holds a second service of its host. `JobType::drive` and each host's runners hand it on beside the timer.
- Runners take a job type and a storage after they are made, before the first step. A host's runners already add `Sleep` to the types they are handed; this adds what the bootstraps serve.
- Two engines on one host share nothing through `Files`: each reads its namespaces' roots, and two that write one file write it as two programs would.
- A browser's assets are fetched relative to the page, so a page served beside its assets, an artifact among them, reads them with no setting.

## Alternatives considered

- **A capability with a controller and a source** (ADR-0019: `Controller`, `Requests`): `Files` pushes each read to the host, and the host's storage brings each result back on a feed. It is how windows are made, but a window is a resource the host makes, and a read is bytes in and bytes out. It would write again, in each host, what jobs already give: running work off the steps, holding it in flight, and answering it.
- **A job any actor may run, with a path to a file.** No capability. Every actor could then read any path the host can, which is what the namespaces exist to stop.
- **Each host's code in the job, chosen by `target_arch`.** No storage on `Running`. The browser's fetch would then be built into a crate every guest builds, and a guest built for the browser's machine would carry it.
- **A job type for each host,** chosen by each host's bootstrap. Two job types for one read, and `Files` generic over which it runs.
- **The program listing `Load` among its jobs.** Nothing served. A program would list one capability in two places, and one listed without its job would answer every read `Faulted`, found only as the program runs.
