# The renderer's own fonts

The renderer compiles in two fonts, `Font::Sans` and `Font::Mono`, so every program draws text with nothing to load (ADR-0052). Both are placeholders until the owner chooses the faces from samples the engine draws.

- `NotoSans-Regular.ttf` is Noto Sans Regular, copied unchanged from Debian's `fonts-noto-core` package (`/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf`, upstream https://github.com/notofonts/noto-fonts). It is `Font::Sans`, and the font module's tests read it: it has TrueType outlines, composite glyphs, and pair kerning in `GPOS` of both formats.
- `NotoSansMono-Regular.ttf` is Noto Sans Mono Regular, copied unchanged from Debian's `fonts-noto-mono` package (`/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf`, same upstream). It is `Font::Mono`.

Both are licensed under the SIL Open Font License 1.1, whose text is in `OFL.txt`.
