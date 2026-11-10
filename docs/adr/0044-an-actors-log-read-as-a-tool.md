# ADR-0044: An actor's log, read as a tool

- **Status:** Proposed
- **Date:** 2026-11-03

## Context

An agent spawns an engine through the hub, calls its tools, and captures its windows (ADR-0040, ADR-0043). It cannot read what the engine's actors say while it runs. When a spawned desktop engine's window came up black, the agent driving it could not learn that the engine was drawing the whole time.

What an actor says is already kept, and read by no one:

- **`turn.log` adds a line to the turn's buffers, and the engine folds it into the `Log` view** (ADR-0009, ADR-0011: `Logger::log`, `Log`, `Entry`). The view keeps an actor's lines with the faults that ended it, the resources dropped on its way, and the mail it could not read, keyed by its seat, as `Engine::log(actor)` reads them.
- **Each actor keeps its last `RECENT` entries,** a constant of 32, and keeps them once it has ended until another actor takes its seat and logs.
- **Only tests read the view.** No turn can: the engine lends turns a read of its table (ADR-0022: `Table`), and of no other view.

What is already decided:

- **A tool is an actor written in the crate its work belongs to, behind that crate's `tools` feature, listed in a program's `Toolbox`** (ADR-0037: `Tool`, `Toolbox`, `Tooling`). An engine's tools reach the agent through the hub's `call_tool` (ADR-0038: `EngineTools`, `CallEngine`), as `capture_frame` does (ADR-0043).
- **A turn reads a view as it stood when its step began, and nothing writes it while turns run** (ADR-0011).
- **A call carries the agent's deadline,** so an engine that does not answer is answered `Late` by the hub (ADR-0041: `Due`).

## Decision

**A turn may read an actor's log, and a tool, `actor_logs`, answers it to the agent. Each entry is numbered within its actor, so a reader pages by number, and how many entries each actor keeps is a setting.**

```
agent ── call_tool { engine, tool: "actor_logs", input } ──> hub ══ link ══> engine's Toolbox ── LogsTool
  LogsTool reads the Log view (turn.logged) as its step began, and answers the entries
```

### The view numbers each actor's entries, keeps as many as it is set to, and is read by id

- **Each entry is numbered within its actor,** from 0, in the order the actor made them. A number never changes, so a reader that asks from a number learns how many entries left the view before it read them.
- **An actor keeps its last `most_logged` entries,** a field of `EngineSettings` (32 by default), which replaces `RECENT`. As before, an entry outlives its actor until another takes the seat and logs.
- **The view is also read by the actor's id,** which a path names whether or not the actor lives (ADR-0022: `Table::child` computes an id). A live actor's entries are read at its seat. Each part of the view also keeps the seat each of its actors logged in, by the actor's id, written only as an actor first logs in a seat, when the seat's last actor leaves it; so an ended actor is found in one lookup a part, not by a pass over every seat, which cost 100 µs at 100,000 seats. So the log of an actor that has ended, usually the one most wanted, is read by its path.
- `Engine::log` reads the view as before.

### A turn reads a log

```rust
impl Turn<'_, A, S> {
    /// What `path` logged, live or ended, as the view stood when the step began.
    pub fn logged(&self, path: &ActorPath) -> Logged<'_>;
}

pub enum Logged<'a> { Nothing, Kept(&'a KeptLog) }   // `Nothing`: no entries the view holds
impl<'a> Logged<'a> {
    pub fn since(self, since: u64) -> impl Iterator<Item = (u64, &'a Entry)>;   // numbered, oldest first
    pub fn first(self) -> u64;                                                  // the oldest entry the view holds
    pub fn next(self) -> u64;                                                   // the number the next entry takes
}
```

- The engine lends it to turns as it lends the table: a read of the view, with no lock, made between folds (ADR-0011). A path whose actor has logged nothing, or whose entries another actor's seat has since replaced, reads as no entries.
- It is a method of `Table` (ADR-0022), which the engine's `Since` answers. `Entry` moves to `aethereal-actor` beside it, with `KeptLog`, one actor's numbered entries. A guest's table reads every log as `Nothing` until a guest needs one, when the boundary gains a call for it.

### `actor_logs`

```rust
pub struct ReadLog { pub actor: ActorPath, pub since: u64, pub level: Level, pub contains: String, pub most: u32 }
pub struct ActorLog {
    pub entries: Vec<LogEntry>,   // oldest first
    pub next: u64,                // the number to ask from next
    pub skipped: u64,             // entries asked from that had left the view
}
pub struct LogEntry { pub n: u64, pub entry: LoggedEntry }
pub enum LoggedEntry {
    Line { level: Level, text: String },
    Failed(String), EndFailed(String), Dropped(String), Unbrought(String), JobFaulted(String), Unread(String),
}
```

- **`LogsTool`, in `aethereal-tools`,** answers `actor_logs` from `turn.logged`. The view is the engine's own, and `aethereal-tools` is the tools crate built on the substrate (the substrate cannot depend on it), so the tool sits there beside `Toolbox`, listed by any program that wants it.
- **`level` keeps lines at that level and above, and every entry that is not a line;** `contains` keeps entries whose text holds it, and every text holds `""`; `most` bounds the answer, and so does the link's `most_bytes`, past which it stops early. `next` is then the first entry it left out.
- **A fault, drop, or unread mail is written as text,** so the agent reads why an actor ended without a schema for every fault. A fault is named by its kind's id and size, as nothing in the engine names a kind by its id; a resource or a source's kind by its name.
- The usual asks: `{ "actor": "aether.render", "since": 0, … }` for what an actor has said, then `{ "since": next }` to follow it.

*Amended by ADR-0053: each part of the view also keeps a ring of its most recent entries across actors, `most_recent` of them between the parts (2,000 by default, a field of `EngineSettings`), each with its actor's id and the step it was folded in, and the table remembers as many ended actors' names, so an entry is named by its actor's path after the actor ends. `Turn::recent(since)` reads the rings merged by step, with the step to read on from and how many entries had left them, and `LogsTool` answers it as a second tool, `read_recent`, which needs no path.*

### What changes for programs

- The `linked_window` example and the hub's own program list `LogsTool` in their toolboxes; a program that does not list it answers no `actor_logs`.
- A program that prints what it wants an agent to read, as ichor's painter writes its said lines to stderr, logs them with `turn.log` instead.

## Consequences

- **An agent reads any engine's actors, spawned or started by hand,** by one tool, while they run, and reads why one ended after it has.
- **The hub changes not at all:** `actor_logs` is proxied by `call_tool` like any engine tool.
- **A log is read only while its engine lives.** An engine that crashed answers no call; how it exited, and the tail of its stderr, are still in `list_engines`' `exited` (ADR-0040), where a panic's message is.
- **A hung engine answers no call either:** the agent is told `Late` by its deadline, which says the engine is stuck, but not what it said last.
- **What a program prints and does not log is not read by the tool.**
- **Raising `most_logged` costs memory per actor that logs:** entries are kept for every seat that has logged, 32 by default.
- **Follow-on work this leaves open:** a tool listing the actors that have logged, so the agent need not know a path first; native hosts writing entries to stderr for a terminal; a larger stderr tail for engines that exit.

## Alternatives considered

- **The hub keeps a ring of each spawned engine's stdout and stderr, read by a hub tool.** It reads an engine that is hung or has crashed, and what it prints. But it misses every engine started by hand, needs reader threads and a ring in the hub for each engine, and still reads none of the actors' log unless the host first prints it, which is the view this record reads directly.
- **Each engine sends its log up the link as it folds it.** It covers every engine, but adds a frame and a ring in the hub for state the engine already keeps, and the hub would then answer for what is the engine's.
- **A log the tool keeps of its own, in order across actors.** The view already keeps every actor's entries; a second copy would be kept by mail to the tool, which the actors do not send.
