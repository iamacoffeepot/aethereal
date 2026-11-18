# The renderer's own fonts

The renderer compiles in two fonts, `Font::Sans` and `Font::Mono`, so every program draws text with nothing to load (ADR-0052). The owner chose them from sheets the engine drew of five candidates each. Both fall back to a third, a colour emoji font, compiled in too.

- `IBMPlexSans-Regular.ttf` is IBM Plex Sans Regular, from IBM's release `@ibm/plex-sans@1.1.0` (https://github.com/IBM/plex, `fonts/complete/ttf/`). It is `Font::Sans`.
- `IBMPlexMono-Regular.ttf` is IBM Plex Mono Regular, from the google/fonts repository (`ofl/ibmplexmono`), the same face IBM releases. It is `Font::Mono`.

- `NotoColorEmoji.ttf` is Noto Color Emoji, from the googlefonts/noto-emoji release `v2026-09-24-unicode18_0` (https://github.com/googlefonts/noto-emoji, `2D/fonts/NotoColorEmoji.ttf`, SHA-256 `15671215ab769fdc7162a045d56fd7d7e477c51b04e6b3c761d914d8fdd6cc44`). Its glyphs are PNG images, in `CBDT`, at one size, 109 pixels to the em. It is the fallback of both.

All three are static TrueType files, copied unchanged, and licensed under the SIL Open Font License 1.1: IBM Plex's text is in `OFL.txt`, and Noto Color Emoji's, as its release ships it, in `NotoColorEmoji-OFL.txt`. "Plex" is a Reserved Font Name: a changed copy may not be called Plex.
