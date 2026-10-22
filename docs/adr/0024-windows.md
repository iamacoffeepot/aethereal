# ADR-0024: Windows and what they hear

- **Status:** Proposed
- **Date:** 2026-10-21

## Context

A window is where a program is drawn and where the person at it points and types. A browser has one, the canvas the renderer draws on (ADR-0021: `Gpu`); a native host has as many as it opens through its windowing library. Both bring in what a window hears the same way: a callback of the host's pushes an event onto a source's feed, and the next bring delivers it to the source's owner as mail from itself (ADR-0010: `Feed::push`, `Engine::bring`). A capability fronts a host system for every other actor (ADR-0019).

The browser chassis brought in the page's pointer, keys, and the controls the page marks through one root of its own, `Input`, which actors subscribed to. Nothing of it was a window: the pointer was in the canvas's pixels and the keys the page's names, and a native chassis would have written its own root with its own kinds, so an actor that reads the pointer would have read it twice.

The two hosts agree on most of what a window hears. A page's `KeyboardEvent.code` is the W3C UI Events name of the key's place on the keyboard (`"KeyW"`, `"ArrowLeft"`), and winit's `KeyCode` names its variants after the same values. A page's pointer and a native window's cursor each have a place, buttons, and a window they are over; each window has a size its surface is drawn at, a scale of the screen's pixels to the host's logical ones, a keyboard focus, and, natively, a close button.

The earlier engine put one window actor on the thread that runs the event loop, which owned every native window and translated every event (its ADR-0164), and later gave each window an addressable actor of its own beneath that manager, while the manager kept the native handles (its ADR-0167).

## Decision

**The window capability is its own crate, `aethereal-window`**, as the renderer is (ADR-0021). It names no windowing library and no page, and builds for every host. A chassis opens its windows with it and feeds them; what is a page's or a windowing library's stays in that chassis.

### One root, and an actor for each window beneath it

```rust
impl ChildOf for Windows { type Parent = Top; }        // the window root: the source, and what is every window's
impl ChildOf for Window  { type Parent = Windows; }    // one per window, keyed by its WindowId

turn.to(path::<Window>(window)).send(&Subscribe)?;     // send me this window's events
```

- **The window root, `Windows`, holds what the host brings in, and anything that spans windows.** It makes an actor for each window beneath it, `Window`, keyed by the window's `WindowId`. Today nothing spans windows, so the root only routes.
- **A `WindowId` is the window capability's number for a window**, `0` for the first opened, then `1`, never a host's own name for it: a page's canvas has none, and winit's `WindowId` is opaque and the host's. A chassis keeps its own map from its windows to the feed of each (below).
- **An actor reaches a window by its typed path**, `path::<Window>(id)`, declared `Tells<Path<Window>, Subscribe>` (ADR-0022), and subscribes with one `Subscribe`; `Unsubscribe` takes it off, and a subscriber that ends is taken off by the watch `Subscribers` keeps (ADR-0016). A subscriber must handle `Event`: the window casts it to `(Event,)`, and logs and ignores one that does not.

### What a window hears

```rust
pub enum Event {
    PointerMoved { x: f32, y: f32 },                      // in the window's surface pixels, from its top left
    PointerDown { x: f32, y: f32, button: Button },       // Main | Middle | Secondary | Back | Forward | Other(u16)
    PointerUp { x: f32, y: f32, button: Button },
    Wheel { x: f32, y: f32, delta_x: f32, delta_y: f32 },  // where the pointer is, and how far the wheel scrolls
    KeyDown { code: String, key: String, repeat: bool },  // "KeyW", "w": the W3C UI Events code, and the key's text
    KeyUp { code: String, key: String },
    Resized(Surface),                                     // Surface { width: u32, height: u32, scale: f64 }
    FocusGained,
    FocusLost,
    CloseRequested,
}
```

- **A position is in the window's surface pixels**: those of the surface the renderer draws in, its backing store, from its top left. An actor that hit-tests what it drew compares numbers of one kind, and never converts.
- **The wheel's turn is in the window's surface pixels too**, as far as the host would scroll what is under the pointer, and carries where the pointer is, as a press does, so an actor zooms or scrolls about the place it points at. **A positive `delta_y` scrolls down**, toward what is below, as a wheel turned toward the person at it does, and a positive `delta_x` scrolls right: a page's `WheelEvent` signs its deltas so. A host that counts the wheel in lines or pages takes **a line as 40 of its logical pixels and a page as 800**, fixed factors that put a notch of a wheel, often three lines, near what a host that counts in pixels gives for one, and then converts those to the surface's pixels as it does a position.
- **A key is named by its W3C UI Events `code`**, its place on the keyboard whatever the layout, and by its `key`, what it means under the layout (`"w"`, `"Shift"`). Both hosts give the `code` names as they are: a page as `KeyboardEvent.code`, and winit as the names of `KeyCode`'s variants, which follow the same specification. This is why it is the shared vocabulary: neither host translates a table of its own, and a name reads as the key it is. `repeat` is set on the keyboard's own repeat while a key is held.
- **A resize carries the surface as the window is now drawn**: its size in its pixels, and its scale, the surface's pixels to each of the host's logical ones (a page's `devicePixelRatio`, or less where the page draws fewer, winit's `scale_factor`).
- **Each window's surface follows its window's size.** The chassis owns each window's surface (ADR-0021: `WindowSurface`), and configures it to the size of each resize it pushes as `Resized` for that window, before it acquires the window's next frame, never during a frame: resizes heard between two frames are configured once, at the last size, and the frames that end on the window follow (ADR-0025). Nothing else configures a surface, so the size the GPU gives its textures is the size they are. The renderer hears nothing of the windows, and draws into the texture of each window's frame it is handed, through the one front it holds for every window, keyed by the window's id (ADR-0021). **This departs from the surface being configured once**, as the chassis opened it, when a window resized was drawn stretched.
- **`CloseRequested` asks; it closes nothing.** What to do is the program's, and closing a window on request is left for later (below).
- Every event of a window is one kind, `Event`, so what a window heard reaches a subscriber in the order its host heard it, across the pointer, the keys, and the rest.

### A window's actor keeps how its window stands

- **A new subscriber is sent the window's surface and focus first**, `Resized` and `FocusGained` or `FocusLost`, so an actor made at any time knows the size it draws at without waiting for it to change. A second `Subscribe` from an actor already subscribed sends nothing.
- **A window sends on only an event that says something new.** A resize to the surface it has, or a focus it has, is dropped: a host may report either again with nothing changed, as a page's `resize` does whenever the page is laid out again.
- `Subscribers` is written in its holder's state as the references it holds, in order, and read back with each found where it is held; the watches that take an ended subscriber off are the engine's, and are not written. **This departs from building the subscriptions on the engine's stage subscriptions** (ADR-0020): those carry only a stage, which holds nothing, from the engine, and an event holds what the host heard and comes from a window.

### One source for every window, routed by the root

```rust
pub struct Heard { window: WindowId, event: Event }    // what the root's one source brings in

impl Handles<Heard> for Windows {                      // brought from the source: from this actor to itself
    type Sends = (ToType<Window, Event>,);
    fn handle(&mut self, heard: Heard, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.to(child::<Window>(heard.window)).send(&heard.event)?;
        Ok(())
    }
}
```

- **The root holds one source for every window**, and each event it brings carries the window it was heard in. The root sends it to that window's actor, `child::<Window>(id)` (ADR-0022), which sends it to its subscribers. **One source is the simplest the engine allows:** what one source brings keeps its order (ADR-0010), so what the host heard across its windows reaches the root as it heard it, and a window opened while the program runs will need no source made for it and handed through mail; the cost is one hop through the root for each event. An event for a window with no actor reaches nothing, as for any empty place.
- **Only what the host brought is sent on.** The root sends on a `Heard` only from itself to itself, as its source delivers; a window's actor sends on an `Event` only from its parent, the root. Mail any other actor sends either is logged and dropped, so a subscriber hears only what the host heard.
- The window capability has a source and no controller (ADR-0019): it asks the host for nothing yet. **This departs from a capability having both**: a controller enters with the first request, such as opening a window.

### Windows are opened as the program is set up

```rust
let mut opening = Opening::new(setup.source::<Heard>());
let canvas: WindowFeed = opening.open(surface, focused);     // the next WindowId, and its feed
let setup = setup.add_root::<Windows>(&(), opening.config())?;
canvas.push(Event::PointerMoved { x, y })?;                  // from the host's callbacks
```

- **A chassis opens its windows with `Opening`**, the capability's host side, as it sets the engine up. Each `open` gives the window the next `WindowId` and answers with its `WindowFeed`, which pushes what that window heard onto the root's one source, tagged with its id. `config` makes the root's config of the source and every window opened, as each stood.
- **The root makes each window's actor in its `begin`**, from how the window stood as it opened, so a root made after the window root reaches every window in its own `begin`. A window whose actor is refused fails the root's `begin` with `Unopened`, naming the window.
- **The ids are given on the host's side, by the capability's own code, not in the root's turn.** The root has no way back to the host to say what it gave; a host that opens a window while the program runs will learn the window's id from that answer (below).

### The browser: each canvas a window

```rust
let mut opening = Opening::new(setup.source::<Heard>());
let (gpu, [first, second]) = open([one, other], &mut opening).await?;  // each canvas a window, `0` then `1`, and WebGPU on them
let setup = setup.add_root::<Windows>(&(), opening.config())?;
let listening = canvas::listen(&one, &first, Density::at_most(2.0))?;  // the page's callbacks, kept while it listens
```

- **The browser chassis opens each canvas the page hands it as a window**, `0`, `1`, … in the order given, and WebGPU on them, an adapter that draws to the first. `canvas::open` opens one standing as the page has it, its backing store as its surface, the page's `devicePixelRatio` as its scale, and focused when `document.hasFocus()`. What is the page's stays in `aethereal-chassis-browser`. Each canvas is listened to for its own resizes.
- **`canvas::listen` registers the page's listeners**, as callbacks the page runs as it hears each event: `pointermove`, `pointerdown`, `pointerup`, and `wheel` on the canvas; `keydown`, `keyup`, `focus`, `blur`, and `resize` on the page's window; and an observer of the size the page lays the canvas out at. Each callback pushes one event onto the window's feed and returns, and the next frame brings in what was pushed, in the order it was pushed. Nothing waits: a callback never touches the engine, and a push to a feed whose source has gone is refused and dropped. Dropping what `listen` returns takes every callback off.
- **The pointer's place is in the canvas's pixels**: its place on the page less the canvas's box, scaled on each axis by the window's size, as it was last pushed, over the size the canvas is shown at. A canvas shown at 400 by 300 CSS pixels and drawn at 800 by 600 reads a point 100 pixels across the page as 200. The size last pushed is the one the window's actors were told, which the backing store reaches as the chassis's next frame begins. A press or release the page numbers below zero is no button, and is not pushed.
- **The wheel over the canvas is the program's, not the page's**: its listener is registered as not passive and calls `preventDefault`, so the page does not scroll while the wheel turns over the canvas, and scrolls as ever over the rest of it. Its deltas are the page's, read in the `deltaMode` it gives, pixels as they are and lines and pages by the fixed factors above in CSS pixels, then scaled to the canvas's pixels on each axis as the pointer's place is. A `deltaMode` the page has no name for is not pushed.
- **The keys are the page's**, heard on its window, not on the canvas: a canvas has the keyboard only once the page makes it focusable, and the page's keys are each listened canvas's, as the page has one keyboard.
- **The canvas is drawn at the size the page lays it out at, and follows it.** The chassis observes the canvas with a `ResizeObserver`, which the page runs once a frame at most, after its animation frame's callbacks and before it paints, and once as it begins observing. Each time it pushes a resize to the size the page lays the canvas out at, in its device pixels: `devicePixelContentBoxSize`, whole pixels snapped as the page shows them, where the page gives it, and otherwise the content box in CSS pixels times `devicePixelRatio`, each side rounded. The chassis configures the canvas's surface to it as its next frame begins, before it acquires the frame, which sets the canvas's backing store. So a window dragged through many sizes is resized once a frame, and drawn at each size it is shown at, never stretched.
- **The page chooses how sharply the canvas is drawn, `Density`**: the screen's device pixels to each CSS pixel, at most a number it gives, `Density::at_most(2.0)`, or every device pixel, `Density::SCREEN`. Past two, a picture is no sharper to the eye and costs more pixels to draw; below one, it is drawn at fewer pixels than it is shown at, and the page scales it up. Each side is at least one pixel and at most the longest side of a texture the GPU makes, 8192 for the device the chassis opens, which asks WebGPU for its default limits.
- **The canvas's backing store is the surface's, not the page's.** A page sets it once at most, before it opens the GPU, for the first frames before the observer is heard; wgpu sets it as the surface is configured, and a page that sets it after reads a texture of one size that the GPU names another.
- **The page's `resize` pushes the window's size as last pushed, at the scale it reads then**, as a zoom changes `devicePixelRatio` and may leave the canvas's device pixels as they were, which the observer then does not see. The window's actor drops one that changed nothing.
- **The canvas never sends `CloseRequested`**: a page is closed without asking the program, and nothing waits on an answer.

### The desktop: the windows the program asks for

```rust
let first = screen.open_window(&mut opening, "first", (640, 480))?;   // a native window, and the GPU on it
let second = screen.open_window(&mut opening, "second", (480, 480))?; // the next, on the same GPU
let (rendering, drawing) = renderer(&mut setup, screen)?;
```

- **The desktop chassis opens the windows the program asks for as it is set up**, each with its title and size, each the next `WindowId`. The first opens the GPU on its surface.
- **It routes what winit says of a window by a map from winit's `WindowId` to the capability's**, and pushes it onto that window's feed. A frame winit asks of any window draws the cycle, and the chassis asks for the next frame of the first window shown.
- **It quits when the last window closes, or as the program says.** `CloseRequested` still only asks: a window is not closed while the program runs, so the close button of a program's one window asks it to stop, and with several the program decides.

### The page's controls are a browser root of their own

```rust
pub struct Control { name: String, value: Value }        // the element's id; Number(f64) | Checked(bool) | Text(String)

let (source, feed) = setup.source::<Control>();
let setup = setup.add_root::<Controls>(&(), ControlsConfig { source })?;
let listening = controls::listen(&feed)?;
```

- **The elements a page marks `data-control` are the page's, not a window's**, so they are a source and a root of their own in the browser chassis, `Controls`, subscribed to with its own `Subscribe`. A native host has none. **This departs from one stream for the pointer, the keys, and the controls**: a control's change and a window's event are no longer ordered against each other, which nothing read.
- `controls::listen` registers `input` on each marked element that has an id, an input, a select, or a text area, and reads each once as it is registered, before the first frame.
- **A control's value is typed by what the element is:** a checkbox or radio button is `Checked`, a range or number input whose value reads as a finite number is a `Number`, and anything else, a select, a text field, a number input left empty, is the `Text` the page holds. An actor reads a slider without parsing it, and a checkbox by whether it is checked, never by its value, which the page holds as `"on"` either way.
- **The root keeps each control as it stands** and sends a new subscriber every one before anything else. A root made with the program subscribes in its `begin`, and whether its `Subscribe` or the controls read before the first frame reach the controls root first is not something either can rely on; with the record, the subscriber hears each control once in either order, and an actor that subscribes long after a control last moved still hears it.
- **The root sends on only what its source brought**, mail from itself to itself. A `Control` another actor mails it is logged and dropped, neither kept nor sent on.

## Consequences

- An actor reads a window's pointer, keys, size, and focus in one vocabulary on every host, in the pixels it draws in, and reaches a window by its type and id alone.
- A native chassis implements the capability by opening its windows with `Opening`, keeping a map from its library's window ids to each `WindowFeed`, and pushing each event in surface pixels with its `KeyCode` variant's name as the `code`; the window crate is unchanged. Its wheel is winit's `MouseWheel`: a `PixelDelta` in the screen's pixels as it is, a `LineDelta` by 40 logical pixels a line and the scale, each negated, as winit signs a delta by where the content moves rather than where the view scrolls.
- Every window event costs one more mail than a root that sent to subscribers directly: the root's to the window's actor.
- **One cycle draws every window shown, so a window on a slower display paces every window to its rate**: the chassis acquires each shown window's frame in turn before the cycle, and natively each acquire waits on its own display. Presentation modes chosen per window are the way out of that (#246).
- The renderer subscribes to no window: the chassis that pushes a window's resizes configures its surface, so a pointer move or a key costs the renderer no mail.
- `Setup` gains `source`, as `Engine` has, so a chassis makes a source as it sets the program up.
- **Left for later**, each its own record when a program needs it:
  - **opening and closing windows on request**, and a window opened while the program runs: decided by ADR-0027;
  - **the cursor**: capturing the pointer, hiding or changing the cursor, and stopping a key's default, each a request;
  - **pointers entering and leaving**;
  - **text input and IME** composition, which the `key` of a key press does not carry;
  - **more than one pointer**, a touch screen's contacts each its own;
  - **a browser screen whose scale changes alone**, as a window moved to another screen does with the page's size unchanged: the page fires no `resize`, and a `matchMedia` query on the resolution would hear it.

## Alternatives considered

- **A source for each window, held by its actor.** Each window's order is kept as well, and the root routes nothing. A window opened while the program runs would need a source made between steps and handed to its actor through mail, and the host would push onto as many feeds as it has windows, for one hop saved.
- **Every window's events on the root, each carrying its id, and subscribers on the root.** One actor fewer for each window. Every subscriber would hear every window and filter, and a window could not be reached, or later closed, by its path.
- **A source and a kind for the pointer, the keys, and each other thing a window hears.** An actor could subscribe to the keys alone. What one window heard would lose its order across kinds, as two sources keep no order between them (ADR-0010).
- **The wheel as the host counts it**, a delta and its mode of pixels, lines, or pages. Nothing is lost, but every actor that reads it converts lines and pages itself, each by a factor of its own, and a native host's lines and a page's differ. A fixed factor is a guess at what a line scrolls, and one made once.
- **Keys as the host's numbers**: scan codes, or a page's `keyCode`. A scan code is a host's own, and `keyCode` is deprecated; the `code` names are what both hosts already give.
- **Positions in the host's logical pixels.** What the host reports natively, but every actor that compares the pointer with what it drew would convert by the scale.
- **The host's own window ids as keys.** winit's `WindowId` is opaque and is the host's, and a page's canvas has none, so the key would not be one kind across hosts.
- **The chassis setting the canvas's backing store as the page lays it out.** The page would be the one to size the canvas, as before, and the renderer would only follow. WebGPU gives a canvas's texture the canvas's size whenever it is asked for one, while wgpu names it the size the surface was last configured to, so a frame begun between the two would draw targets of one size with a texture of another, which WebGPU refuses. With the surface's configure the one place the store is set, the two never differ.
- **The renderer configuring the surface to each `Resized` its window sends it**, subscribed to the window. The chassis would only push. The renderer would be sent every pointer move and key for the resizes among them, and would configure in a turn the surface it then acquires from, which natively waits on the display (ADR-0021).
- **A source of sizes from the chassis to the renderer**, as the renderer's word of what is mapped comes. The renderer would hear only resizes, and a chassis would push each size twice, to the window and to the renderer, onto sources kept in no order with each other.
- **The windows as config only, and no `Opening`.** The chassis would number the windows itself and could give two one id; `Opening` gives each the next, once.
