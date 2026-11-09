# Fonts the tests read

- `NotoSans-Regular.ttf` is Noto Sans Regular, copied unchanged from Debian's `fonts-noto-core` package (`/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf`, upstream https://github.com/notofonts/noto-fonts). The font module's tests read it, and the glyph numbers they check are this file's: it has TrueType outlines, composite glyphs, and pair kerning in `GPOS` of both formats. It is never compiled into the renderer.

It is licensed under the SIL Open Font License 1.1, whose text is in `OFL.txt`.
