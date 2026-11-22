# Spike: a `view!` screen as a guest in the headless engine

Spike code, never merged. Branch `spike/screen-guest`, cut from `main` at `a7a4db5`.

## Verdict

**It runs, after one change outside the screen.** A screen written with `view!` (ADR-0056) builds for wasm32 as a
guest with no change to `aethereal-ui` or its macros, loads into the headless engine through wasmtime, runs its
`Prepare` turns, sends the renderer its texts and one draw list, takes a click on its button, and writes its label's
new text the next frame. The one change: `aethereal-render` turns on wgpu's browser backend for every wasm32 build,
a guest included, and the module that comes out imports wasm-bindgen's functions, which the guest host refuses.

## What was built

- `crates/aethereal-chassis-headless/guests/screen`: one screen actor, `Counter`. A `Column` holds a `Text`,
  `"Clicks: {self.clicks}"`, and a `Row` holding a `Button` whose `Text` reads `Add`. `click => self.add()` counts a
  click. It also answers `CountClicks` with `Clicks`, so a test reads its state by typed mail. It is declared with
  `#[actor(with(aethereal_ui::screen))]` and `aethereal_actor::guest!(Counter)`.
- `build.rs` builds it the way it builds the echo guest, with `RUSTFLAGS=--cfg aethereal_guest`
  (`AETHEREAL_GUEST_SCREEN`).
- `tests/screen_guest.rs` sets up an engine on two parts, running `(Tick, Prepare, Render)`, with:
  - the real window root, `Windows`, made through `Opening::new(setup.source(), setup.controller())` with no windows;
  - a stand-in renderer (below);
  - the screen, through `add_root_from`, drawing into `Frame::Main(window 0)` at 200 by 100.

  What the screen sent comes from the engine's record of the stand-in's turns (`settle_turns`, `Arrival::read`). The
  count comes from asking the screen. The test reads no shared state.

## What broke, and what was done

| # | What | Fix in this spike | Real change needed? |
|---|---|---|---|
| 1 | `aethereal-render` has `[target.'cfg(target_arch = "wasm32")'.dependencies] wgpu = { features = ["webgpu", …] }`, so the guest links wasm-bindgen. The module has 5 foreign imports (`__wbindgen_placeholder__` ×3, `__wbindgen_externref_xform__` ×2), 3,711 `__wbindgen_describe_*` exports, and a 528 KB `__wasm_bindgen_unstable` section, 2.38 MB in all. The host refuses it: `Unfit(Unlinked("unknown import: __wbindgen_placeholder__::__wbindgen_describe has not been defined"))`. | That target table is now `cfg(all(target_arch = "wasm32", not(aethereal_guest)))`, and the guest is built with `--cfg aethereal_guest`. Cargo reads RUSTFLAGS when it evaluates target cfgs. The module then imports only the 22 `aethereal` functions, at 1.26 MB. | **Yes.** It is the inverse of CLAUDE.md's rule that guest code is chosen by a feature or cfg, never by `target_arch`: browser-only code is chosen by `target_arch` here. Either a feature, such as `browser` turned on by `aethereal-chassis-browser`, or a guest cfg the workspace declares, or the render kinds split out of the crate that holds wgpu. |
| 2 | A screen `depends(Renderer, Windows)`. The engine refuses a root whose dependency of that kind (name plus layout, ADR-0006) is not live. Headless has no renderer. The real one needs a GPU, a front, controllers, and a window actor to begin. | A native stand-in with `#[kind(name = "aether.render")]` and the renderer's state layout copied: seven `u64` slots where the handles are, `Owed`, `Watch`, `Subscribers<(GpuTimes,)>`. A tripwire test pins `KindId::of::<StandIn>() == KindId::of::<Renderer>()`. It answers every ask the screen makes with a counted id, answers `ReadMetrics` with the monospace font's real metrics, and draws nothing. | **Yes, for tests.** The copied layout breaks whenever `Renderer`'s fields change. Either `aethereal-render` exports a test renderer (behind a feature) whose kind is the renderer's, or a headless renderer runs on wgpu's noop device. No screen had ever run in any engine outside a desktop window before this spike. |
| 3 | No picking without a real renderer. | The test sends `Pointed { PointerDown }`, then `Pointed { PointerUp, over: Over::Entry { actor: counter.protocol::<Hearer>(), … } }`, from the outside seat to the entry the list marked `Hit … Blocks(Hand)`. This is what the renderer's picking would send. | No, for a test of the screen. Picking belongs to the renderer. |
| 4 | The guest's build compiles `wgpu`, `naga`, `wesl`, `png`, `zune-jpeg`, `pulldown-cmark`, and `aethereal-substrate` for wasm32. `aethereal-window` brings in the substrate through its host-side `host.rs`. | Nothing. Dead-code elimination removes almost all of it: naga 563 B, the substrate 941 B, png and markdown about 0. | Optional. It costs build time (about 45 s for a cold release build of the guest), not module size. The host halves of `aethereal-window` and `aethereal-render` could sit behind a feature. |
| 5 | A debug build of the guest is 40.5 MB (37 MB of it DWARF). That is past the 8 MB blob budget the other headless tests give the engine: `CodeRefused::NoRoom`. | The spike's engine gets a 128 MB budget. With it the debug module loads and draws. | No. Build guests in release, as `build.rs` does. |
| 6 | Nothing else. `view!`, `style!`, `#[actor(with(aethereal_ui::screen))]`, `#[derive(Bound, Portability)]` over a struct holding `Screen`, `Upgrades`, and `guest!` all compiled for wasm32 on the first try. The guest host's `Prepare` support (`prepares`/`prepare`) ran the screen's rebuild. | Nothing. | No. |

## Evidence

The headless engine ran six frames (`a_screen_guest_draws_and_counts_a_click`):

- The screen took 17 turns: `Prepare`, and its answers.
- It made two texts: `CreateText { font: Mono, size: 16.0, runs: ["Clicks: 0"] }` and `["Add"]`.
- It sent one draw list:
  - `Draw { placement 8, Ignores }`, the label;
  - `Hit { Main(window 0), rect (8, 32.8)–(36.8, 53.6), Blocks(Hand) }`, the button;
  - `Draw { placement 9, Ignores }`, the button's text.
- `CountClicks` answered 0.

Then the test pressed and released on entry 1:

- `CountClicks` answered 1.
- The next frame sent `WriteText { text 6, runs: ["Clicks: 1"] }` and no new list.
- A frame with nothing changed sent the renderer nothing (0 renderer turns).

The pinned kind test, `the_stand_in_has_the_renderer_s_kind`, passes.

## Sizes and times

Each time is a single run of `Instant` around the call, so it shows scale, not a benchmark. The engine runs in the
test binary. The workspace builds wasmtime and cranelift at opt-level 3 even in its test profile.

| | release guest | debug guest |
|---|---|---|
| module | 1,256,108 B (code 1,013,739; `name` section 174,575; data 56,499) | 40,523,224 B (code 2,815,375; DWARF ~37 MB) |
| for comparison, the echo guest (about 40 actor types), release | 633,565 B | |
| code by crate, release (rough, from the name section) | `aethereal_ui` ~458 KB, `taffy` ~258 KB, `aethereal_actor` ~128 KB, `aethereal_render` ~47 KB, `aethereal_data` ~40 KB, `core` ~48 KB | |

| | test profile host | release host (fat LTO) |
|---|---|---|
| `put_code`, which compiles the release module | 1.37 s | 0.95 s |
| `put_code`, the debug module | 3.52 s | 2.31 s |
| `add_root_from`, which instantiates and runs `begin` | 0.36 ms | 0.15 ms |
| a click: press and release, 2 turns | 67 µs | 18 µs |
| a frame that writes the label, settled | 0.49 ms | 60 µs |
| a frame with nothing changed, settled | 65 µs | 4 µs |
