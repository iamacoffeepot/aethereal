# ADR-0055: Draw order is declared, and input follows it

- **Status:** Proposed
- **Date:** 2026-11-11

## Context

The owner wants a widget library: screens drawn over a running game, a dev overlay over any program without the program opting in, and an authoring menu over the game. For that, a program has to say what draws in what order, and no actor may have to negotiate its place with another.

What is already decided:

- **Order within one actor's drawing is declared by its list** (ADR-0031: `DrawList`, `Lists`). An actor sends the list of its own placements for a pass, in order, and the list is kept until it sends another. A join is `Loose` (drawn in every frame) or `Listed` (drawn while a list names it) (`Order::{Loose, Listed}`).
- **Order between actors is not promised** (ADR-0031, ADR-0025). The renderer draws a pass's loose meshes first, grouped by program, then loose texts, then each actor's list back to back. It collects them from hash maps (`Lists::of`, and the walk in `renderer/mod.rs` that sorts loose meshes and dispatches with `sort_unstable_by_key` by program). Within one program, and between actors, the sequence of calls is the order of a hash map: nobody's.
- **ADR-0031 already met the fault this causes.** A hash-map order of one program's placements moved a seam between terrain chunks when an unrelated object was made at startup (#294). ADR-0031 fixed it for one actor's placements, and left two items for later: dispatches in lists, and refusing loose placements whose order shows.
- **Order is declared where drawing is described, never taken from the actor tree, when actors were made, or when mail came** (ADR-0025). The renderer does not read the topology.
- **Interface was to be a texture frame** that the main frame's owner composites (ADR-0025). The renderer's own window frame has the passes `Compute` and `Main` (`CreateFrame::own`). There is no interface pass. The overlay draws in `Main`, after the loose placements, because lists come after them (ADR-0053: `Overlay`).
- **The first actor to make a window's main frame owns it** (ADR-0025: `may_replace`), and a window's frame is replaced only by its owner.
- **Input goes to every subscriber of a window**, except keys, text, and the wheel, which go to the holder of key focus while one holds it, with the latest take winning (ADR-0024: `Event`, `Subscribe`; ADR-0053: `TakeKeyFocus`, `ReleaseKeyFocus`, `KeyFocus`). A key that is down keeps whom its press was sent to.
- **A program is set up as a list of roots, each bootstrapped once, in order** (ADR-0029: `Bootstrapping`). A bootstrap has no turn and sends no mail. It begins its root with a config, and makes handles (`hand`, `source`, `controller`). A need is found by type among what roots before it provided.

## Decision

**The sequence of draw calls the renderer issues is fully declared, and the renderer never chooses an order. An actor's draw list holds its own calls, and it can call its direct children: `Entry::call(child::<Hud>(()), area)` draws whatever Hud draws, at that place in the sequence, in the coordinates the area names, and cut to its clip within the caller's. So drawing is a tree of actors, an actor is drawn only where its parent calls it, and a parent's list is the only thing that orders its children. The root is the setup's list of root actors, fixed for the program's life. Actors author their lists in parallel; the renderer walks the tree from the root and issues the calls in that order. `Loose` is removed. Input reads the same sequence: the renderer records what was drawn under the pointer, in call order, and the window delivers the pointer, and keys, top-down through what was declared there.**

```
setup: root list ── [World, Game, Authoring, Overlay]   (fixed for the program's life)
Game:  [Draw(background), call(child::<Hud>, WINDOW), call(child::<Inventory>, rect(panel)), Draw(border)]
Hud:   [Draw(health), Draw(minimap)]
renderer: frames → passes → walk from the root, entering each call in its area → each list as sent, moved and cut
window: pointer ── probe ── stack in call order ── top-down until one blocks ──> actors
                                              └── top entry's cursor (the press's while held) ──> host
         key   ── Keys entries in call order ── top-down until one blocks ──> actors
```

### The sequence is declared

- **The renderer issues a frame's calls in one order, and every step of it is declared.** Frames, then each frame's passes in the order their author wrote (ADR-0025), then the walk from the root, each list exactly as sent, entering each called actor's list where it is called, placed as the call's area says.
- **The renderer never reorders a list.** A list is not grouped by program and not reordered for pipelines. The renderer skips setting a pipeline that is already set, so a list that keeps one program together pays for no switches.
- **What a call does is not the renderer's concern.** An opaque mesh, a blended one, a text, a dispatch: each is issued where it was listed. An actor that sorts by distance, by material, or by nothing sends its list in that order, and that is the order.

### Lists call actors

```rust
#[kind(name = "aether.render.draw_list")]
struct DrawList { entries: Vec<Entry> }   // the sender's list: its own calls, and the actors it calls, in order

enum Entry {
    Draw { placement: PlacementId, pointer: Pointer },        // a mesh or a text placement of the sender's
    Run(DispatchId),                                          // a dispatch of the sender's, in the compute pass it joins
    Call { child: Callee, area: Area },                       // draw what this direct child of the sender draws, here, placed in area
    Hit { frame: Frame, pass: PassName, rect: Rect2, pointer: Pointer }, // drawn into that pass's picking stack only (see Input)
    Keys { frame: Frame, keys: KeySet, blocks: bool },        // the keys its sender hears in that frame's window (see Input)
}
enum Pointer { Blocks(Cursor), Passes(Cursor), Presses, Ignores }  // how the pointer reaches the entry, and what the window shows when it does
enum Cursor { Default, Hidden, Text, Hand, Grab, Grabbing, NotAllowed, Image(CursorImage) }  // what the window shows (see Input)
struct CursorImage { pixels: Bytes, width: u16, height: u16, hotspot_x: u16, hotspot_y: u16 }  // RGBA, a blob

enum Area {
    Inherit { origin: Vec2, clip: Clip },  // the caller's coordinates moved to origin; clip in the caller's coordinates
    Absolute { clip: Clip },               // the window's pixels wherever the caller sits; clip in the window's pixels
}
enum Clip { Whole, To(Rect2) }             // Whole: cut only where the caller is cut

Entry::call(child::<Scene>(window), Area::WINDOW)                            // Absolute { clip: Whole }
Entry::call(child::<Hud>(()), Area::inherit(panel))                         // Inherit: origin at panel.min, cut to panel
Entry::call(child::<List>(()), Area::Inherit { origin, clip: Clip::To(view) })
Entry::call(child::<Tooltip>(()), Area::CALLER)                             // Inherit { origin: ZERO, clip: Whole }
Entry::call(child::<Cursor>(()), Area::absolute(pointer))                   // Absolute: the window's pixels, cut to pointer
```

- **An actor is drawable, by its parent.** `Entry::call(child::<Hud>(()), area)` in a list issues Hud's own list at that place. A parent can draw before its children, between them, and after them: `[Draw(background), call(hud), call(inventory), Draw(border)]`. `Entry::call` is the one way to write a call, and every call says where its child sits.
- **A list calls only its sender's direct children.** A call takes the sender-relative path `Child<D>` (`path/relative.rs`: `child`), with `D` one type whose `ChildOf` parent is the sender's type, so a call of anything else does not compile. The renderer resolves it from the sender's own path, one segment down: it reads no actor tree, and nothing outside the sender can be named. Each actor so has exactly one place it can be drawn, its parent's list, and a second call or a cycle cannot be written.
- **A parent's list is the only order between actors.** Reordering children is the parent sending a new list (ADR-0031: a later list replaces an earlier one). It is one list, so the change lands whole, and nothing below the parent changes. A parent can reorder its children every frame, as one that sorts transparent children by distance does, at the cost of one list.
- **An actor whose parent does not call it is not drawn.** An actor's list is kept until it sends another, and drawn only where its parent calls it. The setup's root list and the parents' lists decide what is on screen.
- **Lists are authored in parallel.** No actor waits on another's list: a parent names the child, and the walk takes whatever list the child last sent. A list that is late shows the last one, never a gap (ADR-0031).
- **An actor's list is the same in every frame and every pass.** The frame and pass of each call are declared through what is listed: a placement's join names its frame and pass (`Join::to::<Interface>(frame, ..)`), a dispatch's names its compute pass (`Dispatch::to::<Compute>(frame, ..)`), and `Hit` and `Keys` name their frame. For each pass of each frame, the walk issues the entries that land there, in order.
- **A dispatch that runs once is listed as any other.** It has an id (ADR-0026: `Runs::Once`) and runs only where its maker's list names it, in walk order, in the first cycle that draws its frame with it listed; then it is released and leaves the list. Every call is in an order a list declares, whatever the call does.

### A call places its child

A program places what it draws by its own program, uniforms, and instances, in the surface's pixels (ADR-0024, ADR-0052), so without more a child must know where on the surface it stands, and nothing stops it drawing over its parent's neighbours. A call says both.

- **A call carries where its child sits, and says which coordinates the child draws in,** so a reader of the call site knows. The area is one of two modes:
  - **`Inherit { origin, clip }`:** the child draws in its caller's coordinates moved to `origin`, and `clip` is in the caller's coordinates. It fits a child that moves with its parent: a list that scrolls, a panel dragged, a widget animated. `Area::inherit(panel)` puts its `0, 0` at the panel's corner and cuts it to the panel; a scroll view gives an origin above its view, and the view as the clip; `Area::CALLER` draws it where its caller does.
  - **`Absolute { clip }`:** the child draws in the window's pixels wherever its caller sits, and `clip` is in the window's pixels. It fits a child that needs to know where it is on screen: a scene that fills its window, `Area::WINDOW`, or what is placed at the pointer, `Area::absolute(rect)`, cut to a rect of the window's pixels.
- **Nesting composes, from the window down.** The walk carries, for each list, where it is drawn in the window's pixels: its origin and its clip. The root list's actors are drawn at the window's origin, cut to the whole surface. An `Inherit` child's origin is its caller's plus its own; an `Absolute` child's is the window's, zero, whatever its ancestors are, and its own `Inherit` children add from there. **Whatever the mode, a child's clip is cut to its caller's:** a child never draws outside its caller's box. The renderer reads no actor tree to find any of it.
- **The same in every pass.** A list is the same in every frame and every pass, and so is where it is drawn. The origin moves a draw in screen space, after its program projects it: its clip-space position is moved by the origin's share of its frame's end, `2·dx/width` across and `−2·dy/height` down, times its `w`, so a 3D child moves as a viewport would, its perspective unchanged. The clip is a scissor. Neither is in an actor's program, uniforms, `Place`, or bindings: they are the renderer's.
- **The shift is made in the vertex stage, by the renderer's own step.** The record of each draw's call that picking binds (see Input) also holds its shift, and before each return of every draw program's vertex stage, and the glyph program's, the renderer adds `position.xy += shift * position.w`, as it adds the picking step before each return of the fragment stage. The record's binding is seen by both stages, and binds nothing a program's budget did not already leave it: the step's group, and a uniform block in each stage, are taken out of what a program may list. Each draw that is moved or picked has a record of its own, keyed no call when it is not picked; one at the surface's origin that the pointer does not reach shares the first, which moves nothing. The walk writes every record of a `Render` before any draw is recorded, and the buffer is fitted to them, so no draw binds a record past it; a draw past the most records a buffer holds is refused and logged, never drawn where its list is not.
- **The clip is a scissor for each draw:** the clip, in the surface's pixels, scaled to the pass's target as the target's scale scales the end (ADR-0048), taking each pixel whose middle is within it, and kept within the target. A draw whose clip covers no pixel is not issued.
- **A hit is moved and cut on the CPU:** its rect, in its sender's coordinates, is moved by its list's origin and cut to its list's clip before the renderer tests the probes against it.
- **What a child draws past its own surface-sized rect is drawn when its origin brings it in.** The shift is made before the GPU clips primitives, so a child moved up by a scroll view's offset shows what it draws below the surface's height.
- **The child is not told its size or its clip.** It draws in its own coordinates, and its parent decides where it lands and what of it shows. Telling a child the space it has is the widget library's.

### The root list is fixed at setup

- **The setup lists the root actors in draw order,** once:

  ```rust
  program
      .root::<World>()
      .root::<Game>()
      .root::<Authoring>()
      .root::<Overlay>()
      .draws::<(World, Game, Authoring, Overlay)>();   // the root list: Overlay last, on top
  ```

  The renderer's config carries it. It is not the order the roots are listed in, which is the setup's dependency order (ADR-0029: `Ordered<I>`).
- **Every actor in the root list is checked as the program is built.** It must be a root of this program, found as `Ordered<I>` finds a root (ADR-0029). And it must be drawable: its begin or one of its handlers declares that it sends a draw list, `Tells<Path<Renderer>, DrawList>` in its `BeginSends` or `Sends`. The check reads the sends the actor already declares, and nothing is added to mark an actor drawable. A root list that names an actor that is not a root here, or one that never draws, does not build.
- **Nobody holds the root list, and it never changes.** No actor can reorder it or put itself above the dev overlay.
- **The same root list applies to every window's main frame,** including windows opened while the program runs (ADR-0027). An actor that draws in a window it opened draws there through its joins.
- **A texture frame's maker is its root:** the maker's list is the frame's root list, and it calls its children to draw there.

### The interface pass

- **Every window's main frame gains a pass `Interface`** after `Main`: drawn onto `Main`'s result, with no depth. The overlay, the authoring menu, and every screen draw there. This reverses ADR-0025's plan of interface as a texture frame composited by the main frame's owner. A screen drawn as a texture is still possible, as any texture frame is, but it is not how the library draws.
- **The renderer adds `Interface` to every frame that ends on a window,** including one whose owner replaces it, so a program that writes its own main frame cannot drop the overlay by leaving the pass out. The main frame's owner (ADR-0025: `may_replace`) still writes the frame's other passes.
- **World-anchored interface (nameplates, health bars) is projected and drawn in `Interface`.** It is then called like any other drawing, and it is picked as anything else is.

### Input reads the sequence

**Picking.** Every program the renderer builds for a pass of a window's main frame also writes, for each fragment, the call's number in the sequence, the actor whose list it is in, its entry, and its `Pointer`. The renderer adds this step itself, and program authors never write it. It follows a call's area by itself: a fragment's position is the moved one, and the scissor discards what is cut before the step runs, so what is cut is never picked.

- **Only at the probes.** Each frame the renderer is given the window's probe points: the pointer's position, and the positions of presses and releases since the last frame, each pinned to its own place whatever the pointer does after it. A fragment that lands on a probe appends `(call, actor, entry, pointer)` to that probe's stack of four slots. Fragments elsewhere write nothing. The cost does not grow with the window's size.
- **The stack is in the order it is seen, whatever the GPU's order.** Fragments land in no fixed order, so each probe's slots are ranked: a later pass above an earlier one, then, in a pass without depth, by call, the last call that drew there on top. A pixel under more than four layers keeps the top four.
- **Within a pass that tests depth, the stack ranks by nearness.** The nearest fragment stands on top, and equal depths fall back to call order, so the top is what the depth test shows, though a fragment records before that test. A `Hit` has no depth: in such a pass it ranks below every fragment drawn there, and by call order among hits.
- **`Hit` draws only into the stack:** a quad with no colour, for a modal's backdrop, an invisible handle, or the world's catch-all, a full-window `Hit` in the first root's list. A `Draw` with `Pointer::Ignores` draws colour and nothing into the stack. A hit names its pass as a join does, `Entry::hit::<Interface>(frame, rect, pointer)`, as the sequence is walked pass by pass and a hit needs a place in it. It draws nothing, so the renderer tests each probe's pixel against its rect itself, numbered in its place among the calls.
- **The readback is a few hundred bytes a frame.** It arrives a frame or two later, so the pointer is tested against what was on screen.

**Pointer events cascade.** The window keeps each probe's newest stack and delivers every pointer event (`PointerMoved`, `PointerDown`, `PointerUp`, `Wheel`) top-down through it: to the actor whose list holds each entry, until the first entry whose pointer is `Blocks`, all in one step, with no answer asked. Each actor is told which of its entries was hit, so it does not test its own widgets again.

- **In the entry's own coordinates.** Each entry of a stack carries the origin its list was drawn at, in the window's pixels (`Reach { actor, entry, origin, hearing }`), and the positions an actor is told through it (`Pointed`) are the surface's less that origin, so a child hears a click as it drew: an `Inherit` child from its own `0, 0`, and an `Absolute` child in the window's pixels.
- **Hover:** when the set of entries delivered to changes, those left get `PointerLeft` and those reached get `PointerEntered`. An entry is its actor and its number, wherever it was drawn, so a parent that moves a child under a resting pointer sends it neither.
- **Capture:** the entries a `PointerDown` reached get every move and the `PointerUp` until the button comes up, wherever the pointer goes, as a key that is down keeps whom its press was sent to (ADR-0053). Their positions are given from the origin each entry was drawn at when the press reached it: a child that moves during a drag hears positions from where it was at the press. Live origins wait for a need.
- **A release says what it was over** (*amended by ADR-0056*, for drag and drop). A `PointerUp` delivered through an entry carries `over: Over`, the topmost entry of the stack at the release's own place, as though no button were held: `Over::Entry { actor, entry, x, y }`, the actor as the stack holds it, a `ProtocolRef<Hearer>`, which a receiver may cast to a protocol of its own and mail; the number of the entry in that actor's list; and the release's place in that entry's coordinates, as that actor would hear it. Where nothing the pointer reaches was drawn, it is `Over::Nothing`. The top is the first entry the pointer reaches there, whether it `Blocks` or `Passes`; an entry that `Ignores` the pointer is never in a stack, and one that hears `Presses` alone is passed over. It may be an entry that holds the capture, and is reported the same way: what a drop on itself means is the receiver's. Every entry the release is sent to hears the same `over`, and a release with no capture carries it too, the top of the stack at its place, which it cascades through. The window reports it and nothing more: dragging is the widget library's.
  - **How fresh it is.** A release is probed at its own place, as a press is, and waits for its stacks as every pointer event does, so `over` is what was under the release's own place in the first frame drawn after it whose stacks came back, never in one drawn before it. A pointer that moves on within that frame does not change it: a drop made with a flick lands where the button came up.
- **An entry can hear presses alone** (*amended by ADR-0056*, for a press into a child). An entry whose pointer is `Presses` is reached by a press, which it passes on to what is beneath it, and by nothing else: no move, release, wheel, or hover, and no capture holds it. A parent lists one over a child it calls, after the call, so a press into the child reaches the parent too, which leaves its own focus; a `Passes` hit there would take the child's cursor, hover, moves, and capture. A stack's entry says which it is, `Hearing::Everything { blocks, cursor }` or `Hearing::Presses`, and the window keeps a `Presses` entry in a press's cascade alone, reading the stack as though it were not there for everything else.
- **The wheel follows the pointer**, so a console that blocks stops the wheel from zooming the game beneath it, which ADR-0053 got by sending the wheel with key focus.
- **The cursor is declared, as the pointer's reach is** (*amended by ADR-0056*, for the cursor). Several actors share a window, so a message that sets the cursor would show whichever arrived last, and arrival order is ruled out above. So the cursor is part of how the pointer reaches an entry: a `Draw` or a `Hit` whose pointer is `Blocks(Cursor)` or `Passes(Cursor)` declares the cursor it shows, and one whose pointer is `Presses` or `Ignores` declares none, and the window chooses from the stacks it already delivers through:
  - **Under the pointer, the topmost entry's.** The window shows the cursor of the first entry of the pointer's newest stack, whether it `Blocks` or `Passes`: the entry seen there, never the first that blocks nor the bottom. An entry that `Ignores` the pointer is never in a stack, and declares no cursor to choose; one that hears `Presses` alone is not in the pointer's stack. Over nothing the pointer reaches, the window shows the default.
  - **While a button is held, the press's.** The cursor of the topmost entry the press reached shows wherever the pointer goes, as its capture keeps every move. It is taken from the press's own stack, and from each later stack under the pointer that holds that entry, so a source that declares the image it drags once its drag begins, a few pixels after the press with the pointer still over it, shows it from then on. Where the pointer has left the entry, the cursor it last showed stands. Once every button is up, the pointer's stack chooses again. Over nothing, a press shows the default until its release.
  - **When it changes.** Under the pointer, as each frame's stacks come back; under a capture, as the press and its release are delivered, and as a stack that holds the pressed entry comes back. No other timing: a list sent while the pointer rests changes the cursor once a frame drawn with it is probed.
  - **The window's actor asks its root, and the root its host.** The actor compares the cursor its cascade answers with the last it asked for, and only on a change mails the window root `ShowCursor { window, cursor }`, which the root acts on only from that window's own actor, and pushes to its host as `Windowing::SetCursor`. Nothing answers it. A window whose main frame ends on a texture has no pointer and no stacks, so nothing there sets a cursor.
  - **The icons are the few the widget library needs,** each one a native window and a page both have: the text I-beam, a pointing hand, an open and a closed hand, and not-allowed, with `Hidden` and `Default`.
  - **An image is a blob** (ADR-0014, ADR-0042): its RGBA pixels, not premultiplied, rows from the top, are too large for a piece of mail, so `pixels` is `Bytes`, beside its size and hotspot. A side is at most 128 pixels, the most a browser shows as a cursor. A list whose image's pixels are not four bytes for each of its pixels, or whose hotspot lies outside it, is refused as it is checked, as any list is.
  - **A host makes each distinct image into a cursor once** and shows the same one each time it is asked for again, kept under the image's content hash, size, and hotspot (`MadeCursors`), so an image put again as another blob is not made again. The desktop sets winit's `set_cursor` with a `CursorIcon`, or a `CustomCursor` made from the pixels, and `set_cursor_visible` for `Hidden`. The browser chassis drives no winit window: it sets the canvas's CSS `cursor`, and an image as a URL of its pixels drawn into a canvas of its own. A host keeps every image it was shown for as long as it runs.
- **Raw subscription stays** for every pointer event and for the window's own events (`Resized`, `Moded`, `FocusGained`, `FocusLost`, `CloseRequested`, `Closed`), as ADR-0024 delivers them.

**Keys cascade the same way.** An actor declares the keys it hears with `Keys { frame, keys, blocks }` in its list, at its place in the sequence; a key is named by its place on the keyboard, its `code` (ADR-0024), whatever the layout. As it draws each window's main frame, the renderer walks the frame's `Keys` entries and sends the window's actor them in reverse call order, `KeysListed`, whenever they have changed since it last sent them. The window takes them only from the subscriber of its probes, the renderer, as it takes the stacks. It delivers each `KeyDown` top-down through the entries whose set holds the key, until the first that blocks, each as `Keyed { window, entry, event }`, so the actor knows which declaration it came through. An entry whose set does not hold the key is passed over, whether it blocks or not.

- **Focus is a declaration.** A screen that takes the keys lists `Keys { frame, keys: KeySet::All, blocks: true }` while it wants them, and drops the entry when it does not. The dev overlay lists `Keys { frame, keys: KeySet::of(["Backquote"]), blocks: true }` in its list, called last in the root list, so the backtick reaches it whatever has focus beneath, and every other key falls through. While its console is open it lists `KeySet::All`, blocking, and a `Hit` over the console's panel that blocks the pointer, so the wheel over the console stops there.
- **Typed text and repeats follow the `KeyDown`** that typed them (ADR-0051): text goes where the latest key that went down was sent, as a host sends a key's text after it, and a key that is down keeps whom its press was sent to, for its repeats and its release, however the entries change (ADR-0053). The window losing the keyboard forgets every key down.
- **A focus change takes effect with the list that declares it**, the frame after the click that caused it.
- **Keys are not sent to raw subscribers.** A raw subscriber would hear keys that a declaration above it blocked, and blocking would then mean nothing. So an actor that hears keys sends a draw list, and is called from the root list or a parent, though it draws nothing else.

## Consequences

- **The draw sequence is reproducible.** The same lists give the same calls in the same order every frame and every run, so ties at equal depth do not flicker, captures compare, and an actor can draw front to back on purpose.
- **Removed:** `Order`, the loose `Join::to` and `TextJoin::to`, `RunDispatch` (folded into `CreateDispatch` as `Runs::Once`), `Listing::Loose`, the renderer's loose-mesh and loose-text walks, and grouping a pass's draws or dispatches by program. A placement or dispatch draws only when a list names it; one that no list names draws nothing, and is not logged, as leaving an entry out of a list is how an actor hides it. With every join listed, `Join::listed` and `TextJoin::listed` said nothing more, and take the plain names `Join::to` and `TextJoin::to`, as `Dispatch::to` names a dispatch's pass. Every drawing actor sends a list. The call sites that drew loose, outside tests and in them, move to lists, called from the root list or a parent.
- **Every pixel of a window traces to an actor.** The picking stack names the actor and entry that drew a pixel, and the chain of calls above it, so an inspector, an agent's "what is at (x, y)", and the authoring menu's "select what I clicked" read what input already reads.
- **Each actor's list can be recorded on its own thread,** as a GPU render bundle, and the bundles executed in the walk's order. Not needed now, but nothing in the order prevents it.
- **Each program in a window frame's passes gets a step it did not write,** and each window frame gets a probe buffer and a readback. Programs run in a texture frame get neither: a texture frame has no pointer. Every draw program and the glyph program also get the vertex stage's shift, in every frame, and a draw that is moved or picked has a record of 32 bytes in one buffer the renderer writes each `Render`.
- **Every entry the pointer reaches says what the window shows there.** A widget library declares the I-beam over a text field, the hand over a link, and the dragged item's image on the entry it drags from, each in the list it already sends, and no actor sets the cursor over another's. A host is asked for a cursor only when the one shown changes.
- **A child draws and hears in the coordinates its call names.** What it places with `Place` and texts' `at` are moved by its list's origin, zero for an `Absolute` child, and cut to its list's clip, and the pointer reaches it there. Raw subscribers still hear the window's events in its surface pixels (ADR-0024).
- **ADR-0023, ADR-0024, ADR-0025, ADR-0031, ADR-0052, and ADR-0053 are edited to match.**
  - ADR-0023's refusal of an order key holds: the root list is written in one place, the setup, and every other order is a parent's list, never a number that actors agree on.
  - ADR-0025's "order is declared where drawing is described" holds and is now total. Its "no order within one pass between actors" and its interface-as-texture-frame are replaced.
  - ADR-0031's lists stand as the form of every list, and its "order between actors is still not promised" is replaced. Both items it left for later are settled: dispatches are listed (`Entry::Run`), and nothing is loose.
  - ADR-0053's key focus slot (`TakeKeyFocus`, `ReleaseKeyFocus`, `KeyFocus`) is replaced by `Keys` entries. The overlay is last in the root list, so it draws on top.
  - ADR-0024's positions in the surface's pixels hold for the window's events and raw subscribers; an entry's `Pointed` is in its list's coordinates. Its left-for-later cursor is settled for hiding and changing it: declared on each entry, never requested.
  - ADR-0052's text placed at a point in its frame's pixels is moved by the origin its list is drawn at, and cut to its list's clip as well as its own.
- **Follow-on work, in order:**
  1. The `Interface` pass, the root list in setup, and `Call`, beside `Loose`. Done.
  2. The overlay moves to `Interface`, last in the root list, as the first client. Done.
  3. Picking and the input cascade in the window. Done.
  4. Every `Loose` site moves to a list, dispatches are listed (`Entry::Run`), and `Order` is deleted. Done.
  5. A call places its child with an origin and a clip. Done.
  6. The widget library's own ADR: screens as actors, layout, telling a child the space it has, widget identity across rebuilds, and a pool of texts.
- **Left open:** texture frames that do not read each other still run in the order of their ids (ADR-0025). Ids come from one count in the order mail was handled, so that order can differ between runs. Whether it must be declared is not decided here.

## Alternatives considered

- **A z-index on each actor or placement.** Numbers that unrelated actors must agree on, which ADR-0023 refused.
- **An app-wide list of actors in draw order**, sent by the window's owner. It orders actors only at one level, and an overlay over any app would need the app to put it in its list.
- **Spans: order coordinates (`[1, 0]` is game → hud) handed down by splitting, each actor submitting its list with its span.** A reorder hands new coordinates down the whole subtree, which tears it across frames as each level adopts, needs counters that never reuse a coordinate, and a parent can only draw under its children. A parent's list that calls its children reorders in one list and draws over them.
- **The order the roots are listed in, as the root list.** That order is the setup's dependency order: a root reaches only roots listed before it (ADR-0029: `Ordered<I>`). A game that reaches the authoring tool would have to be listed after it, and so draw over it.
- **The root list held by one app actor.** That actor could reorder it at any time and put itself above the overlay.
- **Calling any actor, not only a direct child.** An actor could then be drawn in two places, lists could call each other round, and the renderer would have to refuse both at the walk. Showing another actor's drawing elsewhere, such as the game inside an editor's viewport when the game is not the editor's child, goes through a texture the other actor draws into.
- **Keeping `Loose` for drawing whose order "does not show"** (opaque, depth-tested). Draw calls always run in some order, and a loose order is a hash map's. Keeping it would leave two ways to draw, and an undeclared sequence.
- **A pass for each root actor.** It needs no new concept, but a pass that writes depth clears it, so the world's roots could not share one depth buffer, and each pass costs about 35 µs.
- **A GPU viewport for each call, to place a child.** Primitives are clipped to clip space before the viewport maps them, so a child moved by a negative origin, a scroll view's, draws only what falls within its own surface-sized rect, and leaves a blank band where the rest should show. A shift made in the vertex stage, before clipping, moves everything, and the scissor cuts as a viewport's edge would.
- **The origin passed to each actor to place its own drawing.** Every program, uniform, and instance would carry its parent's place, and a parent that moves a child would wait on the child to draw again.
- **Hit testing on rects the renderer derives from entries.** The renderer does not know where a placement lands, because its program places it.
- **Rects sent by each screen to the window.** A second structure, which would have to be kept in the sequence's order by some other means.
- **A full-window id buffer.** Exact, but about 75 MB at 1080p and 300 MB at 4K for four layers. Probes give the same stacks for a few hundred bytes.
- **Key focus as a slot, latest take wins, with global shortcuts registered beside it** (ADR-0053). Two mechanisms, and a sibling that takes focus silences the overlay's backtick.
- **A message that sets the window's cursor,** as ADR-0056 first wrote it. Several actors share a window, so whichever set it last would win: a game that sets its crosshair each frame would cover the console's I-beam, and nothing but arrival order would decide.
- **A capture's cursor fixed at its press.** Simpler, but a drag begins a few pixels after the press (ADR-0056), so its source cannot declare the dragged image in time, and declaring it before the press would show the image on hover.
- **A cursor in a field of its own beside `pointer`.** An entry that `Ignores` the pointer is never in a stack, so its cursor would be a value nothing reads, written `Cursor::Default` by custom. Folded into `Pointer`, only an entry the pointer reaches declares one.
