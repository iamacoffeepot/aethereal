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

  `Engine::has_cycled()` says whether the last cycle due has finished, read from what the engine already holds, as a stage's end is (`Engine::has_finished`). `Engine::elapsed()` is the program's time the host reports in `Cycled`, and `Engine::delta_limit()` is the limit the pacer refuses past, read by its bootstrap.
- **Every host implements the controller:**
  - The headless host's run loop skips its timer while held.
  - The desktop chassis runs each held cycle as a frame of its own, from its loop as soon as the cycle is owed, and never waits for the window to be redrawn. **It does not wait for a redraw**: a Wayland compositor sends a covered window no frame callback, so winit never redraws it, and an advance would wait until the window was uncovered. After a held frame is presented, the chassis waits for the GPU to finish it, so a frame read back that cycle is mapped before the loop next waits. A running clock has a next frame to poll the device for it; a held one has none. The chassis also takes the pacer's requests before each frame reads the clock, so a program that starts held runs no first cycle with a delta measured on its own clock.
  - **A window presented `fifo` is presented only after the compositor has asked for its next frame,** held or not: the chassis calls winit's `pre_present_notify` just before each `fifo` present, so winit holds the window's next redraw until the compositor's frame callback. It draws the next frame ahead, holds it, and presents it as that redraw comes, so the present lands at once, as it did when the driver waited. Mesa's Wayland driver waits, as it presents a `fifo` frame, for the callback of the frame before, and a compositor sends a covered window none, so a present issued earlier waited on the event loop's thread, with every actor it steps, until the window was uncovered. A held cycle owed while a window still holds its frame waits a few frames for that redraw, then runs undrawn; while the clock runs and every window shown has waited that long, the cycles run on undrawn at the frame budget, each telling its frame statistics it drew nothing.
  - The browser host takes animation frames and starts no cycle from them while held.
- **A program may start held** (`PacerConfig { pace: Held }`). Its first `Tick` is then one an advance chose, so two runs of it see the same deltas from their first frame.
- **`Pacer` is listed by the chassis, as the window root is.** A program that lists no `Pacer` keeps the clock it has today, and no actor can hold it.
- **One clock holds every window.** Windows are drawn in one cycle (ADR-0025), so a held clock holds them all, and one advance draws each window that is shown.

**The tool `record_clip`, `RecordTool`, steps a held clock and encodes each frame a window draws into one MP4.** It is the renderer's, in `aethereal-render` behind its `tools` feature, beside `capture_frame`, and cuts and shrinks each frame as `capture_frame` does. That feature then also depends on `aethereal-pace`, `aethereal-files`, and, natively, `openh264` (below: *Clips as MP4s*).

```rust
#[kind(name = "aether.render.record")]
pub struct Record {
    pub window: WindowId,
    pub frames: u32,                   // at most MOST_FRAMES, 3600
    pub frames_per_second: u32,        // 1 to 240
    pub crop: Crop,                    // as capture_frame's
    pub longest: u32,                  // as capture_frame's; each side then rounded down to even
    pub name: FilePath,                // written in Save as clips/<name>.mp4
}

#[kind(name = "aether.render.recorded")]
pub struct Recorded { pub file: FilePath, pub frames: u32, pub frames_per_second: u32, pub width: u32, pub height: u32 }
```

```
record_clip { window, frames, frames_per_second, crop, longest, name }
  → RecordTool ─ SetPace { Held } ─▶ Pacer                      ◀─ PaceSet { was }
  for each frame i:
    → ReadBack { of: Frame(window) } ─▶ Renderer
    → Advance { cycles: 1, delta_nanos: δᵢ } ─▶ Pacer           ◀─ Advanced
    ◀─ Pixels::Read
    → EncodeFrame { encoder: give(e), i, .. } ─▶ a compute job   ◀─ FrameEncoded { encoder }   (e = take(..))
  → FinishClip { encoder: give(e) } ─▶ a compute job            ◀─ ClipFinished { mp4 }
  → WriteFile { Save, clips/<name>.mp4, mp4 } ─▶ Files           ◀─ FileWritten::Written
  → SetPace { was } ─▶ Pacer
  → Called::Output(Recorded)
```

- **Each frame is the frame of exactly one cycle.** The readback is asked before the cycle that draws it. While held, nothing else is drawn, so the frame that answers it is that cycle's.
- **The deltas sum to the clip's length exactly.** Frame `i`'s delta is `⌊(i+1)·10⁹ / fps⌋ − ⌊i·10⁹ / fps⌋` nanoseconds, so after `n` frames the program has moved `⌊n·10⁹ / fps⌋`, never drifting by a rounding a frame.
- **Drawing runs one cycle at a time, and encoding runs beside it.** The next frame is asked for as soon as the last one's pixels arrive. Frames are encoded one at a time, in the order read, as the clip has one encoder. At most four frames are read back and not yet encoded, and drawing waits for one of them to be encoded past that, so a slow encoder bounds what the tool holds.
- **The clock is left as the tool found it.** `PaceSet` answers what the pace was. The tool sets it back once the clip is written, and also when it fails. A program held before the clip stays held, at the time the clip ended.
- **The answer is a value, not an image.** It names the MP4 in `Save` and the clip's size and rate. The agent finds the file under the save directory it gave the engine (ADR-0039: the host's `save` setting), and plays it as it is.
- **Every refusal is a result the agent reads:**
  - the pacer's and the renderer's;
  - a frame count or rate out of range;
  - a crop off the frame, or one rounded to less than two pixels a side;
  - a window that changes size during the clip;
  - a frame the encoder refused, naming the frame;
  - an MP4 past what the tool holds, or past the engine's budget for blobs;
  - a write that failed;
  - a browser's engine, which has no encoder;
  - a program with no `Pacer`, found when `SetPace` reaches no actor.

### Clips as MP4s

A job can be given a resource (ADR-0018: `Running::take`), so an encoder that keeps state from frame to frame encodes each frame on a compute thread.

**`record_clip` writes one H.264 MP4, `clips/<name>.mp4`, which the engine encodes.** The engine encodes it because the machine that runs the engine has no `ffmpeg`, and the hub is the only surface agents have, so a folder of PNGs the agent must encode itself is a clip it cannot play.

```rust
pub struct ClipEncoder { h264: H264, mp4: Mp4, next: u32 }      // a Resource
impl ClipEncoder {
    pub fn new(frames_per_second: u32, width: u32, height: u32) -> Result<Self, Unencodable>;
    pub fn encode(&mut self, frame: u32, rgb: &[u8]) -> Result<(), Unencodable>;   // refuses any but `next`
    pub fn finish(self) -> Result<Vec<u8>, Unencodable>;                           // the MP4, whole
}

#[kind(name = "aether.render.encode_frame")]
pub struct EncodeFrame { pub encoder: Given<ClipEncoder>, pub frame: u32, pub pixels: Bytes, .. }  // -> FrameEncoded

#[kind(name = "aether.render.frame_encoded")]
pub enum FrameEncoded { Encoded { encoder: Given<ClipEncoder> }, Refused(Unencodable) }

#[kind(name = "aether.render.finish_clip")]
pub struct FinishClip { pub encoder: Given<ClipEncoder> }                         // -> ClipFinished { mp4: Bytes }
```

- **The encoder is a resource, passed job to job** (ADR-0018: a job is given a resource and hands it back). The tool makes it, at the first frame's size, and keeps it (`turn.keep`). Each frame's compute job is given it (`turn.give`), takes it (`Running::take`), cuts and shrinks the frame as `capture_frame` does, converts it to YUV 4:2:0, encodes it, and hands the encoder back in its answer (`Running::hand`), which the tool takes (`turn.take`). So only one frame is encoded at a time, the encoder's state never crosses two threads at once, and drawing still runs beside it.
- **The encoder refuses a frame out of order.** It counts the frames it has encoded, and a frame numbered other than the next is refused, so a clip never plays out of order or repeats a frame.
- **The MP4 is written in memory as the frames are encoded,** one H.264 track: each frame a sample in the `mdat`, its NAL units after their lengths; the sequence and picture parameter sets in the `avcC`; a constant rate, each sample lasting one unit at a timescale of `frames_per_second`; and the `moov` after the frames, once the last is in. A last compute job takes the encoder, finishes the file, and answers it as one blob, which `Files` writes whole (ADR-0039: `WriteFile`).
- **The encoder is Cisco's OpenH264, compiled from source into the engine** by the `openh264` crate's `source` feature, under BSD-2-Clause, with no library installed beside the engine. `nasm` is optional at build time: on x86 and x86-64 the assembly is built and used where `nasm` is found, and the C++ alone encodes where it is not; on ARM the C compiler builds its NEON code, with no `nasm`. CI installs `nasm`, so its builds carry the assembly. Each frame is cut into slices and encoded on as many threads as the machine has cores, up to four, OpenH264's most. On one thread and without the assembly, a 1080p frame took about 50 ms to convert and encode on the machine that runs the engine, so a minute at 60 frames a second encoded in about three minutes, beside its drawing. In a timing of 1080p frames on that machine, the assembly encoded a frame about three times as fast as the C++ alone, and four threads took 35 to 50% off each frame's encode against one. The MP4 writer is the renderer's own, about three hundred lines: the maintained crates that write one either read and write every box, with `serde` behind them, or bind a C muxer, for what is one track and a dozen boxes. A browser's engine has no encoder, as OpenH264 is not built for wasm32, and its `record_clip` refuses a clip before holding the clock.
- **Each side of a frame is rounded down to even,** as YUV 4:2:0 stores one colour for each two by two pixels. A frame not shrunk loses its last column or row rather than being resampled for one pixel, so its text stays sharp. Every frame of a clip is one size: a window resized during the clip refuses it.
- **A clip is encoded at constant bit rate, 10 Mb/s at 1080p and 60 frames a second, and at the same bits a pixel at any other size and rate,** with the quantizer kept at 36 or finer so a window's text and lines stay legible where much moves at once, and never skipping a frame. A frame a player may start at comes every two seconds. Colour is BT.601 at video range, as OpenH264 converts it, and the stream says so.
- **The clip's bytes are bounded twice.** The encoder refuses a frame that would take the MP4 past 512 MiB, which it holds in memory. The finished file is put as one blob, so it must also fit the engine's budget for blobs (`blob_bytes`, 64 MiB unless set): a file past it answers the finishing job `Faulted`, and the tool refuses the clip saying so. A minute at 1080p is about 75 MB.
- **A clip that fails drops its encoder.** One the tool holds is given to no mail and dropped as the turn ends; one a job holds comes back in an answer the clip no longer takes. The tool's log records each (ADR-0010).

### Capturing a held clock

The next frame drawn never comes under a held clock until it is advanced, so `capture_frame` cannot wait for it there.

- **The renderer keeps each window's last frame:** one texture a window, of the frame's size and format, copied into from each frame drawn onto the window as its `Render` ends, and made again only when the size or the format changes. A surface that cannot be copied from keeps none.
- **`Readable::Last(window)` reads it at once,** refused `Undrawn` when none is kept. What it shows is the last thing that ran and drew to the screen.
- **`capture_frame` asks the pacer how the clock moves** (`ReadPace`, answered `PaceRead { pace }`), and reads `Last` under a held clock and the next frame while it runs, or when no pacer answers. So a held capture runs nothing and moves nothing, and matches what `read_target` reads.
- **A redraw is not used:** a cycle of `Render` alone re-runs every pass, compute dispatches among them, so a capture would step a program's GPU state and change every frame after it. It would also wait forever on a window not drawn, and answer a capture made during an advance with a frame partway through it.

## Consequences

- **A clip plays in exact real time however slowly its frames are drawn,** and two runs of a deterministic program are the same clip, frame for frame. Recording takes as long as drawing does: faster than real time for a light scene, and slower for a heavy one, with the clip the same either way.
- **What a job brings in lands when it lands.** A held clock does not hold jobs. So a scene still loading textures, or one fed from outside, can differ between two recordings, by when each job finished against the held cycles.
- **A held window draws nothing.** A held program that is resized or uncovered shows its last frame, or none, until it is advanced.
- **Each window costs one frame of GPU memory and one frame-sized copy each frame,** for its last frame: about 8 MB and 20 µs at 1080p.
- **On a display that presents `fifo`, each held cycle waits for the compositor to ask for the window's next frame,** so a desktop may record no faster than the display's rate. Uncapped presenting records as fast as frames draw. On eve, two engines that started held, one covered by the other, each recorded 30 frames at once, and their clips matched byte for byte. A window covered whole is asked for no frame, so its held cycles run undrawn, and a frame read in one is refused as unshown rather than waited on.
- **Frames are encoded as H.264, in the engine, as they are drawn.** The engine links OpenH264, built from its C++ source: every native build compiles it, and the browser's does not. A clip is lossy, where a folder of PNGs would be exact; a frame to compare pixel for pixel is `capture_frame`'s.
- **Actors that animate from `turn.now()` are not held.** Moving them to `Tick` is the fix, and the guide's rule.
- **`Pacer` is listed by each chassis that offers it.** The desktop, headless and browser chassis list it, and a toolbox lists `RecordTool` as it lists `CaptureTool`.
- **The desktop's private `Pacing` keeps its name.** It is not exported, and nothing in the desktop names the controller's `Pacing`, so the two never meet.
- **Later:**
  - a held clock for the headless harness's tests, which already step by count (`Budget::Steps`);
  - injecting input at a held cycle, so a scripted walk replays exactly.

## Alternatives considered

- **A host mode set at launch, with no actor.** The headless or desktop host could take a flag and advance a fixed `dt` a cycle. But nothing could hold a running program, step it from a tool, or let it run again, and the agent would relaunch the engine for each clip.
- **Holding the step's time with the program's.** One clock, and `turn.now()` would follow `Tick`. But a held engine would never let a request expire, a hub call into it would hang, and every job's timer would need a time no thread waits on. The step's time measures waiting, and the program's time is what is shown, so they are two clocks.
- **The clock in the substrate, with no actor.** The lifecycle is the engine's, but holding it is a capability one program has and another does not. So it is a capability in a crate of its own, as windows are (ADR-0024), and the substrate names none of it.
- **An `Advance` that runs while the clock runs.** No `SetPace` would be needed, but cycles would mix chosen deltas with the host's, and an advance would race the host's own cycles.
- **A folder of PNGs, made into a video by the agent's own encoder.** Exact frames, and no encoder in the engine. But the machine that runs the engine has no `ffmpeg`, and the hub is the only surface agents have, so the agent could not make the video it judges motion from.
- **A system encoder, `ffmpeg` or a platform's, called by the engine.** It would add a dependency every machine that runs the engine must install, and differ from one platform to the next; OpenH264 compiled in is the same everywhere natively.
- **Frames returned to the agent as images.** The agent judges motion from video, not from a sequence of images, and 60 images a second costs its tokens for nothing it can play.
- **A delta of `1/fps` rounded once.** At 60 frames a second each delta is 16 666 666 ns, and a minute's clip would end 2.4 µs short. Small, but the clip is stated to be exact.
- **Recording in the renderer, every frame it draws.** No tool steps the clock, but the frames are the host's rate again, which is what drifts today.
