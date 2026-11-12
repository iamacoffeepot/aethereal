# ADR-0051: Typed text, and the input method a program allows

- **Status:** Accepted
- **Date:** 2026-11-09

## Context

ichor's console, and any text field a program draws, needs the text a person typed. A window sends only keys (ADR-0024: `Event::KeyDown { code, key, repeat }`, `Event::KeyUp`), so a text field today rebuilds text from key names. That breaks in four ways:

- **Space:** the desktop named the space bar's `key` `"Space"`, and a console could not type one. Fixed on its own, so the key is `" "` as a page names it (#414), but the next three are not.
- **Dead keys:** on a layout with dead keys, `´` then `e` is two keys whose `key`s are `"Dead"` and `"e"`. The `é` they type is in no key event.
- **Layouts and modifiers:** what a key types depends on the layout, Shift, AltGr and Caps Lock. A program reading `key` must work out text the windowing system has already worked out.
- **Input methods:** Japanese, Chinese and Korean text is composed by an input method (IME) over several keys and committed as a whole string. No key carries it.

What is already decided:

- **What a window hears comes from its chassis, through the window root's one source,** to the window's actor, which sends it to the actors subscribed (ADR-0024: `WindowFeed::push`, `Heard::Event`). Keys are named as the W3C UI Events name them.
- **Input that changes no state either actor keeps can be performed by a tool** (ADR-0046: `Input`, `Perform`, `InputTool`, `send_input`), as though the chassis had heard it.
- **The window root asks its chassis to change a window through `Windowing` requests,** answered through `Heard`, and refuses one a chassis cannot make (ADR-0027: `Windowing::SetMode`, `Unset::{NotOpen, NoHost}`; ADR-0047: `Windowing::Resize`).
- **winit gives the text a key typed,** after the layout, the modifiers but Ctrl, and dead keys, as `KeyEvent.text`. It gives an input method's text as `WindowEvent::Ime`: `Enabled`, `Preedit(text, cursor)` while composing, `Commit(text)` once committed, and `Disabled`. A window receives `Ime` events only once the program allows them (`Window::set_ime_allowed`), which is off by default.
- **An input method that is on can take plain keys.** While it composes, keys such as W, A, S and D go into the composition, not to the program as keys. Games therefore keep the IME off and turn it on only while a text box has focus: SDL with `SDL_StartTextInput` and `SDL_StopTextInput`, Unity with `imeCompositionMode = Auto`, Unreal by following the focused text widget. Movement stays bound to key positions (`code`), which an IME that is off never touches.
- **The old aether lab sent text the same way, and learned three things** (its `aether.text_input`): a keystroke's text and a commit must not both be sent for one character, so key text is dropped while a composition is open; the text of Backspace, Enter, Tab, Escape and Delete is a control character, and those keys must arrive only as keys; and a dead key's text can arrive with the character it resolves to.

## Decision

**A window sends its subscribers the text typed in it, as a new event, `Text`, after the key that typed it. Its input method starts off. A program turns it on for a window while a text field there has focus, and off again after, by asking the window root. `send_input` can perform `Text`, as it performs keys.**

Under ADR-0055 a window's keys and its `Text` go to the actors whose draw lists declare the keys, not to its subscribers: the text goes where the `KeyDown` before it went, as a `Keyed` beside its key.

```
key pressed ── KeyDown { code, key, repeat } ──> subscribers
            └─ Text { text } ──────────────────> subscribers   (when the key typed text)

console opens ── AllowIme { window, allowed: true } ──> window root ── Windowing::AllowIme ──> chassis
  IME composes, then commits "日本" ── Text { text: "日本" } ──> subscribers
console closes ── AllowIme { window, allowed: false } ──> ...       W is KeyDown again, never composed
```

### `Text`

```rust
pub enum Event {
    // ...
    /// Text typed while the window had the keyboard: the characters a key typed under the layout, its modifiers and
    /// any dead key before it, or the string an input method committed. Sent after the `KeyDown` that typed it, and
    /// again for each repeat. Never a control character, never empty.
    Text { text: String },
}
```

- **It is an `Event`, not a kind of its own,** so a subscriber to a window hears it in order with the keys, through the subscription it already has.
- **A key that types text sends `KeyDown`, then `Text`.** A program that binds keys reads `KeyDown`'s `code`; a text field reads `Text` and ignores keys except the ones that edit (Backspace, Enter, the arrows). Neither has to know the layout.
- **Control characters are never sent as text.** Backspace, Enter, Tab, Escape and Delete arrive only as keys.
- **A key pressed with Ctrl or Meta held types no text,** so a chord such as Ctrl+C is only keys, and a console never inserts the `c` of a copy. Alt and AltGr are not counted: they type characters on many layouts (`é` with Option on a Mac, `@` with AltGr on a German keyboard).
- **A held key repeats its text,** as it repeats `KeyDown`, so holding a key types a run of it, as a text field expects.

### The desktop

- **On each pressed key, the chassis pushes `KeyDown`, then `Text` of `KeyEvent.text` with its control characters removed,** unless that leaves nothing, or Ctrl or Super is held. winit's text counts every modifier but Ctrl (Ctrl+C types `"c"`), so the chassis keeps the modifiers winit last reported (`WindowEvent::ModifiersChanged`) and checks them itself.
- **While a composition is open, a key's text is not sent.** A composition is open from a `Preedit` with text until a `Commit`, a `Preedit` with no text, or `Disabled`. The composed string is sent once, as `Text` of the `Commit`. So no character is sent twice.
- **`Preedit` is not sent yet.** A program does not show the uncommitted text until then, and the input method's own candidate window shows it. Showing it inline is follow-on work (below).

### The browser

- **On each `keydown` not composing (`isComposing` is false), with neither Ctrl nor Meta held, whose `key` is one character, the chassis sends `KeyDown`, then `Text` of that `key`.** A named key's `key` is a word (`"Enter"`, `"ArrowLeft"`, `"Dead"`), so one character is the text typed.
- **A canvas has no input method,** so the browser refuses `AllowIme` with `NoHost` until a chassis keeps an editable element for it (follow-on work).

### `AllowIme`

```rust
/// Asks the window root to let `window`'s input method compose, or to stop it: allow it while a text field there has
/// focus, and stop it after, so the keys a program binds stay keys.
#[kind(name = "aether.windows.allow_ime")]
pub struct AllowIme { pub window: WindowId, pub allowed: bool }

#[kind(name = "aether.windows.ime_allowed")]
pub enum ImeAllowed { Allowed, Refused(Unset) }
```

- **The root holds it and asks the chassis through `Windowing::AllowIme { window, allowed }`,** answered through `Heard`, as `SetMode` is (ADR-0027). A window not open is refused `NotOpen`.
- **The desktop calls `set_ime_allowed(allowed)`.** Stopping an input method mid-composition drops the composition without a commit, as winit's `Disabled` says.
- **Every window opens with its input method off.** A program that never asks gets keys and plain `Text` and never a composition, whatever input method the person has on.
- **The root keeps no record of it,** as it keeps none of the input it passes on. A program that allows it is the one that stops it.

### `send_input`

- **`Input` gains `Text { text }`,** performed as `Event::Text` (ADR-0046: `Input`, `Perform`). Its text is at most 256 bytes, refused `Unperformed::TextTooLong { at, most }` past that, so one input is always read within an engine's default `read_limit`, and it is refused `Unperformed::NotText { at }` when it is empty or holds a control character, which no chassis sends.
- **A performed `KeyDown` sends no `Text` with it.** The tool performs what it is given, so an agent typing into a console sends both, as a person's key does.

## Consequences

- **A console or text field reads typed text directly,** with spaces, dead keys, every layout and, once allowed, every input method, and keeps no keymap of its own. ichor drops its own key-to-text table.
- **Game keys stay keys.** A person with an input method on still moves with W A S D in a program that allows it only in its text fields.
- **Each program that edits text owes the window an `AllowIme` when its field gains focus and again when it loses it.** One that forgets to stop it leaves the input method able to take game keys until it does. The dev overlay (ichor's console's successor) does this for itself.
- **An agent tests a text field by performing `KeyDown` and `Text` together.** It cannot test an input method, which only a person's keyboard drives.
- **Follow-on work this leaves open:**
  - `Preedit` sent as an event, so a field shows the uncommitted text inline with its cursor;
  - placing the input method's candidate window at the text field (`set_ime_cursor_area`), which shows at the window's corner until then;
  - an input method in the browser, through an editable element the chassis keeps focused while it is allowed.

## Alternatives considered

- **The input method on for every window from the start.** Nothing to ask, but a person with one on finds movement keys taken into compositions in every program.
- **Only key text now, the input method later.** Smaller, but a Japanese or Chinese player cannot type at all until the later change, and the allowing request would be designed then anyway.
- **Text derived by each program from `key`.** Needs no new event, but every program then rebuilds what the windowing system already knows, and none can see an input method's commit.
- **`Text` as its own kind and subscription.** Separates text from keys, but a subscriber then hears them in two streams whose order it cannot rely on, and a text field needs both.
- **Allowing the input method by a setting a window opens with.** It is fixed for the window's life, and a game needs it on only while a field has focus.
