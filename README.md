# Aethereal

A game engine built as actors that communicate only by mail, on a thin host
that owns I/O, GPU and audio. The host runs natively and in a browser.

The repository is built one piece at a time, lowest layer first. A piece
enters only in its final shape; see
[ADR-0001](docs/adr/0001-pieces-enter-in-final-shape-for-two-hosts.md) for the
entry rules. Design decisions are recorded in [`docs/adr/`](docs/adr/).

Pre-1.0. Nothing is published to crates.io.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE),
at your option.
