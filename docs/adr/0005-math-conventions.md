# ADR-0005: Math conventions

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Every piece that places something in space needs the same few types: vectors, a matrix, a rotation, a box, a ray. They are small, and writing them is not the hard part. The hard part is that their conventions do not show in their types. Which way is up, how a matrix is stored, which side a vector multiplies on, what order three angles are applied in, where the depth range starts: two pieces that disagree on any of these compile, run, and draw the wrong picture.

So the conventions are decided once, here, before the first piece that uses them, and one crate holds the types for every piece above it.

Two earlier decisions bind that crate. Math values travel: a camera sends a matrix, a scene sends a box. So each type is plain data, `Plain` of ADR-0004, with a shape in the vocabulary of ADR-0003. And the same types are used by a native host, a browser host, and guests, so the crate meets the host rules of ADR-0001 and gives the same answer on each.

## Decision

**One crate, `aethereal-math`, holds the engine's math types.** It stands on `aethereal-data` and on nothing else in this repository. What enters with it:

```rust
pub struct Vec2 { pub x: f32, pub y: f32 }
pub struct Vec3 { pub x: f32, pub y: f32, pub z: f32 }
pub struct Vec4 { pub x: f32, pub y: f32, pub z: f32, pub w: f32 }
pub struct Quat { pub x: f32, pub y: f32, pub z: f32, pub w: f32 }
pub struct Mat4 { pub columns: [Vec4; 4] }
pub struct Aabb { pub min: Vec3, pub max: Vec3 }
pub struct Rect2 { pub min: Vec2, pub max: Vec2 }
pub struct Ray { pub origin: Vec3, pub direction: Vec3 }
```

**Every scalar is `f32`.** The types are not generic over a scalar, use no SIMD, allocate nothing, and contain no `unsafe`. Every type is `Copy`, its fields are public, and it compares with `PartialEq` only: a float has no total order, so no math type is `Eq`, `Hash`, or a `Key`.

**Space is right-handed with Y up.** X points right, Y up, and Z toward the viewer, so forward is `-Z`. The cross product follows: `X × Y = Z`.

**Angles are radians, and a positive angle turns counter-clockwise** as seen looking from the tip of the axis back toward the origin, which is the right-hand rule. A quarter turn about Y takes `X` to `-Z`; about X it takes `Y` to `Z`; about Z it takes `X` to `Y`. No function takes degrees.

**`Mat4` is column-major and multiplies a column vector on its right.** It is stored as four columns. `m * v` applies `m` to `v`, and `a * b` applies `b` first, then `a`. A translation is the fourth column.

```rust
let model_view_projection = projection * view * model;
let clip = model_view_projection * position.extend(1.0);
```

Flattened to sixteen floats, a matrix is column 0, then column 1, and so on. That is the layout a GPU uniform expects, so a matrix reaches a shader with no transpose.

**`Quat` is `x`, `y`, `z`, `w` with the scalar last.** The identity is `(0, 0, 0, 1)`. The product is the Hamilton product, `q * v` rotates `v`, and `a * b` applies `b` first, the same reading as a matrix product. The functions that build a rotation return a unit quaternion; the type does not require one.

**Euler angles have one order, YXZ.** `Quat::from_euler(yaw, pitch, roll)` is yaw about Y, then pitch about the turned X, then roll about the turned Z:

```rust
Quat::from_euler(yaw, pitch, roll)
    == Quat::from_axis_angle(Vec3::Y, yaw) * Quat::from_axis_angle(Vec3::X, pitch) * Quat::from_axis_angle(Vec3::Z, roll)
```

It is the order a free-look camera wants: turning left and right never tilts the horizon. No other order is offered.

**A view looks down `-Z`, and clip depth runs from 0 to 1.** `Mat4::look_at` puts the eye at the origin looking down `-Z` with Y up. `Mat4::perspective` and `Mat4::orthographic` take `near` and `far` as positive distances in front of the eye and produce clip coordinates whose `x` and `y`, after the divide by `w`, span `-1` to `1` with `y` up, and whose depth is `0` at `near` and `1` at `far`. A perspective projection writes `w = -z`. This is the depth range of WebGPU, Direct3D, Metal, and Vulkan; OpenGL's `-1` to `1` is not offered. Because the crate has one handedness and one depth range, the function names carry no suffix for either.

**A box is two corners, and a box with `min` above `max` on any axis is empty.** `Aabb` and `Rect2` hold `min` and `max`. Both corners are inside: a point on the boundary is contained, and two boxes that only touch intersect. A box whose `min` equals its `max` on an axis is flat on that axis and still holds its points. `EMPTY` is `min` at positive infinity and `max` at negative infinity, so the union of `EMPTY` with any box is that box, and a bound is built by starting from `EMPTY` and adding points.

`Rect2` has no unit and no fixed direction for Y. Screen pixels, layout space, and texture coordinates all use it, and the piece that uses it says which way its Y runs.

**A ray is an origin and a direction.** `ray.at(t)` is `origin + direction * t`, and a ray covers `t` at or above zero. The direction need not be a unit vector; a function that returns a ray says whether it is.

**Every bit pattern is a value.** No math type holds an invariant between its fields: an unnormalised quaternion, a singular matrix, and a box with `min` above `max` are all values with a stated meaning. This is what lets every type derive `Plain` with nothing to check on decode.

| Type | Shape | Bytes |
|---|---|---|
| `Vec2` | Struct of `x`, `y`, each Float 32 | 8 |
| `Vec3` | Struct of `x`, `y`, `z`, each Float 32 | 12 |
| `Vec4` | Struct of `x`, `y`, `z`, `w`, each Float 32 | 16 |
| `Quat` | Struct of `x`, `y`, `z`, `w`, each Float 32 | 16 |
| `Mat4` | Struct of `columns`, an Array of 4 `Vec4` | 64 |
| `Aabb` | Struct of `min`, `max`, each a `Vec3` | 24 |
| `Rect2` | Struct of `min`, `max`, each a `Vec2` | 16 |
| `Ray` | Struct of `origin`, `direction`, each a `Vec3` | 24 |

By ADR-0002 an array has no count and a struct no tags, so the 64 bytes of a `Mat4` are its sixteen floats in column-major order, little-endian: the encoded matrix is the uniform.

**A question with no answer returns `Option`; a degenerate input never produces NaN.** The inverse of a singular matrix, a point projected onto the plane through the eye, and a ray that never meets a plane have no value to return, and `None` says so. It is not an error: nothing went wrong, and the caller decides what a missing answer means. Where a degenerate input has an answer that is safe to carry on with, the function returns it and its documentation states it: normalising a zero vector gives the zero vector, and normalising a zero quaternion gives the identity. A NaN that the caller passes in is passed through.

A function that compares against a tolerance states the tolerance and why it is relative or absolute. `Mat4::inverse` uses none: a determinant scales with the size of the matrix's columns, so no fixed threshold is sound, and it refuses only a determinant that is zero or not finite.

**`sqrt`, `sin`, `cos`, and `tan` come from `libm`.** The crate is `#![no_std]`, and stable `core` has none of them: on `f32` they are methods of `std`, which forwards them to the platform's C library. `libm` is those functions written in Rust. It needs no `std`, builds for `wasm32-unknown-unknown`, and returns the same bits on a native host, in a browser, and in a guest, which the platform libraries do not promise each other. It is the crate's one dependency from outside this repository. `abs`, `min`, `max`, and `is_finite` are in `core` and are used from there.

**The crate meets the host rules of ADR-0001.** It is `#![no_std]` with no feature that changes that, and never names `target_arch`. It has no state, so it reads no clock, starts no thread, and has nothing to block on. Host and guest compile the same code.

**Not entering yet:**

- Colour. It has conventions of its own (which space, whether alpha is premultiplied) and enters with the first piece that draws.
- A rigid transform (a rotation and a translation as one value). `Mat4::from_rotation_translation` covers building one; the type enters when a piece composes or interpolates them.
- Snapping a rectangle to framebuffer pixels. It assumes a frame (origin at the top-left, Y down) that belongs to the piece that draws.

## Consequences

- A piece that sends a position, a rotation, or a bound says `Vec3`, `Quat`, or `Aabb` in its mail, and every reader in every language knows the bytes from the table above.
- A renderer uploads a `Mat4` as it is. A shader written against it uses `matrix * vector`.
- Math ported from a source with another convention is converted at the port: a Z-up asset is turned on import, and a projection written for a `-1` to `1` depth range is replaced, not adjusted afterwards.
- `f32` holds about seven decimal digits, so a position a few kilometres from the origin resolves to about a millimetre and degrades beyond that. A world larger than that keeps positions relative to a nearby origin. That scheme belongs to the piece that owns the world, above this crate.
- The same inputs give the same floats on both hosts and in a guest, as far as this crate is concerned. A piece that needs two engines to agree bit for bit still has to control everything else that touches its floats.
- Scalar code is slower than SIMD. A piece that measures a hot loop over many vectors solves it there, over its own arrays; the types that travel stay as they are.
- Adding a type, or a convention such as a second Euler order, is an amendment to this ADR.

## Alternatives considered

- **An external math crate such as `glam`.** Mature, fast, and widely known. Its types are not `Plain`, and ADR-0004 allows a foreign type to be plain data only through an implementation in the data layer or a local newtype: the first ties the lowest piece to a math library's versions, the second wraps every type and gives up the library's own operators. Its `Vec4`, `Quat`, and `Mat4` also change representation and alignment with the target's SIMD features, where this crate wants one representation on a native host, a browser host, and a guest. It offers several conventions side by side (both handednesses, two depth ranges, every Euler order), so choosing one would still be a rule kept by review instead of by what exists.
- **`f64` scalars.** More precision, at twice the size in every message, and a conversion at every GPU upload, since shaders take 32-bit floats. The precision problem it solves is a large world, which a local origin solves for both widths.
- **Types generic over the scalar.** Every signature carries a parameter for a second width nothing uses.
- **Row-major storage, or row vectors (`v * m`).** Row-major storage needs a transpose at every upload. Row vectors reverse the reading order of every product relative to the mathematics most references are written in.
- **Z up.** Common in modelling tools. Y up matches clip space and screen space, where Y is already the vertical axis, so a view matrix does not swap axes.
- **Left-handed space.** Forward becomes `+Z`, which reads naturally for depth, and the cross product and every rotation then turn the other way from the right-hand rule most references use.
- **Clip depth from `-1` to `1`.** OpenGL's range. The GPU interfaces both hosts use take `0` to `1`, so a matrix in OpenGL's range would be remapped before every use.
- **`core` or `std` float functions.** `std` is not available to a `no_std` crate. Stable `core` has neither the square root nor the trigonometric functions, and the platform's own results differ between a native host and a browser.
- **A box as a centre and half-extents.** Cheaper to rotate, and it has no natural empty value and makes a union the expensive operation; bounds are built by union far more often than they are rotated.
- **`Option<Aabb>` for the empty box.** It states absence in the type, and turns every accumulation into a match. An empty box that unions as the identity needs no case analysis, and it keeps every bit pattern a value.
