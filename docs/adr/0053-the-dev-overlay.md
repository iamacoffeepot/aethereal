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
- **The band:** a strip across the top of the window, as tall as its left column's three sections need, about a quarter of the window at 1080, in two columns parted by a faint stone line, a logical pixel thick, at the lines between the band and the console's opacity, down the band's content from the frame chart's top to the rows' foot, centred between the gutter's text and the meters'. Two more such lines part the left column's three sections, the frame chart and its figures, the GPU's graph and its figures, and the rows, across the column from the frame chart's left to the gutter's end, each centred in the space between one section's figures and the next; no line parts a graph from its gutter, which is its label.
  - **Left, one 60-second axis, newest at the right, so a spike reads straight down:**
    - the frame chart: each cycle's `delta_nanos`, with the budget as a dashed line, and the median, p90, p99, and the most over the 60 seconds beneath it;
      - each column, a fixed span of time, is three opaque bars the column's width, each from the chart's foot, drawn back to front: its slowest cycle in sand, the tallest, at the back; its median in lichen; and its fastest in slate-teal, the shortest, in front;
      - each bar's colour is fixed by its layer, whatever the budget: over or under is read from the bars' height against the budget line, dashed bone at 70%, which carries the judging;
      - the vertical scale is logarithmic and relative to the budget: a quarter of the budget at the foot, 32 budgets at the top, so the budget stands 2/7 of the way up and every doubling is another 1/7, whatever the budget is; a faint stone line marks twice the budget;
      - the gutter beside the chart, level with its top, says its median, as the GPU graph's gutter says its top: `frame`, and flush right `4.2 ms`; the line beneath it counts the warnings and errors since the console was last opened, `alerts` and flush right `! N` in rust and `× N` in brick, or `no alerts` in stone when there are none, so the line is never blank;
    - GPU time per frame, stacked by named pass, with the share of the frame the GPU was busy:
      - a graph two thirds the frame chart's height, its columns the frame chart's, the same span at the same place, each a stack of the frame's passes from the foot in the order their labels were first heard, each pass at its median over the column's frames, so the stack is a typical frame;
      - its scale is linear, as stacked times are summed, and ranged to what is drawn, as GPU profilers such as Tracy and PIX range theirs: nothing at the foot, and at the top the tallest stack in the 60 seconds rounded up to 1, 2, or 5 × 10ⁿ nanoseconds, no less than 10 microseconds so an idle GPU's noise is not drawn full height. The top moves only between those steps, which keeps it from flickering as the window slides. A pass that took any time stands at least a pixel tall, and a stack over the top is cut there. A budget-relative scale would leave a fast GPU's frame under a pixel at a high refresh rate: 0.08 ms against 4.17 ms at 240 Hz;
      - the budget is the frame chart's dashed bone line only where it falls at or under the top; over it, the graph has no budget line, as `gpu busy` already tells the frame's share;
      - the gutter beside the graph says its top, as the rows' gutter says their values: `gpu`, and flush right `top 0.1 ms`;
      - beneath it, one line: each pass's label, without the renderer's `aether.render.` prefix, and its median over the 60 seconds, each in its pass's colour, so the line is the legend; and `gpu busy`, the GPU's median frame over the frame chart's median cycle;
      - on a device without timestamps, the graph's place holds one stone line, `GPU timing not supported on this device`, and nothing else;
    - one line each for draws, triangles, and bytes uploaded: each a thin row of stone bars over the same columns, each column the most of its frames, linear from nothing to the most shown, with its label and newest value in a gutter at the column's right, bytes in KiB, MiB, and GiB. The graphs above end at the gutter too, so every column stands under the frame chart's. GPU memory is not a row here: it is a memory meter, in the right column, with its own minute of history.
  - **Right:** the memory meters, a row each, the five sharing the column's height evenly: the first's top level with the frame chart's, the last's foot level with the left column's last row, so no block of the column is empty; the band's height is the left column's, and the right column grows and shrinks with it. Each row's text is clipped to the column, so a narrow window cuts it rather than letting it run into the left column's gutter:
    - in order: process, blobs, backlog, guests, GPU (see Memory); each row laid out as the left column's rows are, a graph and a gutter at its right: the gutter the same twenty characters as the left column's, ending at the band's right padding, one line of Mono text centred down the row, the label in stone in its first ten characters and the value set flush right in bone in the rest, so the labels start at one edge and the values' digits end at another; a value that is no figure in stone, `unknown` where the chassis reads no figure, `none`, `no guests` where none runs;
    - left of the gutter, a character's space from it, the meter's graph or bar, from the column's left, every meter's between the same two edges so the graphs stand aligned with each other, as tall as the row less a little padding, `███████░░░░░░░░░░░░░░░ blobs      412.0 MiB`;
    - process, guests, and GPU have no limit, so they draw their last minute: over the same 60 seconds as the left column, as many columns as the graph's width holds, the newest at its right, each column the most of the readings in its span, drawn as the left column's rows are, stone bars linear from nothing to the most shown, over a trough of faint stone as the bars' are, so a minute of nothing, as `no guests`, still reads as a graph; a column no reading fell in carries the reading before it for up to a second, as the four-a-second reads are about a column apart and now and then skip one, so a steady meter draws a steady line, while a longer pause, as while the overlay is hidden, stays a gap;
    - blobs and backlog have a limit, so they draw a bar against it: a trough of faint stone the graph's width and height, and a fill from its left, its share of the trough the bytes' share of their most, lichen under 75%, rust from 75%, brick from 90%; with no most, as when no actor holds bytes unsent, an empty trough and `none`;
    - a meter with a limit writes the bytes it holds alone, as its bar shows their share of the limit, so every value fits the gutter and nothing is cut as a value changes; the backlog's actor is not in the band, which has no room for it in a fixed gutter: the `memory` tool names it;
    - the engine's figures are read at the figures' pace, four times a second, each reading of process and guests kept in its meter's history, and the GPU meter from each frame's statistics, every kind of its memory summed, each frame kept in its history and the newest shown as the figures are read; a text is written again only when what it shows changes;
    - the column ends after the meters: the GPU adapter, its backend, and its driver do not change while the program runs, so the band does not keep them in view; `render_stats` answers them, called from the console or by an agent (see Frame statistics).
- **The band and the console:** the console beneath the band, a third of the window: scrollback, the line being typed, and a grey completion ahead of the cursor. While the console is open the overlay holds the keys: it declares in its list `Keys { frame, keys: KeySet::All, blocks: true }` while the console is open, and only the backtick while it is not, and a `Hit` over the console that blocks the pointer, so the wheel over it scrolls the console alone.

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
- **A line shows its time, its level, its actor, and its text.** Errors and warnings are shown in the band as well, as a count since the console was last opened, in the gutter beside the frame chart.

### Key focus

**Keys reach an actor through the declared draw order** (ADR-0055: `Keys`). An actor declares the keys it hears with a `Keys` entry in its list, and each key goes top-down through those entries until one blocks. The overlay is last in the setup's root list, in the `Interface` pass, so its backtick reaches it whatever holds the keys beneath. A UI built of several actors needs no scope of its own: each actor's `Keys` entry sits at its place in the declared order.

- **A key that is down keeps who heard its press,** and its repeats and its release go to them, so a character walking as the console opens stops. The window losing the keyboard forgets the keys down, as their releases never come.

### Tools held in every engine

**Holding tools moves out of `Link` into a root of its own, `Tools`, that every engine lists.** `Tools` is the engine's `Holder`: it takes each `Offer`, keeps the sheets, and asks each call of the worker that offered it. It answers `ListTools` with every sheet, and `CallTool { tool, revision, input }` with what the worker answered: `revision` is that of the `Sheets` the caller read the tool's input schema from, and `Tools` refuses a call of a revision it has changed since in the turn it looks the tool up, so a tool withdrawn and offered again under its name is never called with bytes written for its old schema. `Link`, where it runs, asks `Tools` for its sheets and hears each change, writes them to the hub as `Up::Sheets` as now, and asks `Tools` for each call the hub sends. An engine with no hub holds tools all the same, and the overlay is one more caller.

### Frame statistics

- **The renderer sends each frame's statistics to its subscribers,** only while it has one:
  - `FrameStats`, after each `Render`: the cycle's `elapsed_nanos`, so it lines up with `Tick`; the budget; draws, triangles, and bytes uploaded; GPU memory by what it holds, meshes, textures, targets, and the glyph atlas, as the renderer counts what it made, with what the driver reserved where wgpu reports it; how many meshes, textures, programs, placements, and texts are alive; and how full the glyph atlas is.
  - `GpuTimes`, as a frame's timestamps are mapped back: the same `elapsed_nanos`, and each pass's nanoseconds with its pass's name: the type id placements join it by, and its label, the name of its type's kind (`aether.render.main`), which a frame's passes may not repeat and which the band and `render_stats` show. Not sent on a device without timestamps, where the band leaves the GPU graph out and says so.
- **The budget is the frame time the program aims at:** for `capped:N`, a second over N; for `fifo`, the refresh of the monitor the window is on, as winit reports it, read again as each frame is acquired, since Wayland names a window's monitor only once its first frame is shown, with no event as it does or as the window moves to another, and 60 Hz stands in until it is known; for `uncapped`, the monitor's refresh too, drawn as a reference rather than a limit. In the browser, the refresh is measured from the frames' own intervals.
- **The adapter, its backend, and its driver** are answered once to `DescribeGpu`, which `render_stats` asks as it begins and answers with each frame's statistics: its `adapter`, `name`, `backend`, `driver`, and `driver_info`.
- **`render_stats` is a tool** that answers the latest `FrameStats` and `GpuTimes`, so an agent reads the same meters without a screenshot.

### Memory

**The band shows one meter for each limit the engine can hit,** from `EngineStats`, which the substrate answers, each a row of the band's right column (see The three stops):

- **process:** the resident memory the operating system reports for the program, `1.2 GiB`; in the browser, the script heap it exposes, labelled `heap`, `84.0 MiB`; or `unknown`; its last minute, no bar;
- **blobs:** the bytes the blob store holds, `412.0 MiB`, with a bar against its limit;
- **backlog:** the most bytes any one actor has waiting unsent, with a bar against that actor's limit; `none` when no actor has said it holds any; which actor it is, `memory` names;
- **guests:** the memory of every guest, added up, `no guests` when none runs; its last minute, no bar;
- **GPU:** the sum of the renderer's kinds in the newest `FrameStats`; its last minute, each frame's sum, no bar.

**Only the engine sees every actor, so it finds the largest itself.** `EngineStats` also answers the eight guests with the most memory and the eight actors with the most bytes unsent, each by path. The substrate keeps them as it folds, as it keeps the totals, so no turn walks the seats and no answer grows with the number of actors.

**How each figure is read:**

- **Process:** each chassis hands the engine its own way of asking, read when a turn reads the figures: `/proc/self/status` on Linux, `proc_pidinfo` on macOS, and in the browser `performance.memory.usedJSHeapSize` where it exists. Elsewhere the figure is a named unknown.
- **Backlog:** what an actor holds unsent is its own state, which the engine cannot see, so the actor tells it with `Turn::holds_unsent(bytes, most)`. That is an effect, folded into a view of its own keyed by the actor's seat, and each part ranks its own actors as it folds. What an actor said last stands until it says again or ends. A TCP connection tells it whenever what its socket has not taken changes, against its `most_unsent_bytes`.
- **Guests:** a guest's memory is that of an instance of a blob's code in one part, which every actor of the blob seated there shares. The instance is named by the first of those actors. Each part's instances are measured as the part is put back after it runs.
- **The read:** a turn reads the figures with `Turn::engine_stats`, and a guest reads nothing, as it reads no log. The `engine_stats` tool answers them.

### Colours

The overlay's colours are slate and earth. Healthy is one cool colour; only warm colours mean trouble.

| Name   | Colour    | For                                                                    |
| ------ | --------- | ---------------------------------------------------------------------- |
| slate  | `#15181b` | the panels, at 85% opacity, so the game shows behind them             |
| bone   | `#e4e0d6` | text; the chart's budget line                                         |
| stone  | `#8d918f` | labels, timestamps, info lines                                        |
| lichen | `#9dbfb0` | the typed command, healthy lines and meters; the chart's median bar   |
| sand   | `#6b5f4c` | the chart's slowest bar                                                |
| slate-teal | `#3f5a57` | the chart's fastest bar                                            |
| slate blue | `#6f8aa6` | the GPU graph's first pass in each three, and its figure          |
| muted violet | `#8a7ca8` | the GPU graph's second pass in each three, and its figure       |
| mauve  | `#a88aa0` | the GPU graph's third pass in each three, and its figure               |
| ochre  | `#d9b86c` | marks                                                                  |
| rust   | `#d08a4c` | warnings; a meter past 75%                                             |
| brick  | `#cc5f52` | errors; a meter past 90%                                               |

The GPU's passes are told apart by slate blue, muted violet, and mauve, cycling in the order their labels were first heard; none is lichen, sand, or slate-teal, which keep their one meaning each in the frame chart. Every text is `Font::Mono`, so figures line up.

### Where it lives

- **`aethereal-overlay`,** a crate of its own. It reads the window, the renderer, the tools, and the lifecycle, and both the desktop and browser chassis list it, so no capability's crate can hold it.
- **`Tools` lives in `aethereal-tools`; the key cascade in `aethereal-window`; frame statistics in `aethereal-render`; `EngineStats` in `aethereal-substrate`.** Each is useful without the overlay.

## Consequences

- **Any program lists `Overlay` and has the band and the console,** on desktop and in the browser, without writing any of it. ichor's own console pass goes.
- **A tool is a console command once its input marks its flags,** and a person at the window and an agent reach the same tools.
- **An engine with no hub holds tools,** so tools can be tested and called in a program that never links.
- **Key focus serves any text field,** not only the console: a text field declares `Keys` in its list while it wants them (ADR-0055).
- **The renderer counts what it makes and draws each frame,** and spends nothing on it while no one subscribes.
- **Changes to decided work:** ADR-0028 gains positional flags and comma lists; ADR-0037's `ToolSheet` gains `flags`; ADR-0038's `Link` stops holding tools and forwards `Tools`'s; ADR-0044's `Log` view gains a ring in each part, and its logs tool `read_recent`; ADR-0024's key events are routed through the declared `Keys` entries (ADR-0055).
- **Follow-on work this leaves open:**
  - a binary that calls a tool from a shell, with the same flags;
  - pipeline statistics (vertex and fragment invocations), on GPUs that have them;
  - a mouse on the overlay's panels;

## Alternatives considered

- **Generic dot paths for every field (`--show.Sheets capture_frame`).** Every tool works with nothing declared, but the paths are long, and a variant with fields is awkward to pick.
- **RON-like nesting (`Sheets([a, b])`).** One rule for any shape, but brackets and quotes to type for every list.
- **A tree of words each tool writes beside its sheet, as Minecraft's Brigadier.** The most freedom, but a second description of each input to keep in step with it, where attributes on the fields cannot drift.
- **Commands the overlay knows itself.** A second set beside the tools, which agents could not call.
- **The overlay in front of the program's input,** the program subscribing to it rather than the window. Every program would be wired through the overlay.
- **Key focus as a slot on each window's actor,** as aether's ADR-0248 §9 has it, without its scope: `TakeKeyFocus` and `ReleaseKeyFocus`, the latest take wins, the holders told `KeyFocusGained` and `KeyFocusLost`, and the wheel sent with the keys. Two mechanisms beside global shortcuts, and a sibling that takes focus silences the overlay's backtick (ADR-0055).
- **Key focus held by the window root,** the take naming its window. The root does not send a window's events to its subscribers, so it would hand each change to the window's actor, and a take sent to the root would bring no reply the sender's type is checked against.
- **Key focus as a stack.** A release would hand keys back to the one before, but every holder would have to release in order; aether's single slot has served.
- **A flag for the budget.** It would be wrong the moment the present mode or the monitor changed.
- **A GPU chart only of the latest frame.** It says what is slow now, not when it got slow, and a spike is read against the frame chart above it.
- **A read of every actor's log, by a pass over the seats.** It needs nothing new, but costs a pass over every seat on each read, which ADR-0044 measured at 100 µs for 100,000 seats.
- **One ring for the whole engine.** One order for every entry, but every part's fold would write to it, and the parts are folded apart.
- **The overlay inside the renderer's crate.** It reads the window and the tools too, which the renderer does not.
