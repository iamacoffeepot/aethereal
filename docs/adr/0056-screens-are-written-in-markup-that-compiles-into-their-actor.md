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
        <Tooltip>
            <ItemSlot item="{item}" click="{self.select(item.id)}" drag="{item.id}" drop="{self.place(item.slot)}"/>
            <Text>{item.name}</Text>
        </Tooltip>
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
- **An item's node is told apart by its key, not its place** (*settled by the library's scroll stage*): the segment of its key path is `Segment::Item(ItemKey)` where every other node's is `Segment::Place(n)`. An `ItemKey` is a number: the key's type converts with `From`, which the library gives every integer and a program writes for its own ids, so a key that is not a number is a build error. Focus on an item, or within one, clears when the item leaves, as for any key that goes.
- **Two items of one list with one key are refused, never merged** (*settled by the library's scroll stage*): the first keeps the key, each later one is left out of the tree, with its descendants, and logged at each layout. The rest of the screen still shows, since the fault is in the game's data, not the screen's markup. Two siblings at one place in the markup, which only a tree built by hand can hold, still refuse the whole tree.

**What `items` compiles into** (*settled by the library's scroll stage*). The tree compares by value, so `items` is not a closure the tree holds: the view builds the items' nodes itself, each at its key.

```rust
// <Grid columns="6" items="{self.items}" as="item" key="{item.id}"><ItemSlot item="{item}"/></Grid>
let slots = self.items.iter().map(|item| Node::new(Segment::Item(ItemKey::from(item.id)), ItemSlot { item: item.clone() }, slot_style(), vec![]));
Node::new(Segment::Place(1), Grid, grid_style(), slots.collect())
```

`Column`, `Row`, `Grid`, and any other container take the same shape. A `Scroll` builds only the items it shows: the view asks the screen which, by the scroll's key path, and keeps beside each item built its place in the list.

```rust
// <Scroll items="{self.items}" as="item" key="{item.id}"><Text>{item.name}</Text></Scroll>
let list = KeyPath::of(&[Segment::Place(0), Segment::Place(2)]);   // the scroll's own path, known to the build
let mut rows = self.ui.rows(&list, self.items.len());              // the window, from the table and the last layout
for (at, item) in self.items.iter().enumerate() {
    let key = ItemKey::from(item.id);
    if rows.builds(at, key) {
        rows.push(at, Node::new(Segment::Item(key), Text::new(&item.name), name_style(), vec![]));
    }
}
let (lines, built) = rows.finish();                                // Lines::Window { count, at }, and the rows
Node::new(Segment::Place(2), Scroll::down().list(lines), list_style(), built)
```

- **A scroll's window is read from the table as it stands** (*settled by the library's scroll stage*). `Screen::rows` takes the scroll's offset from its state and the pitch of its lines, where they start and how far apart they are, from its last layout, and answers the lines its content box shows with a margin of two each way, and, wherever it stands, the item that has the focus or holds the widget that has. So a turn of the wheel builds what it shows in the very next view, Tab finds the next row built, and a focused row scrolled out of view keeps its focus and its state. The view stays plain code over the screen's fields, and an unchanged list at an unchanged offset is the same tree, so it is skipped.
- **The lines of a list in a scroll are one length** (*settled by the library's scroll stage*): the distance from the first row built to the second, or with one row, its length and the gap. Each built row is placed at its item's line, and what the scroll holds runs to the list's last line, so the content's length, the thumb, and the end are known without building every row. A list whose rows differ in length still shows and scrolls, but its rows overlap or part, and its window may be measured anew each frame: a list of rows of differing lengths is a column in a scroll, built whole.
- **What it costs.** The view names the screen's library field and the scroll's key path, which the build knows. A scroll never laid out builds its first row to measure, and shows the rest a frame later, as it does after a resize. Each item is visited once per view to ask whether it is built, and the tree holds only those built.

### Events call the screen's methods

```rust
fn select(&mut self, turn: &mut Turn<'_, Self, InventorySends>, item: ItemId) -> Result<(), Fault> { … }
fn rename(&mut self, turn: &mut Turn<'_, Self, InventorySends>, item: ItemId, text: String) -> Result<(), Fault> { … }
```

- **An event names the method and its arguments:** `click="{self.select(item.id)}"`. The turn is passed implicitly, as Rust passes `self`.
- **An event's arguments are copied when the view is built, and must be `Copy`.** That steers them to ids, never to a snapshot of an item that may have changed between the draw and the click.
- **A widget that hands a value with its event hands it after the arguments:** a text input's `change` and `submit` the text, a toggle's `change` whether it is on, a slider's `change` its value. `change="{self.rename(item.id)}"` calls `rename(turn, item, text)`.
- **The build checks the call against the method's signature.**
- **A method an event calls takes the turn of the screen's input handlers, `Turn<'_, Self, S>`, with `S` what the screen's methods may send** (*settled by the library's input stage*). A turn names what its handler sends, so there is no `Turn<'_, Self>`; the screen declares `S` once, beside its methods, and the library adds its own ask of the window's input method (below), and the drop a release mails to another actor and its refusal of one (*added by the library's drag stage*: Drag and drop is one path).

**What the markup compiles into** (*settled by the library's input stage*). The tree compares by value, so an event is a value, never a closure: each event attribute of a screen's markup becomes a variant of one enum, holding the arguments the view copied, and the screen's `on` matches each variant to the method it names. A widget raises its event with the value it hands, as one of `Raised::{Event(E), Text(E, String), Flag(E, bool), Number(E, f32), Index(E, u32), Dropped(E, Payload)}`, `Index` a choice counted from zero, as tabs' (*added by the library's scroll stage*), and `Dropped` the payload a drag carried, as a drop's and a `drop_outside`'s (*added by the library's drag stage*).

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

aethereal_ui::screen!(Inventory, ui);   // the handlers of `Prepare`, `Pointed`, `Keyed`, the library's answers, and its sleeps
```

```rust
// inventory.rs, by hand
type InventorySends = aethereal_ui::sends![Tells<Path<Game>, Equip>];   // the library's sends, then the screen's own

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
- **A widget whose style fills its box blocks the pointer there,** with the default cursor, so a panel is solid to the game beneath and a click on its empty space walks no character. A widget within one that blocks the pointer lists no hit of its own unless it says so, so a button's filled label does not take the press from the button (*within one that blocks, not one the pointer reaches, settled by the library's popup stage*: a tooltip's hit passes the pointer on, and the panel it holds stays solid).
- **The wheel is not a widget's** (*settled by the library's scroll stage*): it slides the innermost scroll at or around the widget the pointer reaches that can still move that way along its axis, and one at its end lets it pass outward, so a list in a panel scrolls before the panel does.
- **A click is a press and a release of the main button on the same widget:** the release's `over` (ADR-0055) names the hit it was over, so a press dragged off and released elsewhere clicks nothing. Moves and the release reach a widget wherever the pointer goes while its press is held, as the window's capture sends them, so a slider dragged past its end still sets its end.
- **Captured input goes to the widget the press reached, kept by the screen** (*settled by the library's drag stage*): beside its focus, the screen keeps what each entry a press reached was until every button on it is up, so a move or release the capture sends through it reaches that widget though a widget before it began or stopped drawing and renumbered it, and reaches nothing once that widget's key has left the tree.

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
- **Tab and Shift-Tab move focus through focusable widgets in markup order,** which is the tree's order, round from the last to the first. There is no tab index; reorder the markup to change the order. Each scroll around the widget Tab reaches slides as little as it can to show it (*settled by the library's scroll stage*).
- **A key goes first to the focused widget, then to each widget it is within that hears the key, innermost first, then to the focus keys, Tab and Esc, and a key none uses goes to the screen's `fn key(&mut self, turn, key: Key)`** (*the widgets around the focus settled by the library's scroll stage*, so a scroll hears Page Up and Page Down around a focused row; the screen declares their keys beside the focused widget's). Hotkeys are an ordinary `match` there, and the screen declares the set they use beside the method, as `Controls::HOTKEYS` in the same impl (*settled by the library's input stage*). A `Key` carries its `code`, its `key`, whether it repeats, and the modifiers held, so a chord is read there too.
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

*Settled by the library's popup stage:*

```xml
<Popup side="below" inside="closes">
    <Row class="field"><Text>{self.sort_name()}</Text></Row>   <!-- its anchor: what it opens from -->
    <Column class="menu">…</Column>                              <!-- its content, in the top layer while it is open -->
</Popup>

<Tooltip>
    <ItemSlot item="{item}"/>                                    <!-- its anchor -->
    <Text class="tip">{item.name}</Text>                         <!-- its content, once the pointer has rested -->
</Tooltip>
```

- **A popup's first child is its anchor, and the rest is its content.** The anchor, the child at the first place in the markup, is laid out in the popup's box as a `Stack`'s child is, and the popup is the widget that holds it: what its content is placed against. The content is laid out and drawn only while the popup is open, each piece as a root of its own, as wide and tall as it asks within the screen, out of the flow of every box around it. Content of more than one widget lies at one place, as a `Stack`'s children do, so content is one container.
- **The content is placed on the popup's `side`: `below`, the default, `above`, `right`, or `left`.** Below or above, it starts at the anchor's left edge; right or left, at its top edge. Where it does not fit on its side and the other side has more room, it flips; then it shifts as little as it can to stay within the screen, and one longer than the screen starts at its start. It is placed where the anchor stands once every scroll around the anchor has slid it, so it moves with the scroll, and it is cut by nothing but the screen.
- **Each open popup's content is a layer of its own,** listed after everything else of its screen in markup order, so a popup within another's content draws over it. A popup's content is a surface of its own: the wheel, the keys the widgets around the focus hear, and the scrolls Tab slides to show the focus stop at it, so a popup's content never slides the scroll its anchor stands in.
- **Whether a popup is open is its own state, and it is the button of its anchor.** Its tint, focus ring, and hit are listed over its anchor's, so a click on its anchor opens it and a second shuts it, as Enter and Space do while it has focus, and an anchor that is itself pressed, a button, is its label. So no screen method opens one, and a dropdown is a composite.
- **Its screen shuts it** when a press it hears lands outside it, on anything it lists a hit for, as a press clears focus; when Escape is pressed and it is the topmost open popup, focus within its content going back to it, which the screen declares Escape for while a popup is open; when its anchor goes from the tree, after which it stays shut; and, with `inside="closes"`, when a click within its content raises an event, after the event is raised. A click that raises nothing, on a nested popup's anchor or on empty space, leaves it open. A press on the game beneath, which the screen does not hear, leaves it open: a popup that must shut on any click is modal.
- **A modal popup's backdrop is a hit over the whole screen, blocking the pointer with the default cursor,** listed in its layer before its content: a press on it shuts the popup and every popup within it. While a modal popup is open, the screen declares Tab and Escape, and Tab moves only through the focusable widgets of the topmost modal popup's content.
- **A tooltip is placed as a popup is,** below by default, and opens once the pointer has rested on its anchor for its `delay`, half a second unless it says otherwise. It shuts as the pointer leaves its anchor, and a press while the pointer is on it shuts it until the pointer leaves and comes back.
- **A tooltip never takes focus and never blocks the pointer.** Its own hit, under its anchor's, passes the pointer on with the default cursor, so an anchor that lists no hit of its own is still known to be under the pointer, and a press through it leaves focus where it was. Its content lists no hit, and Tab passes it.
- **A tooltip's wait is a `Sleep` job, matched to its hover by number.** The library keeps the hits the pointer has come onto and not left. At each `Prepare` after one changed, a tooltip whose anchor, or a hit within it, the pointer has come onto takes the next number of its screen's hovers, which never repeats, keeps it in its state as the hover it waits for, and the library runs `Sleep` until the step's time plus its delay, with the tooltip's path and that number as its context. A tooltip the pointer has left goes back to waiting for nothing. When a sleep ends, the tooltip at its path opens only if it still waits for that number, so a hover that ended before its sleep, and a stale sleep ending during a later hover, open nothing. A stale sleep is not cancelled: its id cannot be kept in a widget's plain state, and a sleep costs its host one timer. The hovers are looked at in `Prepare` rather than on each pointer event, so the pointer passing from one hit to another within an anchor, which the window tells as a leave and a come, does not start the wait again.
- **The sleep costs a screen's sends nothing.** A job is mail to no actor, so no send lists it (ADR-0018), and `Orders::send` runs it from the turn that decided it, a `Prepare`. It builds only for an actor that handles its answer, `Answer<Slept, Tipping>`, which `handles!` adds and `screen!` writes the handler of.
- **A popup is known to the library by how it arranges its children, as a scroll is.** A primitive that arranges them as `Arrange::Popup(layer)` claims a layer of its screen, its `Layer` naming the side its content is placed on and what opens it, and the library opens and shuts it as its screen's input says through `Widget`'s methods alone, as it slides a scroll for the wheel. One opened by its own input, `Opens::Pressed { modal, inside }` as `Popup` is, says whether it is open (`open`) and is shut by the library (`shut`). One opened by the pointer resting on its anchor, `Opens::Rested { delay }` as `Tooltip` is, keeps a `Tip` in its state, which the library reads (`tip`) and keeps (`tip_to`) as the pointer comes onto its anchor, rests, leaves, and presses, the hover number in `Tip::Waiting` its screen's to compare. Each method has a default, so a widget that claims no layer says nothing of them, and a program's own primitive claims a layer as the stock ones do.

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

*Settled by the library's child stage:*

```rust
// minimap_hud.rs, by hand, in `begin`: the screen spawns its child and keeps what places it, a field `minimap: Called`
let spawned = turn.spawn::<Minimap>(&(), config)?;
let minimap = Called::new(child::<Minimap>(()), spawned);

// minimap_hud_view.rs, generated from <child actor="{self.minimap}" area="inherit" class="minimap"/>
Node::call(Segment::Place(2), self.minimap.clone(), CallArea::Inherit, minimap_style())
```

- **A child a screen places is a `Called`:** where the renderer finds it beneath the screen, a call's `Callee`, and the reference its size is mailed through, a `ProtocolRef<(Resize,)>`. `Called::new(child::<C>(key), reference)` builds only for a type that may stand beneath the screen's and takes `Resize`, from the reference its spawn answered. It is a value compared by value, so the view copies it into its slot at each build, and a tree whose children stand as they stood is still skipped.
- **The slot is a leaf that draws nothing of its own,** sized by its style alone. Its style's background and border draw beneath the child, and it is reached by the pointer only where its style fills its box, as a panel is. The library lists the call after the slot's own draws and hit, at the slot's place in the tree's order, placed at the slot's content box, so a slot's border and padding frame its child, and cut to that box within what the slot is cut to. So a child in a scroll slides with it and is cut to it, and one in a popup's content is called in that popup's layer of the top layer while the popup is open. A slot hidden, by `show` or a shut popup, is not called, and nothing is sent to its child.
- **`area="inherit"` is `Area::Inherit` at the content box's corner,** in the screen's coordinates. **`area="absolute"` is `Area::Absolute`,** the slot's box taken to be in the window's pixels, which holds for a screen that is a root or is itself called in the window's pixels. An absolute child is told its size and never where its slot is, so it is for a child that draws where it is by the window's pixels, as a scene does; a child that is a screen lays out from its own corner, and is called `inherit`.
- **The size is `Resize { viewport }`,** a plain kind, as it carries no reference: the slot's content box's size in pixels, in the screen's frame and at its UI scale. The `Prepare` that lays a slot out mails it when it is not what that child was told by the layout before, so a move sends nothing, and a resize, or a change of the screen's frame or scale, sends one, once. A child shown again after it was left out is told again, as it may have missed a change while it was hidden.
- **A child that is a screen lays out to it, as a root screen lays out to its window's size:** `handles!` lists `Resize`, and `screen!` writes its handler, which hands the viewport to `Screen::resize`, so the screen lays out at its next `Prepare`. Every screen takes it, so a screen can be placed by another as it is; a root is never sent one. A child that is not a screen handles `Resize` itself.
- **What it costs:** every screen handles one kind more, `Resize`, whose handler sends the library's sends. Those, which `Prepare` and the handlers of the library's answers declare, gain one send place, `ToAny<(Resize,), Resize>`, which reaches a child of any type through the reference its `Called` holds; so a screen's own sends, its `Controls::Sends`, gain nothing, and a child of a new type adds nothing to either.
- **Each screen keeps its own focus, and Tab does not cross into a child or out of one.** Tab and Shift-Tab move through the screen's own tree, which holds the slot and not the child's widgets, and the slot cannot hold focus. A press into the child leaves its parent as a press elsewhere in the parent does: the library lists a hit over each shown child, after its call and over what the child is cut to, whose pointer is `Presses` (ADR-0055), so it hears the press alone, passes it on to the child, declares no cursor, and takes no hover, move, wheel, release, or capture. On that press the parent's focus clears, stopping its input method, and each open popup it lands outside of shuts; nothing is raised, and the child's own focus is the child's. A child in a popup's content has its hit in the top layer with its call, and a hidden child lists none. Keys reach declarations top-down, and a screen lists its keys before its widgets, so a child, called after its parent declared its keys, hears a key first: while both hold focus, the child's focused widget takes the keys it hears, and the parent's, a focused text input among them, the keys the child passes.

### Drag and drop is one path

```xml
<ItemSlot item="{item}" drag="{item.id}" drop="{self.place(item.slot)}" drop_outside="{self.discard()}"/>
```

```rust
fn place(&mut self, turn: &mut Turn<'_, Self, InventorySends>, to: SlotId, dragged: ItemId) -> Result<(), Fault> { … }
fn discard(&mut self, turn: &mut Turn<'_, Self, InventorySends>, dragged: ItemId) -> Result<(), Fault> { … }
```

1. **A press on a widget with `drag`, then a move of a few pixels, starts a drag.** The payload is copied when the view is built, as an event's arguments are. The held button keeps every move at the source.
2. **The dragged item is the pointer.** The source declares the item's image as the cursor of the entry the press reached, once its drag begins (ADR-0055), and the window shows the press's cursor wherever the pointer goes while the button is held, so it is always on top and never lags.
3. **On release, the window reports what is under the pointer:** the topmost actor, the entry of its list, and the position in that entry's coordinates.
4. **If that is the source's own actor,** the library finds the widget at the position and calls its `drop` method, with the payload appended as the last argument.
5. **If it is another actor,** the source casts it to the protocol that accepts the payload's kind and mails it the drop: the payload, the entry, and the position. That actor's library finds the widget and calls its `drop` method the same way.
6. **If nothing under the pointer accepts the payload,** the source raises `drop_outside`. Dropping an item on the ground is the game's decision, made in that method.

- **The build checks that the payload's type is the `drop` method's last parameter.**
- **A target in another actor does not light up while a drag passes over it,** because it hears nothing until the drop. Targets in the source's own screen can.

*Settled by the library's drag stage:*

**What the markup compiles into.** `drag` and `drop` are settings of the element's node, as `items` is, so any widget, a composite's root among them, can be dragged or dropped on; they compare by value with the rest of the tree.

```rust
// <ItemSlot item="{item}" drag="{item.id}" drop="{self.place(item.slot)}" drop_outside="{self.discard()}"/>
Node::new(Segment::Item(ItemKey::from(item.id)), ItemSlot { item: item.clone() }, slot_style(), vec![])
    .with_drag(DragSource::new(item.id).outside(InventoryEvent::Discard))
    .with_drop(DropTarget::new(InventoryEvent::Place(item.slot), Self::place))

// in `on`: the payload comes after the arguments the view copied, read as the kind the method takes
Raised::Dropped(InventoryEvent::Place(slot), payload) => aethereal_ui::dropped(turn, &payload, |turn, item| self.place(turn, slot, item)),
Raised::Dropped(InventoryEvent::Discard, payload) => aethereal_ui::dropped(turn, &payload, |turn, item| self.discard(turn, item)),
```

- **A payload is a kind, `Copy` and compared by value,** an id as an event's arguments are, and is held erased: its kind's id and its bytes (`Payload`). It crosses actors, so it is a kind, and it is erased so that one kind of mail carries every program's payloads, one handler takes them, and one send place mails them. A drop kind generic over its payload would be a kind for each payload, which every screen would have to list as a handler and a send place, and the library cannot know them.
- **A target takes the kind its method takes last.** `DropTarget::new(event, Self::place)` names the method its drop calls, and the kind is that method's last parameter's, read by the compiler from its signature (`DropMethod`); nothing calls it there. So the build's check is rustc's: the markup's compiler writes the method's path, and a payload of any other kind is refused at runtime, by its kind's id.
- **The payload is copied from the tree the press reached,** and kept with the drag: a view rebuilt during the drag, its list changed under it, still drops what was pressed.
- **A drag begins past `DRAG_THRESHOLD`, 4 logical pixels** at the screen's UI scale, from where the main button went down on the source, measured on the moves its capture sends through the source's own hit. A press released within it is still a click; one dragged past it is no click, wherever it is released, its release taken as the drop.
- **The drag's cursor is the source's image, or a closed hand.** `DragSource::image` gives a `CursorImage` the program made, fixed when the view builds the source; without one it is `Cursor::Grabbing`. From the first `Prepare` after the drag begins until its release, the hit the press reached declares that cursor in place of its own (ADR-0055), so the window shows it wherever the pointer goes while the button is held, and the source's own cursor before and after.
- **A widget that drags or takes drops is reached by the pointer, though it says nothing of it:** a source that says nothing is listed with a hit that blocks with an open hand, so a press reaches it, and a target with a hit that passes the pointer on with the default cursor, so a release is over it and what is beneath still hears the pointer. A widget that says how the pointer reaches it keeps what it says.
- **A drop on the source's own screen lands on the innermost target that takes its kind,** at or around the widget whose hit the release was over, on that widget's surface as the wheel's scroll is found, so a panel that takes items takes one dropped on a button inside it. The target raises its `drop` with the payload.
- **A drop on another actor is mail, `Dropped`, to the protocol `Drops = (Dropped,)`.** The source casts the actor its release was over to `Drops`, and mails it the payload, as its kind's id and its bytes as a blob (ADR-0042, as bytes between actors are), the entry, the release's place in that entry's coordinates, and the source's key path. A cast reaches a tuple of kinds, never a named protocol, so the drop is a tell, not an ask. A screen's library takes it in as its own drop: the innermost target around the entry's widget that takes the kind raises its `drop`. An actor that is not a screen handles `Dropped` itself.
- **The target replies only to refuse.** Where nothing at the entry takes the payload, it replies `DropRefused` with the payload and the source's path handed back, and the source raises its `drop_outside` with the payload, as the view then builds it. A drop taken is answered with nothing, so an actor that takes every drop it is handed writes no reply. A cast that fails, the actor taking no drops, raises `drop_outside` at once.
- **A target in the source's screen lights up:** while a drag is under way, the target it would land on, found as the drop finds it from the topmost hit the pointer reaches, is drawn with `Draws::drop_over` true, as `Draws::focused` tells focus; the stock widgets show their focus ring then. The pointer coming onto or leaving a hit during a drag lays the screen out at the next `Prepare`.
- **What it costs.** A screen handles two kinds more, `Dropped` and `DropRefused`, whose handlers call its methods and so send its `Controls::Sends`; those gain two places from `sends!`, `ToAny<Drops, Dropped>` for the drop a release mails and `ToSender<DropRefused>` for the refusal, so a screen's own sends are at most 21. The library's own sends, those of `Prepare` and its answers, gain nothing. A drop on another actor costs a blob for its payload, which a refusal hands back unchanged.

### Stock widgets

| Kind | Widgets |
|---|---|
| Layout | `Row`, `Column`, `Grid`, `Stack` (children layered on one spot) |
| Content | `Text`, `Image` (a texture or a region of one, with nine-slice), `Bar` (a filled fraction) |
| Interaction | `Button`, `Scroll` (clips; builds only visible rows when given `items`), `TextInput` (`change`, `submit`), `Toggle`, `Slider`, `Tabs`, `Popup`, `Tooltip` |

**`Scroll` and `Tabs`** (*settled by the library's scroll stage*):

- **A primitive that scrolls arranges its children as `Arrange::Scroll(axis)`:** one after another along the axis, never shrunk to fit, cut to its content box, and slid by its offset. Its offset is its state; it says how far it has slid (`offset`) and is slid (`scroll_to`) by the library, which clamps it to what it can scroll as it lays out, so it never stays past the end when what it holds shrinks. A widget may keep `room` inside its border for what it draws of its own, and its `Laid` says how far its children reach, `extent`, the items it did not build counted.
- **`Scroll`** keeps room along its far side for its bar. A press on the thumb drags it under capture, held where it was grabbed; a press elsewhere on the bar jumps the thumb's middle there. It can hold focus, and Page Up, Page Down, Home, and End slide it while it or a widget within it has focus.
- **`Tabs` takes the open tab as a param and raises `change` with the tab chosen, counted from zero, as `Raised::Index`:** which tab is open is game state, so it never opens one itself. It is one stop of the focus, its ring on the open header, and the arrows choose the tab beside the open one, Home and End the first and the last. Its headers are its own drawing, in its room above its panel.

```xml
<Tabs selected="{self.tab}" change="{self.open}">
    <Tab title="Items">…</Tab>
    <Tab title="Skills">…</Tab>
</Tabs>
```

```rust
// Tabs { titles, selected, change } over the open tab's panel alone, at that tab's place, so each panel keeps its keys
let panels = match self.tab { 0 => vec![items_panel], 1 => vec![skills_panel], _ => Vec::new() };
Node::new(Segment::Place(3), Tabs::new(&["Items", "Skills"], self.tab, InventoryEvent::Open), tabs_style(), panels)
```

Dropdowns, rich text, trees, and colour pickers are left out. A dropdown can later be a composite of `Popup` and `Button`: a `Popup` with `inside="closes"`, its anchor what shows the choice and its content a `Button` for each option, raising the composite's `change`. Opening and shutting it are the popup's own, so it needs no method of the screen's (*settled by the library's popup stage*).

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
- **A child actor learns a new size a turn after its parent decides it, and lays out to it at its own next `Prepare`, a frame later:** the `Prepare` that lays its slot out mails it (*settled by the library's child stage*). The clip contains the mismatch for that frame, and moves cost nothing. Content whose size changes every frame, such as a window being resized by dragging, belongs in widgets in one actor.
- **The dev overlay moves onto the library,** as its first user.
- **A text input does not show what an input method is still composing.** The window does not send a composition until it is committed (ADR-0051: `Preedit` is follow-on work there); once it does, the text input shows it at its caret, as its own state.
- **Follow-on work, each waiting for a need:**
  - drags between actors that light up targets in other actors;
  - popups above every screen, as a window-wide overlay actor;
  - focus moved by direction;
  - focus that crosses into a child, so Tab walks from a screen into its child and out again (a press into a child leaving its parent is settled by the library's child stage);
  - a child screen placed in the window's pixels that is told where its slot is, and so moves by mail;
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
- **A drop kind generic over its payload,** `Dropped<ItemId>`: a kind, a handler, and a send place for every payload kind a program drags, which each screen would list, though the library cannot know them. One erased kind serves them all, and the target compares kind ids (*settled by the library's drag stage*).
- **A drop asked of its target, and answered taken or refused:** a cast reaches a tuple of kinds, not a named protocol, and only a named protocol can be asked, so a drop to an actor the source knows only from `over` is a tell; the target replies only to refuse, with what the source needs handed back, and an actor that takes every drop writes no reply (*settled by the library's drag stage*).
- **The source assuming every drop on another actor is taken:** an item dropped on a panel that takes only spells would vanish, never put back nor dropped on the ground (*settled by the library's drag stage*).
- **Writing our own flex and grid layout:** Taffy already does it, and is used by Bevy, Zed, Dioxus, and Lapce's UI library.
- **Marking changes by hand,** a call that tells the library a screen changed: one forgotten call leaves a screen silently stale.
- **A hook in the actor framework after every batch of a screen's turns:** nothing may be added to the framework, and a shared stage between `Tick` and `Render` gives the same point in the frame to every actor that draws.
