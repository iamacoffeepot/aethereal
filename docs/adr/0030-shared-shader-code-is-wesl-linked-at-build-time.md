# ADR-0030: Shared shader code is WESL, linked at build time

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

Every draw program, compute program, and effect hands the renderer its whole WGSL text, as a blob (`CreateProgram.wgsl`, `CreateCompute.wgsl`, `CreateEffect.wgsl`, each a `BlobRef`, ADR-0021, ADR-0026). The renderer parses and validates that text with `naga` (`wgsl::parse_str` and `Validator` in `aethereal-render/src/program/mod.rs`) before it makes anything from it.

Programs that share functions each carry a copy of them. A program on Aethereal that bakes its scene in compute and then draws it repeats the same generic functions across several modules today, and puts each module together by concatenating strings in Rust. The functions shared so far are generic, not that program's own:
- an integer hash of `(i, j, k, seed)` to a fraction in [0, 1), mixed as JavaScript's `Math.imul` mixes, so a WGSL port of JavaScript can match it bit for bit: the top 24 bits, exact in `f32`;
- value noise over that hash, and a sum of two octaves of it;
- a smooth step and a smooth minimum;
- colour: Oklab to linear sRGB, linear to sRGB encoding, and a filmic tone curve;
- bilinear sampling of a grid of floats held in a storage buffer. WebGPU cannot filter 32-bit float textures by default.

Concatenating strings has three costs.
- **What a module needs is written out by hand,** in the right order, by every program that uses it.
- **All names share one scope,** because WGSL has no modules. A piece's function and a program's own can collide.
- **WGSL reserves many names a port reaches for** (`smooth`, `patch`, `from`, `filter`, `target`, …), and a module-scope declaration named like a built-in (`floor`) shadows it. Each is found only when the module is parsed, in a page, at run time.

WESL is a strict superset of WGSL that adds:
- imports between modules (`import package::noise::value_noise;`, `super::`, renames with `as`, and other packages by name);
- conditional translation (`@if`);
- packages shared through cargo and npm.

A linker turns a WESL module and what it imports into one WGSL module. It includes the declarations the main module uses, recursively, and renames each imported declaration after its full path, a segment holding underscores marked with their count: the spec's example is `bevy_pbr::lighting::main`, linked as `_1bevy_pbr_lighting_main`. The main module's own declarations are not renamed, so its entry points and the names the host sees stay as written; the spec marks that rule as still under discussion. The Rust linker is the `wesl` crate. It runs in a build script, giving the WGSL to the program as a string (`include_wesl!`), or at run time.

## Decision

**Shared shader code is written in WESL and linked to WGSL when the program is built.** The renderer is unchanged: a program still hands it one WGSL text, which it parses and validates with its own `naga` as it does now.

- **A program's shader may be WGSL or WESL.** WGSL is valid WESL, so a program that shares nothing changes nothing.
- **A program that imports shared code links it in its build script** with `wesl`, and includes the WGSL that results as a string, which becomes its program's blob.
- **Nothing is linked at run time.** The linker and its parser stay out of the program, which matters most in the browser's wasm.

**`aethereal-render` ships the generic pieces as a WESL package,** in modules by concern:

| module | holds |
|---|---|
| `hash` | the integer hash of `(i, j, k, seed)` to a fraction, `Math.imul`-compatible, the top 24 bits |
| `noise` | value noise over the hash, and its two-octave sum |
| `shape` | the smooth step and the smooth minimum |
| `colour` | Oklab to linear sRGB, linear to sRGB encoding, and the filmic tone curve |
| `grid` | the four cells and two weights of a bilinear sample of a grid of floats in a storage buffer, which the program reads and blends |
| `segment` | a segment of two points, with the meanings of `aethereal-math`'s `Segment` (ADR-0005); WGSL has no enums, so its side is one of three `i32` constants, the sign it names |

- **A program imports what it uses,** and the linker includes what that uses in turn: importing `noise` brings `hash`. No program lists what a piece needs, or orders it.
- **Names cannot collide,** because the linker renames every imported declaration after its path. A piece's names need no prefix, and a program names its own functions as it likes.
- **The package is offered through cargo, as `wesl` packages a crate's shaders.** `aethereal-render`'s build script packages the modules as the WESL package `aethereal_render`, and the crate exports it as a static, `aethereal_render::shaders::PACKAGE`. A program's build script depends on `aethereal-render` and names that static among its compiler's dependencies; `wesl` finds no package a crate carries on its own, and reads no dependencies from a `wesl.toml`. Each piece is declared `public`, as WESL requires of a declaration another package imports.
- **`grid` does not read the buffer.** A module of a package cannot name a storage buffer the program declares, and passing one by pointer needs WGSL's `unrestricted_pointer_parameters`, which WebGPU does not enable by default. It answers the indices of the four cells around the point and the two weights, and the program reads the four cells and blends them.

**Each piece is linked and validated as it is built.** A test in `aethereal-render` links each module of the package, alone, into a WGSL module, then parses and validates it with the `naga` the renderer uses. A reserved name, a built-in shadowed, or WGSL that WESL's parser and `naga` read differently fails there, before any program or page runs. The hash also has a test that it matches the JavaScript it ports, on a fixed set of inputs. The segment follows the table of cases `aethereal-math` asserts its own `Segment` against, and a test runs it on a GPU where the machine has one and compares its answers with the CPU's.

**`wesl` is a build dependency, at an exact version.** It is before its first stable release, so a newer version is taken on purpose, with the validation test as its gate. The package's static is written in the types of `wesl-core`, two structs of static strings with no dependencies of their own, which `aethereal-render` depends on at the version `wesl` uses.

## Consequences

- A program shares shader code by importing it. Programs stop carrying copies, and stop assembling modules from strings.
- The renderer, its kinds, and what a program sends are unchanged. Linking happens before any of them.
- A shader that imports shared code is linked when the program is built, so a program cannot choose its pieces while it runs. Conditional translation (`@if`) chooses among variants known when it is built.
- A shader is read by two parsers, WESL's when it is linked and `naga` when the renderer makes it. The validation test catches where they disagree for the shipped pieces. A program's own shader meets `naga` when the renderer makes it, as it does now.
- Each program that imports shared code has a build script, which depends on `aethereal-render` as well as on `wesl`. The renderer, and `wgpu` with it, is then compiled again for the build script.
- **Left for later:**
  - pieces chosen at run time, which would need the renderer to link, or the linker at run time;
  - publishing the package to npm, for pages built in JavaScript.

## Alternatives considered

- **Pieces as Rust strings, assembled by a function** that walks what each piece needs. That is a module system written again, with no imports in the shader text and no renaming, so every piece would carry a prefix to avoid collisions.
- **Named pieces in the engine,** `CreateProgram` naming library pieces beside its own WGSL for the renderer to assemble. It adds mail and a registry to the renderer, and gains nothing over linking before: the renderer would still parse one module.
- **naga-oil's `#import`.** A preprocessor tied to `naga`'s version, so each `wgpu` upgrade would wait on its release, and shader files stop being WGSL.
- **WESL linked at run time.** It puts the linker and its parser in every program, the browser's wasm among them, to choose pieces no program here needs to choose while it runs.
