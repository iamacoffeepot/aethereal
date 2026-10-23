# ADR-0027: Windows opened and closed while the program runs

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

A chassis opens its windows as it sets the program up (ADR-0024: `Opening::open`, `WindowsConfig`), and since #253 it may open several, each with a surface of its own that the render root draws each frame through (ADR-0021: `WindowSurface`, `Front`, `Present`). No actor can ask for a window once the program runs, nor close one, nor show one fullscreen. ADR-0024 left both for a record of their own: "an `OpenWindow` asked of the root, a controller to the host, the host's answer pushed onto the source with the window's id, and the window's actor ended when its window closes". ADR-0019 sketches the same road for every resource the host makes (`Controller`, `Requests`, `Engine::hand`).

What the engine already has to build on:

- A capability asks its host through a controller, and hears back through a source, in the order it asked (ADR-0019: `Controller::push`, `Requests::take`, `Feed::push`). The host acts between steps, on its own thread, where a native window can be made: winit makes one only inside the callbacks it lends an `ActiveEventLoop`.
- A receiver that cannot answer in the turn of the request holds it, and answers from a later turn (ADR-0017: `turn.hold`, `turn.answer`).
- The window root makes an actor for each window beneath it, keyed by its `WindowId`, and routes what its one source brings to that actor (ADR-0024: `Windows`, `Window`, `Heard`). What one source brings keeps its order (ADR-0010).
- The render root keeps one front for every window, keyed by its id, so a window that comes and goes hands it no resource and leaves none kept (ADR-0021: `Front`). It learns each window and its format from its config (`RendererConfig::windows`).
- A page owns its canvases, and an engine in a page cannot make one.

## Decision

**An actor asks the window root for a window, and is answered once the host has made it.**

```rust
type Sends = (Asks<Path<Windows>, OpenWindow, Inventory>, Asks<Path<Windows>, CloseWindow, Inventory>);

turn.to(path::<Windows>(())).ask(&OpenWindow { title, size: Logical { width: 640.0, height: 480.0 } }, &Inventory)?;

impl Handles<Answer<Opened, Inventory>> for Hud { .. }        // Opened::Window { window, surface } | Opened::Refused(Unopened)

turn.to(path::<Windows>(())).ask(&CloseWindow { window }, &Inventory)?;
impl Handles<Answer<Closed, Inventory>> for Hud { .. }        // Closed::Closed | Closed::Refused(Unclosed)

turn.to(path::<Windows>(())).ask(&SetMode { window, mode: Mode::Fullscreen }, &Toggle)?;
impl Handles<Answer<ModeSet, Toggle>> for Game { .. }         // ModeSet::Set { surface } | ModeSet::Refused(Unset)
```

- **A window is opened at a size in the host's logical pixels**, the size a person sees whatever the screen, since its scale is the host's to know only once the window is on a screen. Its answer carries the surface as it was opened, its size in its pixels and its scale (ADR-0024: `Surface`), so the asker draws at the size the window is, as a subscriber learns from `Resized`.
- **The answer is the window's id**, and from then on it is a window as one opened at setup is: an actor reaches it by `path::<Window>(window)` and subscribes to it, and makes a frame that ends on it (ADR-0025: `Ends::Window`).
- **The window root gives the id, in the turn that asks for the window**, from a count that starts after the windows opened at setup, which its config lists. ADR-0024 gave every id on the host's side, as the root had no way back to the host. It has one now, its controller, and the id goes with the request. **This departs from every id being given by the host**: one count, kept in one actor's turns, so ids never repeat and the order windows were asked for is the order of their ids.

**The window root asks the host through a controller, and hears back through its one source.**

```rust
pub enum Windowing {                                   // what the root pushes to the host
    Open { window: WindowId, title: String, size: Logical },
    Close { window: WindowId },
    SetMode { window: WindowId, mode: Mode },
}

pub enum Heard {                                       // what the root's one source brings
    Event { window: WindowId, event: Event },          // a window heard it, as before
    Opened { window: WindowId, surface: Surface, focused: bool },
    Unopened { window: WindowId, why: Unopened },
    Closed { window: WindowId },
    Unclosed { window: WindowId, why: Unclosed },
    Set { window: WindowId, surface: Surface },
    Unset { window: WindowId, why: Unset },
}
```

- **One controller and one source, and the source is the one the root already has.** ADR-0019 gives a host system whose order of use can be observed one controller, so windows opened and closed land in the order the root's turns asked. And a window's opening reaches the root before anything the window heard, as both come through one source, in the order the host pushed them: no event reaches a window's actor before the actor is made. **This departs from ADR-0024, under which the window capability had a source and no controller.**
- **The root holds the request** (ADR-0017: `Held`), pushes `Windowing::Open`, and answers once its source brings `Opened` or `Unopened` for that id: on `Opened` it makes the window's actor beneath it, as it makes those of the windows opened at setup, then answers. A window whose actor is refused is answered `Unopened::Refused`, and the root asks the host to close it.
- **An asker that ends before it is answered does not stop the window.** The window is opened, the answer goes to no one (ADR-0017: an answer to an ended actor is dropped), and the window stands until it is closed. Opening a window is not undone by its asker going, as nothing a capability pushed to its host is undone (ADR-0019).

**The host makes the window, its surface, and the render root's slot for it, between steps.**

- Between steps the chassis takes the root's requests. For each `Open` it makes the native window, then its surface on the GPU the render root draws with, configured in the GPU's format, and adds the window's slot to the render root's front; then it pushes, in this order:
  1. onto the render root's source, `Surfaced::Opened { window, format }`, so the render root knows the window and its format;
  2. onto the window root's source, `Heard::Opened`.

  Both are brought at the same bring. The asker is answered in the step after, and its first frame on the window reaches the render root after that, so the render root always knows a window before any actor can name it to it.
- **The render root hears of windows through a source of its own, `Source<Surfaced>`**, beside the one its mappings come in through. A window is the GPU's to draw to only once its surface is made, which only the host knows; two sources keep no order between them, and a mapping and a window owe each other none.
- **The front's slots change only between steps**, as the host adds and drops them, so the render root, which reads the front only in its turns, never meets a slot half made. The per-frame path, putting a frame and taking it, stays lock-free (ADR-0021).
- **A window the host cannot make is answered `Unopened`**, naming why: `NoHost` from a host that opens no windows, and `Failed` when the windowing library or the GPU would not make it or its surface. A browser's chassis answers every `Open` with `NoHost`: the page owns its canvases, and the engine cannot make one. A page that wants another window hands the chassis another canvas as it sets the program up.

**Closing a window is asked of the root, and ends its actor.**

- `CloseWindow` names a window by its id. The root pushes `Windowing::Close`, and the host drops the window's surface, takes its slot out of the front, closes the native window, and pushes `Surfaced::Closed` onto the render root's source, then `Heard::Closed` onto the window root's.
- **On `Closed`, the window's actor sends its subscribers `Event::Closed`, the last thing they hear of it, and ends.** The root answers the request.
  - An actor holding the window's path then reaches an empty place, as for any ended actor (ADR-0008).
  - A subscriber that wants to stop its work for the window does so on `Closed`.
- **The render root drops the window's main frame**, and the targets it made for it, on `Surfaced::Closed`. Placements that joined only that window's main frame are drawn nowhere until their makers release them. Ids are never given twice, so nothing joins a window's main frame by mistake once it has closed.
- **Any actor that may ask the root may close any window.** Who owns a window is not decided here; when a program needs it, the root keeps who asked for each, as the render root keeps the owner of a main frame (ADR-0025).
- `CloseWindow` for a window that is not open, or still opening, is answered `Unclosed::NotOpen` by the root. A host that closes no window pushes `Heard::Unclosed` with `NoHost`, as a browser's chassis does, and the root answers with it.
- **Once a window has closed, the root drops what its source still brings for it**, so `Closed` is the last thing a subscriber hears of it, and `Event::Closed` pushed as an ordinary event is refused: closing is the host's to say.

**The close button closes the window, unless the program says it only asks.**

```rust
WindowsConfig { .., closing: Closing::ByButton }      // the default; or Closing::ByProgram
```

- **`Closing::ByButton`**: on `CloseRequested`, the window's actor sends it to its subscribers, as before, and the root closes the window as if `CloseWindow` had been asked.
- **`Closing::ByProgram`**: `CloseRequested` only asks, as ADR-0024 decided. The program closes the window, or does not.
- **A desktop chassis asks the program to stop once its last window has closed.** This replaces the rule, since #253, that the close button of a program's only window quits it. A program with several windows needs no code of its own to be closed one window at a time.
- **This departs from ADR-0024's "`CloseRequested` asks; it closes nothing"**: a program that draws one window and handles nothing would otherwise never close. A program that confirms before closing says so once, in its config.

**A window is shown in a mode: chosen at setup, asked of the root while the program runs, and heard when it changes.**

```rust
pub enum Mode {
    Windowed,      // a window at its size; returning to it gives back the size it had
    Maximized,     // fills the screen's work area, with its frame and title bar
    Fullscreen,    // borderless, over the monitor the window is on
}

WindowSettings { title, size: [1280, 720], mode: Mode::Fullscreen }   // at setup, each window's settings name its mode
--mode fullscreen                                                      // the first window's, WindowsSettings::FIRST_MODE
```

- **At setup, each window opens in the mode its settings name**, and its actor is made standing in that mode (`WindowConfig::mode`), as it stands at its surface and focus; the first window's mode is a flag, as its size is (ADR-0028). A window asked for while the program runs opens `Windowed`; a mode given in `OpenWindow` is left for later.
- **While the program runs, `SetMode` is asked of the root**, as `OpenWindow` and `CloseWindow` are: the root holds the request, pushes `Windowing::SetMode`, and answers once its source brings `Heard::Set` or `Heard::Unset` for the window. The host answers the requests in the order it was asked them, so the root answers what it holds for a window in that order.
- **The answer carries the surface as the host draws the window now**, so the asker draws at it without waiting; the window's subscribers hear `Resized` as for any resize. No size rides on the request: resizing a window is its own request, left for later.
- **Subscribers hear `Event::Moded(Mode)` when the mode changes**, whether the program asked or the person at the window changed it, through the platform's own fullscreen control or by leaving fullscreen. A new subscriber is sent the mode first, with the surface and focus (ADR-0024), and the window's actor drops a `Moded` that says nothing new, as it drops a resize to the size it has.
- **Asking for the mode a window has answers `Set` with its surface at once**, and sends no event: the root keeps each open window's surface and mode as its source brings them, and asks the host nothing. While another request to set the window's mode is unanswered the root asks the host all the same, as only the host knows the mode that one leaves.
- **`SetMode` for a window that is not open, as one still opening or closing, is answered `Unset::NotOpen`** by the root, as `CloseWindow` is. A request the host has not answered when the window closes is answered `Unset::NotOpen` too.
- **The desktop reads the window's mode back on each resize**, as winit says it, fullscreen whether or not it is maximized beneath, and pushes `Moded` only when it differs from the mode it last pushed for that window, which it keeps beside the window. A mode changes the window's size, so every change the person makes reaches the program through the resize winit reports for it. It reads the mode again once it has set one, so a platform that applies it at once is heard before the answer, and one that applies it later is heard as it resizes.
- **The browser answers `Unset::NoHost`**, as it does `OpenWindow`: a page enters fullscreen only while it handles a person's click or key, and the engine's turns run from `requestAnimationFrame`, outside any.

**What each host does.**

| | `Open` | `Close` | `SetMode` | the close button |
|---|---|---|---|---|
| desktop | a native window, its surface, a front slot | all three dropped | winit's borderless fullscreen, or maximized, and back | `Closing` decides |
| browser | `Unopened::NoHost` | `Unclosed::NoHost` | `Unset::NoHost` | none: a canvas has none |

## Consequences

- An actor opens and closes windows, and shows one windowed, maximized, or fullscreen, by asking, from any turn, and a window opened while the program runs is reached, subscribed to, and drawn to exactly as one opened at setup.
- The window capability gains a controller and three requests; its source's kind becomes `Heard`, an enum of an event in a window and of what the host did. A window's events gain `Moded`, and its actor's config its mode. The render root gains a source, `Source<Surfaced>`, and the front's slots come and go between steps.
- The host's `Opening` still opens the windows of setup and gives their ids; after setup the root gives them. The chassis keeps the host's end of the controller and a way to make a window's feed for an id the root gave.
- Every window opened costs the program a step before its asker is answered, and one more before the asker's first frame reaches the render root. A window's first `Render` may come a frame later than the request.
- A window on a slower display still paces every window to its rate when the windows are presented `fifo` (ADR-0021). A window opened while the program runs is presented as the renderer's settings present every window, as one opened at setup is; a presentation of its own, given in `OpenWindow`, is left for later.
- The browser opens no window on request and sets no window's mode, and a program written for both hosts handles `Unopened::NoHost` and `Unset::NoHost`.
- A mode set on a platform that applies it later is heard as `Moded` once the window resizes, after its asker was answered with the surface as it stood.
- **Left for later:** who owns a window; moving, resizing, retitling, or hiding a window, each a request of the same controller; a mode given in `OpenWindow`; exclusive fullscreen at a chosen video mode, which Wayland does not offer and which brings a video-mode match and a refusal on each platform; choosing the monitor a window is fullscreen on; and a window opened by the host alone, such as a dialog the platform makes.

## Alternatives considered

- **The host gives every id, and answers with it.** What ADR-0024 did for the windows of setup. The asker's request and the host's answer would be matched by a token the root makes anyway, so the token might as well be the id; and two requests' ids would be in the order the host made them, not the order the turns asked.
- **A window resource handed to the asker.** ADR-0019's sketch hands the made window on. A native window is the chassis's, and what an actor needs of it is its events and a place to draw, which its id gives; a resource held by an actor would be one more thing kept per window, and kept until that actor ended.
- **A front, a source, or a surface resource per window, handed to the render root as each opens.** A kept resource leaves its owner only when the owner ends (ADR-0010), so each window closed would leave its resource kept in the render root until the program ended.
- **A source of its own for the window root's openings and closings.** Two sources keep no order between them (ADR-0010), so a window's first events could reach the root before it had made the window's actor.
- **`CloseRequested` only ever asks.** ADR-0024's rule, unchanged. A program that handles nothing could not be closed by its button, and every program would carry the same few lines to close it.
- **The opener alone may close.** Simple to state, and wrong for a window opened by an actor that has since ended: no one could close it but its button. Ownership enters when a program needs it.
- **Every `SetMode` asked of the host**, the root keeping no mode. The host would answer a mode the window has as well, but the root would forward a request it can answer, and a host that sets no mode, the browser, would refuse a request for the mode its canvas is always in.
- **`Moded` only for a mode the program asked for.** A person who leaves fullscreen with the platform's own control would leave the program believing the window fullscreen, and a toggle would ask for the mode the window already has.
- **A mode pushed as winit reports it, without the host keeping the last.** winit has no event for a mode, and reading it on each resize would push one for every resize; the window's actor would drop them, at a mail each through the root.
