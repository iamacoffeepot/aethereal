# ADR-0040: The hub spawns its engines

- **Status:** Proposed
- **Date:** 2026-11-02

## Context

The last slice of the hub stack. ADR-0038 linked engines to their hub, each started by hand and dialing in. ADR-0039 gave the hub a store of checked-in binaries, each named by its hash, and kept a build while something holds it, so a hub that starts only what was checked in needs no list of what it may run. What is left: the hub starts an engine from the store, knows which connection is the process it started, stops it, and tells the agent how it went.

What is already decided:

- **An engine dials its hub, and `Hello` opens the link** (ADR-0038: `Link`, `Engines`, `Engine`, `Up::Hello`, `REVISION`). A hand-started engine dials with no proof of who it is, and the hub, listening on loopback, takes it.
- **A token, minted by the hub for the engine it spawns and passed in its arguments, is good for one `Hello`** (ADR-0038, "Who dialed"). Read from the process list after the engine has linked, it gains nothing.
- **The store keeps a held build, and a hold lasts while its holder lives** (ADR-0039: `Store`, `Hold`, `StoreHash`, `StoreName`). The store never answers a binary's bytes.
- **No actor ends another; one asks it to end, by mail** (ADR-0009), **and an actor may end its program** (ADR-0020: `turn.quit`).
- **Work that waits is a job, and what a job needs of its host is a service its capability defines** (ADR-0018, ADR-0034). A blocking service waits on threads of its own (ADR-0032, ADR-0033).

Nothing in the engine starts a process yet. The earlier engine's hub started its engines, stopped them with a signal, a grace period, and a kill, and ran each from a copy of the stored bytes named for the binary, so a desktop engine's menu bar showed its name, not a hash.

## Decision

**A new capability, `Processes`, in `aethereal-process`, starts programs and says when each exits. `Engines` spawns an engine from the store through it: the store hands it a copy to run and holds the build, the engine is started with a token, and the `Hello` that brings the token back is that engine's link. A spawned engine ends with its hub.**

```
agent ── spawn_engine { binary, args } ──> Engines ── spawns Spawned:n
  Spawned ── Launch { binary } ──> Store              a launch copy to run; the build held while Spawned lives
  Spawned ── Start { program, args } ──> Processes ── Process:m          the engine's process
  engine  ══ Hello { revision, name, token } ══> Engines ── tells Spawned:n it linked, as engine id k
  Spawned answers spawn_engine: Linked { engine: k }, or how it failed
```

### Processes

```
Processes      root, singleton, asked at path::<Processes>(()); native hosts only
 └─ Process:m  one program running, keyed by the root's count
```

```rust
impl Carries<Start> for Processes { type Answers = (Started,); }

pub struct Start { pub program: String, pub args: Vec<String> }    // a path on the host's machine
pub enum Started {
    Running { process: ActorRef<Process> },
    Refused,                                                       // the host would not start it
}

pub struct Exited { pub status: ExitStatus, pub stderr: String }   // told to whoever started it, as the process ends
pub enum ExitStatus { Code(i32), Signal(i32) }
pub struct Stop;                                                   // asks a process to end
```

- **A process is an actor that owns nothing the engine keeps.** The host's service (`Launcher`, a service of this crate's) starts the program in a process group of its own, with no stdin and its stdout discarded, and keeps the last few kilobytes of its stderr. A job, `Wait`, run by the `Process` as it begins and only by it, answers once the program has exited, with its status and that stderr. The `Process` then tells `Exited` to the actor that asked `Start`, and ends.
- **`Stop` is asked of the `Process`,** which signals its group to end (SIGTERM), runs a `Sleep` for the setting `stop_within_millis`, and kills the group (SIGKILL) if it is still running when the sleep is answered. Either way, `Exited` follows, from `Wait`.
- **A `Process` whose starter ends stops its program.** It watches the starter, as `Tcp`'s root watches who asked it (ADR-0033), so a program never outlives the actor that wanted it.
- **`Processes` starts any program it is asked to.** It is a capability a program lists, as `Tcp` is; what a hub lets an agent start is `Engines`' rule, below, not this crate's.

### Spawning from the store

```rust
impl Carries<Launch> for Store { type Answers = (Launched,); }

pub struct Launch { pub binary: Binary }
pub enum Binary { Named(StoreName), Hash(StoreHash) }
pub enum Launched { Ready { program: String, hash: StoreHash }, NotStored }
```

- **The store copies the entry to a launch copy,** `running/<name>-<n>` beneath its root, marked executable, and answers its path. The copy is named for the binary, so the process is shown by that name (a desktop's menu bar takes the file's), and the store's entries are never executable. Copying a binary of a few megabytes takes milliseconds.
- **`Launch` holds the build for its asker,** as `Hold` does (ADR-0039), and the store removes the launch copy when the asker ends. Each spawn has its own copy, so a rebuild checked in under the same name changes nothing that is running.
- **A name that spells a hash is refused at check-in,** 64 lowercase hexadecimal digits, so the agent's `binary`, which is one string, is a hash when it spells one, and a name otherwise. This amends ADR-0039's rule for a name.

### `Engines` spawns

```
Engines
 ├─ Engine:k     one engine linked (ADR-0038)
 └─ Spawned:n    one engine the hub started: holds its build, owns its process
```

```rust
impl Carries<SpawnEngine> for Engines { type Answers = (Spawn,); }

pub struct SpawnEngine { pub binary: Binary, pub args: Vec<String> }
pub enum Spawn {
    Linked { engine: u32 },                              // started, and linked as engine `engine`
    NotStored,
    Refused,                                             // the host would not start it
    Exited { status: ExitStatus, stderr: String },       // it ended before it linked
    Unlinked,                                            // it ran, and did not link within `link_within_millis`
}

impl Carries<TerminateEngine> for Engines { type Answers = (Terminated,); }
pub struct TerminateEngine { pub engine: u32 }
pub enum Terminated { Stopping, NotSpawned, NotLinked }
```

- **`Engines` spawns a `Spawned` for each `SpawnEngine`,** handing it the held request. It asks the store to `Launch`, so the build is held while it lives, then asks `Processes` to `Start` the copy with the agent's arguments and the link's own: `--hub`, `--port`, and `--hub-token`, a token `Engines` mints from the host's randomness and keeps beside the spawn.
- **The token ties the link to the process.** An `Engine` whose `Hello` brings a token asks `Engines` whose it is; `Engines` takes it out of its table, so it is good once, and tells the `Spawned` its engine's id. A token it never minted, or one used, is refused, as a wrong revision is (`Down::Refused`). The `Spawned` answers `Linked`.
- **The agent is answered once the engine is usable:** `Linked` when it has said `Hello`, `Exited` with its status and the tail of its stderr if it ended first, `Unlinked` if it did not link within `link_within_millis` (the process is then stopped), `NotStored` and `Refused` at once.
- **A `Spawned` lives as long as its process,** and ends when told `Exited`, so the store's hold and the launch copy go with it. `TerminateEngine` asks the engine's `Spawned` to `Stop` its process.
- **A hand-started engine still links,** with no token, as in ADR-0038, unless the setting `require_tokens` is on. The hub cannot stop it, as it holds no process of it: `TerminateEngine` answers `NotSpawned`.

### A spawned engine ends with its hub

- **`LinkSettings` gains `token`,** read from `--hub-token`: an enum, `Token::{None, Given(String)}`, not an empty string. `Hello` carries it: `Up::Hello { revision, name, token }`, and `REVISION` is `2`.
- **A `Link` that was given a token ends its program when its connection ends** (`turn.quit`, ADR-0020), where one with none dials again (ADR-0038). Its token is spent, and a hub that restarted has none of its own to take, so it could never link again. So no engine the hub spawned outlives the hub, on every host, with no signal from a parent that may have died first.

### What the agent sees

`EngineTools` (ADR-0038) gains two tools, and `list_engines` says where each engine came from:

```
spawn_engine { binary: String, args: [String] }   -> { engine }        // refused with how it failed
terminate_engine { engine }                       -> {}
list_engines {}                                   -> { engines: [{ engine, name, tools, origin }], exited: [{ binary, status, stderr }] }
                                                  // origin: { Spawned: { binary, hash } } | "ByHand"
```

- `exited` keeps the last few engines the hub spawned that have ended, with how, bounded by the setting `most_exited`, so an engine that crashed after linking is not just gone from the list.

## Consequences

- **The arc is whole:** an agent checks a build in, spawns it, calls its tools, and terminates it, all through one hub, and the hub runs only what was checked in.
- **A spawned engine's process is the hub's to stop,** and ends with the hub whatever kills it. A hand-started one is the user's.
- **`Processes` can start any program a program asks it to.** It is listed by a hub, and only `Engines` asks it there; an engine has no reason to list it.
- **`Hello` changes,** so an engine and a hub of different revisions refuse each other (ADR-0038), as they should.
- **ADR-0039's names lose the 64-hex-digit spelling.**
- **Each spawn copies its binary.** A hub spawning many engines of one build writes one copy each; they go as each engine ends, and a crash leaves them in `running/`, which the store clears as it begins.

## Alternatives considered

- **Run the store's entry in place.** No copy, but the entry must be executable, and the process is named by its hash wherever it is shown.
- **Spawned engines redial when the hub goes.** A restarted hub cannot take their spent tokens, so they would wait unlinked for good, and the hub would have to kill its children as it exits, which no host promises if the hub is killed.
- **Only spawned engines link.** Tighter, but an engine run by hand while it is written would have to be checked in first. It is a setting, `require_tokens`.
- **Spawning inside `Engines`, with no `Processes`.** Starting and watching a process is not the link's, and would be written again by the next thing that starts one.
- **Answer `spawn_engine` as soon as the process starts.** The agent would then poll `list_engines` to learn when it can call the engine's tools, or that it never could.
