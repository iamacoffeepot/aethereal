# ADR-0040: The hub spawns its engines

- **Status:** Proposed
- **Date:** 2026-11-02

## Context

The last slice of the hub stack. ADR-0038 linked engines to their hub, each started by hand and dialing in. ADR-0039 gave the hub a store of checked-in binaries, each named by its hash, and kept a build while something holds it, so a hub that starts only what was checked in needs no list of what it may run. What is left: the hub starts an engine from the store, knows which connection is the process it started, stops it, and tells the agent how it went.

What is already decided:

- **An engine dials its hub, and `Hello` opens the link** (ADR-0038: `Link`, `Engines`, `Engine`, `Up::Hello`, `REVISION`). A hand-started engine dials with no proof of who it is, the hub on loopback takes it, and its `Link` dials again whenever the hub goes.
- **A token minted by the hub for an engine is good for one `Hello`** (ADR-0038, "Who dialed"), so one read from the process list after the engine has linked gains nothing.
- **The store keeps a held build, and a hold lasts while its holder lives** (ADR-0039: `Store`, `Hold`, `StoreHash`, `StoreName`). The store never answers a binary's bytes.
- **No actor ends another; one asks it to end, by mail** (ADR-0009), **and an actor may end its program** (ADR-0020: `turn.quit`).
- **Work that waits is a job, and what a job needs of its host is a service its capability defines** (ADR-0018, ADR-0034). A blocking service waits on threads of its own (ADR-0032, ADR-0033).

Nothing in the engine starts a process yet. The earlier engine's hub started its engines, stopped them with a signal, a grace period, and a kill, and ran each from a copy of the stored bytes named for the binary, so a desktop engine's menu bar showed its name, not a hash.

## Decision

**Every engine links with a token the hub minted for it, and every engine ends when its link ends. The hub mints one as it spawns an engine from the store, through a new capability, `Processes`, and mints one on request for an engine started outside it.**

```
agent ── spawn_engine { binary, args } ──> Engines ── spawns Spawned:n, mints its token
  Spawned ── Launch { binary } ──> Store              a launch copy to run; the build held while Spawned lives
  Spawned ── Start { program, args } ──> Processes ── Process:m          the engine's process
  engine  ══ Hello { revision, name, token } ══> Engines ── tells Spawned:n it linked, as engine id k
  Spawned answers spawn_engine: Linked { engine: k }, or how it failed

agent ── mint_token {} ──> Engines ── answers a token, and the arguments to start an engine with
  the agent starts the engine itself; its Hello brings the token, and it is listed as external
```

### Tokens

- **A token is 16 bytes of the host's randomness,** minted by `Engines` and kept in its table with what it is for: a spawn (`Spawned:n`) or an engine started outside the hub.
- **It is good for one `Hello`.** An `Engine` whose `Hello` brings a token asks `Engines` whose it is, and `Engines` takes it out of its table. A token it never minted, or one used, is refused, as a wrong revision is (`Down::Refused`), and that connection is shut. A `Hello` with no token cannot be read: the field is required.
- **A token minted for an engine outside the hub lasts `token_within_millis`** (ten minutes by default), long enough to start an engine by hand, and the table keeps at most `most_tokens` of them; past that, the oldest goes. A spawn's token lasts as long as its `Spawned`.
- **The host's randomness is a service the link crate defines,** `Randomness`, which the headless and desktop hosts serve from the kernel's generator as they bootstrap `Engines`, and which a `Draw` job reads (ADR-0034). So `Engines` runs on native hosts only, as the store does.
- **A refusal names its reason:** `Down::Refused(Refusal::Revision { revision })` for a revision the hub does not speak, and `Down::Refused(Refusal::Token)` for a token it does not hold.
- So the hub knows what every connection is: the spawn it belongs to, or an engine started outside it, and ADR-0038's links that bring no proof are gone.

### Every engine ends when its link ends

- **`LinkSettings` gains `token`,** read from `--hub-token`, required: an engine started without one fails as it begins, naming the setting.
- **A `Link` ends its program when its connection ends,** when its dial is refused, or when the hub refuses its `Hello` (`turn.quit`, ADR-0020). Its token is spent, and a hub that restarted has none of its own to take, so it could never link again. One rule holds for every engine, spawned or started outside the hub: no engine outlives its link, on every host, with no signal from a parent that may have died first.
- **This replaces ADR-0038's redial.** `Link` dials once, as it begins; its `Sleep`, its waits between dials, and `redial_millis` go. ADR-0038's links with no token, and the token its "Who dialed" planned for spawned engines alone, are replaced by this section.
- `Up::Hello` is `{ revision, name, token }`, and `REVISION` stays `1`: no build that speaks the first link's frames is kept anywhere, so there is nothing to tell apart.

### Processes

```
Processes      root, singleton, asked at path::<Processes>(()); native hosts only, in aethereal-process
 └─ Process:m  one program running, keyed by the root's count
```

```rust
impl Carries<Start> for Processes { type Answers = (Started,); }

pub struct Start { pub program: String, pub args: Vec<String>, pub starter: ProtocolRef<Exits> }  // `program`: a path on the host's machine
pub enum Started {
    Running { process: ActorRef<Process> },
    Refused,                                                       // the host would not start it
}

pub struct Exited { pub status: ExitStatus, pub stderr: String }   // told to whoever started it, as the process ends
pub enum ExitStatus { Code(i32), Signal(i32) }
pub struct Stop;                                                   // asks a process to end
```

- **A process is an actor that owns nothing the engine keeps.** The host's service (`Launcher`, a service of this crate's) starts the program in a process group of its own, with no stdin and its stdout discarded, and keeps the head and the tail of its stderr. A job, `Wait`, run by the `Process` as it begins and only by it, answers once the program has exited, with its status and that stderr. The `Process` then tells `Exited` to the starter, and ends. `Start` names the starter, usually its asker, by a reference of the protocol `Exits`, as `Serve` names its handler (ADR-0035), so an actor that cannot take `Exited` cannot start a program.
- **A program's stderr is kept as its head and its tail, each bounded.** Every line past 200 characters is cut and ends in `…`, then the first 1 KiB and the last 512 bytes of what remains are kept, the head ended and the tail begun at a line's end and never inside a character, with a line between them saying how many lines and bytes were left out (`… 412 lines, 41233 bytes, left out …`, counted after the line cuts); a stderr that fits is kept whole. A panic's first `panicked at` line is near the head, and how the program ended is in the tail, where a backtrace of nested generic type names would otherwise fill the whole of what is kept. The bounds are the launcher's constants, `HEAD_BYTES`, `TAIL_BYTES`, and `LONGEST_LINE_CHARACTERS`, not settings. **This departs from this record as first written**, in which the last few kilobytes were kept, as many as the setting `most_stderr_bytes` said, so the panic's first line was lost and `list_engines` answered tens of kilobytes of type names.
- **`Stop` is asked of the `Process`,** which signals its group to end (SIGTERM), runs a `Sleep` for the setting `kill_after_millis`, and kills the group (SIGKILL) if it is still running when the sleep is answered. Either way, `Exited` follows, from `Wait`.
- **A `Process` whose starter ends stops its program.** It watches the starter, as `Tcp`'s root watches who asked it (ADR-0033), so a program never outlives the actor that wanted it.
- **`Processes` starts any program it is asked to.** It is a capability a program lists, as `Tcp` is; what a hub lets an agent start is `Engines`' rule, below, not this crate's.

### Spawning from the store

```rust
impl Carries<Launch> for Store { type Answers = (Launched,); }

pub struct Launch { pub binary: Binary }
pub enum Binary { Named(StoreName), Hash(StoreHash) }
pub enum Launched { Ready { program: String, hash: StoreHash }, NotStored, Failed }   // `Failed`: the host could not copy it
```

- **The store copies the entry to a launch copy,** `running/<name>-<n>` beneath its root, marked executable, and answers its absolute path. `<name>` is the name the launch asked by; a launch by hash takes the first name pointing at it, in the order of their bytes, or the hash's first 12 hexadecimal digits when none does. `<n>` counts the store's launches since it began. The copy is named for the binary, so the process is shown by that name (a desktop's menu bar takes the file's), and the store's entries are never executable. Copying a binary of a few megabytes takes milliseconds.
- **`Launch` holds the build for its asker,** as `Hold` does (ADR-0039), and the store removes the launch copy when the asker ends, and clears `running/` as it begins. Each spawn has its own copy, so a rebuild checked in under the same name changes nothing that is running.
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

impl Carries<MintToken> for Engines { type Answers = (Minted,); }
pub struct MintToken;
pub struct Minted { pub token: [u8; 16], pub args: Vec<String> }   // `args`: --hub, --port, --hub-token

impl Carries<TerminateEngine> for Engines { type Answers = (Terminated,); }
pub struct TerminateEngine { pub engine: u32 }
pub enum Terminated { Stopping, NotLinked }                      // `NotLinked`: no engine of that id is linked
```

- **`Engines` spawns a `Spawned` for each `SpawnEngine`,** handing it a token it mints for it. `Engines` keeps the held request, as only the actor that holds a request can answer it, and answers it with the `Spawn` the `Spawned` tells it. The `Spawned` asks the store to `Launch`, so the build is held while it lives, then asks `Processes` to `Start` the copy with the agent's arguments and the link's own: `--hub`, `--port`, and `--hub-token`.
- **The `Hello` that brings its token is its link:** `Engines` tells the `Spawned` its engine's id, and the `Spawned` answers `Linked`.
- **The agent is answered once the engine is usable:** `Linked` when it has said `Hello`, `Exited` with its status and the head and tail of its stderr if it ended first, `Unlinked` if it did not link within `link_within_millis` (the process is then stopped), and `NotStored` and `Refused` at once.
- **A `Spawned` lives as long as its process,** and ends when told `Exited`, so the store's hold and the launch copy go with it. `TerminateEngine` asks the engine's `Spawned` to `Stop` its process, and answers `Stopping`.
- **An engine started outside the hub** links with a token from `MintToken`, whose `args` are what to start it with: `--hub` names the setting `EnginesSettings.host`, where the hub listens. The hub holds no process of it, so `TerminateEngine` shuts its link and answers `Stopping`, and the engine then ends by the rule above.

### What the agent sees

`EngineTools` (ADR-0038) gains three tools, and `list_engines` says where each engine came from:

```
spawn_engine { binary: String, args: [String] }   -> { engine }        // refused with how it failed
mint_token {}                                     -> { token, args }   // to start an engine by hand
terminate_engine { engine }                       -> {}                // refused if no engine of that id is linked
list_engines {}                                   -> { engines: [{ engine, name, tools, origin }], exited: [{ binary, status, stderr }] }
                                                  // origin: { Spawned: { binary, hash } } | "External"
```

- `exited` keeps the last few engines the hub spawned that have ended, with how, bounded by the setting `most_exited`, so an engine that crashed after linking is not just gone from the list. Each one's stderr is its head and its tail, bounded as above, so the list stays a few kilobytes per ended engine.

## Consequences

- **The arc is whole:** an agent checks a build in, spawns it, calls its tools, and terminates it, all through one hub, and the hub runs only what was checked in.
- **The hub knows every connection,** and every engine follows one rule: it links once, with a token, and ends when its link ends. An engine started by hand needs a token first, and is started again with a fresh one after its hub goes.
- **`Link` loses its redial,** which a spent token could not use.
- **`Processes` can start any program a program asks it to.** A hub lists it, and only `Engines` asks it there; an engine has no reason to list it.
- **`Hello` changes without a new revision,** since nothing outside this repository speaks the first. From here the store keeps builds, so an engine checked in before a change can meet a hub built after it, and a change an older side would misread takes the next revision (ADR-0038).
- **ADR-0039's names lose the 64-hex-digit spelling.**
- **Each spawn copies its binary.** Copies go as each engine ends, and a crash leaves them in `running/`, which the store clears as it begins.

## Alternatives considered

- **Links with no token from engines started by hand.** A connection the hub cannot place, and two rules for engines: one ends with its hub, one redials. Minting a token for it on request gives every engine the same rule.
- **A plain spawn id, not a random token.** It pairs a connection with its spawn as well, but a local process could guess one and take a spawn's place in the list. Sixteen random bytes per engine close that.
- **Engines redial when the hub goes.** A restarted hub cannot take their spent tokens, so they would wait unlinked for good.
- **Run the store's entry in place.** No copy, but the entry must be executable, and the process is named by its hash wherever it is shown.
- **Spawning inside `Engines`, with no `Processes`.** Starting and watching a process is not the link's, and would be written again by the next thing that starts one.
- **Answer `spawn_engine` as soon as the process starts.** The agent would then poll `list_engines` to learn when it can call the engine's tools, or that it never could.
