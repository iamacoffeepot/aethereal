# ADR-0052: Text the renderer lays out and draws

- **Status:** Accepted
- **Date:** 2026-11-09

## Context

No program can draw text. ichor's console uses an 8×8 bitmap font and a pass written by hand. The dev overlay, labels, nameplates, damage numbers, menus, and chat will all need text, so text is used by nearly every program, and it belongs where everything drawn is made.

Drawing text takes five steps:

1. **A font:** each glyph's outline, as curves. A browser has no fonts the engine can read, so the engine carries its own.
2. **Shaping:** a string turned into glyphs and positions, using the font's character map, advances, and kerning; Arabic and Indic scripts also need reordering and joining.
3. **Rasterising:** a glyph's outline turned into pixels.
4. **Caching:** each glyph rasterised once into an atlas on the GPU, and reused.
5. **Drawing:** a quad for each glyph, sampled from the atlas, in a pass.

What is already decided:

- **An actor asks the render root to make what it draws,** and is answered with its id or a refusal saying why (ADR-0021: `CreateMesh`, `CreateTexture`, `CreateProgram`, `Made`, `Refused`). What is drawn is kept on the render root, and only what changes is sent (ADR-0023).
- **A placement draws a mesh in the passes it joins,** with a program and bindings for each join (ADR-0023, ADR-0025: `Place`, `Join`, `Frame::{Main, Texture}`). A placement is drawn only while it is in its maker's draw list, in the list's order (ADR-0031: `DrawList`), and order between actors is declared by the lists that call them (ADR-0055: `Call`).
- **The renderer knows no camera** (ADR-0048). A program's camera keeps the view and projection.
- **A position in a window is in its surface pixels,** from the top left (ADR-0024: `Event`).
- **Heavy work runs in a job, off the render root's turn,** as `capture_frame` encodes its PNG (ADR-0043).

A program that lays out around its text, as a widget library lays out a screen in one turn, needs each text's size without placing it.

## Decision

**The render root makes text, as it makes a mesh: an actor asks for a text, laid out from runs in a font, and places it in the passes of a frame at a point in that frame's pixels. The text is drawn with the renderer's own glyph program, from an atlas of multi-channel signed distance fields (MSDFs) that the renderer fills from TrueType fonts as glyphs are needed. The renderer carries a default proportional font and a default monospace font.**

```
program ── CreateFont { source, fallbacks } ──────────────────────> render root ── Made<FontId>
program ── ReadMetrics { font } ───────────────────────────────────> render root ── FontMetrics { own, fallbacks }: blobs
program: text::measure(own, fallbacks, size, runs, wrap) ────────> Measured { width, height, lines }, in its own turn
program ── CreateText { font, size, runs, wrap, align, outline } ──> render root ── Made<TextId>
program ── PlaceText { text, at, layer, opacity, joins } ─────────> render root ── Made<PlacementId>
program ── DrawList { frame, pass, [panel, line_1, line_2, cursor] } ─> drawn in that order
program ── WriteText { text, runs } ──> laid out again          program ── MoveText { placement, at, opacity } ──> moved, not laid out
```

### A text

```rust
CreateText {
    font: Font,          // Font::Sans or Font::Mono, the renderer's own, or Font::Made(FontId)
    size: f32,           // the em size, in the pixels of the frames it is drawn in
    runs: Vec<Run>,      // the text, a colour for each part
    wrap: Wrap,          // Wrap::None, or Wrap::At { width }: lines broken at spaces to fit width pixels
    align: Align,        // Left, Centre, Right: each line within the widest line, or within the wrap width
    outline: Outline,    // Outline::None, or Outline::Of { width, colour }: a rim around each glyph, width in pixels
} -> Made<TextId> | Refused

struct Run { text: String, colour: Colour }   // linear RGBA, as the renderer's colours are

WriteText { text: TextId, runs: Vec<Run> }     // no answer, as a write is not (ADR-0023)
```

- **A text is what is laid out, not where it is drawn.** It keeps its glyphs, their positions, and its lines, as a mesh keeps its vertices. Where it is drawn is its placement's. One text may be placed more than once.
- **A run is a part of the text with one colour,** so a console line has a dim timestamp, a coloured level, and the message in one text. One font and one size for each text keep the layout simple; mixing them in one text is later work.
- **`\n` breaks a line.** Other control characters are not drawn.
- **A write lays the text out again.** Its placements draw the new text from the first `Render` at which all of the new text's glyphs are in the atlas. Until then they draw the text as it was, so a rewritten line never flickers or shows half its glyphs. A new text's placements draw nothing until all its glyphs are in the atlas, so a new text is answered once it can be drawn: at once when its glyphs are all in the atlas, and otherwise once the last it waits on is written in. An asker that lists a text beside what else it draws then shows them together. A glyph that fits nowhere in the atlas, or has no outline, is not waited on.
- **Refused as it is made:** a size outside 1 to 1024 pixels, an outline wider than the size allows, a font not made or released, and runs over 64 KB. A refused write is logged, and the text keeps what it had.
- **A character no font has never refuses a text.** It draws the font's missing glyph, a box, and the renderer logs each such character once. People type anything, and one emoji must not refuse a chat line.
- **Released as anything made is, by its maker,** with its placements.

### Placing a text

```rust
PlaceText {
    text: TextId,
    at: Point,             // where the first line's top left is, in the pixels of the frame it is drawn in
    layer: Layer,          // Layer::Over: drawn over what the pass drew; Layer::At { depth }: tested at depth, 0 near, 1 far
    opacity: f32,          // 0 to 1, multiplying every run's alpha
    clip: Clip,            // Clip::None, or Clip::To(Rect): nothing drawn outside the box, in the frame's pixels
    joins: Vec<TextJoin>,  // TextJoin { frame: Frame, pass: PassName }: TextJoin::to::<P>(frame)
} -> Made<PlacementId> | Refused

MoveText { placement: PlacementId, at: Point, layer: Layer, opacity: f32, clip: Clip }   // no answer
```

- **A placed text is a placement,** so it goes in its maker's draw list and is ordered with the maker's other placements (ADR-0031). A console lists its panel, its lines, and its cursor in that order. A placed text is drawn only while its maker lists it, as any placement is (ADR-0055). Leaving a placement out of the list stops drawing it, and the text stays made.
- **`at` is in the coordinates its maker's list is drawn in,** which its parent's call names (ADR-0055): a window's main frame's surface pixels, from its top left, or a texture frame's, moved by the list's origin, zero for a list drawn in the window's pixels. The text is cut to the list's clip as well as its own, whose box is in the same coordinates as `at`. Text is placed on the screen only. A program that wants text at a point in its world projects the point with its own camera and moves the placement, giving `Layer::At` the point's depth so that what is nearer hides it.
- **A join names no program and no bindings.** The renderer draws a text with its own glyph program, made for each shape of pass it joins: its target's format, samples, and depth. A join into a pass that reads targets is refused, as the glyph program reads none (ADR-0025: `JoinRefused::Reads`).
- **`clip` cuts what is drawn to a box,** so a console's scrollback, moved a line at a time, never draws past its panel. The glyph program discards what falls outside the box, so placements that clip differently still draw together.
- **`MoveText` does not lay the text out again.** A damage number that rises and fades is one write a frame of a few bytes. Its sender is the placement's maker, as for every write.

### Fonts

```rust
CreateFont {
    source: FontSource,      // FontSource::Sans or FontSource::Mono, the renderer's own, or FontSource::Bytes(Bytes)
    fallbacks: Vec<FontId>,  // fonts made before it, tried in order for a character it lacks
} -> Made<FontId> | Refused(FontNotMade)

enum FontNotMade { Read(FontRefused), Fallback { fallback: u32 }, Unstored }
```

- **A font is made from a source: one of the renderer's own, or the bytes of a TrueType file** (`.ttf`, outlines of quadratic curves), which the program reads with the file capability. The render root reads the font's tables as it is made, in its turn, and refuses a font it cannot read, saying which table (`FontNotMade::Read`). CFF outlines (most `.otf` files), WOFF2, and variable fonts are refused for now.
- **A font made from `Sans` or `Mono` shares the renderer's own:** its parsed font, its metrics' blob, and its glyphs in the atlas, which are keyed by the face they were read from, not by the font's id. Nothing is read, encoded, or generated a second time, so a program makes `Sans` with fallbacks of its own for the cost of the fallbacks.
- **The renderer carries two fonts, `Font::Sans` and `Font::Mono`,** open-licensed (SIL OFL), compiled in, so every program, browser or desktop, draws text with nothing to load. They are IBM Plex Sans and IBM Plex Mono, one family for both, which the owner chose from sheets of five candidates each, drawn by the engine.
- **A font's fallbacks are fixed as it is made, and are one level deep.** A character a text's font lacks is looked for in each of its fallbacks in turn, then drawn as the font's missing glyph. Each fallback's own face is what the font keeps: a fallback's own fallbacks are never tried, so what a font draws depends only on the fonts its maker named, and no chain of fallbacks can loop. The renderer's own two have no fallbacks. Japanese, Chinese, and Korean fonts are many megabytes, so none is carried: a program that needs one makes it and names it as a fallback.
- **A fallback that is not made, or is released, refuses the font,** naming its place in the list (`FontNotMade::Fallback { fallback }`). A fallback released later changes nothing about a font that named it, or about the texts laid out in that font: the font keeps what it needs of each fallback, its parsed face and its metrics' blob, as a text keeps its font after the font is released.
- **A font whose metrics would take the render root past its share of the budget for blobs is refused** (`FontNotMade::Unstored`, ADR-0042).
- **Shaping is simple:** each character is mapped to a glyph by the font's character map, advanced by its advance, with the pairs the font kerns kerned (`kern`, and pair adjustment in `GPOS`). That serves Latin, Greek, Cyrillic, and Han, kana, and Hangul text, which needs no joining. Scripts that join or reorder, such as Arabic and the Indic scripts, are drawn in their characters' plain forms until shaping is extended.
- **Weights and italics are fonts of their own,** each its own file.

### Metrics and measuring

```rust
ReadMetrics { font: Font } -> FontMetrics::Read { own: Bytes, fallbacks: Vec<Bytes> } | FontMetrics::Refused(MetricsRefused::Font)

font::Metrics::decode(bytes: &[u8]) -> Result<Metrics, MetricsMalformed>
text::measure(own: &Metrics, fallbacks: &[Metrics], size: f32, runs: &[Run], wrap: Wrap) -> Measured
struct Measured { width: f32, height: f32, lines: u32 }
```

- **A font's metrics are a value, and layout runs on it.** `font::Metrics` holds the units per em, the line metrics, each glyph's advance, the character map, and the pairs kerned: all that shaping, the missing glyph, and laying a text out read. A read font is its metrics and its outlines; the outlines, and the fields generated from them, stay with the renderer, which needs them only to draw. `lay_out` takes the metrics of a text's font and of its fallbacks, so the render root lays a text out to draw it, and an actor lays it out to measure it, with one function.
- **A font's metrics are answered as a blob the render root keeps.** Mail is decoded within the engine's `read_limit`, 1024 bytes by default, and a font's metrics are tens of kilobytes (IBM Plex Sans) to well over a hundred (a Japanese, Chinese, or Korean font), so they cannot be a piece of mail. When a face is read, the renderer's own two as the render root begins and each other as its font is made, its metrics are encoded once (`Metrics::encode`) and put as a blob (ADR-0042: `turn.put`), kept for as long as any font holds the face. `ReadMetrics` is answered with the font's own blob and each fallback's, in the order they are tried, and every answer for a font hands over the same blobs: nothing is read or encoded again. A font not made, or released, is refused.
- **`Metrics::decode` is pure and bounded, and refuses what is malformed.** It reads exactly what `encode` wrote, every count checked against the bytes left before anything is taken for it, so it holds no more than its bytes describe. Bytes cut short or left over, a character map whose groups are out of order or name a glyph past the font's, and kerning whose pairs or classes are out of order or listed twice are refused, as each would break a lookup. Kerning is held as sorted lists searched by halves, the shape it is written in, so a decoded font kerns exactly as the renderer's does.
- **Measuring a text is a pure function,** `text::measure`, which lays the text out with the same `lay_out` and answers its box, in the pixels of the frames it would be drawn in. Its width is its widest line's, trailing spaces left out, wrapped or not, so a box laid out around it shrinks to its text: a tooltip that wraps at 300 pixels and holds a phrase 120 pixels wide is 120 pixels wide. Its height is its lines times its own font's line height, ascender to descender and the gap after, which its fallbacks do not change. Alignment sets each line within the box and does not change it, and an outline does not change the box either, so `measure` takes neither: a rim draws up to the outline's width outside the measured box.
- **A text is drawn in exactly its measured box** by measuring it with the wrap width it is offered, making it with `Wrap::At { width: measured.width }`, and aligning it within that width. Every line fits the measured width, so its lines break at the same places they were measured at. A word wider than the offered width is left whole and measured wider than the offer, and wrapping at that may set together lines the offer broke apart: such a text does not fit what it was offered.
- **An actor measures in its own turn, with no mail:** it asks a font's metrics once, decodes them, and measures every text it lays out from them.

### The atlas

- **The render root keeps one atlas,** an `Rgba8` texture of MSDF glyphs shared by every text, and grows it as glyphs are added, up to the GPU's longest side. A glyph is keyed by its font and its glyph number, and is generated once at one size, 48 pixels to the em, from which every size is drawn.
- **A glyph is generated in a job** (ADR-0043: a compute job), from its outline, off the render root's turn, and written into the atlas once its job finishes. So no `Render` waits on a glyph.
- **Every size, and every outline, comes from one glyph.** The glyph program reads the distance to the glyph's edge and draws the edge at the text's size, and the outline at a second distance.
- **When the atlas is full, glyphs no text has drawn the longest are dropped,** and generated again when a text needs them.

### Where it lives

- **In `aethereal-render`, in two modules:** `font`, which reads TrueType into metrics and outlines, maps and kerns characters, encodes and decodes metrics, and generates a glyph's MSDF, as pure functions; and `text`, which lays out and measures texts as pure functions, and keeps texts, placements, fonts, and the atlas. Nothing here is behind a feature: every program with a renderer draws text. An actor that measures depends on `aethereal-render` for `Metrics` and `measure`, as it does for the kinds it mails the renderer.

## Consequences

- **Any program draws text in one ask and one placement,** ordered with its own drawing, on desktop and in the browser alike. ichor drops its bitmap console pass, and the dev overlay draws with this.
- **Text stays sharp at every size and under scaling,** and an outline costs nothing, so a damage number over the painted world is readable.
- **The renderer grows a font reader and an MSDF generator.** They are pure functions, tested on their own, and the render root only keeps their results.
- **MSDF is slightly softer than a glyph rasterised at its exact size at the smallest sizes,** around 12 to 14 pixels. Whether a console line reads well enough is judged on samples before this lands.
- **A text whose glyphs are new is answered a frame or so after it is asked for.** Text already on the screen never waits.
- **World text is a program's work for now:** it projects, and moves, each label.
- **A program lays out around its text in its own turn,** measuring each text as the renderer will lay it out, from metrics it asked for once per font.
- **A font's fallbacks are its maker's,** fixed and one level deep, so no actor changes another's text, and a measure is of the text the renderer draws.
- **Follow-on work this leaves open:**
  - placing a text at a point in the world, through a matrix the program hands the renderer (a change to ADR-0048's rule);
  - fonts and sizes mixed in one text;
  - shaping for scripts that join and reorder;
  - CFF outlines, WOFF2, and variable fonts.

## Alternatives considered

- **A separate text actor.** Its placements would be ordered with the program's own only where the program's list calls it (ADR-0055), and every text field would be two actors exchanging mail.
- **A library a program uses to make glyph meshes itself.** Every program would keep an atlas of its own, and text would not be shared.
- **Fonts and shaping in a crate of their own.** No second user needs them. A module moves to a crate when one does. Measuring adds none: an actor that measures already depends on `aethereal-render` for the kinds it mails the renderer.
- **A font's metrics as mail.** Past the engine's `read_limit` for any font, and many times past it for a Japanese, Chinese, or Korean one.
- **The renderer measuring a text when asked.** A round of mail for each text, so an actor laying out a screen waits a turn for each size, where it needs them all in the turn it lays the screen out in.
- **Metrics encoded at each ask.** Tens of kilobytes encoded and put again for every actor that asks, where one blob serves them all.
- **Fallbacks one list for the render root,** `FallbackFonts { fonts }`, set by any actor, the last list sent replacing the one before. One actor's list changes every other actor's text, a text's fallbacks change under it as the list does, and a measure could not know which list a text will be laid out with.
- **Fallbacks followed to their own fallbacks.** What a font draws would depend on fonts its maker never named, and a chain could loop.
- **Kerning held in hash maps, rebuilt as metrics are decoded.** Their order is not their written order, so a decode would rebuild every map; sorted lists are the shape written, and are searched by halves in a few comparisons for a pair.
- **Coverage rasterised at each size.** Crisper at the smallest sizes, but each size is another set of glyphs, scaling and rotating re-rasterise, and an outline is another raster.
- **Fonts baked into atlases ahead of time.** Simple, but only for glyphs known when baking; chat, names, and Han text are open sets.
- **A text that holds its own position.** Moving it would lay it out again, and one text could not be drawn in two places.
- **Text at a point in the world, projected by the renderer through the camera's uniforms.** One write of the camera's matrix would move every label, but the renderer would read a camera's layout, which ADR-0048 rules out. It is left for later, as screen pixels are simpler to reason about.
- **Text drawn immediately, said again each frame.** Most text changes rarely, and a counter that changes each frame is one write a frame either way.
