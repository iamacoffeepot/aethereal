# The renderer's own fonts

The renderer compiles in two fonts, `Font::Sans` and `Font::Mono`, so every program draws text with nothing to load (ADR-0052). The owner chose them from sheets the engine drew of five candidates each.

- `IBMPlexSans-Regular.ttf` is IBM Plex Sans Regular, from IBM's release `@ibm/plex-sans@1.1.0` (https://github.com/IBM/plex, `fonts/complete/ttf/`). It is `Font::Sans`.
- `IBMPlexMono-Regular.ttf` is IBM Plex Mono Regular, from the google/fonts repository (`ofl/ibmplexmono`), the same face IBM releases. It is `Font::Mono`.

Both are static TrueType files, copied unchanged, and licensed under the SIL Open Font License 1.1, whose text is in `OFL.txt`. "Plex" is a Reserved Font Name: a changed copy may not be called Plex.
