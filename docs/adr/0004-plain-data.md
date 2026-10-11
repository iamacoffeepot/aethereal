# ADR-0004: Plain data

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

A value may have to cross a boundary: from one actor to another, from one engine to another. Whether it can depends on the value. A number can go anywhere. A reference to a live actor means something only inside the engine that holds the actor. A handle to a pending reply means something only to the actor that holds it.

The engine this repository succeeds answered with one codec for every value and marker traits beside it that said where a value was allowed to go. The codec itself did not know, so decoding a value in the wrong place compiled, and was refused when it ran. It also carried a second, serde-based path that had to produce the same bytes as the first.

Most values are of the first sort: they are only their bytes. This ADR names that sort and gives it one codec. Values bound to a boundary, and the boundaries themselves, are later ADRs; they are sketched under Consequences so that this one can be judged against where it leads.

## Decision

**`Plain` is data that is only its bytes.** It crosses any boundary, needs nothing from wherever it is encoded or decoded, and means the same thing everywhere.

```rust
pub trait Plain: Schema + Sized {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError>;
    fn decode(input: &mut &[u8], budget: &mut Budget) -> Result<Self, DecodeError>;
}
```

- The bytes are those of ADR-0002 for the type's `Schema::SHAPE` (ADR-0003). `Schema` is a supertrait so that a type cannot have a codec without a description, or the two disagree.
- `encode` fails only when a collection holds more elements than a count can state.
- `decode` returns a value that owns its data. `budget` is the allocation limit of ADR-0002.

**One derive emits both** the schema and the codec from one field list:

```rust
#[derive(Plain)]
struct Tick {
    frame: u64,
    delta_nanos: u64,
}
```

Every field must be `Plain`; a field that is not is a compile error naming it. Enum variants are numbered by position unless a variant states its selector.

**There is one byte path.** No serde implementation exists beside it, and none is derived. JSON is produced by walking a schema (a later piece), not by a second trait on the type.

**The Rust types that are `Plain`**, each mapped onto a shape of the vocabulary:

| Rust type | Shape | Rule the decoder enforces beyond the shape |
|---|---|---|
| `bool` | Boolean | |
| `u8` `u16` `u32` `u64` `i8` `i16` `i32` `i64` | Integer | |
| `f32` `f64` | Float | |
| `String` | String | |
| `char` | Integer, unsigned 32-bit | is a Unicode scalar value |
| the `NonZero` integers | Integer | is not zero |
| `Option<T>` | Option | |
| `Result<T, E>` | Enum, `Ok` = 0 and `Err` = 1 | |
| `Vec<T>`, `VecDeque<T>` | Sequence | |
| `Vec<u8>` | Bytes | |
| `BTreeSet<T>` | Sequence | ascending encoded bytes, none repeated |
| `[T; N]` | Array | |
| `BTreeMap<K, V>` | Map | |
| `()` and tuples to twelve elements | Tuple | |
| `Duration` | Struct of `secs: u64` and `nanos: u32` | `nanos` below one billion |
| a derived struct, with named, unnamed, or no fields | Struct | |
| a derived enum | Enum | |

With the `std` feature, `HashMap<K, V>` is a Map and `HashSet<T>` a Sequence under the `BTreeSet` rule. Every map is sorted by encoded key bytes on encode whatever its type, so a hash map costs nothing a `BTreeMap` does not.

**Not `Plain`, and why:**

- `usize`, `isize`: their width differs between a 64-bit host and a 32-bit guest.
- `u128`, `i128`: not in the vocabulary (ADR-0003).
- `Box`, `Rc`, `Arc`: `Box` would admit recursive types; `Rc` and `Arc` promise sharing that bytes cannot keep.
- `Cow`, `&str`, `&[T]`: decode owns its data.
- `PhantomData`: no user yet.

## Consequences

- A piece holding only plain data has no notion of actors, engines, or boundaries, so it can enter first and stay as it is when they arrive.
- **Where this leads.** A later ADR adds boundaries as types and a codec per boundary:

  ```rust
  pub trait Encode<B: Boundary> { .. }
  pub trait Decode<B: Boundary>: Sized { .. }

  impl<B: Boundary, T: Plain> Encode<B> for T { .. }     // plain data crosses every boundary
  ```

  A value bound to a boundary, such as a reference to a live actor, implements the codec for the boundaries it can cross and no others. A derived type crosses exactly the boundaries all its fields cross, so sending a value where it cannot go is a compile error. `Plain` has to be a named trait for this: Rust cannot ask whether a type implements a trait for every `B`. One relation between boundaries is already settled: whatever crosses between engines also crosses between actors.
- An author chooses between two derives, `Plain` and the boundary codec. A struct of plain fields that derives the second still crosses every boundary, but cannot be used where `Plain` is required.
- A value decoded from a large input is copied out of it. Views that borrow from the input are a separate trait beside `Plain` if they are ever needed, and do not change it. Large payloads have another answer: a handle that crosses actors without a copy.
- On a little-endian machine an implementation may encode a padding-free struct of fixed-width fields by copying it (ADR-0002). Nothing in the trait shows whether it does.
- Types from other crates are `Plain` only through an implementation here or a local newtype.

## Alternatives considered

- **One codec for all values, with markers for where they may go.** The earlier engine's shape. The markers and the codec can disagree, and the disagreement is found at run time.
- **Plain data as `impl<B: Boundary> Encode<B>` with no named trait.** One trait fewer, and nothing can then require plain data, because the bound cannot be written.
- **serde as the codec.** A widely known interface, and a second description of every type beside its schema that must be kept in agreement with it.
- **A lifetime on `decode` so values may borrow.** Every bound over `Plain` would carry the lifetime for the sake of the few types that borrow.
