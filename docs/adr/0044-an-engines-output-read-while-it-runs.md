# ADR-0044: An engine's output, read while it runs

- **Status:** Proposed
- **Date:** 2026-11-03

## Context

An agent spawns an engine through the hub, calls its tools, and captures its windows (ADR-0040, ADR-0043). What it cannot do is read what the engine says while it runs. When a spawned desktop engine's window came up black, the agent driving it had no way to learn that the engine was drawing the whole time, which the frame timings it printed would have shown at once.

Two things keep an engine's words from the agent:

- **The hub keeps almost none of a program's output.** The host's launcher starts each program with its stdout discarded and its stderr piped to a thread that keeps the last `most_stderr_bytes` (4096 by default) (ADR-0040: `Launcher`, the headless host's `Tail`). That tail is handed over only as the program exits, in `Exited { status, stderr }`, which reaches the agent as `Spawn::Exited` or as an entry of `list_engines`' `exited` (`ExitedEngine`).
- **An actor's log lines never leave the engine.** `turn.log` adds a line to the turn's buffers, and the engine folds it into the `Log` view, which keeps the last `RECENT` (32) entries for each actor's seat, with the faults that ended it (ADR-0009, ADR-0011: `Log`, `Entry`, `Engine::log`). No host reads that view, and only tests call `Engine::log`. So the renderer's warnings, a failed turn's fault, or a job a host lost to a panic are seen by no one while the engine runs, and by no one after it ends.

What is already decided:

- **Work that waits is a job, and what a job needs of its host is a service its capability defines** (ADR-0018, ADR-0034: `Running::service`). A launcher's waits block on threads of their own and wake the job once they are done (ADR-0040: `Launcher::wait`).
- **The hub knows which spawn each engine is,** by the token its `Hello` brought (ADR-0040: `Engines`, `Spawned:n`, `LinkedAs`). An engine started outside the hub is a connection only: the hub holds no process of it.
- **A tool's output is a value of its output schema, bounded by the link's `most_bytes`** (ADR-0038, ADR-0042).

## Decision

**A native host writes each entry it folds into its log to stderr, as one line. The launcher pipes both of a program's streams into a ring of numbered lines, which its process answers while the program runs. The hub reads a spawned engine's ring by its engine id, through a new tool, `engine_logs`.**

```
actor ── turn.log ──> Log view (as before) ──> the native host writes it to stderr: "WARN aether.render: …"
engine process: stdout, stderr ──> launcher threads ──> Output ring, lines numbered from 0
agent ── engine_logs { engine, from, contains, most } ──> Engines ──> Spawned:n ──> Process:m ── Read job ──> ring
```

### Log entries reach stderr

- **The engine hands its host each entry it folds,** in the order it folds them (`Engine::take_said`, taken between steps as a feed's items are). The `Log` view keeps them as before.
- **The headless host, and the desktop host through it, write each one to stderr as one line:** its level (`ERROR`, `WARN`, `INFO`, `DEBUG`), the actor's path, and the entry: a line's text, or the fault, drop, or unread mail it records. A text holding newlines is written as several lines, each with the same head. The writes are buffered and flushed once a step.
- **This is a host's, not the engine's:** the substrate stays target-neutral, and the browser host, which has a console instead, is left for when one needs it.
- So a hand-started engine's terminal shows what its actors say, and a spawned engine's says it into the ring below.

### The launcher keeps a ring of output lines

```rust
pub struct OutputLine { pub n: u64, pub stream: Stream, pub at_millis: u64, pub text: String }
pub enum Stream { Out, Err }
```

- **Both streams are piped,** each read by a thread of its own, which amends ADR-0040's "stdout discarded". A program that writes faster than the ring keeps never blocks: the threads always drain the pipes.
- **A line is what ends at a newline,** taken as it arrives, numbered from 0 in the order the threads took them, and stamped with the milliseconds since the program started. The two streams are interleaved in the order they were read, which may not be the order they were written within a few milliseconds.
- **A line longer than `most_line_bytes`** (4096 by default) is cut there, and the rest begins the next line. Bytes that are not UTF-8 read as U+FFFD. What is left in a pipe as it ends is a last line.
- **The ring keeps at most `most_output_bytes` of text** (1 MiB by default, per program), the oldest lines leaving first. A line's number never changes, so a reader that asks from a number it was given learns how many it missed.
- **`Exited` keeps its tail, now taken from the ring:** `stderr` is the last `most_stderr_bytes` of the stderr lines, as before, and `tail` is the last `most_tail_bytes` (64 KiB by default) of every line, both streams, as `OutputLine`s.

### A process answers its output

```rust
impl Carries<ReadOutput> for Process { type Answers = (Output,); }

pub struct ReadOutput { pub from: From, pub contains: String, pub most: u32 }
pub enum From {
    Line(u64),    // lines numbered from this one on
    Last(u32),    // the last this many lines of those that match
}
pub struct Output {
    pub lines: Vec<OutputLine>,   // oldest first
    pub next: u64,                // the number to ask from next
    pub skipped: u64,             // lines asked from that had left the ring
    pub ended: bool,              // both pipes have ended
}
```

- **The `Process` runs a `Read` job, asked of the `Launcher` by the program's number,** which copies the matching lines under the ring's lock and answers at once. A launcher's `read` does not block.
- **`contains` keeps only lines whose text holds it,** before `most` is counted. Every line holds the empty text, so `""` keeps every line.
- **An answer holds at most `most` lines and `most_read_bytes` of text** (256 KiB by default), and stops early past either. `next` is then the first line it left out, so a reader pages by asking `From::Line(next)`.

### `Engines` reads an engine's output

```rust
impl Carries<ReadEngineOutput> for Engines { type Answers = (EngineOutput,); }

pub struct ReadEngineOutput { pub engine: u32, pub from: From, pub contains: String, pub most: u32 }
pub enum EngineOutput {
    Read(Output),
    External,     // started outside the hub: its output is its own
    NotLinked,    // no engine of that id is linked, or kept among those that exited
}
```

- **A linked engine the hub spawned is read live:** `Engines` asks its `Spawned` to read its `Process`, and answers what it was told.
- **An engine that linked and has exited is read from its tail,** the `Exited.tail` its `Spawned` was told, which `Engines` keeps with its `ExitedEngine` among the last `most_exited`. The read answers as a live one does, with `ended` set. `ExitedEngine` gains `engine: Linked`, `Linked::{As(u32), Never}`, so the agent can name it; an engine that ended before it linked was answered with its tail already, as `Spawn::Exited`.
- **An engine started outside the hub is answered `External`:** the hub holds no pipe of it, and its own stderr, now holding its actors' lines, is wherever it was started.

### What the agent sees

```
engine_logs { engine, from: { Line: n } | { Last: n }, contains, most }
    -> { lines: [{ n, stream, at_millis, text }], next, skipped, ended }   // refused: External, NotLinked
```

- `EngineTools` (ADR-0038, ADR-0040) gains it. `list_engines`' `exited` entries gain `engine`, and keep their short `stderr`.
- The usual ask is `{ "Last": 50 }` to see what an engine is saying now, then `{ "Line": next }` to follow it.

## Consequences

- **An agent reads a spawned engine while it runs:** its actors' warnings and faults, and anything it prints, by one tool, live and after it exits.
- **Every native engine's actors now speak on stderr,** spawned or not. A busy actor's logs cost a formatted write each, buffered once a step; an actor that logs every frame fills the ring sooner, and is read with `contains`.
- **Each spawned engine costs two reader threads, and up to `most_output_bytes` of memory in the hub,** while it runs, and each exited one up to `most_tail_bytes` among the last `most_exited`.
- **An engine started outside the hub gains nothing from the tool.** Its lines go to its own stderr.
- **Follow-on work this leaves open:** a ring kept for an engine started outside the hub, carried up its link; the browser host writing its entries to the console; a level filter, if `contains` proves too coarse.

## Alternatives considered

- **Capture the process's output alone, leaving actors' logs in the engine.** It would catch what a program prints and its panics, but not one `turn.log` line or failed turn, which is most of what an engine says.
- **Send the `Log` view up the link, and leave stdout and stderr alone.** It would cover engines started outside the hub, and keep each entry's actor and level as data. But the view keeps 32 entries per seat and drops them as another actor takes the seat, holds no order across actors, and misses what a program prints and how it panicked; and every engine would carry the code, where the hub already holds the pipes of the engines it spawned.
- **Stream lines to the agent as they arrive.** MCP's tools answer once, and an agent reads in turns; paging by line number gives it the same lines when it asks, with no subscription to hold open.
- **A ring bounded by line count.** A few long lines and many short ones would cost the hub very different memory; a byte budget bounds it, and line numbers still say what was missed.
