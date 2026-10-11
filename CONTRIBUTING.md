# Contributing to Aethereal

## Licensing of contributions

Aethereal is dual-licensed under [MIT](LICENSE-MIT) or
[Apache License 2.0](LICENSE-APACHE), at the recipient's option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.

## Before you push

```
cargo fmt -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-features --target wasm32-unknown-unknown -- -D warnings
```

GitHub Actions is the merge gate. `CI pass` aggregates `Format`, `Clippy`,
`Test` and `Browser host`; `Lint title` checks that the pull request title is a
Conventional Commit (`type(scope): subject`). `main`
squash-merges with the title as the commit subject.

A change enters under the rules in
[ADR-0001](docs/adr/0001-pieces-enter-in-final-shape-for-two-hosts.md). A
load-bearing design decision is its own ADR, started from
[`docs/adr/TEMPLATE.md`](docs/adr/TEMPLATE.md) and reviewed as a pull request.
