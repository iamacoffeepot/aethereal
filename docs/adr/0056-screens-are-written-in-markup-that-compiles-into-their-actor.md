# ADR-0056: Screens are written in markup that compiles into their actor

- **Status:** Proposed
- **Date:** 2026-11-12

## Context

Programs need screens: the dev overlay (`aethereal-overlay`, drawn by hand today), and a game's HUD, inventory, chat, and settings. Agents will write most of them, and people will read and review them, so a screen has to be readable text whose structure and intent show at a glance, and it has to diff and merge as text.

An earlier widget library in another engine failed in two ways, and both are hard constraints here:

- **It added things to the actor framework** that only the library needed: inline children, handler sets, input shells, widget kinds. They got in the way of everything else. This library adds nothing to the actor framework; it is plain code inside the actor that uses it.
- **It caused draw-order problems for other actors.** This library draws only through its actor's own list.

What is already decided:

- **Draw order is declared, and input follows it** (ADR-0055: `DrawList`, `Entry::{Draw, Run, Call, Hit, Keys}`). An actor's list orders its own draws and calls its direct children. A call places its child with an area: `Area::Inherit { origin, clip }` draws the child in the caller's coordinates, and `Area::Absolute { clip }` draws it in window pixels (`Area::inherit`, `Area::absolute`, `Area::WINDOW`, `Area::CALLER`, `Clip::{Whole, To}`). A child is always cut to its caller's clip.
- **The pointer reaches what was drawn under it, top-down** (ADR-0055: `Pointed { window, entry, event }`, `Pointer::{Blocks(Cursor), Passes(Cursor), Ignores}`). `entry` is the index in the list the actor last sent, and positions arrive in the entry's own coordinates. A held button keeps sending to the entry it pressed.
- **Keys reach declarations top-down** (ADR-0055: `Entry::Keys { frame, keys, blocks }`). Focus is a declaration: an actor that wants every key lists every key, blocking.
- **Text is laid out and drawn by the renderer** (ADR-0052: `CreateText`, `WriteText`, `MoveText { at, layer, opacity, clip }`). Measuring a text without placing it was left as later work.
- **A running guest moves onto new code, and its state moves with it by schema** (ADR-0054).
- **An actor waits by running a `Sleep` job** (ADR-0018: `Sleep`).

## Decision

**A screen is an actor. Its view is written in XML, in its own file: each element is a widget, each attribute a typed setting, children give layout and draw order, and braces name what the view reads from the screen in a small subset of Rust. A build script compiles the markup, and the sheets that style it, into the actor's Rust. Nothing reads XML at runtime. In every `Prepare`, the screen builds its widget tree and compares it with the last; only when it changed, or something outside it did, does the screen lay it out with Taffy and send what changed of its one draw list. Widgets are library code with no actors of their own. A panel with a life of its own is a child actor, placed by a call's area.**

```
inventory.xml + sheets ── build script ──> inventory_view.rs ── include! ──> Inventory (an actor)

Inventory:  mail ──> turn (&mut self)
            Prepare ──> widget tree ──> same as the last, nothing outside changed? ──> nothing sent
                                          └─> otherwise: Taffy ──> draws, kept by key ──> what changed, DrawList
            Pointed / Keyed ──> widget by entry ──> primitive's state ──> event ──> the screen's &mut self method
```

### A screen is an actor

- **The screen's struct is its model, its methods are its controller, and its markup is its view.** It talks to game actors by mail and keeps its own copy of what it shows.
- **Widgets are not actors.** They are code in the screen's actor, and nothing about them is mail.
- **A screen rebuilds in `Prepare`.** `Prepare` is the shared stage between `Tick` and `Render` (ADR-0020): it begins once every turn of the frame's `Tick` has run, and finishes before `Render` draws, so a rebuild sees every turn of the frame and what it sends is handled before the frame is drawn. A program that has screens runs it, `with_cycle::<(Tick, Prepare, Render)>()`; the library subscribes its screen to it as the screen begins. Nothing is added to the actor framework.
  - **In every `Prepare` the screen builds its tree, which is cheap, and compares it with the last.** Identical, it does nothing more: no layout and no mail, and the renderer keeps the last list (ADR-0031). Different, it lays the tree out, compares its draws with the last by key, and sends only what changed. So nothing marks changes and nothing can be forgotten, and layout still runs only when something changed.
  - **Some changes come from outside the tree,** and each makes the next `Prepare` lay out though the tree is identical: a font's metrics coming, the screen's size, frame, or UI scale changing, a widget's state changing on input, and the screen's focus moving. Each goes through the library, so the library knows.
- **Draws are kept by widget key.** A rebuild compares the new tree with the last and sends only what changed: a moved text is a `MoveText`, a changed one is a `WriteText`, and an unchanged widget sends nothing.

### Markup

```xml
<!-- inventory.xml -->
<sheets>
    <sheet src="theme.sheet"/>
    <sheet src="inventory.sheet"/>
</sheets>

<Column class="inventory">
    <Text class="title">Inventory</Text>
    <Text>Gold: {self.gold}</Text>

    <Grid columns="6" items="{self.items}" as="item" key="{item.id}">
        <ItemSlot item="{item}" click="{self.select(item.id)}" drag="{item.id}" drop="{self.place(item.slot)}">
            <Tooltip><Text>{item.name}</Text></Tooltip>
        </ItemSlot>
    </Grid>

    <Column class="detail" show="{self.selected_item()}" as="item">
        <Text>{item.name}</Text>
        <Button click="{self.equip(item.id)}">Equip</Button>
    </Column>
</Column>
```

- **An element is a widget, and its tag is the widget's Rust type name, exactly:** `<ItemSlot>` is `ItemSlot`, so a search finds the type and every use. Tags are capitalized because types are. That is ordinary XML, as in XAML, Panorama, and Unity's markup.
- **A lowercase element is a markup keyword, not a type:** `sheets`, `sheet`, `widget`, `param`, `event`, `child`. A reader tells a widget from a declaration on sight.
- **An attribute is one of the widget's settings,** in snake_case as its Rust field is. A value without braces is a literal, parsed by the setting's type: `columns="6"`, `class="detail"`.
- **Children are both layout order and draw order.** A later child draws over an earlier one.
- **Text content is the widget's text,** and may hold references: `Gold: {self.gold}`.

### References are a small subset of Rust

- **Braces mark everything the view takes from the screen,** so a reference never looks like a literal.
- **`self` is the screen.** A field reads as `{self.gold}`, and a method call as `{self.selected_item()}`: the parentheses show it is a call. A name without `self` is a local made by `as`: `{item.name}`.
- **Only paths, field access, and method calls whose arguments are paths are allowed.** No operators, literals, closures, or blocks. A view that needs a computed value calls a `&self` method that computes it, so a negation is a method such as `unlocked()`. Logic lives in Rust, never in the view.
- **The build parses each block as a Rust expression and refuses anything outside the subset,** naming the file and line.
- **The attribute decides when the call runs.** A setting (`show`, `items`, a text) runs when the view is built and must be a `&self` method. An event (`click`, `change`, `drop`) runs when the event fires and must be a `&mut self` method.

### Lists and conditions are settings, not elements

Every element is a widget; there is no `<each>` or `<if>`.

- **`items` repeats a widget's one child, once per item.** `items="{self.items}" as="item" key="{item.id}"` names the list, the local for each item, and the key that tells items apart. A widget with `items` has exactly one child, its template; a header above a list is a sibling in an outer widget.
- **The widget that lays the items out owns them.** A `Scroll` with `items` builds only the rows that are on screen.
- **`show` takes a `bool` or an `Option`.** With a `bool`, the widget shows while it is true. With an `Option`, the widget shows while it is `Some`, and `as` names the value inside.
- **The key keeps each item's widget state with the item** when the list reorders, and drops that state when the item leaves.

### Events call the screen's methods

```rust
fn select(&mut self, turn: &mut Turn<'_, Self, InventorySends>, item: ItemId) -> Result<(), Fault> { … }
fn rename(&mut self, turn: &mut Turn<'_, Self, InventorySends>, item: ItemId, text: String) -> Result<(), Fault> { … }
```

- **An event names the method and its arguments:** `click="{self.select(item.id)}"`. The turn is passed implicitly, as Rust passes `self`.
- **An event's arguments are copied when the view is built, and must be `Copy`.** That steers them to ids, never to a snapshot of an item that may have changed between the draw and the click.
- **A widget that hands a value with its event hands it after the arguments:** a text input's `change` and `submit` the text, a toggle's `change` whether it is on, a slider's `change` its value. `change="{self.rename(item.id)}"` calls `rename(turn, item, text)`.
- **The build checks the call against the method's signature.**
- **A method an event calls takes the turn of the screen's input handlers, `Turn<'_, Self, S>`, with `S` what the screen's methods may send** (*settled by the library's input stage*). A turn names what its handler sends, so there is no `Turn<'_, Self>`; the screen declares `S` once, beside its methods, and the library adds its own ask of the window's input method (below).

**What the markup compiles into** (*settled by the library's input stage*). The tree compares by value, so an event is a value, never a closure: each event attribute of a screen's markup becomes a variant of one enum, holding the arguments the view copied, and the screen's `on` matches each variant to the method it names. A widget raises its event with the value it hands, as one of `Raised::{Event(E), Text(E, String), Flag(E, bool), Number(E, f32)}`.

```rust
// inventory_view.rs, generated from inventory.xml
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InventoryEvent {
    Select(ItemId),        // <ItemSlot click="{self.select(item.id)}"/>
    Rename(ItemId),        // <TextInput change="{self.rename(item.id)}"/>
}

impl View for Inventory {
    type Event = InventoryEvent;

    fn view(&self) -> Tree { … }   // Button::new(InventoryEvent::Select(item.id)), built from the item now

    fn on(&mut self, turn: &mut Turn<'_, Self, Self::Sends>, raised: Raised<InventoryEvent>) -> Result<(), Fault> {
        match raised {
            Raised::Event(InventoryEvent::Select(item)) => self.select(turn, item),
            Raised::Text(InventoryEvent::Rename(item), text) => self.rename(turn, item, text),
            unmatched => aethereal_ui::unmatched(turn, &unmatched),
        }
    }
}

aethereal_ui::screen!(Inventory, ui);   // the handlers of `Prepare`, `Pointed`, `Keyed`, and the library's answers
```

```rust
// inventory.rs, by hand
type InventorySends = aethereal_ui::sends![Tells<Path<Game>, Equip>];   // the library's ask, then the screen's own

impl Controls for Inventory {
    type Sends = InventorySends;
    const HOTKEYS: &'static [&'static str] = &["KeyI", "Escape"];

    fn key(&mut self, turn: &mut Turn<'_, Self, InventorySends>, key: Key) -> Result<(), Fault> {
        match key.code.as_str() { "KeyI" | "Escape" => self.close(turn), _ => Ok(()) }
    }
}
```

- **The whole of the generated code is the enum, one `impl View`, and the call of `screen!`,** so nothing generated shares an impl with what is written by hand.
- **The input handlers take in a turn of input, then call the screen's methods,** in the order the events were raised, then its `key` with each key left for it. After any method runs, the view is built again for the input that follows, so a second click before the next frame carries the arguments the view copies now, not those of the last `Prepare`. The next `Prepare` lays it out.
- **A widget's event type is the screen's,** `Button<InventoryEvent>`. A tree holds widgets of any type, so what a widget raises is held without its type and taken back as the screen's: only a tree built by hand can hold another screen's event, and that is logged.

### Styles are sheets, compiled in

- **A sheet is its own file of classes.** A theme is a sheet.
- **A screen lists its sheets in order, and where two set the same property of the same class, the later sheet wins.** There is no specificity.
- **There is no inline style and no style block in a screen.** A screen that needs a look of its own gets its own small sheet, listed last. A widget's look has one source.
- **Sheets compile into each screen that lists them.** Classes become concrete values in the screen's Rust, so nothing is stored or mailed at runtime. Editing a shared sheet means rebuilding and publishing the screens that list it (ADR-0054).
- **A sheet sets layout as well as look:** `direction`, `gap`, `padding`, `width`, `columns`, and the rest of Taffy's flex and grid settings, as well as colours, borders, fonts, and sizes. An attribute on an element can set the same properties for that element.

### Your own widgets: composites in markup, primitives in Rust

A composite is built from other widgets, and is written in markup:

```xml
<!-- item_slot.xml -->
<widget name="ItemSlot">
    <param name="item" type="Item"/>
    <event name="click"/>

    <Button class="slot" click="{click}">
        <Stack>
            <Image src="{item.icon}"/>
            <Text class="count" show="{item.stacks()}">{item.count}</Text>
        </Stack>
    </Button>
</widget>
```

- **A composite is a function of its params.** It has no state and no `self`, so its references are its params and its locals. It compiles to a Rust type, `ItemSlot`, with the params as fields.
- **A composite declares the events it raises and passes them on:** the screen's `click` goes to the slot, and the slot hands it to its `Button`.
- **A primitive draws or lays out something new, and is written in Rust.** It implements `Widget`, and the stock widgets are written the same way:

```rust
#[derive(Default, Plain)]
#[kind(name = "aether.ui.press")]
struct Press { hovered: bool, pressed: bool }

impl<E: Event> Widget for Button<E> {
    type State = Press;
    fn arrange(&self) -> Arrange { Arrange::Stack }
    fn draw(&self, state: &Press, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) { … }
    // Input, each with a default, so a widget that takes none says nothing of it:
    fn pointer(&self, state: &Press) -> Pointer { Pointer::Blocks(Cursor::Hand) }
    fn focusable(&self) -> bool { true }
    fn hears(&self) -> Hears { Hears::Keys(&["Enter", "NumpadEnter", "Space"]) }
    fn pointed(&self, state: &mut Press, pointing: Pointing, input: &mut Input<'_, '_>) { … input.raise(Raised::Event(self.click)) }
    fn key(&self, state: &mut Press, key: &Key, input: &mut Input<'_, '_>) -> Used { … }
    fn typed(&self, state: &mut Press, text: &str, input: &mut Input<'_, '_>) { … }
}
```

- **Either kind is used by its type name.** The screen brings it into scope with an ordinary `use`, and the stock widgets come from a prelude. There is no registry: rustc resolves every tag. Rewriting a composite as a primitive changes no screen that uses it.
- **Composites are shared as source,** like sheets: each screen that uses one compiles it in.

### Widget state is kept by key

- **Game state belongs to the screen.** It is what the game means: the selected item, the open tab, the list of items. Widgets read it through params and change it through events.
- **Interaction state belongs to primitives:** hover, pressed, scroll offset, caret and selection, an open popup, a tooltip's delay. A primitive declares it as `type State`.
- **The screen keeps a table of widget state, keyed by each widget's place in the tree and the list keys on the way to it.** The library makes a state with `Default` the first time its key appears, keeps it across rebuilds, and drops it when the key is gone. No widget makes or frees its own state.
- **The table is part of the screen's state,** so hover, scroll, and focus survive a move onto new code (ADR-0054).
- **Input reaches a primitive by its entry.** The library maps each entry of the list it sent to the widget that listed it. `Pointed` goes to that widget's `pointed`, in its own coordinates, from its border box's top left, which updates its state and may raise an event, and the event calls the screen's method.
- **A widget the pointer reaches is listed with a hit over its box** (*settled by the library's input stage*): an `Entry::Hit` after its draws and before its children's, whose `Pointer` the widget declares, `Blocks(Hand)` for a button and `Blocks(Text)` for a text input, as its state stands, a slider's open hand turning closed as it drags. Its draws stay `Ignores`. A hit, not a draw, is reached, so a button with no background is still pressed, and one widget is one entry whatever it draws.
- **A widget whose style fills its box blocks the pointer there,** with the default cursor, so a panel is solid to the game beneath and a click on its empty space walks no character. A widget within one the pointer reaches lists no hit of its own unless it says so, so a button's filled label does not take the press from the button.
- **A click is a press and a release of the main button on the same widget:** the release's `over` (ADR-0055) names the hit it was over, so a press dragged off and released elsewhere clicks nothing. Moves and the release reach a widget wherever the pointer goes while its press is held, as the window's capture sends them, so a slider dragged past its end still sets its end.

### Layout is Taffy

- **Layout is flexbox and CSS grid, computed by the Taffy crate.** There is no document flow, float, or inline layout.
- **Layout runs in the screen when the tree or something outside it changed, never every frame.** The rects it gives place the draws, and the pointer follows the draws.
- **A text tells layout its size** through Taffy's measure step. It is measured with the font's metrics, asked of the renderer once for each font, and with the same layout code the renderer draws text with, so the measured size is the drawn size (see Consequences).
- **Sizes are logical pixels times a UI scale,** so one scale setting enlarges everything without touching markup.
- **A root screen learns the window's size from the window, and a child screen learns its size from its parent** (see Child actors). A size change is mail, and costs one rebuild.
- **Taffy is pinned to an exact version, without default features.** The library calls Taffy's algorithms over its own tree and does not use Taffy's tree type.

### Focus and keys

- **Each screen has at most one focused widget,** kept in its widget table. A focused key that disappears, or whose widget can no longer hold focus, clears focus as the tree is laid out.
- **Clicking a focusable primitive focuses it.** Clicking elsewhere in the screen, on anything it lists a hit for, clears focus, and so does Esc.
- **Tab and Shift-Tab move focus through focusable widgets in markup order,** which is the tree's order, round from the last to the first. There is no tab index; reorder the markup to change the order.
- **A key goes first to the focused widget, then to the focus keys, Tab and Esc, and a key neither uses goes to the screen's `fn key(&mut self, turn, key: Key)`.** Hotkeys are an ordinary `match` there, and the screen declares the set they use beside the method, as `Controls::HOTKEYS` in the same impl (*settled by the library's input stage*). A `Key` carries its `code`, its `key`, whether it repeats, and the modifiers held, so a chord is read there too.
- **The library writes the screen's `Entry::Keys`.** While a `TextInput` is focused, it declares every key, blocking. Otherwise it declares the screen's hotkey set, and, while a widget has focus, Tab, Esc, and the keys that widget hears, blocking (*settled by the library's input stage*). So a shown screen whose widgets have no focus takes only its hotkeys, and a HUD with buttons never takes the game's Tab or Esc; a screen that wants Tab to bring focus to its first widget lists Tab among its hotkeys.
- **The modifiers are watched, not taken** (*settled by the library's input stage*). A key event carries no modifiers (ADR-0024), so beside a set short of every key the library declares Shift, Control, and Meta in a second entry that does not block, and keeps which are down. Shift-Tab and a text input's Shift-arrow read them, and the game still hears Shift.
- **A focused text input uses every key that types text,** a key whose meaning is one character with neither Control nor Meta held, as its text comes after it (ADR-0051); so a letter typed into it never reaches the screen's hotkeys, and a key it passes, F2 or Tab, still does.
- **The library allows the window's input method while a widget that types has focus, and stops it after** (ADR-0051: `AllowIme`), each asked only as it changes, from the turn of input that moved focus, or from the `Prepare` that found the focused key gone. So the screen's input handlers send that ask besides the screen's own, which `sends![…]` adds.
- **Moving focus by direction, for a gamepad or arrow keys, is left out** until something needs it.

### Popups draw in one top layer per screen

- **A `Popup` sends its children to the screen's top layer.** The top layer draws after the rest of the screen, is not cut by any ancestor widget, and draws its popups in markup order.
- **A popup is placed against the widget that holds it,** and flips or shifts to stay inside its screen.
- **`modal="true"` puts an `Entry::Hit` that blocks the pointer behind the popup,** so the rest of the screen hears no clicks.
- **A `Tooltip` is a popup that opens itself** when its anchor has been hovered for a delay. The delay is the tooltip's own state, and the library waits for it with a `Sleep` job.
- **The top layer stays in its screen's place in the draw order.** A screen drawn later can still cover it. A popup that must be above every screen waits for a need.

### Child actors

A panel with a life of its own is a child actor:

- its own data source, such as a minimap following the world or chat following the network;
- its own rate of change;
- able to fail or be published on its own;
- reused across screens.

Everything else is widgets in one actor. An actor per widget would cost a mailbox, a turn, and a copy of its data for every row, and would spread one click across several turns.

```xml
<child actor="{self.minimap}" area="inherit" class="minimap"/>
```

- **`child` is a keyword, not a widget type.** It takes a slot in the layout like any widget, sized by sheets.
- **The screen spawns and holds its children in Rust.** Markup only places them: `show` hides a child and never stops it.
- **The library calls the child where its slot is,** with `Area::inherit(slot)` or `Area::absolute(slot)`. `area` says which: `inherit` for a child that moves with the screen, and `absolute` for one that needs to know where it is in the window (ADR-0055).
- **The library mails each child its size when it changes.** A child that is a screen lays out to that size. Moving a child sends nothing; only resizing does.
- **The pointer reaches the child directly,** since it drew what is under the pointer. The child talks to its parent by mail, handled by the parent's ordinary handlers, not by markup events.

### Drag and drop is one path

```xml
<ItemSlot item="{item}" drag="{item.id}" drop="{self.place(item.slot)}"/>
```

```rust
fn place(&mut self, turn: &mut Turn<'_, Self>, to: SlotId, dragged: ItemId) { … }
```

1. **A press on a widget with `drag`, then a move of a few pixels, starts a drag.** The payload is copied when the view is built, as an event's arguments are. The held button keeps every move at the source.
2. **The dragged item is the pointer.** The source declares the item's image as the cursor of the entry the press reached, once its drag begins (ADR-0055), and the window shows the press's cursor wherever the pointer goes while the button is held, so it is always on top and never lags.
3. **On release, the window reports what is under the pointer:** the topmost actor, the entry of its list, and the position in that entry's coordinates.
4. **If that is the source's own actor,** the library finds the widget at the position and calls its `drop` method, with the payload appended as the last argument.
5. **If it is another actor,** the source casts it to the protocol that accepts the payload's kind and mails it the drop: the payload, the entry, and the position. That actor's library finds the widget and calls its `drop` method the same way.
6. **If nothing under the pointer accepts the payload,** the source raises `drop_outside`. Dropping an item on the ground is the game's decision, made in that method.

- **The build checks that the payload's type is the `drop` method's last parameter.**
- **A target in another actor does not light up while a drag passes over it,** because it hears nothing until the drop. Targets in the source's own screen can.

### Stock widgets

| Kind | Widgets |
|---|---|
| Layout | `Row`, `Column`, `Grid`, `Stack` (children layered on one spot) |
| Content | `Text`, `Image` (a texture or a region of one, with nine-slice), `Bar` (a filled fraction) |
| Interaction | `Button`, `Scroll` (clips; builds only visible rows when given `items`), `TextInput` (`change`, `submit`), `Toggle`, `Slider`, `Tabs`, `Popup`, `Tooltip` |

Dropdowns, rich text, trees, and colour pickers are left out. A dropdown can later be a composite of `Button` and `Popup`.

### Built by a build script, never read at runtime

- **Each screen's markup is its own `.xml` file beside its Rust,** and each composite is its own file.
- **A crate with screens calls one helper from its build script.** The helper reads the markup and sheets, writes Rust into the build's output directory, and the screen includes it with `include!`.
- **Every generated line carries its source as a comment,** `/* inventory.xml:14 */`, so a rustc error on generated code shows the markup line.
- **The build script reports what it checks itself, with the file and line.** That covers malformed XML, a reference outside the subset, and an attribute a composite does not declare. Names it cannot see, such as Rust primitives and the screen's methods, are checked by rustc.
- **The helper declares every file it reads,** so Cargo reruns it when one changes, and writes the same output for the same input.
- **The generated Rust is not committed.** Review reads the markup.
- **No XML, sheet, or name is read, parsed, or looked up at runtime.** A misspelled reference is a build error, never a blank label in a running game.

### Found, read, and pressed by tools

- **Every widget can be found, read, and pressed from outside, so agents can test screens through hub tools.** A screen built with the library answers a request for its widget tree with each widget's key path, type, rect in window pixels, text, and interaction state. It accepts a press by key path, which the library delivers as the pointer would.
- **These are one request and one tell that the library adds to the screen it builds,** not new machinery in the actor framework.

## Consequences

- **Screens read as their structure.** Markup shows what a screen is, Rust holds what it does, and a reviewer reads one without the other.
- **A screen cannot break the game.** It holds only its own copy of what it shows and acts only by mail, and it can be published onto new code alone (ADR-0054).
- **Mistakes are build errors:** an unknown tag, attribute, or reference, a wrong type, an event's signature, or a drop's payload type.
- **The library adds nothing to the actor framework.** It draws through its actor's list, receives input that already exists, and uses the window, the renderer, and jobs as any actor does.
- **Two crates are added.**
  - `aethereal-ui` is the runtime: widgets, the widget table, layout, input routing, and diffing. It compiles into guests, so it cannot live in `aethereal-render` (native, wgpu) or `aethereal-actor` (the framework, which this library must not touch), and it is not the overlay, which is one of its users.
  - `aethereal-ui-build` is the build-time compiler. It depends on an XML parser and `syn`, which must never enter a guest, so it cannot be part of `aethereal-ui`.
- **Taffy becomes a dependency of `aethereal-ui`,** pinned exactly, without default features, and checked in CI for supply-chain changes.
- **The window gains two things, each an amendment to ADR-0055 and ADR-0024:**
  - a release under a held button reports the topmost actor under the pointer, its entry, and the position in that entry's coordinates;
  - an actor can show an image as the window's cursor, and the default again. *Settled by ADR-0055 as a declaration, not a message that sets it:* each `Draw` and `Hit` entry the pointer reaches carries a `Cursor` in its `Pointer`, an icon or an image whose pixels are a blob, and the window shows the topmost reached entry's, or the press's while a button is held. A message would let whichever actor set it last win over the others sharing the window.
- **Text must be measurable in the actor, which amends ADR-0052.** ADR-0052 left measuring a text without placing it as later work, and layout needs it in the same turn.
  - A font is supplied by an actor and kept by the renderer, and other actors hold only its `FontId`.
  - A screen asks the renderer once for each `FontId` it meets. The renderer answers with the font's metrics (its glyph map, advances, kerning, and line height), which is the same value the renderer lays out with. The metrics come as a blob the renderer keeps for the font's life (ADR-0042), since they are far larger than a piece of mail.
  - The screen then measures every string itself, in the same turn, with the same layout code, so a measured size is exactly the drawn size.
  - This is one round trip per font, never per string.
  - A font is made from a source (a built-in, or a font file's bytes) and names its fallbacks when it is made, in place of one renderer-wide list. So a `FontId`'s metrics never change: a screen keeps them for good, and nothing is ever sent to update them.
  - A text's outline draws outside its measured box, so `Text` pads its layout size by the outline's width.
  - A text's measured width is its widest line, so `Text` makes the text wrap at that width and it fills exactly its box (ADR-0052). A word too long to break measures wider than the width offered. `Text` then draws at the offered width, and its box cuts the word.
- **A child actor learns a new size a turn after its parent decides it.** The clip contains the mismatch for that turn, and moves cost nothing. Content whose size changes every frame, such as a window being resized by dragging, belongs in widgets in one actor.
- **The dev overlay moves onto the library,** as its first user.
- **A text input does not show what an input method is still composing.** The window does not send a composition until it is committed (ADR-0051: `Preedit` is follow-on work there); once it does, the text input shows it at its caret, as its own state.
- **Follow-on work, each waiting for a need:**
  - drags between actors that light up targets in other actors;
  - popups above every screen, as a window-wide overlay actor;
  - focus moved by direction;
  - themes switched while running, with sheet values as named tokens that a theme actor mails;
  - dropdowns, rich text, trees, and colour pickers;
  - animation and transitions.

## Alternatives considered

- **A Rust builder API or an `rsx!`/`view!` macro:** structure is buried in calls and closures, and is hard to read. Bevy's UI and Unreal's Slate show the cost.
- **Elm-style message enums, or immediate-mode closures as in egui:** logic mixes with structure, and a screen reads as control flow.
- **Templates with expressions, as in Vue, Svelte, and RmlUi:** the view grows logic. Here it holds only paths and calls.
- **KDL, YAML, RON, or Pug instead of XML:** less familiar to agents and readers, with no gain over XML for a tree of typed elements.
- **Markup loaded and resolved at runtime, as in Panorama, Unity, and XAML:** a typo becomes a blank label in a running game rather than a build error.
- **Markup inside the `.rs` file as a macro:** rustc could point errors at the exact markup, but the markup would sit inside Rust, and editors and agents would not treat it as XML.
- **A visual designer saving binary assets, as in Unreal's UMG:** the assets do not diff or merge, and agents cannot write them.
- **`<each>` and `<if>` elements:** they would be the only elements that are not widgets, and a scroll area could not tell which of its children were rows.
- **Inline styles, style blocks in screens, a CSS cascade, or a theme actor:** each gives a widget's look a second source, or does work at runtime that the build can do.
- **Composites with their own state:** changing it would need logic in markup.
- **An actor for each widget:** a mailbox, a turn, and a copy of its data for every row, and a click spread across several turns.
- **Child actors placed only in absolute window pixels:** every move would cascade down the subtree as mail, a turn per level, so children would trail their parents. Each call states its mode instead (ADR-0055).
- **Drag payloads carried by the window:** the window would need to know every payload kind. Reporting what is under the pointer is enough, and the actors use mail.
- **Writing our own flex and grid layout:** Taffy already does it, and is used by Bevy, Zed, Dioxus, and Lapce's UI library.
- **Marking changes by hand,** a call that tells the library a screen changed: one forgotten call leaves a screen silently stale.
- **A hook in the actor framework after every batch of a screen's turns:** nothing may be added to the framework, and a shared stage between `Tick` and `Render` gives the same point in the frame to every actor that draws.
