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
2. A map's pairs are written in ascending key order, and no key appears twice. Only the shapes below may be a key, and each has one stated order.
3. A float is written bit for bit. Two NaNs with different bits are different bytes.

**Map keys.** A key is one of these shapes, ordered as stated:

| Key shape | Ascending order |
|---|---|
| Integer, any width, signed or unsigned | by numeric value |
| String | by its UTF-8 bytes, compared byte by byte; a string that is a prefix of another comes first |
| Bytes | byte by byte; a prefix comes first |
| Array of 8-bit unsigned integers | byte by byte |
| Struct with exactly one field, itself a key shape | as that field |

No other shape is a key. Adding one is an amendment to this table that states its order.

The order is the one each language already has for the value, with one trap: ordering a string by UTF-8 bytes is ordering by code point, and a language whose strings are UTF-16 (JavaScript, Java, C#) compares by code unit by default, which differs for characters outside the Basic Multilingual Plane. An encoder in such a language compares by code point.

These rules bind every encoder in every language. A decoder refuses a map whose keys are out of order or repeated.

**A decoder refuses** a boolean or option byte other than `0` or `1`, a string that is not UTF-8, a selector that names no variant, input that ends early, and input with bytes left over after the value.

**A count is not trusted.** It is read from the input, so a decoder does not allocate from it directly. A decoder is given a budget of bytes it may allocate and refuses input that would exceed it.

## Consequences

- Any language can implement the layout from this page and a description of the shape.
- Alignment is not preserved: a 64-bit integer may start at any offset. A reader that wants to view bytes in place handles that itself.
- On a little-endian machine, a struct of fixed-width fields with no padding has the same bytes in memory as on the wire. An implementation may copy such a value in one step. That is an optimisation of this layout, never a second layout.
- Bytes do not say what they are. Whatever carries them also carries, or already knows, the shape.
- A changed shape is a different layout. Reading old bytes with a new shape is not defined here; it belongs to whatever stores bytes across versions.
- A map held in key order in memory is written without sorting. A map held in another order is sorted on encode.
- A float, a boolean, an option, a sequence, a tuple, an enum, and a struct of several fields cannot be a map key until this ADR is amended to give each an order.

## Alternatives considered

- **Variable-length integers.** Smaller for small numbers, but a value then has to be decoded to be skipped or compared, and every codec carries the extra rule.
- **Self-describing bytes (field names or tags inline).** Readable without the shape, and tolerant of change, at several times the size and with names repeated in every message. The schema of ADR-0003 gives tools the names without putting them in every value.
- **Any shape as a key, ordered by its encoded bytes.** One rule for every key with no list to maintain. The order is unnatural (the 32-bit key 256 sorts before 1, a longer string after a shorter one whatever their text), so no language's ordered map holds it and every encoder sorts every map. A short list of keys with their natural order is checkable by reading it.
- **An existing format (postcard, bincode, CBOR, MessagePack).** Each brings choices this layout does not want (variable-length integers, tags, or several encodings of one value), and a dependency in the lowest piece.
