# ADR-0006: Kind identity

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Mail is bytes, and ADR-0002's bytes do not say what they are. Whatever carries a piece of mail also carries a statement of what it is, small enough to ride on every piece and exact enough that a receiver never reads the bytes as something else. A **kind** is plain data (ADR-0004, `Plain`) given a name, and a **kind id** is that statement.

Three things could decide whether two kinds are the same kind: the name, the bytes the value is written as, and the names inside its schema (ADR-0003, `Shape`). ADR-0003 left open which of them count.

The engine also loads guests it did not build and does not trust. A guest states its kinds itself, so an id is a claim made by its sender, and the engine has to be able to check it.

## Decision

**A kind is a described type with a name.**

```rust
pub trait Kind: Schema {
    const NAME: KindName;
    /// The kind's node, which its id is computed from: a static, for a type with no parameters.
    const KIND: KindPtr = /* a node made in a constant */;
}

#[derive(Plain)]
#[kind(name = "aether.audio.note_on")]
struct NoteOn { pitch: u8, velocity: u8 }
```

The name is all an author writes. A kind is declared by `#[kind(name = "…")]` on its `Plain` or `Bound` derive, and the derive writes the implementation beside the schema and the codec:

```rust
impl Kind for NoteOn {
    const NAME: KindName = KindName::new("aether.audio.note_on");
    const KIND: KindPtr = /* this kind's node */;
}
```

For a type with no parameters, the derive also writes the kind's **node**, a static holding its name and its schema, which its id is computed from, and `KIND` points at it. It bounds neither the schema nor the kind on its fields' types: a field whose type is not `Schema` is refused at that field's shape, and a bound would leave a kind that reaches itself through a reference unprovable. A type with parameters keeps its bounds, and its node is made in a constant.

The attribute holds the name and nothing else. A second `kind`, any other key, a name that is not a string literal, and a `kind` with no name are each a compile error, and so is a `kind` on a field or a variant. Whether the name conforms is decided by `KindName::new` and not by the derive, and for a type with no parameters the derive also reads the name in a constant of its own, so a name that does not conform stops the build even where nothing uses the kind.

A type whose `Schema` is written by hand writes its `impl Kind` by hand too, as a type whose schema is written by hand writes its codec by hand. The longhand stands where the derive does not, and is not a second way to declare a kind.

A kind does not state where it may go. The types of its fields decide that (ADR-0004): a kind of plain fields crosses every boundary, and a kind holding a value bound to its own actor is a kind that no send accepts. Mail is the door between actors, and requires of the kind it carries the codec for that boundary.

**A name is a `KindName`, and every `KindName` conforms.** A name is one or more segments separated by `.`. A segment is a lowercase ASCII letter followed by any number of lowercase ASCII letters, digits, and `_`.

```rust
pub struct KindName(..);                                // its text is private

impl KindName {
    pub const fn new(name: &'static str) -> Self { .. }
}
```

`new` is for constants: a name that does not conform stops the build of whatever uses the kind. A name that arrives at run time arrives as bytes. `KindName` is plain data with the shape of a string, and its decoder refuses text that does not conform with `DecodeError::InvalidKindName`, as ADR-0004's decoders do for the rules a type adds beyond its shape.

A `KindName` can be compared, printed, and sent. It does not hand out its text as a string, so code that holds one has nothing to split, match a prefix of, or build another name from. A name says which kind this is and carries no other meaning.

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
    pub fn from_description(name: &KindName, schema: &SchemaNodes) -> Self { .. }
}
```

`KindId::of::<K>()` is evaluated at compile time. `from_description` computes the same id from a name and a schema that arrived as bytes; a member of a cycle arrives with its whole cycle (below). Both go through one function that turns a name and a layout hash into an id, and nothing else computes one, so an id cannot be written by hand. A `KindId` is plain data, and one that was decoded is a claim like any other.

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

`Json` was added by ADR-0038 with the tag `12` and nothing after it, so it never hashes as String does.

A reference that names its protocol (ADR-0016) has the tag `13`, then the layout hash of the sixteen bytes it is written as, then one byte: `0` for a tuple of tells, `1` for a named protocol. Then come the kinds the protocol names, as a set: the count of those outside the hashed kind's cycle, and their ids ascending, each followed, for a named protocol, by its answers as a count and their ids ascending. A reference inside a cycle then names the members of the cycle it reaches (below).

Whether a struct's or a variant's fields are named or unnamed is a matter of names, and does not appear.

**A kind id is the hash of the name's UTF-8 bytes followed by the layout hash of its schema,** eight bytes little-endian.

A layout hash is built from its children's hashes for one reason: it costs one step per node in both forms of a schema. A `Shape` is walked as the tree it is. A `SchemaNodes` shares equal shapes between parents, and is hashed node by node in the order it is stored, without unfolding the sharing, so a received schema cannot make the engine do work out of proportion to its size.

**A kind that reaches itself through references is hashed with its cycle.** A reference's layout names its protocol's kinds by their ids, and a kind may hold a reference to a protocol that takes it, directly or through other kinds: `aether.window.pointed` holds, for a release, a reference to the actor it was over, which hears `aether.window.pointed`. A kind's id cannot be an input to its own hash. So the kinds a kind's references reach, and theirs in turn, are a graph, and its id is computed from the graph:

1. The graph is found from the kind's node and broken into its strongly connected components. Each component is hashed after every component it reaches, so every kind outside it already has its id.
2. A component of one kind with no reference to itself is hashed as the table says, each kind its references name written as its id.
3. Any other component is a **cycle**. Its members are ordered by their names' bytes, then by their layouts with the members each one names counted but not told apart. Two members this order does not tell apart stop the build.
4. Each member's layout is hashed with the members its references name written as their positions in that order: after a reference's kinds outside the cycle, the count of the members it names, and their positions ascending, each followed, for a named protocol, by the hash of its answers.
5. The cycle is hashed once: the tag `14`, the member count, and for each member in order the length and bytes of its name and its layout hash.
6. Each member's id is made as every id is, from its name and a layout hash: here the hash of the tag `14`, the cycle's hash, and its position.

```
cycle       = hash(14, 1, len, "aether.window.pointed", layout(Pointed, its reference to itself as position 0))
id(Pointed) = from_layout("aether.window.pointed", hash(14, cycle, 0))
```

A change to any member changes the cycle's hash, so it changes the id of every member, and of every kind outside the cycle that names one. A kind in no cycle hashes as it would if no cycle were possible, so a kind with no references keeps its id.

**A node is a static, and a reference's protocol points at nodes.** A kind's node holds its name, its schema, and its **site**, the module, type, file, line, and column it is defined at. A reference's protocol points at its kinds' nodes, not at their ids, so nodes can reach each other in a loop, as constants cannot. The walk tells nodes apart by their sites, since a constant cannot compare pointers; a site is never hashed, so one kind defined twice alike has one id. `KindId::of::<K>()` walks the graph from `K::KIND` in a constant, with explicit stacks, and a graph of more than 128 kinds or 1,024 references stops the build.

**A cycle's members are kinds with no parameters, whose `Kind` the derive wrote.** A generic kind's node, and the node of a kind whose `Kind` is written by hand, is made in a constant and has no site, and a cycle through one stops the build.

**A member of a cycle is described with its whole cycle.** A received description of one lists the cycle's members, each with its name and schema, its references to fellow members written as their places in the list, and says which member it describes. Whatever admits it checks that the list is one cycle and every place is in it, orders the members itself, refuses a tie, and computes the id: the order the sender listed them in does not count. A schema that holds a reference to a member of a cycle names it by its id.

**An id is a claim, and whatever admits kinds checks it.** A registry of kinds holds the name and schema behind each id. It computes the id from the name and schema it was given, never taking the sender's word for it. A second description arriving under an id already held is accepted when it has the same name and the same layout, and is refused otherwise. For that test:

```rust
impl SchemaNodes {
    pub fn same_layout(&self, other: &Self) -> bool { .. }
}
```

It compares the two schemas shape by shape, ignoring names, and does not compare hashes. It gives each node of both schemas a number for its layout, the same number for the same layout, and asks whether the two roots got the same one, so its work is in proportion to the two schemas' sizes however their nodes are shared. FNV-1a is not hard to collide on purpose. With this check a collision is a refused load, and is never mail delivered as the wrong kind.

## Consequences

- Renaming a field is free. Any change to the bytes makes a new kind, and so does renaming the kind.
- Mail of a kind's old layout is not readable as its new one, whether it is in flight or stored. What stores mail across versions decides what it does about that.
- An id says nothing about what sort of id it is. A reader learns that from the schema of the value that holds it, where a `KindId` is a struct of that name.
- **Changing how an id is computed changes every id.** While nothing stores ids, that costs a rebuild. Once a piece stores them, it forks whatever was stored. Reserving bits of the id for another purpose is such a change, and the last cheap moment for one is before the first piece that stores ids.
- A name does not determine an id by itself. A generic type that implements `Kind` has one name and an id for each layout it is used at, and two builds of one kind with different layouts share a name. Whether an engine holds two layouts of one name at once belongs to the registry of kinds.
- Changing what a name may be written as changes which programs build, and never an id: an id is computed from the name's bytes, whatever the rule that admitted them.
- `KindName::new` called outside a constant with text that does not conform panics. Nothing needs to call it there: a kind's name is a constant, and a received name is decoded.
- A schema's layout may differ where the bytes agree: a struct and a tuple of the same fields are different layouts, and so are an enum's variants declared in another order with the same selectors. The id is never the same where the bytes could differ.
- A 64-bit id written as JSON is subject to whatever the JSON mapping decides for 64-bit integers.
- A kind may reach itself through the references it holds, and a message may introduce a peer that takes the same message. A change to any member of a cycle changes every member's id.
- The derive writes `unsafe`: a kind's `KIND` is a raw pointer to its static node. It points at an immutable static and is only read.
- A cycle costs compile time with its size: measured, about 4 to 6 ms an id for shapes the engine has, and about 200 ms for each member of a ring of 64.
- The order of a cycle's members is not canonical for every cycle: two cycles that differ only by a contrived symmetry among members of one name may hash differently. Both sides of a connection compute the same id from the same definitions, which is what an id needs.
- A kind whose references reach more than 128 kinds does not build.

## Alternatives considered

- **The name alone.** An id that survives every change to the kind, so two versions of a kind share an id and disagree about its bytes. The disagreement is found by a decoder, when it is found at all.
- **The name and the full schema, names included.** Renaming a field would cut two peers apart though their bytes agree.
- **A bare string for the name.** One type fewer. Nothing would then check a name, and a string invites uses a name does not have: parsing it for a namespace, matching on a prefix, building one at run time.
- **An id written into the trait as a constant.** `K::ID` reads well, and an implementation written by hand could then state an id that is not its kind's.
- **An attribute macro that writes a kind's derives from flags.** The earlier engine's did: `copy`, `eq`, `bound`, and others each turned into derives the kind carried. The codec derive already is the stack a kind needs, and the standard derives are each type's own needs, which flags on the kind would only restate. That vocabulary was also never total, so the longhand lived on beside it and there were two ways to declare a kind.
- **A cryptographic hash, or a wider id.** Collisions could not be made on purpose. The id rides on every piece of mail, such a hash is not computable in a constant, and the registry's check gives the same safety for 64 bits.
- **Bits of the id reserved to say what sort of id it is.** The earlier engine did this so that an id met with no type beside it could still be recognised. Here every id has a type and a schema beside it.
- **A prefix on the hashed bytes for each sort of id.** It keeps the id spaces of different types apart, and the types already do.
- **Small numbers handed out by each engine as kinds are met.** Dense, fast to index, and unknown until two engines have spoken, so no id could be a constant and every connection would begin by agreeing on them.
- **Hashing the encoded bytes of a flat, nameless schema.** One definition for both forms, and building the shared flat form in a constant needs allocation a constant does not have.
- **No kind may reach itself through references,** and the one that does holds an `ErasedActorRef` its receiver casts. The hash stays a tree. A message that introduces a peer taking the same message, a relay, gossip, a ring, could then never hold a typed reference, and each such design would be bent around the rule.
- **A reference naming its protocol's kinds by name.** No cycle, since a name does not depend on a layout. A kind holding a reference would keep its id when a kind of its protocol changed its layout, and a reference from an older build would claim a protocol nothing proved.
- **Ids computed when the program starts.** The walk would be ordinary code. Every id that is a constant today, in declarations, guests' sections, and tripwires, would stop being one.
- **A node reached by a `&'static` reference.** No `unsafe`. A constant holding a reference to a static has the static evaluated first, and a static that reaches itself through constants never finishes.
- **Members ordered by refining their layouts until nothing changes,** which makes every cycle's ids canonical. More code in a constant, for cycles no program writes.
