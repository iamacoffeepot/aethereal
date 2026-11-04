# ADR-0045: A clock tools can hold, and recording clips

- **Status:** Proposed
- **Date:** 2026-11-03

## Context

An agent judges animation from clips that play in real time. Today every clip comes from a browser page a script drives, at whatever rate the page draws. A frame that renders slowly stretches the time between frames, so the clip's timing drifts, and two runs of one scene never match frame for frame. A before-and-after pair is then two different clips of two different timings.

What a program animates by is the time `Tick` carries, `delta_nanos` and `elapsed_nanos` (ADR-0020: `Stage::made`, `Cycled`). The engine reads no clock (ADR-0001, rule 4). The host says a cycle is due and passes its own time for it, `Engine::cycle(at)`. The engine takes each `Tick`'s delta as that time less the last cycle's, limited to `Engine::with_delta_limit`, a quarter of a second unless set (`Staging::ticked`, `Staging::delta_limit_nanos`). Each host decides when a cycle is due:

- the headless host, on a timer's grid (`Headless::run`, `running::Timer`);
- the desktop host, as each frame is presented or on a capped grid (`running::pacing::Pacing`);
- the browser host, on each animation frame.

No actor can change any of these.

A step's time is a second clock, also the host's: `Engine::step(now, ..)`, read by `turn.now()`. A request's deadline is a time on it (ADR-0041: `Due::Before`). The host's timer, which `Sleep` and every job wait on, runs on it too (ADR-0018: `Timer`, `Running::until`).

An actor reaches its host through a controller and hears back through a source (ADR-0019: `Controller::push`, `Feed::push`). The window root opens and closes windows that way (ADR-0027: `Windowing`, `Heard`).

`capture_frame` reads a window's frame back, and has a compute job crop, shrink and encode it as a PNG (ADR-0043: `ReadBack { of: Frame(window) }`, `Encode`). The `Files` actor writes a file into the namespace `Save` (ADR-0039: `WriteFile`, `FileWritten`).

## Decision

**A program's clock is held or runs, and a held clock moves only when it is advanced.**

```rust
#[kind(name = "aether.pace.pace")]
pub enum Pace { Real, Held }

#[kind(name = "aether.pace.set_pace")]
pub struct SetPace { pub pace: Pace }                      // -> PaceSet { was: Pace }

#[kind(name = "aether.pace.advance")]
pub struct Advance { pub cycles: u32, pub delta_nanos: u64 }

#[kind(name = "aether.pace.advanced")]
pub enum Advanced {
    Advanced { elapsed_nanos: u64 },                       // the program's time once the last cycle finished
    Refused(Unadvanced),
}

#[kind(name = "aether.pace.unadvanced")]
pub enum Unadvanced { Running, NoCycles, NoTime, PastLimit { limit_nanos: u64 }, Stopping }
```

- **`Real` is the clock as it is today.** The host says when a cycle is due, and each `Tick`'s delta is the host's time between cycles.
- **While the clock is `Held`, the host says no cycle is due.** Steps still run. Sources still bring what comes in, jobs still finish, and tools are still answered. No `Tick`, `Render` or `Present` is sent, so nothing that moves by the cycle moves, and no window is drawn.
- **`Advance` runs `cycles` cycles, one after another, each with a `Tick` of exactly `delta_nanos`.** Each cycle begins only once the one before it has finished, as every cycle does (ADR-0020). It is answered once the last has finished, with the program's elapsed time then.
- **A held delta is never limited, but one past the limit is refused** (`PastLimit`). The limit exists because a stall makes a long delta that nobody chose. A held delta is chosen, so it is taken exactly. But every actor is written for deltas within the limit, so a longer one is refused rather than handed to them.
- **`Advance` is refused in these cases:**
  - the clock runs (`Running`);
  - `cycles` is zero (`NoCycles`);
  - `delta_nanos` is zero (`NoTime`);
  - the program is stopping (`Stopping`).

  A tool that wants to step holds the clock first, so a clock that runs is never frozen as a side effect.
- **Several `Advance`s are run in the order they were asked**, each once the one before has been answered. Advancing one cycle at a time is how a tool sees each cycle's frame.
- **Setting the clock running again counts from that moment.** The first `Tick` after `Real` is set has as its delta the host's time since then, so a clock held for a minute adds no minute to the program's time. `elapsed_nanos` goes on from where the last held cycle left it.
- **Only the program's time is held: the step's time runs.** `turn.now()`, request deadlines, `Sleep` and every job's timer stay on the host's own clock. A held program keeps answering its tools before their deadlines, its jobs still time out, and a request from the hub into a held engine still expires (ADR-0041). The program's time is what an actor animates by, and an actor that animates from `turn.now()` is not held. The guide says to animate from `Tick`.

**The clock is a root of its own, `Pacer`, in a new crate `aethereal-pace`, which reaches its host through a controller and hears back through a source.**

```rust
pub enum Pacing {          // what Pacer pushes to the host
    Hold,
    Run,
    Cycle { delta_nanos: u64 },
}

pub enum Paced {           // what Pacer's source brings
    Cycled { elapsed_nanos: u64 },
}

#[kind(name = "aether.pace.config")]
pub struct PacerConfig { pub pace: Pace }       // how the clock starts: Real unless set
```

- **`Pacer` keeps the pace, and the host does what it is told.** `SetPace` is answered in its own turn: the pace changes as the host takes `Hold` or `Run` between steps, before any later cycle is due. `Advance` is held (ADR-0017: `turn.hold`). For each cycle, `Pacer` pushes `Cycle`. The host calls `Engine::cycle_by(delta)` and pushes `Cycled` once that cycle has finished. Only then does `Pacer` push the next `Cycle`, and after the last it answers the `Advance`. So `Pacer`'s state alone says how far an advance has got, and the host keeps no count of it.
- **The engine gains two calls for the host:**
  - `Engine::cycle_by(delta)`: a cycle due now, whose `Tick` carries `delta` exactly, measured from no host time.
  - `Engine::resume(at)`: the next cycle the host says due counts its delta from `at`.

  `Engine::has_cycled()` says whether the last cycle due has finished, read from what the engine already holds, as a stage's end is (`Engine::has_finished`).
- **Every host implements the controller:**
  - The headless host's run loop skips its timer while held.
  - The desktop host's pacing gains the held case: no frame is asked for until a `Cycle` comes.
  - The browser host takes animation frames and starts no cycle from them while held.
- **A program may start held** (`PacerConfig { pace: Held }`). Its first `Tick` is then one an advance chose, so two runs of it see the same deltas from their first frame.
- **`Pacer` is listed by the chassis, as the window root is.** A program that lists no `Pacer` keeps the clock it has today, and no actor can hold it.
- **One clock holds every window.** Windows are drawn in one cycle (ADR-0025), so a held clock holds them all, and one advance draws each window that is shown.

**The tool `record_clip`, `RecordTool`, steps a held clock and writes each frame a window draws as a PNG.** It is the renderer's, in `aethereal-render` behind its `tools` feature, beside `capture_frame`, and reuses its `Encode` job. That feature then also depends on `aethereal-pace` and `aethereal-files`.

```rust
#[kind(name = "aether.render.record")]
pub struct Record {
    pub window: WindowId,
    pub frames: u32,                   // at most MOST_FRAMES, 3600
    pub frames_per_second: u32,        // 1 to 240
    pub crop: Crop,                    // as capture_frame's
    pub longest: u32,                  // as capture_frame's
    pub name: FilePath,                // the folder in Save, written as clips/<name>/00000.png, ..
}

#[kind(name = "aether.render.recorded")]
pub struct Recorded { pub folder: FilePath, pub frames: u32, pub frames_per_second: u32, pub width: u32, pub height: u32 }
```

```
record_clip { window, frames, frames_per_second, crop, longest, name }
  → RecordTool ─ SetPace { Held } ─▶ Pacer                      ◀─ PaceSet { was }
  for each frame i:
    → ReadBack { of: Frame(window) } ─▶ Renderer
    → Advance { cycles: 1, delta_nanos: δᵢ } ─▶ Pacer           ◀─ Advanced
    ◀─ Pixels::Read
    → Encode ─▶ a compute job                                   ◀─ Encoded { png }
    → WriteFile { Save, clips/<name>/<i>.png, png } ─▶ Files     ◀─ FileWritten::Written
  → SetPace { was } ─▶ Pacer
  → Called::Output(Recorded)
```

- **Each frame is the frame of exactly one cycle.** The readback is asked before the cycle that draws it. While held, nothing else is drawn, so the frame that answers it is that cycle's.
- **The deltas sum to the clip's length exactly.** Frame `i`'s delta is `⌊(i+1)·10⁹ / fps⌋ − ⌊i·10⁹ / fps⌋` nanoseconds, so after `n` frames the program has moved `⌊n·10⁹ / fps⌋`, never drifting by a rounding a frame.
- **Drawing runs one cycle at a time, and encoding and writing run beside it.** The next frame is asked for as soon as the last one's pixels arrive. At most four frames are being encoded or written at once, and drawing waits for one of them to finish past that, so a slow disk bounds what the tool holds.
- **The clock is left as the tool found it.** `PaceSet` answers what the pace was. The tool sets it back once the clip is written, and also when it fails. A program held before the clip stays held, at the time the clip ended.
- **The answer is a value, not an image.** It names the folder in `Save` and the clip's size and rate. The agent finds the folder under the save directory it gave the engine (ADR-0039: the host's `save` setting), and makes an MP4 from it with a tool of its own, such as `ffmpeg -framerate <fps> -i %05d.png`.
- **Every refusal is a result the agent reads:**
  - the pacer's and the renderer's;
  - a frame count or rate out of range;
  - a crop off the frame;
  - a write that failed, naming the frame;
  - a program with no `Pacer`, found when `SetPace` reaches no actor.

## Consequences

- **A clip plays in exact real time however slowly its frames are drawn,** and two runs of a deterministic program are the same clip, frame for frame. Recording takes as long as drawing does: faster than real time for a light scene, and slower for a heavy one, with the clip the same either way.
- **What a job brings in lands when it lands.** A held clock does not hold jobs. So a scene still loading textures, or one fed from outside, can differ between two recordings, by when each job finished against the held cycles.
- **A held window draws nothing.** A held program that is resized or uncovered shows its last frame, or none, until it is advanced.
- **On a display that presents `fifo`, each held cycle still waits for the display,** so a desktop records at most at the display's rate. Uncapped presenting records as fast as frames draw.
- **Frames are kept as PNGs, not encoded as video.** An MP4 needs an encoder the engine does not have, and the folder is what any encoder reads. A clip of 3600 frames at 1280 × 800 is a few gigabytes, so the cap bounds it.
- **Actors that animate from `turn.now()` are not held.** Moving them to `Tick` is the fix, and the guide's rule.
- **`Pacer` is listed by each chassis that offers it.** The desktop, headless and browser chassis list it, and a toolbox lists `RecordTool` as it lists `CaptureTool`.
- **The desktop's private `Pacing` is renamed `Presented`,** since `Pacing` is now the controller's message.
- **Later:**
  - a held clock for the headless harness's tests, which already step by count (`Budget::Steps`);
  - injecting input at a held cycle, so a scripted walk replays exactly;
  - an MP4 written by the engine.

## Alternatives considered

- **A host mode set at launch, with no actor.** The headless or desktop host could take a flag and advance a fixed `dt` a cycle. But nothing could hold a running program, step it from a tool, or let it run again, and the agent would relaunch the engine for each clip.
- **Holding the step's time with the program's.** One clock, and `turn.now()` would follow `Tick`. But a held engine would never let a request expire, a hub call into it would hang, and every job's timer would need a time no thread waits on. The step's time measures waiting, and the program's time is what is shown, so they are two clocks.
- **The clock in the substrate, with no actor.** The lifecycle is the engine's, but holding it is a capability one program has and another does not. So it is a capability in a crate of its own, as windows are (ADR-0024), and the substrate names none of it.
- **An `Advance` that runs while the clock runs.** No `SetPace` would be needed, but cycles would mix chosen deltas with the host's, and an advance would race the host's own cycles.
- **Frames returned to the agent as images.** The agent judges motion from video, not from a sequence of images, and 60 images a second costs its tokens for nothing it can play.
- **A delta of `1/fps` rounded once.** At 60 frames a second each delta is 16 666 666 ns, and a minute's clip would end 2.4 µs short. Small, but the clip is stated to be exact.
- **Recording in the renderer, every frame it draws.** No tool steps the clock, but the frames are the host's rate again, which is what drifts today.
