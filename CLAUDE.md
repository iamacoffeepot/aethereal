# CLAUDE.md

Guidance for Claude Code working in this repository. The code wins where this file and the code disagree.

## Status

Pre-1.0 Rust project (edition 2024). A game engine: a thin native or browser **host** owns I/O, GPU, and audio and runs **actors**, which communicate only by **mail**. The repository is built one piece at a time, lowest layer first, and holds only what has entered so far — read the tree, do not assume a crate exists.

Load-bearing design is recorded as ADRs in `docs/adr/NNNN-title.md` (start one from `docs/adr/TEMPLATE.md`). Read the cited ADR before changing what it governs.

## Entry rules (ADR-0001)

- A piece enters only in its final shape, with its reasons in ADR text here. Nothing is carried in to be reworked later, and a reference to another repository's issue or ADR is not a reason.
- Every crate builds for `wasm32-unknown-unknown` **as a host**, unless it is a native host crate that says so.
- Guest-only code is selected by a feature or custom cfg, never `target_arch = "wasm32"`.
- Nothing outside the guest-runtime seam names a wasm runtime's types.
- Nothing blocks, sleeps, or spawns a thread; the scheduler is complete in its single-threaded pumped mode.
- Time and devices are handed in by the host: no `Instant::now()`, no `block_on`.

## Public repository

Every commit is public. No personal names, no absolute home paths, no host, mount, or drive details, and no secrets in code, docs, commit messages, or pull requests. The README describes mechanism.

## Workflow

- **Design** is worked in chat with the owner, on code shapes. A decision that is load-bearing becomes an ADR, reviewed as a pull request.
- **Branches**: `type/short-slug` (`feat/data-layer`, `docs/adr-guest-runtime`).
- **Commits and PR titles** follow Conventional Commits (`type(scope): subject`), checked by `Lint title`. `main` squash-merges with the PR title as the commit subject.
- **PRs** are small: one piece or one concept each. `main` changes only through a pull request; `CI pass` is the verdict to wait on. Claude does not merge without the owner's authorization.

## Commands

- Build: `cargo build`
- Test: `cargo test` (single test: `cargo test <name>`)
- Lint: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- Browser-host lint: `cargo clippy --workspace --all-features --target wasm32-unknown-unknown -- -D warnings`
- Format: `cargo fmt` (check-only: `cargo fmt -- --check`)

## Code rules

- **Tests earn their place.** Before writing a test, name the plausible bug it catches. Do not test code this repository does not own (std, the compiler, serde, third-party crates) or what a derive emits. A flat assertion against a fixed value is kept only when the value is computed (a hash, a byte layout) and carries a `// Tripwire:` comment naming the invariant.
- **No nullable state.** `Option` is the return of a lookup that can miss, or a value fixed when its owner is built and never changed. State that changes is an enum with named cases: never a `None` meaning "not yet", "already used", or "the other behaviour", and no `.take()` state machines.
- **Fallible returns an error; infallible does not.** A failure the caller can meet is a returned error, never a panic.
- **Recursion**: prefer an explicit work stack in any algorithm whose depth follows its input. Recursive code over user-controlled or geometric data enforces a depth cap that returns an error.
- **No section-divider banner comments** (`// ----------`). Use a plain comment or split the module. Enforced at edit time by `.claude/hooks/check-no-divider-comments.sh`.
- **Naming**: spell units out (`millis`, `nanos`, `bytes`), never `ms` / `ns` / `kb`. Do not encode a value's Rust type in its name.
- **Chains and paragraphs**: when a value flows through successive calls, write the chain; a single-use intermediate earns a `let` only when its name documents a non-obvious value. One blank line between logical units, none inside one. Line width belongs to rustfmt (`max_width = 120`).
- **Conditions read as names**: bind each part of a compound condition with `let` and combine the names. No call unwrapped or converted inside a condition, no closure inside one, no negated field-by-field chain. A simple condition (one comparison, or one predicate call) may stay inline.
- **Arguments and results**: no `if` or `match` inside call or macro arguments, and none wrapped directly (`Ok(match …)`); bind it first.
- **No reaching through another type's fields**: the owning type gets a named method.
- **Printed output is an interface**: a printed `key=value` value stays one token and a status goes before the key.
- **Module structure**: break a crate into submodules by concept, tests beside the code they exercise, so the file listing reads as the design. A new sibling mirrors the shape of its siblings unless it honestly does not fit.
- **Locks and atomics** each say why one-mail-at-a-time does not already cover them.
