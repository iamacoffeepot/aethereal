# ADR-0046: Input a tool performs

- **Status:** Proposed
- **Date:** 2026-11-03

## Context

An agent drives a spawned engine through its tools. It can step its clock and record its windows (ADR-0045), but it cannot do what a person at a window does. To try a walk in the desktop program (click to move, W A S D, Shift to run), to replay what the owner did, or to check an interaction before asking the owner to try it, the agent needs to send a window clicks, pointer moves, and keys, at the window's coordinates, as a person would.

What is already decided:

- **What a window hears comes from its chassis, through the window root's one source,** to the actor of that window, which sends it to the actors subscribed (ADR-0024: `WindowFeed::push`, `Heard::Event`; `Windows`, `Window`, `Subscribe`). A position is in the window's surface pixels; a key is named as the W3C UI Events name it (`Event::KeyDown { code, key, repeat }`).
- **A window's actor sends on only the events its root sent it** (`Handles<Event> for Window` checks `turn.parent()`). Some events say how the window stands: `Resized`, `Moded`, `FocusGained`, `FocusLost`, `CloseRequested`, `Closed`. The window's actor and the root keep that state, and only the chassis may say it, so no other actor can tell a window's subscribers it closed or changed size.
- **The input events change no state either actor keeps:** `PointerMoved`, `PointerDown`, `PointerUp`, `Wheel`, `KeyDown`, `KeyUp` (`Window::heard` answers `true` for each and keeps nothing; `Shown::heard` ignores them).
- **A window's actor is reached at `path::<Window>(window)`,** beneath the root, keyed by its `WindowId` (ADR-0027). A request whose receiver is gone is answered `Answered::No` by the engine (ADR-0017: `Unanswered`).
- **An actor handles the mail it had at a step before the mail that arrives later,** and carried items come first in the next step (ADR-0011).
- **A tool is an actor in the crate its work belongs to, behind that crate's `tools` feature,** and reuses that crate's kinds (ADR-0037: `Tool`, `Toolbox`). One tool may offer several (`Tool::sheets`). The agent reaches it through the hub's `call_tool` (ADR-0038).
- **The pacer holds and advances the program's clock when asked** (ADR-0045: `Pacer`, `SetPace`, `Advance`), but only `record_clip` asks it. An agent has no tool that holds the clock or advances it, so it cannot press a key, run exactly 60 cycles, and release it.

## Decision

**A window's actor takes a request to perform input: a list of input events, which it sends its subscribers as though its chassis had heard them, and then answers. A tool, `send_input`, asks it on the agent's behalf. Two more tools, `set_pace` and `advance`, ask the pacer, so an agent spaces its input by exact cycles.**

```
agent ── call_tool { engine, tool: "send_input", input: { window, inputs } } ──> hub ══ link ══> InputTool
  InputTool ── Perform { inputs } ──> Window at path::<Window>(window)
  Window ── each input, as an Event ──> its subscribers; then answers Performed
```

### What a window's actor takes

```rust
/// One thing a person does at a window: an `Event` that changes nothing about how the window stands.
#[derive(Plain)]
pub enum Input {
    PointerMoved { x: f32, y: f32 },
    PointerDown { x: f32, y: f32, button: Button },
    PointerUp { x: f32, y: f32, button: Button },
    Wheel { x: f32, y: f32, delta_x: f32, delta_y: f32 },
    KeyDown { code: String, key: String, repeat: bool },
    KeyUp { code: String, key: String },
}
impl From<Input> for Event { /* the variant of the same name */ }

/// Asks a window's actor to send its subscribers `inputs`, in order, as its chassis would have.
#[kind(name = "aether.window.perform")]
pub struct Perform { pub inputs: Vec<Input> }

#[kind(name = "aether.window.performed")]
pub enum Performed { Performed, Refused(Unperformed) }

pub enum Unperformed {
    /// No window's actor is at the path: the window never opened, or it has closed. The tool answers this.
    NotOpen,
    /// Input `at`, counted from 0, has a position or a delta that is not finite, which no chassis sends.
    NotFinite { at: u32 },
    /// More inputs than the most one request performs.
    TooMany { most: u32 },
}
```

- **`Input` is its own enum, not `Event`,** so a request cannot say a window resized, changed mode, gained or lost the keyboard, or closed. Those stay the chassis's to say.
- **`Window` handles `Perform` from any sender** (`Carries<Perform>`, answered `Performed`), and checks every input before it sends any. A refused request sends nothing.
- **Its subscribers are sent the inputs in the turn that answers,** so each handles them before anything arriving after the answer, such as the `Tick` of a cycle the agent asks the pacer for once it has the answer (ADR-0011). Pressing a key, then advancing the held clock, gives the press to the cycle's first `Tick`.
- **Performed input is sent whether or not the window has the keyboard,** and at any position, inside the surface or not. A chassis sends neither of those to a covered window. The agent drives windows that are covered, and a program that ignores keys while unfocused still does.
- **At most 1024 inputs a request.** That is far more than a script's step needs, and it keeps one turn's sends bounded.

### `send_input`

```rust
#[kind(name = "aether.window.send_input")]
pub struct SendInput { pub window: WindowId, pub inputs: Vec<Input> }
// answered with `Performed`
```

- **`InputTool`, in `aethereal-window` behind its `tools` feature,** asks `Perform` of the window's actor at `path::<Window>(window)` and answers what it answered. It answers `Answered::No` as `Unperformed::NotOpen`.
- **A click is three inputs, as a person's is:** `PointerMoved` to the point, then `PointerDown` and `PointerUp` there. A held key is a `KeyDown` in one call and its `KeyUp` in a later one, with the clock advanced between them.
### `set_pace` and `advance`

- **`PaceTool`, in `aethereal-pace` behind a new `tools` feature,** offers two tools that take the pacer's own kinds and answer its answers: `set_pace` takes `SetPace` and answers `PaceSet`, and `advance` takes `Advance` and answers `Advanced`. It asks `path::<Pacer>(())`, and answers `Answered::No` as a refusal saying the program lists no pacer.
- **It keeps no state.** A clock it held stays held until the agent sets it running, or `record_clip` sets it back to how it found it.
- **The usual script:**
  1. `set_pace { pace: Held }`
  2. `send_input { window: 0, inputs: [KeyDown { code: "KeyW", key: "w", repeat: false }] }`
  3. `advance { cycles: 60, delta_nanos: 16666667 }`, one second at 60 Hz; or `record_clip { frames: 60, frames_per_second: 60, … }` to record that second of the walk.
  4. `send_input { window: 0, inputs: [KeyUp { code: "KeyW", key: "w" }] }`
  5. `capture_frame` to see where it ended, then `set_pace { pace: Real }`.

### What changes for programs

- `linked_window` lists `InputTool` and `PaceTool` in its toolbox. A program that does not list `InputTool` answers no `send_input`, and its windows still take `Perform` from its own actors.

## Consequences

- **An agent plays any desktop or browser program as a person at it would,** through one tool, and with the held clock (ADR-0045) the same script gives the same frames every run.
- **A program cannot tell performed input from a person's.** That is the point. It also means any actor in the engine may perform input on a window, as any actor may already send it mail.
- **The root and the window's actor keep no input state,** so nothing goes stale. A key the tool pressed and never released stays pressed for every program that tracks keys itself, until a `KeyUp` comes. The tool does not release keys for an agent that stops partway through.
- **A person's input and performed input interleave** in the order they reach the window's actor. An agent scripting a window the owner is also using shares it with them.
- **A headless program has no windows,** so it answers every `send_input` with `NotOpen`.
- **Follow-on work this leaves open:**
  - recording a person's input as a list of `Input`s with the cycle each came in, to replay later;
  - a script tool that takes inputs and advances in one call;
  - the focus and resize events a tool might also want to fake, if a program needs them tested.

## Alternatives considered

- **The chassis performs the input, through the window root's controller,** pushing it onto the root's feed as though the windowing library had heard it. Subscribers get the same events, but every chassis needs a new request and handling for it, and the root a request to hold, to reach the actor it already sends to.
- **Synthetic events through the OS** (uinput on Linux, `CGEvent` on macOS). That is truly what a person's input is, but it needs a focused, uncovered window, OS permissions, and a separate path for each platform and the browser. It also moves the pointer of the desktop the owner is using.
- **Letting other actors send `Event` to a window's actor.** It needs no new kind, but anyone could then say a window resized or closed, which the root and the window's actor keep as state and only the chassis knows.
- **`send_input` takes a cycle count to wait between inputs.** One call could script a walk, but the tool would then step the clock itself, which `advance` already does. The agent also could no longer look at a frame partway through.
- **The tool keeps the keys and buttons it pressed, and releases them when it ends or the call fails.** That guards against a stuck key, but a call ending is the normal way to hold a key across an advance, so the tool cannot tell a held key from a forgotten one.
