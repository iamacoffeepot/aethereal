# ADR-0007: Plain data as JSON

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Plain data travels as the bytes of ADR-0002, which nobody reads or writes by hand. A person at a console, a tool, and a script in a browser all want the same value as text they can read, build, and send back. ADR-0004 gave that job to a walk over the schema (ADR-0003, `SchemaNodes`) and ruled out a second trait on every type.

JSON is the text. Its readers are not all alike, and one difference decides part of this ADR: JavaScript's `JSON.parse`, and every parser built like it, reads each number as a 64-bit float. An integer above 2^53 comes back as a different integer, with no error. A browser host puts such a parser beside the engine by definition, and identifiers computed by hashing are 64 bits wide and almost always above 2^53.

JSON is a view of plain data for people and tools. Mail between actors and engines stays bytes.

## Decision

**JSON is produced from bytes and a schema, and bytes from JSON and a schema.** Neither direction needs the Rust type.

```rust
pub fn to_json(schema: &SchemaNodes, bytes: &[u8], budget: &mut Budget) -> Result<String, JsonError> { .. }
pub fn from_json(schema: &SchemaNodes, json: &str, budget: &mut Budget) -> Result<Vec<u8>, JsonError> { .. }
```

Each takes a budget, because each is handed input it does not trust. ADR-0002's rule that a count is not trusted holds for text as it does for values: a sequence of elements that occupy no bytes can state any count, so `to_json` charges the text it writes, and four bytes of input cannot make it write gigabytes. `from_json` charges what it holds while reading.

Both live in `aethereal-data`, in a `json` module behind the `json` feature, so a program that never speaks JSON does not carry it. The module reads and writes JSON text itself and depends on no JSON library.

**Each shape has one JSON form.**

| Shape | JSON | The reader also refuses |
|---|---|---|
| Boolean | `true` or `false` | |
| Integer of 8, 16, or 32 bits | a number | a fraction, an exponent, a value out of range |
| Integer of 64 bits | a string of decimal digits, with a leading `-` when negative | a number; a leading `+`, a leading zero, a value out of range |
| Float | a number; or the string `"NaN"`, `"Infinity"`, or `"-Infinity"` | any other string |
| String | a string | |
| Bytes | a string of lowercase hexadecimal, two digits to a byte | an odd length, an uppercase digit |
| Option | `null`, or the value | |
| Sequence | an array | |
| Array | an array of exactly its length; an array of 8-bit unsigned integers is a hexadecimal string, as Bytes is | a wrong length |
| Map | an object | a repeated key |
| Struct | an object keyed by field name | an unknown field, a repeated field, a missing field |
| Tuple | an array of exactly its elements | a wrong length |
| Enum | the variant's name as a string when it has no fields; otherwise an object with one member, the variant's name, holding its fields | an unknown variant |

**A 64-bit integer is always a string.** It is a string whatever its value, so a field does not change its JSON type when it grows past 2^53, and a reader that received a number has a parser that may already have changed it. A JavaScript reader passes the string to `BigInt`, or hands it back untouched.

```rust
#[derive(Plain)]
struct Tick { frame: u64, delta_millis: u32 }
```

```json
{"frame":"18446744073709551615","delta_millis":16}
```

**A float is written as the shortest decimal that reads back as the same bits, with no exponent.** A number is read as the nearest float, and one whose nearest float is not finite is refused. JSON has no spelling for a float that is not finite, so those three are strings. A NaN is written `"NaN"` whatever its bits and read back as the one quiet NaN, which is the single place this mapping does not return the bytes it was given.

**Fields that are named by position are written without their names.** ADR-0003 names an unnamed field by its position, `"0"`, `"1"`. A struct or variant with exactly one such field is written as that field's value alone, and with several, as an array of them.

```rust
struct PlayerId(u64);                       // "42"
struct Span(u32, u32);                      // [3, 9]
enum Shape { Dot, Circle(f32), Box { w: f32, h: f32 } }
                                            // "Dot"   {"Circle":1.5}   {"Box":{"w":2,"h":3}}
```

A struct with no fields is `{}`.

**A map's keys are written as strings,** since a JSON object allows no other. An integer key is its decimal digits, a string key is itself, a bytes or byte-array key is hexadecimal, and a struct of one field is that field's key.

**An option whose value could itself be written `null` is written as `null` or a one-element array.** `null` would otherwise stand for both "no value" and "a value that is none". This covers an option of an option, and an option of a struct that is written as its single field when that field is an option. No other option takes this form.

**A missing field is read as none when its shape is an option,** and is refused otherwise. This is the one leniency: a person writing JSON by hand leaves out what they do not set.

**`to_json` refuses bytes that are not a value of the schema,** by every rule a decoder of ADR-0002 has: a boolean byte that is neither `0` nor `1`, a selector that names no variant, map keys out of order, bytes left over. What it writes is therefore always text its own reader accepts.

**The writer has one output for one value.** It writes no whitespace, a struct's fields in declared order, and a map's members in the key order of ADR-0002. A string escapes only `"`, `\`, and the control characters below `U+0020`.

**The reader accepts any JSON text of RFC 8259 and refuses** a repeated member name in any object, text after the value, and anything the table lists. It reads object members in whatever order they come and writes bytes in declared order. A map's members are sorted into key order. It does not recurse on its input. The number `-0` is zero where an integer is expected.

**Both directions follow the schema, to a depth of at most 128.** A schema can arrive as bytes, so its nesting is not trusted either, and one nested more deeply is refused.

**A refusal says where.** `JsonError` carries the path to the place in the value it concerns, such as `players[3].id`, beside what was wrong there. A refusal of the text itself, before it is any value, carries its offset in the text.

## Consequences

- A tool needs a schema and these rules to show or build any plain value. It needs no Rust type and no code generated from one.
- Bytes written by `from_json` are valid for the schema's shape. Rules a Rust type adds beyond its shape (ADR-0004's table: a `char` that is a scalar value, a `NonZero` that is not zero) are checked when the bytes are decoded as that type, and are not known here.
- The JSON form follows the names in a schema, so renaming a field changes the JSON and leaves the bytes alone.
- Bytes in hexadecimal are twice their length. Values large enough for that to matter do not belong in JSON.
- A 64-bit identifier appears as a string of digits. A form that also says what sort of identifier it is can be layered on by a tool that knows the type's name from the schema.
- Field docs and units are not in a schema (ADR-0003) and so are not in the JSON.

## Alternatives considered

- **64-bit integers as JSON numbers.** Correct JSON, and silently wrong after any parser that reads numbers as floats.
- **A number when it fits in 53 bits, a string otherwise.** Shorter for small values, and a field's JSON type would then depend on its value.
- **A JSON library for the text.** Less code here. The widely used one brings a serialisation framework into the workspace, which ADR-0004 keeps out, and reads numbers through a float unless told otherwise.
- **Base64 for bytes.** A third shorter. It has several alphabets and padding rules, and a hash shown in it is not the hash anyone recognises.
- **An enum as an object with a tag member beside its fields.** Flatter, and the tag's name can collide with a field's.
- **Every option wrapped, so nesting needs no special case.** Uniform, and every optional field in every value would pay for a case that almost never occurs.
- **Positional fields as an object keyed `"0"`, `"1"`.** One rule for every struct, and a wrapper around a single value would be written as an object.
