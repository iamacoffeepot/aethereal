# ADR-0053: The dev overlay

- **Status:** Proposed
- **Date:** 2026-11-09

## Context

A program has no way to show, while it runs, how the engine is doing: how long its frames take, what the GPU spends them on, how close it is to a limit, what its actors logged. Nor can a person at the window ask the engine anything; only an agent can, through the hub's tools. Runite had an overlay that did both, and the owner wants it back, as one actor every program can list, that knows nothing of any game.

What is already decided:

- **Tools.** A tool is a worker that takes bytes of its input schema and answers bytes of its output schema (ADR-0037: `Tooling`, `Call`, `Called`), offered with a `ToolSheet` whose `input` is a `SchemaNodes`. In an engine, the root that holds tools is `Link`, which dials the hub and hands it every sheet (ADR-0038: `Holder`, `Link`, `Up::Sheets`). An engine with no hub runs no `Link`, and so holds no tools.
- **Flags.** A settings kind suggests a flag for each field it marks `#[flag]` (ADR-0028: `Flags`, `FlagTable`), and a flag's text is read into the field's JSON value by a `Form` picked from the field's schema: a variant `capped:60`, a size `1280x720`, a switch `--x` / `--no-x`, a number, text, or JSON for anything else.
- **Typed text and keys.** Keys come to a window's key subscribers, and typed text comes after the `KeyDown` that typed it (ADR-0024, ADR-0051: `Event::Text`). Every key subscriber hears every key.
- **Text and drawing.** An actor makes a text and places it in its own draw list, in the order it lists it (ADR-0031, ADR-0052: `CreateText`, `PlaceText`, `DrawList`). `Font::Mono` is IBM Plex Mono.
- **Time.** Every `Tick` carries `delta_nanos`, the time since the last cycle (ADR-0020: `Tick`). The renderer times a frame's passes on the GPU when asked, a frame or more later (ADR-0023: `TimeFrame`, `Timed::Took`), and in pass order, without their names.
- **Logs.** Each actor keeps its most recent entries, and the `actor_logs` tool reads one actor's (ADR-0044: `Log`, `LogsTool`).

## Decision

**The dev overlay is one actor, `Overlay`, in a crate of its own, `aethereal-overlay`. It is drawn with the renderer's text and meshes, in three stops that the backtick key steps through: hidden, a band of meters along the top, and the band with a console beneath it. The console's commands are the engine's tools, typed as a command line that each tool declares through `#[flag]`, as settings do, and answers are printed in the same text forms. For it, the engine gains an in-engine tool holder, key focus, a stream of each frame's statistics, and engine-wide memory figures.**

```
backtick ──> Overlay: hidden → band → band + console → hidden
Overlay ── Tick (delta_nanos), FrameStats, GpuTimes, EngineStats ──> band: frame chart, GPU graphs, memory meters
console line ── argv ── flag table from the ToolSheet ── Form ──> JSON ── schema ──> Call ──> Tools ──> worker
Called::Output ── schema ── Form, printed ──> scrollback
```

### The three stops

- **Hidden:** nothing is drawn, and the overlay listens only for the backtick.
- **The band:** a strip across the top of the window, about a tenth of its height, in two columns.
  - **Left, one 60-second axis, newest at the right, so a spike reads straight down:**
    - the frame chart: each cycle's `delta_nanos`, with the budget as a dashed line, and the median, p90, p99, and the most over the 60 seconds beneath it;
    - GPU time per frame, stacked by named pass, with the share of the frame the GPU was busy;
    - one line each for draws, triangles, bytes uploaded, and GPU memory.
  - **Right:** the memory meters, each a bar against its limit where it has one, and the GPU adapter, its backend, and its driver.
- **The band and the console:** the console beneath the band, a third of the window: scrollback, the line being typed, and a grey completion ahead of the cursor. While the console is open the overlay holds key focus, and gives it up as the console closes.

### Commands are tools

- **The console calls the engine's tools,** the same ones an agent calls through the hub, so a person at the window and an agent share one set of commands. A program adds a command by offering a tool.
- **A tool declares its command line on its input kind,** with the `#[flag]` attribute settings use. Settings and tools share the attribute, the flag table, and `Form`, which gain two things, for both:
  - **`#[flag(position)]`:** a field filled by the next bare word, in declared order;
  - **a comma list** for a sequence, so a variant of one sequence reads `sheets:capture_frame,record_clip`.

  ```rust
  #[derive(Plain)]
  #[kind(name = "aether.link.describe_tools")]
  pub struct DescribeTools {
      /// The engine to describe.
      #[flag(position)]
      pub engine: u32,
      /// `names`, or `sheets:capture_frame,record_clip`.
      #[flag(short = 's')]
      pub show: ToolsShown,
  }
  ```

  typed `describe_engine 0 -s sheets:capture_frame,record_clip`.
- **A line is split into words as a shell splits it,** then read by the same parser that reads a program's argv. The same declaration can later serve a binary that calls a tool from a shell.
- **The flag table travels in the tool's sheet.** `ToolSheet` gains `flags`, written by the derive. An `Offer` whose flags name a field the input lacks, or a `Form` the field's schema cannot take, is refused, so a tool's command line never drifts from its input.
- **A field that is not an `Option` must be given.** There are no defaults beyond `Option`: a default kept only in the console would make the console's tool differ from the agent's.
- **A tool with no flags** takes its input as one JSON word: `present {"window":"main"}`. JSON is also the escape hatch for any field whose `Form` is JSON.
- **`help <tool>`** prints the tool's description and each flag with its field's doc comment. **Tab** completes tool names, flag names, and a variant's name.
- **The overlay's own commands are tools it offers:** `mark <text>`, which pins a mark at this moment on the band's axis and in the scrollback, and `memory`, which prints `EngineStats`' largest guests and longest backlogs (see Memory). An agent may call them as well, so an agent's mark shows on the owner's screen.

### Answers are printed as they would be typed

- **A value prints in the forms `Form` reads:** a leaf as its flag's text would be (`capped:60`, `1920x1080`), a struct one `field: value` a line, and a sequence of structs as blocks parted by a blank line, indented inside a field. JSON only where `Form` reads JSON, as a map.
- **An image prints one line,** its size, its bytes, and its text; `--save <path>` on any command that answers an image writes it through the file capability.
- **A refusal prints its text,** as the agent reads it.
- **The scrollback keeps 2,000 lines;** history keeps the last 200 commands, stepped with up and down.

### Logs in the console

- **No reader names the actors.** Today a log is read by one actor's path (ADR-0044: `Turn::logged`), and the `Log` view keeps entries by actor, in parts of seats folded apart (ADR-0011: `BySeat`), with no list across actors. Walking every seat to find what was logged is the pass ADR-0044 avoided.
- **So each part of the view also keeps a ring of its most recent entries,** filled by the same fold that keeps each actor's: each entry with its actor's path and the step it was folded in. The parts stay folded apart, as no part writes to another's ring. The rings keep 2,000 entries between them, a setting beside `most_logged`. An ended actor's entries, and the fault that ended it, are in the ring with the rest.
- **A turn reads the rings since a step,** merged by step, oldest first: within one step, turns ran at once, so the step is the only order between two actors' entries, and none is made up. The read answers the step to read on from, and how many entries had left the rings before it.
- **The logs tool gains this read,** `read_recent`, which needs no actor. The overlay reads it once a second while the console is open, and an agent reads it to see what the whole engine said without knowing which actor to ask.
- **A line shows its time, its level, its actor, and its text.** Errors and warnings are shown in the band as well, as a count since the console was last opened.

### Key focus

**The window root gains key focus, as aether's ADR-0248 §9 has it, without its scope:**

```rust
TakeKeyFocus { window }      // no answer; the sender holds the window's slot
ReleaseKeyFocus { window }   // no answer; empties the slot if the sender holds it
KeyFocusGained { window }    // to the new holder
KeyFocusLost { window }      // to the holder it replaced, the one that released, or every holder of a closed window
```

- **Each window has one slot,** empty or held by one actor. While it is held, that window's key events, `Event::Text`, and the input method's events go only to the holder.
- **The latest take wins,** and the holder it replaced hears `KeyFocusLost`. A release, or the holder's end, empties the slot; nothing is handed back.
- **A key that is down keeps who heard its press,** and its release goes to them, so a character walking as the console opens stops.
- **The take is refused as the program is built** for an actor that does not handle both notices.

### Tools held in every engine

**Holding tools moves out of `Link` into a root of its own, `Tools`, that every engine lists.** `Tools` is the engine's `Holder`: it takes each `Offer`, keeps the sheets, and asks each call of the worker that offered it. It answers `ListTools` with every sheet, and `CallTool { tool, revision, input }` with what the worker answered: `revision` is that of the `Sheets` the caller read the tool's input schema from, and `Tools` refuses a call of a revision it has changed since in the turn it looks the tool up, so a tool withdrawn and offered again under its name is never called with bytes written for its old schema. `Link`, where it runs, asks `Tools` for its sheets and hears each change, writes them to the hub as `Up::Sheets` as now, and asks `Tools` for each call the hub sends. An engine with no hub holds tools all the same, and the overlay is one more caller.

### Frame statistics

- **The renderer sends each frame's statistics to its subscribers,** only while it has one:
  - `FrameStats`, after each `Render`: the cycle's `elapsed_nanos`, so it lines up with `Tick`; the budget; draws, triangles, and bytes uploaded; GPU memory by what it holds, meshes, textures, targets, and the glyph atlas, as the renderer counts what it made, with what the driver reserved where wgpu reports it; how many meshes, textures, programs, placements, and texts are alive; and how full the glyph atlas is.
  - `GpuTimes`, as a frame's timestamps are mapped back: the same `elapsed_nanos`, and each pass's nanoseconds with its pass's name. Not sent on a device without timestamps, where the band leaves the GPU graph out and says so.
- **The budget is the frame time the program aims at:** for `capped:N`, a second over N; for `fifo`, the refresh of the monitor the window is on, as winit reports it; for `uncapped`, the monitor's refresh too, drawn as a reference rather than a limit. In the browser, the refresh is measured from the frames' own intervals.
- **The adapter, its backend, and its driver** are answered once to `DescribeGpu`.
- **`render_stats` is a tool** that answers the latest `FrameStats` and `GpuTimes`, so an agent reads the same meters without a screenshot.

### Memory

**The band shows one meter for each limit the engine can hit,** from `EngineStats`, which the substrate answers:

- **process:** the resident memory the operating system reports for the program; in the browser, what it exposes, or nothing;
- **blobs:** the bytes the blob store holds, against its limit;
- **backlog:** the most bytes any one actor has waiting unsent, against the limit;
- **guests:** the memory of every guest, added up;
- **GPU:** from `FrameStats`.

**Only the engine sees every actor, so it finds the largest itself.** `EngineStats` also answers the eight guests with the most memory and the eight actors with the most bytes unsent, each by path. The substrate keeps them as it folds, as it keeps the totals, so no turn walks the seats and no answer grows with the number of actors.

### Colours

The overlay's colours are slate and earth. Healthy is one cool colour; only warm colours mean trouble.

| Name   | Colour    | For                                                                    |
| ------ | --------- | ---------------------------------------------------------------------- |
| slate  | `#15181b` | the panels, at 85% opacity, so the game shows behind them             |
| bone   | `#e4e0d6` | text                                                                   |
| stone  | `#8d918f` | labels, timestamps, info lines                                        |
| lichen | `#9dbfb0` | the typed command, healthy lines and meters                           |
| ochre  | `#d9b86c` | marks                                                                  |
| rust   | `#d08a4c` | warnings; a frame over budget; a meter past 75%                       |
| brick  | `#cc5f52` | errors; a frame over twice the budget; a meter past 90%               |

The GPU's passes are told apart by lichen, a slate blue, and a muted violet, in pass order. Every text is `Font::Mono`, so figures line up.

### Where it lives

- **`aethereal-overlay`,** a crate of its own. It reads the window, the renderer, the tools, and the lifecycle, and both the desktop and browser chassis list it, so no capability's crate can hold it.
- **`Tools` lives in `aethereal-tools`; key focus in `aethereal-window`; frame statistics in `aethereal-render`; `EngineStats` in `aethereal-substrate`.** Each is useful without the overlay.

## Consequences

- **Any program lists `Overlay` and has the band and the console,** on desktop and in the browser, without writing any of it. ichor's own console pass goes.
- **A tool is a console command once its input marks its flags,** and a person at the window and an agent reach the same tools.
- **An engine with no hub holds tools,** so tools can be tested and called in a program that never links.
- **Key focus serves any text field,** not only the console: a chat box takes it the same way.
- **The renderer counts what it makes and draws each frame,** and spends nothing on it while no one subscribes.
- **Changes to decided work:** ADR-0028 gains positional flags and comma lists; ADR-0037's `ToolSheet` gains `flags`; ADR-0038's `Link` stops holding tools and forwards `Tools`'s; ADR-0044's `Log` view gains a ring in each part, and its logs tool `read_recent`; ADR-0024's key events are routed by key focus.
- **Follow-on work this leaves open:**
  - a binary that calls a tool from a shell, with the same flags;
  - pipeline statistics (vertex and fragment invocations), on GPUs that have them;
  - a mouse on the overlay's panels;
  - key focus for a UI built of several actors (aether's `Subtree` scope).

## Alternatives considered

- **Generic dot paths for every field (`--show.Sheets capture_frame`).** Every tool works with nothing declared, but the paths are long, and a variant with fields is awkward to pick.
- **RON-like nesting (`Sheets([a, b])`).** One rule for any shape, but brackets and quotes to type for every list.
- **A tree of words each tool writes beside its sheet, as Minecraft's Brigadier.** The most freedom, but a second description of each input to keep in step with it, where attributes on the fields cannot drift.
- **Commands the overlay knows itself.** A second set beside the tools, which agents could not call.
- **The overlay in front of the program's input,** the program subscribing to it rather than the window. Every program would be wired through the overlay.
- **Key focus as a stack.** A release would hand keys back to the one before, but every holder would have to release in order; aether's single slot has served.
- **A flag for the budget.** It would be wrong the moment the present mode or the monitor changed.
- **A GPU chart only of the latest frame.** It says what is slow now, not when it got slow, and a spike is read against the frame chart above it.
- **A read of every actor's log, by a pass over the seats.** It needs nothing new, but costs a pass over every seat on each read, which ADR-0044 measured at 100 µs for 100,000 seats.
- **One ring for the whole engine.** One order for every entry, but every part's fold would write to it, and the parts are folded apart.
- **The overlay inside the renderer's crate.** It reads the window and the tools too, which the renderer does not.
