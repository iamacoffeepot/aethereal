# ADR-0002: The byte layout of plain data

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Everything the engine moves is bytes at some point: mail between actors, mail between engines, a value handed to a guest, a value shown to a tool as JSON. One layout serves all of them. It has to be simple enough to implement in any language, because not every reader or writer of these bytes is Rust: a browser host has JavaScript beside it, and tools are written in whatever suits them.

This ADR fixes only how a value's shape becomes bytes. What shapes exist is ADR-0003. Which Rust types have a shape is ADR-0004. Neither traits nor Rust types appear here.

## Decision

A value is encoded by its shape alone. The bytes carry no type names, no field names, no field tags, and no version. Reading them requires knowing the shape.

| Shape | Bytes |
|---|---|
| Boolean | one byte, `0` or `1` |
| Integer, 8 / 16 / 32 / 64 bits, signed or unsigned | its declared width, little-endian, two's complement |
| Float, 32 / 64 bits | its IEEE 754 bits, little-endian |
| String | a count, then that many bytes of UTF-8 |
| Bytes | a count, then that many bytes |
| Option | one byte, `0` or `1`; then the value if `1` |
| Sequence | a count, then that many elements |
| Array of fixed length | the elements, with no count |
| Map | a count, then that many key and value pairs |
| Struct | each field in declared order |
| Tuple | each element in order |
| Enum | a selector, then the selected variant's fields in order |

A **count** and a **selector** are each an unsigned 32-bit integer, little-endian. A selector is the variant's declared number, not its position.

**One value, one encoding.** The same value always produces the same bytes, so bytes can be compared and hashed in place of values. Three rules make that hold:

1. Integers are fixed width. There is no variable-length form.
2. A map's pairs are written in ascending order of their encoded key bytes, compared byte by byte, and no key appears twice. This is an order over bytes, not over the keys' meaning: the 32-bit keys 1 and 256 encode as `01 00 00 00` and `00 01 00 00`, so 256 is written first.
3. A float is written bit for bit. Two NaNs with different bits are different bytes.

These rules bind every encoder in every language. A decoder refuses a map whose keys are out of order or repeated.

**A decoder refuses** a boolean or option byte other than `0` or `1`, a string that is not UTF-8, a selector that names no variant, input that ends early, and input with bytes left over after the value.

**A count is not trusted.** It is read from the input, so a decoder does not allocate from it directly. A decoder is given a budget of bytes it may allocate and refuses input that would exceed it.

## Consequences

- Any language can implement the layout from this page and a description of the shape.
- Alignment is not preserved: a 64-bit integer may start at any offset. A reader that wants to view bytes in place handles that itself.
- On a little-endian machine, a struct of fixed-width fields with no padding has the same bytes in memory as on the wire. An implementation may copy such a value in one step. That is an optimisation of this layout, never a second layout.
- Bytes do not say what they are. Whatever carries them also carries, or already knows, the shape.
- A changed shape is a different layout. Reading old bytes with a new shape is not defined here; it belongs to whatever stores bytes across versions.
- An encoder must sort map keys by their encoded bytes, which costs a sort per map on every encode.

## Alternatives considered

- **Variable-length integers.** Smaller for small numbers, but a value then has to be decoded to be skipped or compared, and every codec carries the extra rule.
- **Self-describing bytes (field names or tags inline).** Readable without the shape, and tolerant of change, at several times the size and with names repeated in every message. The schema of ADR-0003 gives tools the names without putting them in every value.
- **Map order by the keys' own meaning.** Natural in one language, and a rule each other language would have to reproduce per key type. Byte order needs no knowledge of the key.
- **An existing format (postcard, bincode, CBOR, MessagePack).** Each brings choices this layout does not want (variable-length integers, tags, or several encodings of one value), and a dependency in the lowest piece.
