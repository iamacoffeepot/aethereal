# ADR-0039: Saving, and the store

- **Status:** Proposed
- **Date:** 2026-11-02

## Context

The hub stack's last two slices are a hub that spawns its engines, and one that keeps the files they are spawned from. A hub that may start any program it is told of needs a list of what is allowed, a way to tell two builds of one name apart, and an answer for a binary rebuilt while an engine of it runs. A hub that starts only what was checked in to a store of its own has none of these to settle: a binary is named by the hash of its bytes, so a hash names one build, and the store keeps what a running engine was started from. So the store comes first, and spawning is the next ADR.

What is already decided:

- **`Files` reads in named namespaces, by a checked path, through a job only it makes** (ADR-0032: `Files`, `Namespace`, `FilePath`, `Load`, `Storage`). It planned saving next: namespaces that are written, each file written whole by writing a sibling and renaming it over the file, natively, and through the origin private file system (OPFS) in a browser.
- **Work that waits is a job, and what a job needs of its host is a service its capability defines** (ADR-0018: `Job`; ADR-0034: `Running::service`). A native service that blocks does so on threads of its own (ADR-0032).
- **A blob's content hash is BLAKE3** (ADR-0014), which the workspace depends on.
- **A byte array is written in JSON as a hexadecimal string** (ADR-0007), so a 32-byte hash reads as 64 hex digits.
- **A tool is written in the crate it belongs to, and a program lists it** (ADR-0037, ADR-0038: `Tool`, `Toolbox<L, R>`).

The earlier engine's hub kept such a store. Its lessons kept here: binaries are read from a path on the hub's machine, as one is too large for a tool call; a name is a tag that moves to the latest build, with older builds kept until pruned; a byte budget prunes the least recently used, but never one that is named, pinned, or that a running engine was started from (its pin and hold were each added after a binary in use was evicted); and the index is rebuilt from files beside the entries, so names and pins survive a restart. Its check-in ran each binary with `--describe` to learn what it was, and so ran untrusted code to take it in, which its own notes warned of.

## Decision

**`Files` gains a namespace that is written, `Save`, and a `WriteTool`. A new capability, `Store`, in `aethereal-store`, keeps binaries checked in from a path, each named by the BLAKE3 hash of its bytes, under names that move to the latest build, pruned to a byte budget.**

### Saving

```rust
pub enum Namespace {
    Assets,   // the files the program ships with: read, never written
    Save,     // what the program saves: read and written
}

impl Carries<WriteFile> for Files { type Answers = (FileWritten,); }

pub struct WriteFile { pub namespace: Namespace, pub path: FilePath, pub bytes: Vec<u8> }
pub enum FileWritten {
    Written,
    ReadOnly,      // the namespace is not written, as `Assets` is not
    Unavailable,   // the host has nowhere to write it
    Failed,        // the host could not write it
}
```

- **A write is a job only `Files` makes** (`Save`, beside `Load`), and the host's `Storage` gains `write`. The native storage writes a sibling of the file and renames it over the file, on its own threads, and makes the directories the path names. A browser's writes to OPFS, whose writable stream replaces a file whole as it is closed. A reader never sees half a file.
- **`ReadOnly` is answered in the turn that asks,** with no job run.
- **The bytes are inline,** as a TCP write's are (ADR-0033): a save is written once, and a blob would cost a put and a hash for bytes not read again.
- **Each host's settings gain the root of `Save`:** `save`, beside the program, natively, and nothing to set in a browser, whose OPFS is the origin's.
- **`WriteTool`** (`aethereal-files`, behind `tools`) writes text: `write_file { namespace, path, text } -> {}`, refused for `Assets`. With `ReadTool`, an agent reads and writes a program's saves.

### The store

```
Store        root, singleton, asked at path::<Store>(()); a hub lists it
```

```rust
pub struct StoreHash(pub [u8; 32]);   // BLAKE3 of the bytes

impl Carries<CheckIn>  for Store { type Answers = (CheckedIn,); }
impl Carries<ListStored> for Store { type Answers = (Stored,); }
impl Carries<Pin>      for Store { type Answers = (Pinned,); }
impl Carries<Unpin>    for Store { type Answers = (Pinned,); }
impl Carries<Hold>     for Store { type Answers = (Held,); }

pub struct CheckIn { pub path: String, pub name: StoreName }    // a path on the hub's machine
pub enum CheckedIn {
    Stored { hash: StoreHash, bytes: u64, fresh: bool },        // `fresh`: these bytes were not already held
    Missing,                                                    // nothing at the path
    NotAFile,                                                   // a directory, a device, or a pipe
    TooLarge,                                                   // past `most_file_bytes`
    Full,                                                       // no room within the budget, after pruning
    Failed,                                                     // the host could not read or write it
}
```

- **Check-in reads any path.** The hub is a program running as its user, and copying a file it is named is what any program does: no root or namespace confines it, as none would stop a caller that can already have the user's files read some other way. **The store never gives bytes back:** an entry is listed, pinned, held, and (in the next ADR) spawned, and no ask answers its contents. Two checks keep a check-in well behaved, not safe: only a regular file is read, so a device or a pipe cannot hang it, and one past the setting `most_file_bytes` is refused, so a wrong path cannot fill the disk.
- **The copy is a job only `Store` makes, and the bytes never enter the engine.** `Stock` runs on `Io`, and the native store's service copies the file into the store's directory on threads of its own, hashing as it copies, to a sibling it renames to `entries/<hash>`. Bytes already held are not copied twice: the copy is discarded and the entry kept. A binary of tens of megabytes costs no blob budget and no mail. A browser serves no store, and `Store` is bootstrapped only by the native hosts (ADR-0029).
- **Nothing is run to check a binary in.** An entry records its hash, size, and when it was checked in. What an engine offers is learned once it links, from its `Sheets` (ADR-0038), so the store need not ask the binary.

### Names, pins, and holds

```rust
pub struct StoreName(String);   // checked at decode: 1 to 128 of ASCII letters, digits, `_`, `-`, and `.`

pub struct Stored { pub entries: Vec<Entry> }   // newest first
pub struct Entry  { pub hash: StoreHash, pub names: Vec<StoreName>, pub bytes: u64, pub pinned: bool, pub held: bool }
```

- **A name never spells a hash:** 64 lowercase hexadecimal digits are refused, so text naming a binary is a hash when it spells one (amended by ADR-0040).
- **A name points at the hash last checked in under it.** A check-in moves its name. A hash that loses its last name stays in the store, unnamed, until it is pruned; so a hash always names one build, and an older build is reached by its hash.
- **A pin keeps an entry from being pruned,** asked by hash with `Pin` and cleared with `Unpin`. A name keeps it too.
- **A hold keeps an entry while its holder lives.** `Hold { hash }` is answered `Held` and watched (`turn.watch` on the asker): when the holder ends, the hold goes. It is what the next ADR's spawner asks before it starts an engine of a hash, so a name moved by a rebuild never prunes a binary that is running. A hold is kept in the root's state, not on disk: a hub's restart ends the engines it ran.

### The budget

- **The store holds at most `most_bytes`** (16 GiB by default). After each check-in it prunes the entries least recently used, checked in or held, that are neither named, pinned, nor held, until it is within the budget. A check-in that cannot fit after pruning is answered `Full`, and its copy discarded.
- Pruning is a job of the store's service, as a check-in is, run by the root with the hashes it chose.

### On disk, and across a restart

```
<root>/entries/<hash>           the bytes
<root>/entries/<hash>.entry     its record: size, when it was checked in and last used, and whether it is pinned
<root>/names                    every name, and the hash it points at
```

- **Each record is written whole,** a sibling renamed over it, as a save is. So a crash leaves the old record or the new one.
- **The root rebuilds its index from the records as it bootstraps,** with a job that reads them, so names and pins survive a hub's restart. An entry without its record, or a record without its entry, is a check-in a crash cut short, and is removed.
- The store's settings are `StoreSettings { root, most_bytes, most_file_bytes }`: `store` beside the program, 16 GiB, and 1 GiB by default.

### The tools

`aethereal-store`, behind `tools`, offers a hub four, listed in its toolbox:

```
check_in { path: String, name: String }   -> { hash, bytes, fresh }
list_stored {}                            -> { entries: [{ hash, names, bytes, pinned, held }] }
pin { hash }                              -> {}
unpin { hash }                            -> {}
```

## Consequences

- **The next ADR's spawner starts only what was checked in,** by hash or by name, so a hub needs no list of the programs it may run, and the agent never names a path to run.
- **A rebuild is a check-in,** under the same name. Engines already running keep their build, held.
- **Check-in executes nothing.** What a binary is, beyond its bytes, is known once it runs and links.
- **The store's bytes are never read back through it.** An ask that answered them would make check-in a way to read any file the hub's user can, and would need this ADR revisited.
- `Files` writes one namespace, `Save`. `Config`, and listing a directory, are still to come (ADR-0032).
- A browser host has no store, as it has no processes to spawn.

## Alternatives considered

- **Check-in confined to roots the hub's settings list.** It would keep ADR-0032's rule that no actor reads a raw host path. Rejected: the hub runs as its user, so a root confines nothing a caller could not reach otherwise, and the store gives nothing back.
- **Bytes through `Files`, as a blob read and written back.** It reuses `Files`, but every binary would need to fit the blob budget and cross the engine as mail. The store's own job copies file to file.
- **Run each binary with `--describe` at check-in.** It tells more before a spawn, but runs untrusted code to take a file in. A linked engine tells what it offers.
- **A name bound to one hash for good.** Nothing changes under a name, but every rebuild needs a new one. A hash already names one build for whoever needs that.
- **Inline bytes over a tool call.** A binary is too large for one.
