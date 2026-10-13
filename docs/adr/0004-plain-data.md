# ADR-0004: Plain data

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

A value may have to cross a boundary: from one actor to another, from one engine to another. Whether it can depends on the value. A number can go anywhere. A reference to a live actor means something only inside the engine that holds the actor. A handle to a pending reply means something only to the actor that holds it.

The engine this repository succeeds answered with one codec for every value and marker traits beside it that said where a value was allowed to go. The codec itself did not know, so decoding a value in the wrong place compiled, and was refused when it ran. It also carried a second, serde-based path that had to produce the same bytes as the first.

Most values are of the first sort: they are only their bytes. This ADR gives every value one codec that states the boundary it is crossing, and names the first sort. The values bound to a boundary, and the boundaries between actors and between engines, enter with the pieces that own them; they are sketched under Consequences so that this ADR can be judged against where it leads.

## Decision

**A boundary is a type, and the codec takes it as a parameter.**

```rust
pub trait Boundary {
    type WriterOffers<'a>;
    type ReaderOffers<'a>;
}

pub struct Writer<'a, B: Boundary> { .. }      // the bytes written so far, and B's offers
pub struct Reader<'a, B: Boundary> { .. }      // the bytes still to read, the budget, and B's offers

pub trait Encode<B: Boundary> {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError>;
}

pub trait Decode<B: Boundary>: Sized {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError>;
}
```

- The boundary is fixed where the code is written. No codec asks, when it runs, which boundary it is at, and no value carries one: `B` is a type, chosen by the door that encodes or decodes.
- Encoding or decoding a value at a boundary it cannot cross does not compile, and the error names the type.
- There is one writer type and one reader type, each over the boundary. Every writer holds the bytes written so far, and every reader holds the bytes still to read and the allocation limit of ADR-0002. A boundary states only what it offers beside them, whatever its bound values need: a table of attached values, a registry to prove a name against. An offer is a value the writer or reader holds and is always there.
- The boundary is in the type of what a codec is handed, so a codec that calls another names nothing: `self.secs.encode(out)`.
- `encode` fails when a collection holds more elements than a count can state, or when the boundary refuses a bound value.
- `decode` returns a value that owns its data.

**A type encodes the same at every boundary.** A type has one `Encode` impl and one `Decode` impl, each written over every boundary `B`, so a value's bytes are the same wherever it is encoded. They are the bytes of ADR-0002 for the type's `Schema::SHAPE` (ADR-0003). The identity of a kind (ADR-0006) and the JSON walk (ADR-0007) read bytes through the schema alone, so both hold for every value at every boundary. A value that is written inline in one place and as a handle in another is two types.

**A value crosses a boundary exactly when the boundary offers what the value needs.** An offer is a trait on what a boundary's writer or reader holds, and a type's one impl is bounded by the offers it uses:

```rust
impl<B: Boundary> Encode<B> for u64 { .. }                                    // needs nothing: every boundary

impl<B: Boundary, T: Encode<B>> Encode<B> for Vec<T> { .. }                   // whatever its elements cross

impl<B: Boundary> Encode<B> for Payload                                       // a later piece's bound value:
where                                                                         // only a boundary whose writer
    for<'a> B::WriterOffers<'a>: Attach,                                      // can attach a payload
{ .. }
```

A type that needs nothing is plain data. No impl names one boundary, so no type can encode one way here and another way there.

**`Bare` is the boundary that offers nothing.**

```rust
pub struct Bare;

impl Boundary for Bare {
    type WriterOffers<'a> = ();
    type ReaderOffers<'a> = ();
}
```

**`Plain` is data that is only its bytes.** It needs nothing from wherever it is encoded or decoded, and means the same thing everywhere.

```rust
pub trait Plain: Schema + Encode<Bare> + Decode<Bare> {}

pub fn to_bytes<T: Plain>(value: &T) -> Result<Vec<u8>, EncodeError> { .. }
pub fn from_bytes<T: Plain>(bytes: &[u8], budget: &mut Budget) -> Result<T, DecodeError> { .. }
```

`Schema` is a supertrait so that plain data cannot have a codec without a description. `Plain` adds no method: it is the name by which other code requires data that is only its bytes.

**One derive emits the schema and the codec from one field list:**

```rust
#[derive(Plain)]
struct Tick {
    frame: u64,
    delta_nanos: u64,
}
```

```rust
// what the derive writes
impl Schema for Tick { .. }
impl<B: Boundary> Encode<B> for Tick where u64: Encode<B> { .. }
impl<B: Boundary> Decode<B> for Tick where u64: Decode<B> { .. }
impl Plain for Tick {}
```

The codec is bounded by each field's type, so a derived type crosses exactly the boundaries all its fields cross. `impl Plain` is the author's statement that the type is only its bytes: a field that is not `Plain` is a compile error naming it. Enum variants are numbered by position unless a variant states its selector with `#[plain(selector = N)]`.

**`Key` is plain data that may be a map key or a set element.** ADR-0002 lists the key shapes and the order of each.

```rust
pub trait Key: Plain + Ord {}
```

`Ord` on a `Key` is the order ADR-0002 states for its shape. It is implemented for the integers, `char`, the `NonZero` integers, `String`, `Vec<u8>`, and `[u8; N]`. `#[derive(Key)]` is accepted on a struct with exactly one field, itself a `Key`, and writes the struct's `PartialEq`, `Eq`, `PartialOrd`, and `Ord` as the field's, so the struct orders as its field does and cannot be given another order.

```rust
#[derive(Plain, Key)]
struct PlayerId(u64);
```

**There is one byte path.** A type's codec is written once and serves every boundary the type crosses. No serde implementation exists beside it, and none is derived. JSON is produced by walking a schema (ADR-0007), not by a second trait on the type.

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
| `Box<T>` | `T`'s shape | |
| `Result<T, E>` | Enum, `Ok` = 0 and `Err` = 1 | |
| `Vec<T>`, `VecDeque<T>` | Sequence | |
| `Vec<u8>` | Bytes | |
| `BTreeSet<T>`, `T: Key` | Sequence | ascending key order, none repeated |
| `[T; N]` | Array | |
| `BTreeMap<K, V>`, `K: Key` | Map | ascending key order, none repeated |
| `()` and tuples to twelve elements | Tuple | |
| `Duration` | Struct of `secs: u64` and `nanos: u32` | `nanos` below one billion |
| a derived struct, with named, unnamed, or no fields | Struct | |
| a derived enum | Enum | |

With the `std` feature, `HashMap<K, V>` is a Map and `HashSet<T>` a Sequence under the same rules, for `K: Key + Hash` and `T: Key + Hash`. A `BTreeMap` or `BTreeSet` is already in key order and is written as it iterates. A hash map or hash set is sorted by `Ord` on encode.

A `Box<T>` is its contents. It has `T`'s shape, `T`'s bytes, and `T`'s layout, so boxing a field changes no schema and no kind id (ADR-0006). The box says only where Rust keeps the value. It does not admit a type that contains itself: that type's schema would be a constant that refers to itself (ADR-0003).

**Not `Plain`, and why:**

- `usize`, `isize`: their width differs between a 64-bit host and a 32-bit guest.
- `u128`, `i128`: not in the vocabulary (ADR-0003).
- `Rc`, `Arc`: they promise sharing that bytes cannot keep.
- `Cow`, `&str`, `&[T]`: decode owns its data.
- `PhantomData`: no user yet.
- As a map key or set element: anything that is not a `Key`, including `bool`, floats, tuples, enums, and structs of several fields. Each needs an order stated in ADR-0002 first.

## Consequences

- A piece holding only plain data names no actor and no engine, so it can enter first and stay as it is when they arrive. `Bare` is the only boundary it knows.
- **Where this leads.** The pieces that own the other boundaries add them as types:

  | Boundary | What is crossed | A value that stops here |
  |---|---|---|
  | `Bare` | nothing | |
  | `Within` | time: stored and read back by one actor | a reply it holds and has not yet given |
  | `Actors` | between actors in one engine | a reference to a live actor, a handle to a large payload |
  | `Engines` | between engines | a path its receiver must prove |

  A bound value is one hand-written codec, bounded by the offers it uses of its boundary's writer and reader. It crosses each boundary that makes those offers and stops at the rest. A type holding one derives the same codec as any other type and does not state `Plain`.
- **A door states the boundary; the value does not.** Mail is the door between actors: a send requires `Encode<Actors>` of what it carries, and a handler `Decode<Actors>`. A door out of the engine requires `Engines`. A type that never crosses between actors is still encoded, by the actor that keeps it.
- Whatever crosses between engines also crosses between actors, because `Actors` makes every offer `Engines` makes.
- An author chooses between two derives: `Plain`, and the one for a type holding a bound value, which enters with the first such value. They write the same codec. `Plain` also states that the type is only its bytes.
- A value decoded from a large input is copied out of it. Views that borrow from the input are a separate trait beside `Plain` if they are ever needed, and do not change it. Large payloads have another answer: a handle that crosses actors without a copy.
- On a little-endian machine an implementation may encode a padding-free struct of fixed-width fields by copying it (ADR-0002). Nothing in the trait shows whether it does.
- Types from other crates are `Plain` only through an implementation here or a local newtype.

## Alternatives considered

- **One codec for all values, with markers for where they may go.** The earlier engine's shape. The markers and the codec can disagree, and the disagreement is found at run time.
- **A separate codec for plain data, carried to every boundary by one impl** (`impl<B: Boundary, T: Plain> Encode<B> for T`). Plain data then needs no boundary in its signatures. A container is plain when its elements are, so that impl already covers `Vec<T>`, and Rust refuses a second impl for a `Vec` of bound values as overlapping. A bound value could then never sit in a `Vec`, an `Option`, or a map.
- **Plain data as `impl<B: Boundary> Encode<B>` with no named trait.** One trait fewer, and nothing can then require plain data, because Rust cannot ask whether a type implements a trait for every `B`.
- **The writer and reader as types each boundary names** (`type Writer<'a>`, `type Reader<'a>` on `Boundary`, with a trait for what every writer and reader must hold). A boundary then shapes its own writer. A writer's type does not say whose writer it is, so Rust cannot find the boundary from what a codec is handed, and a hand-written codec calling a concrete type's codec must name it: `<u64 as Decode<B>>::decode(input)`.
- **The codec over the writer and reader types, with no boundary type** (`Encode<W>`, `Decode<R>`). A boundary would be two names, one for each direction, with nothing tying them together, and every door would state its reader for every lifetime.
- **A boundary's offers as optional hooks on one shared reader.** One reader type for every boundary. A decode at a boundary that lacks a hook is then refused when it runs.
- **serde as the codec.** A widely known interface, and a second description of every type beside its schema that must be kept in agreement with it.
- **A lifetime on `decode` so values may borrow.** Every bound over `Plain` would carry the lifetime for the sake of the few types that borrow.
