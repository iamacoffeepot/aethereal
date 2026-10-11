# ADR-0006: Kind identity

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Mail is bytes, and ADR-0002's bytes do not say what they are. Whatever carries a piece of mail also carries a statement of what it is, small enough to ride on every piece and exact enough that a receiver never reads the bytes as something else. A **kind** is plain data (ADR-0004, `Plain`) given a name, and a **kind id** is that statement.

Three things could decide whether two kinds are the same kind: the name, the bytes the value is written as, and the names inside its schema (ADR-0003, `Shape`). ADR-0003 left open which of them count.

The engine also loads guests it did not build and does not trust. A guest states its kinds itself, so an id is a claim made by its sender, and the engine has to be able to check it.

## Decision

**A kind is plain data with a name.**

```rust
pub trait Kind: Plain {
    const NAME: &'static str;
}

#[derive(Plain)]
struct NoteOn { pitch: u8, velocity: u8 }

impl Kind for NoteOn {
    const NAME: &'static str = "aether.audio.note_on";
}
```

The name is all an author writes. There is no derive: the implementation is one line, and it is the whole declaration.

**A kind's id is computed from its name and its layout, and from nothing else.** The **layout** of a schema is the schema with every name inside it removed: field names, variant names, and the names of the structs and enums it is built from. Two schemas with the same layout are written as the same bytes.

So two kinds are the same kind when they have the same name and the same layout:

| Change | Same kind? |
|---|---|
| A field or a variant is renamed | yes |
| A struct or enum inside it is renamed | yes |
| A field is added, removed, reordered, or changes type | no |
| A variant's selector changes | no |
| The kind is renamed | no |

Two programs built from different versions of a kind therefore disagree on its id exactly when they could disagree on its bytes. The older one sees a kind it does not know, and never reads a `u32` as an `f32`.

**The id is 64 bits, and its value is private.**

```rust
#[derive(Debug, Clone, Copy, Hash, Plain, Key)]
pub struct KindId(u64);

impl KindId {
    pub const fn of<K: Kind>() -> Self { .. }
    pub fn from_description(name: &str, schema: &SchemaNodes) -> Self { .. }
}
```

`KindId::of::<K>()` is evaluated at compile time. `from_description` computes the same id from a name and a schema that arrived as bytes. Both go through one function that turns a name and a layout hash into an id, and nothing else constructs a `KindId`, so an id cannot be written by hand and cannot disagree with its kind.

**The hash is FNV-1a, 64 bits,** over bytes this ADR states, so that a codec in any language computes the same id. Its offset basis is `0xcbf29ce484222325` and its prime is `0x100000001b3`.

A shape's **layout hash** is the hash of one tag byte followed by the shape's parts. A child shape appears as its own layout hash, eight bytes little-endian. A count, a length, and a selector are each an unsigned 32-bit integer, little-endian, as in ADR-0002.

| Shape | Tag | Then |
|---|---|---|
| Boolean | `0` | |
| Integer | `1` | one byte: `0` to `7` for U8, U16, U32, U64, I8, I16, I32, I64 |
| Float | `2` | one byte: `0` for F32, `1` for F64 |
| String | `3` | |
| Bytes | `4` | |
| Option | `5` | the value's layout hash |
| Sequence | `6` | the element's layout hash |
| Array | `7` | the element's layout hash, then the length |
| Map | `8` | the key's layout hash, then the value's |
| Struct | `9` | the field count, then each field's layout hash in declared order |
| Tuple | `10` | the element count, then each element's layout hash in order |
| Enum | `11` | the variant count, then for each variant in declared order: its selector, its field count, and each field's layout hash |

Whether a struct's or a variant's fields are named or unnamed is a matter of names, and does not appear.

**A kind id is the hash of the name's UTF-8 bytes followed by the layout hash of its schema,** eight bytes little-endian.

A layout hash is built from its children's hashes for one reason: it costs one step per node in both forms of a schema. A `Shape` is walked as the tree it is. A `SchemaNodes` shares equal shapes between parents, and is hashed node by node in the order it is stored, without unfolding the sharing, so a received schema cannot make the engine do work out of proportion to its size.

**An id is a claim, and whatever admits kinds checks it.** A registry of kinds holds the name and schema behind each id. It computes the id from the name and schema it was given, never taking the sender's word for it. A second description arriving under an id already held is accepted when it has the same name and the same layout, and is refused otherwise. For that test:

```rust
impl SchemaNodes {
    pub fn same_layout(&self, other: &Self) -> bool { .. }
}
```

It compares the two schemas shape by shape, ignoring names, and does not compare hashes. FNV-1a is not hard to collide on purpose. With this check a collision is a refused load, and is never mail delivered as the wrong kind.

## Consequences

- Renaming a field is free. Any change to the bytes makes a new kind, and so does renaming the kind.
- Mail of a kind's old layout is not readable as its new one, whether it is in flight or stored. What stores mail across versions decides what it does about that.
- An id says nothing about what sort of id it is. A reader learns that from the schema of the value that holds it, where a `KindId` is a struct of that name.
- **Changing how an id is computed changes every id.** While nothing stores ids, that costs a rebuild. Once a piece stores them, it forks whatever was stored. Reserving bits of the id for another purpose is such a change, and the last cheap moment for one is before the first piece that stores ids.
- A name does not determine an id by itself. A generic type that implements `Kind` has one name and an id for each layout it is used at, and two builds of one kind with different layouts share a name. Whether an engine holds two layouts of one name at once belongs to the registry of kinds.
- What a name may be written as is not decided here. The id is computed from whatever bytes the name has.
- A schema's layout may differ where the bytes agree: a struct and a tuple of the same fields are different layouts, and so are an enum's variants declared in another order with the same selectors. The id is never the same where the bytes could differ.
- A 64-bit id written as JSON is subject to whatever the JSON mapping decides for 64-bit integers.

## Alternatives considered

- **The name alone.** An id that survives every change to the kind, so two versions of a kind share an id and disagree about its bytes. The disagreement is found by a decoder, when it is found at all.
- **The name and the full schema, names included.** Renaming a field would cut two peers apart though their bytes agree.
- **An id written into the trait as a constant.** `K::ID` reads well, and an implementation written by hand could then state an id that is not its kind's.
- **A derive for `Kind`.** A second way to write one line.
- **A cryptographic hash, or a wider id.** Collisions could not be made on purpose. The id rides on every piece of mail, such a hash is not computable in a constant, and the registry's check gives the same safety for 64 bits.
- **Bits of the id reserved to say what sort of id it is.** The earlier engine did this so that an id met with no type beside it could still be recognised. Here every id has a type and a schema beside it.
- **A prefix on the hashed bytes for each sort of id.** It keeps the id spaces of different types apart, and the types already do.
- **Small numbers handed out by each engine as kinds are met.** Dense, fast to index, and unknown until two engines have spoken, so no id could be a constant and every connection would begin by agreeing on them.
- **Hashing the encoded bytes of a flat, nameless schema.** One definition for both forms, and building the shared flat form in a constant needs allocation a constant does not have.
