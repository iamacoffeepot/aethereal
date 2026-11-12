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

**The sequence of draw calls the renderer issues is fully declared, and the renderer never chooses an order. A pass's order is a tree of spans. A span is a place in that order, and the lists sent into it fill it. A list is ordered as sent, and it can name child spans, which its sender hands to others. The root is split once, as the program is set up, and the split is the same in every pass and never changes; each piece is handed forward to the root actor that draws there. Every list is in a span, and `Loose` is removed. Input reads the same sequence: the renderer records what was drawn under the pointer, in call order, and the window delivers the pointer, and keys, top-down through what was declared there.**

```
setup: root ── split once ──> [World, Game, Authoring, Dev], each handed to its root actor   (fixed for the program's life)
Game ── its list in its span: [Draw(sky panel), Span(hud), Span(inventory), Draw(frame)]
hud, inventory ── each its own list, in its own span
renderer: frames → passes → root spans in order → each list as sent, descending at each Span entry
window: pointer ── probe ── stack in call order ── top-down until one blocks ──> actors
         key   ── Keys entries in call order ── top-down until one blocks ──> actors
```

### The sequence is declared

- **The renderer issues a frame's calls in one order, and every step of it is declared.** Frames, then each frame's passes in the order their author wrote (ADR-0025), then the root's spans in the order the setup split them, then each span's list exactly as sent, descending into a child span where the list names it.
- **The renderer never sorts or reshuffles.** A list is not grouped by program and not reordered for pipelines. The renderer skips setting a pipeline that is already set, so a list that keeps one program together pays for no switches.
- **What a call does is not the renderer's concern.** An opaque mesh, a blended one, a text, a dispatch: each is issued where it was listed. An actor that sorts by distance, by material, or by nothing sends its list in that order, and that is the order.

### Spans

```rust
enum Span { Root(u32), Child { maker: ActorId, n: u32 } }   // a piece of the setup's split, or a parent's child

#[kind(name = "aether.render.draw_list")]
struct DrawList { span: Span, entries: Vec<Entry> }       // where in the order, and the calls in order

enum Entry {
    Draw { placement: PlacementId, pointer: Pointer },        // a mesh or a text placement of the sender's
    Run(DispatchId),                                          // a dispatch of the sender's, in a compute pass
    Span(u32),                                                // the child span `Child { maker: sender, n }`, issued here
    Hit { frame: Frame, rect: Rect, pointer: Pointer },       // drawn into that frame's picking stack only (see Input)
    Keys { frame: Frame, keys: KeySet, blocks: bool },        // the keys its sender hears in that frame's window (see Input)
}
enum Pointer { Blocks, Passes, Ignores }
```

- **A span is a name, and it is minted by its maker without asking anyone.** A child span is `Child { maker, n }`, where `maker` is the actor that names it in its list and `n` is a number it chooses. Splitting is data: the maker puts the child's name into the config of the actor it spawns, or hands it on, and lists it where the child belongs. No message asks the renderer for a span, and nothing waits for an answer.
- **The parent's list places its children.** A parent can draw a background, name a child, then draw a border over it: `[Draw(background), Span(0), Draw(border)]`. Reordering children is sending a new list (ADR-0031: a later list replaces an earlier one). A library offers `split`, `push`, `insert_before`, and `reorder` as helpers that build that list.
- **One actor lists into a span.** The first list sent into a span claims it for its sender, until the sender ends. A list from another actor into it is refused and logged, naming the lister, as `may_replace` refuses a second frame owner. Two actors in one span would put two lists' order at the mercy of mail arrival, which is the fault this ADR removes. No other check is made: a span's name is not secret, and an actor that names another's span is a bug, refused loudly.
- **A span named by no list is not drawn,** and a list into a span no parent names is kept and not drawn. Nothing reaches the sequence except through the tree.
- **A span is universal: it is an actor's place in the order, and nothing else.** It is the same in every frame and every pass. The frame and pass of each call are declared by the actor, through what it lists: a placement's join names its frame and pass (`Join::listed::<Interface>(frame, ..)`), a dispatch's names its compute pass, and `Hit` and `Keys` name their frame. For each pass of each frame, the renderer issues the entries of a list that land there, in list order. An actor has one span.
- **The tree is walked iteratively, with a cap on depth.** Cycles cannot reach the sequence: a child is named only by its maker, and the walk keeps the spans it has entered and refuses one it meets again. The renderer flattens the tree when a list changes, never at `Render`, and keeps the flattened sequence per root span, so a list that changes every frame re-flattens only its own root span.

### The root is split once, at setup

- **The setup splits the root once, and the split's order is the draw order in every pass.** Each piece goes forward into its root actor's config as a span, the same way a parent puts a child's span into the config of the child it spawns:

  ```rust
  let [world, game, authoring, dev] = setup.split();   // the renderer's config gets this order

  program
      .root::<World>(WorldConfig { span: world, .. })
      .root::<Game>(GameConfig { span: game, .. })
      .root::<Authoring>(AuthoringConfig { span: authoring, .. })
      .root::<Overlay>(OverlayConfig { span: dev, .. });
  ```

  A root's span is `Span::Root(n)`, its place in the split. An actor draws in a pass by sending a list there, in the span it was handed, so which passes an actor uses is not declared anywhere. Roots and children get their spans the same way, so the top of the tree has no special case. Shorthand over this can come later; this is the canonical form.
- **The root is never handed to anyone, and its order never changes.** No actor holds the whole root, so none can reorder it or slip something above the dev overlay. Each actor gets only its own span.
- **The same root order applies to every window's main frame,** including windows opened while the program runs (ADR-0027), which have no setup of their own. An actor that draws in a window it opened lists into its own span with that window's frame.
- **A texture frame's maker declares that frame's root order** in `CreateFrame`, fixed for the frame's life, and lists into it as its own.

### The interface pass

- **Every window's main frame gains a pass `Interface`** after `Main`: drawn onto `Main`'s result, with no depth. The overlay, the authoring menu, and every screen draw there. This reverses ADR-0025's plan of interface as a texture frame composited by the main frame's owner. A screen drawn as a texture is still possible, as any texture frame is, but it is not how the library draws.
- **The renderer adds `Interface` to every frame that ends on a window,** including one whose owner replaces it, so a program that writes its own main frame cannot drop the overlay by leaving the pass out. The main frame's owner (ADR-0025: `may_replace`) still writes the frame's other passes.
- **World-anchored interface (nameplates, health bars) is projected and drawn in `Interface`.** It is then in the tree, and it is picked as anything else is.

### Input reads the sequence

**Picking.** Every program the renderer builds for a pass of a window's main frame also writes, for each fragment, the call's number in the sequence, its span, its entry, and its `Pointer`. The renderer adds this step itself, and program authors never write it.

- **Only at the probes.** Each frame the renderer is given the window's probe points: the pointer's position, and the positions of presses since the last frame. A fragment that lands on a probe appends `(call, span, entry, pointer)` to that probe's stack of four slots. Fragments elsewhere write nothing. The cost does not grow with the window's size.
- **The stack is in call order, whatever the GPU's order.** Fragments of one draw land in no fixed order, so each probe's slots are sorted by call number after readback. The top of the stack is the last call that drew there, which is what is seen there. A pixel under more than four layers keeps the top four.
- **`Hit` draws only into the stack:** a quad with no colour, for a modal's backdrop, an invisible handle, or the world's catch-all, a full-window `Hit` in the bottom span. A `Draw` with `Pointer::Ignores` draws colour and nothing into the stack.
- **The readback is a few hundred bytes a frame.** It arrives a frame or two later, so the pointer is tested against what was on screen.

**Pointer events cascade.** The window keeps each probe's newest stack and delivers every pointer event (`PointerMoved`, `PointerDown`, `PointerUp`, `Wheel`) top-down through it: to each entry's lister, until the first entry whose pointer is `Blocks`, all in one step, with no answer asked. Each actor is told which of its entries was hit, so it does not test its own widgets again.

- **Hover:** when the set of entries delivered to changes, those left get `PointerLeft` and those reached get `PointerEntered`.
- **Capture:** the entries a `PointerDown` reached get every move and the `PointerUp` until the button comes up, wherever the pointer goes, as a key that is down keeps whom its press was sent to (ADR-0053).
- **The wheel follows the pointer**, so a console that blocks stops the wheel from zooming the game beneath it, which ADR-0053 got by sending the wheel with key focus.
- **Raw subscription stays** for every pointer event and for the window's own events (`Resized`, `Moded`, `FocusGained`, `FocusLost`, `CloseRequested`, `Closed`), as ADR-0024 delivers them.

**Keys cascade the same way.** An actor declares the keys it hears with `Keys { frame, keys, blocks }` in its list, at its place in the sequence. The renderer sends the window each frame's `Keys` entries in reverse call order whenever they change. The window delivers each `KeyDown` top-down through the entries whose set holds the key, until the first that blocks.

- **Focus is a declaration.** A screen that takes the keys lists `Keys { frame, keys: KeySet::ALL, blocks: true }` while it wants them, and drops the entry when it does not. The dev overlay lists `Keys { frame, keys: KeySet::of([BACKTICK]), blocks: true }` in the last span of the split, so the backtick reaches it whatever has focus beneath, and every other key falls through.
- **Typed text and repeats follow the `KeyDown`** that typed them (ADR-0051), and a key that is down keeps whom its press was sent to (ADR-0053).
- **A focus change takes effect with the list that declares it**, the frame after the click that caused it.
- **Keys are not sent to raw subscribers.** A raw subscriber would hear keys that a declaration above it blocked, and blocking would then mean nothing.

## Consequences

- **The draw sequence is reproducible.** The same lists give the same calls in the same order every frame and every run, so ties at equal depth do not flicker, captures compare, and an actor can draw front to back on purpose.
- **Removed:** `Order`, `Join::to`, `Listing::Loose`, the renderer's loose-mesh and loose-text walks, and grouping a pass's draws or dispatches by program. A placement or dispatch draws only when a list names it, and one that no list names is logged. Every drawing actor sends a list. The 17 call sites of `Join::to` outside tests, `TextJoin::to`, and the tests that use `Order::Loose` move to lists in their actor's span.
- **Every pixel of a window traces to an actor.** The picking stack names the span and entry that drew a pixel, so an inspector, an agent's "what is at (x, y)", and the authoring menu's "select what I clicked" read what input already reads.
- **Each program in a window frame's passes gets a step it did not write,** and each window frame gets a probe buffer and a readback. Programs run in a texture frame get neither: a texture frame has no pointer.
- **ADR-0023, ADR-0025, ADR-0031, and ADR-0053 are edited to match.**
  - ADR-0023's refusal of an order key holds: the root order is split in one place, the setup, and every span is handed down by whoever holds the one above it, never a number that actors agree on.
  - ADR-0025's "order is declared where drawing is described" holds and is now total. Its "no order within one pass between actors" and its interface-as-texture-frame are replaced.
  - ADR-0031's lists stand as the form of every list, and its "order between actors is still not promised" is replaced. Both items it left for later are settled: dispatches are listed (`Entry::Run`), and nothing is loose.
  - ADR-0053's key focus slot (`TakeKeyFocus`, `ReleaseKeyFocus`, `KeyFocus`) is replaced by `Keys` entries. The overlay is handed the last span of the setup's split, so it draws on top.
- **Follow-on work, in order:**
  1. The `Interface` pass, the root split in setup, and lists into spans, beside `Loose`.
  2. The overlay moves to the last span of the split, in `Interface`, as the first client.
  3. Picking and the input cascade in the window.
  4. Every `Loose` site moves to a span, and `Order` is deleted.
  5. The widget library's own ADR: screens as actors, layout, widget identity across rebuilds, a pool of texts, and clipping for scroll views.
- **Left open:** texture frames that do not read each other still run in the order of their ids (ADR-0025). Ids come from one count in the order mail was handled, so that order can differ between runs. Whether it must be declared is not decided here.

## Alternatives considered

- **A z-index on each actor or placement.** Numbers that unrelated actors must agree on, which ADR-0023 refused.
- **An app-wide list of actors in draw order**, sent by the window's owner. It orders actors only at one level, and an overlay over any app would need the app to put it in its list.
- **Root spans named by layer types** (`.layers::<Interface, (Game, Authoring, Dev)>()`), each found by a root through its needs. A trait and a lookup that a span passed straight into the config does not need.
- **A split for each pass.** It declares again which passes each actor draws in, which its lists already say, and an actor drawing in two passes would need two spans.
- **The order the roots are listed in.** That order is the setup's dependency order: a root reaches only roots listed before it (ADR-0029: `Ordered<I>`). A game that reaches the authoring tool would have to be listed after it, and so draw over it.
- **The root handed to one app actor, which splits it.** That actor could reorder the root at any time and put itself above the overlay. Splitting at setup gives the same order with no holder.
- **Spans as resources made by the renderer, in a slab with generations, path keys packed in a `u128`, and a cap.** Each split would be a round trip before a child could be spawned with its span. A path key changes when a sibling is inserted, and the flattened walk already is the order.
- **Keeping `Loose` for drawing whose order "does not show"** (opaque, depth-tested). Draw calls always run in some order, and a loose order is a hash map's. Keeping it would leave two ways to draw, and an undeclared sequence.
- **A pass for each root span.** It needs no new concept, but a pass that writes depth clears it, so the world's root spans could not share one depth buffer, and each pass costs about 35 µs.
- **Hit testing on rects the renderer derives from entries.** The renderer does not know where a placement lands, because its program places it.
- **Rects sent by each screen to the window.** A second structure, which would have to be kept in the sequence's order by some other means.
- **A full-window id buffer.** Exact, but about 75 MB at 1080p and 300 MB at 4K for four layers. Probes give the same stacks for a few hundred bytes.
- **Key focus as a slot, latest take wins, with global shortcuts registered beside it** (ADR-0053). Two mechanisms, and a sibling that takes focus silences the overlay's backtick.
