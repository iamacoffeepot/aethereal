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

**The sequence of draw calls the renderer issues is fully declared, and the renderer never chooses an order. An actor's draw list holds its own calls, and it can call another actor: `Call(hud)` draws whatever Hud draws, at that place. So drawing is a tree of actors, and a parent's list is the only thing that orders its children. The root is the setup's list of root actors, fixed for the program's life. Actors author their lists in parallel; the renderer walks the tree from the root and issues the calls in that order. `Loose` is removed. Input reads the same sequence: the renderer records what was drawn under the pointer, in call order, and the window delivers the pointer, and keys, top-down through what was declared there.**

```
setup: root list ── [World, Game, Authoring, Overlay]   (fixed for the program's life)
Game:  [Draw(background), Call(hud), Call(inventory), Draw(border)]
Hud:   [Draw(health), Draw(minimap)]
renderer: frames → passes → walk from the root, entering each Call → each list as sent
window: pointer ── probe ── stack in call order ── top-down until one blocks ──> actors
         key   ── Keys entries in call order ── top-down until one blocks ──> actors
```

### The sequence is declared

- **The renderer issues a frame's calls in one order, and every step of it is declared.** Frames, then each frame's passes in the order their author wrote (ADR-0025), then the walk from the root, each list exactly as sent, entering each called actor's list where it is called.
- **The renderer never reorders a list.** A list is not grouped by program and not reordered for pipelines. The renderer skips setting a pipeline that is already set, so a list that keeps one program together pays for no switches.
- **What a call does is not the renderer's concern.** An opaque mesh, a blended one, a text, a dispatch: each is issued where it was listed. An actor that sorts by distance, by material, or by nothing sends its list in that order, and that is the order.

### Lists call actors

```rust
#[kind(name = "aether.render.draw_list")]
struct DrawList { entries: Vec<Entry> }   // the sender's list: its own calls, and the actors it calls, in order

enum Entry {
    Draw { placement: PlacementId, pointer: Pointer },        // a mesh or a text placement of the sender's
    Run(DispatchId),                                          // a dispatch of the sender's, in a compute pass
    Call(ActorRef),                                           // draw what this actor draws, here
    Hit { frame: Frame, rect: Rect, pointer: Pointer },       // drawn into that frame's picking stack only (see Input)
    Keys { frame: Frame, keys: KeySet, blocks: bool },        // the keys its sender hears in that frame's window (see Input)
}
enum Pointer { Blocks, Passes, Ignores }
```

- **An actor is drawable.** `Call(hud)` in a list issues Hud's own list at that place. A parent can draw before its children, between them, and after them: `[Draw(background), Call(hud), Call(inventory), Draw(border)]`.
- **A parent's list is the only order between actors.** Reordering children is the parent sending a new list (ADR-0031: a later list replaces an earlier one). It is one list, so the change lands whole, and nothing below the parent changes. A parent can reorder its children every frame, as one that sorts transparent children by distance does, at the cost of one list.
- **Only an actor some list calls is drawn.** An actor's list is kept until it sends another, and drawn only where a list calls it. The setup's root list and the parents' lists decide what is on screen.
- **The walk issues each actor once.** A second call of an actor already entered in the walk, which a cycle also is, is refused and logged.
- **Lists are authored in parallel.** No actor waits on another's list: a parent names the child, and the walk takes whatever list the child last sent. A list that is late shows the last one, never a gap (ADR-0031).
- **An actor's list is the same in every frame and every pass.** The frame and pass of each call are declared through what is listed: a placement's join names its frame and pass (`Join::listed::<Interface>(frame, ..)`), a dispatch's names its compute pass, and `Hit` and `Keys` name their frame. For each pass of each frame, the walk issues the entries that land there, in order.

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
- **Nobody holds the root list, and it never changes.** No actor can reorder it or put itself above the dev overlay.
- **The same root list applies to every window's main frame,** including windows opened while the program runs (ADR-0027). An actor that draws in a window it opened draws there through its joins.
- **A texture frame's maker is its root:** the maker's list is the frame's root list, and it calls whatever else draws there.

### The interface pass

- **Every window's main frame gains a pass `Interface`** after `Main`: drawn onto `Main`'s result, with no depth. The overlay, the authoring menu, and every screen draw there. This reverses ADR-0025's plan of interface as a texture frame composited by the main frame's owner. A screen drawn as a texture is still possible, as any texture frame is, but it is not how the library draws.
- **The renderer adds `Interface` to every frame that ends on a window,** including one whose owner replaces it, so a program that writes its own main frame cannot drop the overlay by leaving the pass out. The main frame's owner (ADR-0025: `may_replace`) still writes the frame's other passes.
- **World-anchored interface (nameplates, health bars) is projected and drawn in `Interface`.** It is then called like any other drawing, and it is picked as anything else is.

### Input reads the sequence

**Picking.** Every program the renderer builds for a pass of a window's main frame also writes, for each fragment, the call's number in the sequence, the actor whose list it is in, its entry, and its `Pointer`. The renderer adds this step itself, and program authors never write it.

- **Only at the probes.** Each frame the renderer is given the window's probe points: the pointer's position, and the positions of presses since the last frame. A fragment that lands on a probe appends `(call, actor, entry, pointer)` to that probe's stack of four slots. Fragments elsewhere write nothing. The cost does not grow with the window's size.
- **The stack is in call order, whatever the GPU's order.** Fragments of one draw land in no fixed order, so each probe's slots are sorted by call number after readback. The top of the stack is the last call that drew there, which is what is seen there. A pixel under more than four layers keeps the top four.
- **`Hit` draws only into the stack:** a quad with no colour, for a modal's backdrop, an invisible handle, or the world's catch-all, a full-window `Hit` in the first root's list. A `Draw` with `Pointer::Ignores` draws colour and nothing into the stack.
- **The readback is a few hundred bytes a frame.** It arrives a frame or two later, so the pointer is tested against what was on screen.

**Pointer events cascade.** The window keeps each probe's newest stack and delivers every pointer event (`PointerMoved`, `PointerDown`, `PointerUp`, `Wheel`) top-down through it: to the actor whose list holds each entry, until the first entry whose pointer is `Blocks`, all in one step, with no answer asked. Each actor is told which of its entries was hit, so it does not test its own widgets again.

- **Hover:** when the set of entries delivered to changes, those left get `PointerLeft` and those reached get `PointerEntered`.
- **Capture:** the entries a `PointerDown` reached get every move and the `PointerUp` until the button comes up, wherever the pointer goes, as a key that is down keeps whom its press was sent to (ADR-0053).
- **The wheel follows the pointer**, so a console that blocks stops the wheel from zooming the game beneath it, which ADR-0053 got by sending the wheel with key focus.
- **Raw subscription stays** for every pointer event and for the window's own events (`Resized`, `Moded`, `FocusGained`, `FocusLost`, `CloseRequested`, `Closed`), as ADR-0024 delivers them.

**Keys cascade the same way.** An actor declares the keys it hears with `Keys { frame, keys, blocks }` in its list, at its place in the sequence. The renderer sends the window each frame's `Keys` entries in reverse call order whenever they change. The window delivers each `KeyDown` top-down through the entries whose set holds the key, until the first that blocks.

- **Focus is a declaration.** A screen that takes the keys lists `Keys { frame, keys: KeySet::ALL, blocks: true }` while it wants them, and drops the entry when it does not. The dev overlay lists `Keys { frame, keys: KeySet::of([BACKTICK]), blocks: true }` in its list, called last in the root list, so the backtick reaches it whatever has focus beneath, and every other key falls through.
- **Typed text and repeats follow the `KeyDown`** that typed them (ADR-0051), and a key that is down keeps whom its press was sent to (ADR-0053).
- **A focus change takes effect with the list that declares it**, the frame after the click that caused it.
- **Keys are not sent to raw subscribers.** A raw subscriber would hear keys that a declaration above it blocked, and blocking would then mean nothing.

## Consequences

- **The draw sequence is reproducible.** The same lists give the same calls in the same order every frame and every run, so ties at equal depth do not flicker, captures compare, and an actor can draw front to back on purpose.
- **Removed:** `Order`, `Join::to`, `Listing::Loose`, the renderer's loose-mesh and loose-text walks, and grouping a pass's draws or dispatches by program. A placement or dispatch draws only when a list names it, and one that no list names is logged. Every drawing actor sends a list. The 17 call sites of `Join::to` outside tests, `TextJoin::to`, and the tests that use `Order::Loose` move to lists, called from the root list or a parent.
- **Every pixel of a window traces to an actor.** The picking stack names the actor and entry that drew a pixel, and the chain of calls above it, so an inspector, an agent's "what is at (x, y)", and the authoring menu's "select what I clicked" read what input already reads.
- **Each actor's list can be recorded on its own thread,** as a GPU render bundle, and the bundles executed in the walk's order. Not needed now, but nothing in the order prevents it.
- **Each program in a window frame's passes gets a step it did not write,** and each window frame gets a probe buffer and a readback. Programs run in a texture frame get neither: a texture frame has no pointer.
- **ADR-0023, ADR-0025, ADR-0031, and ADR-0053 are edited to match.**
  - ADR-0023's refusal of an order key holds: the root list is written in one place, the setup, and every other order is a parent's list, never a number that actors agree on.
  - ADR-0025's "order is declared where drawing is described" holds and is now total. Its "no order within one pass between actors" and its interface-as-texture-frame are replaced.
  - ADR-0031's lists stand as the form of every list, and its "order between actors is still not promised" is replaced. Both items it left for later are settled: dispatches are listed (`Entry::Run`), and nothing is loose.
  - ADR-0053's key focus slot (`TakeKeyFocus`, `ReleaseKeyFocus`, `KeyFocus`) is replaced by `Keys` entries. The overlay is last in the root list, so it draws on top.
- **Follow-on work, in order:**
  1. The `Interface` pass, the root list in setup, and `Call`, beside `Loose`.
  2. The overlay moves to `Interface`, last in the root list, as the first client.
  3. Picking and the input cascade in the window.
  4. Every `Loose` site moves to a list, and `Order` is deleted.
  5. The widget library's own ADR: screens as actors, layout, widget identity across rebuilds, a pool of texts, and clipping for scroll views.
- **Left open:** texture frames that do not read each other still run in the order of their ids (ADR-0025). Ids come from one count in the order mail was handled, so that order can differ between runs. Whether it must be declared is not decided here.

## Alternatives considered

- **A z-index on each actor or placement.** Numbers that unrelated actors must agree on, which ADR-0023 refused.
- **An app-wide list of actors in draw order**, sent by the window's owner. It orders actors only at one level, and an overlay over any app would need the app to put it in its list.
- **Spans: order coordinates (`[1, 0]` is game → hud) handed down by splitting, each actor submitting its list with its span.** A reorder hands new coordinates down the whole subtree, which tears it across frames as each level adopts, needs counters that never reuse a coordinate, and a parent can only draw under its children. A parent's list that calls its children reorders in one list and draws over them.
- **The order the roots are listed in, as the root list.** That order is the setup's dependency order: a root reaches only roots listed before it (ADR-0029: `Ordered<I>`). A game that reaches the authoring tool would have to be listed after it, and so draw over it.
- **The root list held by one app actor.** That actor could reorder it at any time and put itself above the overlay.
- **Keeping `Loose` for drawing whose order "does not show"** (opaque, depth-tested). Draw calls always run in some order, and a loose order is a hash map's. Keeping it would leave two ways to draw, and an undeclared sequence.
- **A pass for each root actor.** It needs no new concept, but a pass that writes depth clears it, so the world's roots could not share one depth buffer, and each pass costs about 35 µs.
- **Hit testing on rects the renderer derives from entries.** The renderer does not know where a placement lands, because its program places it.
- **Rects sent by each screen to the window.** A second structure, which would have to be kept in the sequence's order by some other means.
- **A full-window id buffer.** Exact, but about 75 MB at 1080p and 300 MB at 4K for four layers. Probes give the same stacks for a few hundred bytes.
- **Key focus as a slot, latest take wins, with global shortcuts registered beside it** (ADR-0053). Two mechanisms, and a sibling that takes focus silences the overlay's backtick.
