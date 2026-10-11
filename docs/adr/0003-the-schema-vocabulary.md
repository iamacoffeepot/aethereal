# ADR-0003: The schema vocabulary

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

The bytes of ADR-0002 do not say what they are. Something has to describe a value's shape as data, for three users: a codec in another language that reads and writes the bytes, a tool that shows a value as JSON and builds one from JSON, and the engine, which names a kind of mail by its shape. A **schema** is that description.

The set of shapes a schema can describe is the **vocabulary**. Every codec in every language implements all of it, so each shape in it is a cost paid once per language. The rule for what belongs:

> A shape is in the vocabulary only if any language's codec must understand it to read the bytes. A Rust type that is laid out like a shape already in the vocabulary is that shape, not a new one.

`Box<T>` shows the rule at work. Its bytes are `T`'s bytes, so a reader in another language never sees it, and it adds nothing to the vocabulary. What it would add is a type that contains itself, and with that a schema that refers back to itself, which every codec would then have to follow. The vocabulary has no such shape.

## Decision

**The vocabulary** is the twelve shapes of ADR-0002, and nothing else:

```rust
pub enum Shape {
    Boolean,
    Integer(Integer),                                   // U8, U16, U32, U64, I8, I16, I32, I64
    Float(Float),                                       // F32, F64
    String,
    Bytes,
    Option(&'static Shape),
    Sequence(&'static Shape),
    Array { element: &'static Shape, length: u32 },
    Map { key: &'static Shape, value: &'static Shape },
    Struct { name: &'static str, fields: &'static [Field] },
    Tuple { elements: &'static [Shape] },
    Enum { name: &'static str, variants: &'static [Variant] },
}

pub struct Field {
    pub name: &'static str,
    pub shape: Shape,
}

pub struct Variant {
    pub name: &'static str,
    pub selector: u32,
    pub fields: Fields,
}

pub enum Fields {
    Named(&'static [Field]),
    Unnamed(&'static [Shape]),
}
```

A struct with unnamed fields (`struct Pair(u32, u32)`) is `Struct` with fields named by position, `"0"` and `"1"`. A variant with no fields is `Unnamed(&[])`.

A `Map`'s key is one of the key shapes ADR-0002 lists: an integer, a string, bytes, an array of bytes, or a struct of exactly one such field. A schema with any other key is not a schema.

Names are part of a schema and not part of the bytes. They exist for the readers that need them: JSON, tools, and error messages.

**A type states its schema as a constant:**

```rust
pub trait Schema {
    const SHAPE: Shape;
}
```

A constant, so that anything computed from a schema can be computed at compile time. A constant cannot refer to itself, so the compiler refuses a recursive type here with no rule of ours.

**No shape contains itself.** A schema is a finite tree. Data shaped like a tree or a graph is written flat, as a sequence of nodes that name each other by index:

```rust
struct Expression { nodes: Vec<Node> }                  // the last node is the root
enum Node { Number(i64), Add(u32, u32) }                // indices into `nodes`
```

A decoder's nesting is therefore bounded by the type being decoded and never by the input.

**A schema that travels is itself flat.** A schema sent to a tool or another engine is plain data like any other, so it follows its own rule: a sequence of nodes, each naming its children by index, children before parents, the root last.

```rust
pub struct SchemaNodes { nodes: Vec<SchemaNode> }

pub enum SchemaNode {
    Boolean,
    Integer(Integer),
    Float(Float),
    String,
    Bytes,
    Option(u32),
    Sequence(u32),
    Array { element: u32, length: u32 },
    Map { key: u32, value: u32 },
    Struct { name: String, fields: Vec<(String, u32)> },
    Tuple { elements: Vec<u32> },
    Enum { name: String, variants: Vec<VariantNode> },
}
```

It is built by walking a `Shape`, and equal shapes share one node. A decoder refuses a node that names an index at or above its own, so a received schema is a finite tree by the same check that reads it. It refuses a map whose key is not a key shape.

There are two forms because there are two places a schema lives: `Shape` in a compiled program, where nothing can be allocated, and `SchemaNodes` in bytes, where nothing can be a reference.

## Consequences

- A codec in another language implements twelve shapes and is complete.
- Recursive types cannot be described, so they cannot be plain data. An author flattens them.
- Field docs, units, and display hints are not in the vocabulary. They can be added as a parallel description without changing a schema, and they enter with the first tool that reads them.
- 128-bit integers are not in the vocabulary, since several languages have none. A value that needs 128 bits is an array of 16 bytes.
- The size of an integer in the vocabulary is always stated. A machine-sized integer (`usize`) has no shape.
- Whether a name takes part in what makes two kinds the same kind is not decided here. It belongs to the ADR on kind identity.

## Alternatives considered

- **One recursive form with boxed children.** One type for both places, at the cost of a recursive wire type, a nesting limit in every decoder, and the only use of `Box` in plain data.
- **A schema built by a function at run time.** Simpler to write than constants, and it gives up computing anything from a schema at compile time.
- **A larger vocabulary (sets, durations, characters, wide integers).** Each is expressible as a shape already present, so each would be a cost in every language for no new capability.
