# ADR-0047: Listing and placing windows

- **Status:** Proposed
- **Date:** 2026-11-03

## Context

An agent driving an engine has to name a window for `capture_frame`, `record_clip`, and `send_input` (ADR-0043, ADR-0045, ADR-0046). It cannot ask which windows there are, so it guesses the first window is `0`. It also cannot lay windows out for the owner: it can show one fullscreen (`SetMode`), but cannot resize or move one. And the hub's `list_engines` shows each engine by its link name, `"engine"` unless the program was started with `--name`.

What is already decided:

- **The window root keeps how each open window stands,** with the requests it holds for it (ADR-0027: `Windows`, `Standing`, `Shown`). `Shown` keeps the surface and mode, and takes them in from the events it sends on (`Shown::heard`). It keeps no title and no focus. A window opened as the program is set up is given no title by the window capability: the desktop chassis passes its settings' title straight to winit (`WindowSettings::title`, `gpu::native`). A window asked for while the program runs has one (`OpenWindow::title`).
- **The root asks its chassis through its controller, and the chassis answers through the root's source** (ADR-0027: `Windowing`, `Hosting`, `Heard`). `SetMode` is the pattern: the root holds the request, the chassis does it between steps and pushes `Heard::Set` with the surface as it draws it now, and the window's subscribers hear `Resized` as for any resize.
- **A page opens, closes, and sets the mode of no window on request** (`Hosting::refuse`, `Unset::NoHost`): it owns its canvases.
- **Tools reuse their capability's kinds** (ADR-0037, ADR-0046).
- **On Wayland a program cannot place its own window, or read where it is.** winit's `set_outer_position` does nothing there and `outer_position` answers `NotSupported`; the compositor alone places windows. Resizing a window that is not maximized or fullscreen works everywhere winit runs (`request_inner_size`). eve's desktop is Wayland.

## Decision

**The window root keeps each window's title and focus, answers a list of its windows, and takes two more requests: to resize a window and to move one. A tool, `WindowTool`, offers each of them, and `SetMode`, to the agent.**

### The root keeps a title and focus

- **`Opening::open` takes the window's title,** as `OpenWindow` does, and `Initial` carries it. The desktop passes its settings' title; the browser passes the canvas's id.
- **`Shown` keeps the title and whether the window has the keyboard,** taking focus in from `FocusGained` and `FocusLost` as it takes the surface from `Resized`.

### Listing

```rust
#[kind(name = "aether.windows.list_windows")]
pub struct ListWindows;

#[kind(name = "aether.windows.window_list")]
pub struct WindowList { pub windows: Vec<Listed> }   // open windows, in the order of their ids

pub struct Listed { pub window: WindowId, pub title: String, pub surface: Surface, pub mode: Mode, pub focused: bool }
```

- **Answered at once, from what the root keeps,** with no ask of the chassis. A window still opening is not listed, as it cannot be named yet; one asked to close still is, until its chassis says it closed.

### Resizing and moving

```rust
#[kind(name = "aether.windows.resize_window")]
pub struct ResizeWindow { pub window: WindowId, pub size: Logical }
#[kind(name = "aether.windows.resized")]
pub enum Resized { Resized { surface: Surface }, Refused(Unplaced) }

#[kind(name = "aether.windows.move_window")]
pub struct MoveWindow { pub window: WindowId, pub at: Place }   // the outer frame's top left, logical pixels
#[kind(name = "aether.windows.moved")]
pub enum Moved { Moved, Refused(Unplaced) }

pub struct Place { pub x: f64, pub y: f64 }

pub enum Unplaced {
    /// The window is not open: it never was, it is still opening, it is closing, or it closed first.
    NotOpen,
    /// It is maximized or fullscreen, which sets its size and place: set it windowed first.
    NotWindowed,
    /// The chassis resizes or moves no window, as a page does not, or as Wayland lets no program move its own.
    NoHost,
}
```

- **Each is held and asked of the chassis,** as `SetMode` is, through two more `Windowing` requests, `Resize` and `Move`, answered through two more `Heard` variants, `Resized` and `Moved`, in the order asked.
- **A window not shown windowed is refused at once,** from the mode the root keeps, with nothing asked.
- **`Resized` answers the surface as the chassis draws it now.** A windowing system that resizes later answers the size before; the window's subscribers hear `Resized` once it does, as for any resize.
- **The desktop moves a window with `set_outer_position`,** and refuses `NoHost` where winit cannot place windows: on Wayland, every move. The browser refuses both.

### `WindowTool`

`WindowTool`, in `aethereal-window` behind its `tools` feature beside `InputTool`, offers four tools, each taking the root's kind and answering its answer:

| tool | takes | answers |
|---|---|---|
| `list_windows` | `ListWindows` | `WindowList` |
| `resize_window` | `ResizeWindow` | `Resized` |
| `move_window` | `MoveWindow` | `Moved` |
| `set_window_mode` | `SetMode` | `ModeSet` |

It asks `path::<Windows>(())`, which a program must list before it, as `PaceTool` asks the pacer (ADR-0046).

### The hub's engine names

- **The hub is unchanged.** An agent that spawns an engine names it with `--name` among the spawn's args, and `list_windows` reads each engine's window titles. `linked_window` keeps its default.

## Consequences

- **An agent learns an engine's windows instead of guessing,** with each one's id, title, size, mode and focus, and names any of them to the other tools.
- **It can lay out windows where the platform lets a program:** resize anywhere a window is windowed, and move on X11, macOS and Windows. On eve's Wayland desktop, moving is refused, and the owner or the compositor places windows.
- **A browser lists its canvases and resizes and moves none of them.**
- **Every window has a title,** so the desktop and browser chassis each change how they open their first windows.
- **Follow-on work this leaves open:** `SetTitle`; a window's place as winit reports it, where the platform reports one; the hub showing what an engine says of itself beyond its name.

## Alternatives considered

- **Listing from the window actors,** each asked for how it stands. Each already keeps its surface, mode and focus, but a list would need an ask of every window and a gather of the answers, where the root holds all of it but focus and title in one place.
- **The link names an engine after its first window's title.** It would fix `list_engines` with no flag, but the link capability would have to know the window capability, which is not its to know, and a headless engine has no window.
- **Leaving move out, as eve cannot do it.** It costs one request and one refusal, and the agent may drive the desktop on other platforms; the refusal tells it why rather than leaving it to guess.
