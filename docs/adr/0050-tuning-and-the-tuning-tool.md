# ADR-0050: Tuning, and the tool that reads and sets it

- **Status:** Proposed
- **Date:** 2026-11-04

## Context

An agent tuning a look changes values while the program draws: a blade's facing, the far distance grass lies flat at, the fade of a layer. Ichor wrote its own pair of tools for it, `scene_settings` and `set_scene_setting`, over its own table of settings, each with its name, a description, what it takes (a number in a range, a switch, a choice), and its default. Each value is a hand-written `Value` the painter matches by name. The next program that wants its knobs reachable would write the same tools, table, and matching again, and a capability with knobs would write its own (`PaceTool`'s `set_pace` is one).

What is already decided:

- **A program's configuration is read once, as it is set up** (ADR-0028). Each root's section is a settings kind, declared with `#[settings]`, whose fields carry their doc comments (`Settings::FIELDS`, `Declared::doc`). ADR-0028 left "settings changed while the program runs, as mail" for later.
- **Every kind has a schema, and JSON is read and written through it** (ADR-0003, ADR-0007: `from_json`, `to_json`, `json_schema`). A field that does not exist or a value of the wrong shape is refused with its path (`JsonPath`).
- **A tool may take or answer a value whose schema is known only when it runs,** as a `Json` field (ADR-0038: `SchemaNode::Json`).
- **A program lists its tools by type in one root,** `Toolbox<L, R>`, and a tool asks a root at its typed path, which the program must list before it (ADR-0037, ADR-0046: `path::<Pacer>(())`, `NeedsIn`).
- **A tool can name any actor by its written path** and reach it as a protocol it handles (`Turn::resolve`, `Turn::cast`), as `actor_logs` names one (ADR-0044).

## Decision

**An actor's tuning is one settings kind it keeps, the values it takes while the program runs, read and replaced whole by two kinds of mail any actor may send. One tool, `TuningTool<L>`, lists the tuning of the roots `L` and sets any of its fields, by JSON checked against the kind's schema.**

### Tuning

```rust
/// An actor tuned while the program runs: one settings kind, read whole and set whole.
pub trait Tunable: Actor + Handles<ReadTuning> + Handles<Tune> {
    type Tuning: Settings;
    /// Its tuning as it stands.
    fn tuning(&self) -> Self::Tuning;
    /// Takes `tuning` in place of what it held, or refuses it, saying why: a value past what it can draw.
    fn tune(&mut self, tuning: Self::Tuning) -> Result<(), String>;
    /// This actor as one tuned: `me.protocol()`, as a tool's `worker` is.
    fn tunes(me: ActorRef<Self>) -> ProtocolRef<Tunes>;
}

pub struct Tunes;   // the protocol: ReadTuning answered with Tuning, Tune with Tuned

#[kind(name = "aether.tuning.read_tuning")]
pub struct ReadTuning;                                 // answered with Tuning
#[kind(name = "aether.tuning.tuning")]
pub struct Tuning { pub bytes: Bytes }                 // the actor's `Tuning`, as plain data

#[kind(name = "aether.tuning.tune")]
pub struct Tune { pub bytes: Bytes }                   // a whole `Tuning`, as plain data
#[kind(name = "aether.tuning.tuned")]
pub enum Tuned { Tuned(Tuning), Refused(String), Unread }
```

- **The tuning is a settings kind,** `#[settings]`, as a section is: each field's doc comment is what an agent reads of it, and the derive already keeps it. A range a number keeps to is said in its doc comment and held by `tune`, as Ichor's `Sort::Number` slider was.
- **It is set whole.** `Tune` carries a whole value, which the actor decodes as its `Tuning` (`Unread` when it is not one) and hands to `tune`. Its answer is the value as it now stands, so the setter sees what was taken. A field-by-field patch is the tool's, below; the actor sees only a typed value.
- **The handlers are a line each,** `tuning::read(self, turn)` and `tuning::tune(self, tune, turn)`, as no blanket impl of `Handles` may be written for every `Tunable` actor. They live in `aethereal-actor`, beside `Carries`, with the trait and the kinds, so a capability is tuned without depending on the tools, and a program's own actors (a settings page, a test) can read and tune it too.
- **Tuning is its own kind, apart from the actor's section.** "Settings" stay what the program is set up with, read once as it starts (what is baked, what a window opens at); tuning is what it takes while it draws. A field in both would say it can be tuned when it cannot. An actor whose tuning begins from its section copies it in its `begin`.

### `TuningTool<L>`

```rust
#[kind(name = "aether.tuning.list_tuning")]
pub struct ListTuning;
#[kind(name = "aether.tuning.tuning_list")]
pub struct TuningList { pub actors: Vec<ActorTuning> }
pub struct ActorTuning {
    pub actor: ActorPath,      // the root, as `actor_logs` names it
    pub values: Json,          // its `Tuning` as it stands
    pub schema: Json,          // its `Tuning`'s JSON Schema
    pub fields: Vec<About>,    // each field and its doc comment
}
pub struct About { pub name: String, pub doc: String }

#[kind(name = "aether.tuning.tune_fields")]
pub struct TuneFields { pub actor: ActorPath, pub values: Json }   // the fields to set
// answered with the FieldsSet: only the fields it set, as they then stand, or refused naming the field and why
#[kind(name = "aether.tuning.fields_set")]
pub struct FieldsSet { pub actor: ActorPath, pub values: Json }
```

- **`L` is a tuple of tunable roots,** `TuningTool<(Painter, Renderer)>`, listed in the program's `Toolbox` as any tool is. It reaches each through `Turn::found(&path::<A>(()))`, which answers an `ActorRef<A>` only when an `A` stands live at that path, and asks it through `Tunes`, so a program that lists a root it does not have does not build, and the tool knows each `A::Tuning`'s schema and fields from its type. **`Turn::found` is new here**: a typed lookup of a path, beside `resolve` and `cast`, as a tool asks through one protocol whatever type it lists, and a generic list cannot name each type's `Sends` apart.
- **`list_tuning` reads each root,** answering its values as JSON, the kind's JSON Schema (`json_schema`), and each field's doc comment, in the order `L` lists them.
- **`tune` sets only the fields it is given.** It reads the actor's tuning, lays `values` over it field by field, each field it names replaced whole, as ADR-0028 lays a layer over the defaults, decodes the result with `A::Tuning`'s schema, and sends it whole as `Tune`. A field the kind does not have, or a value of the wrong shape, is refused with its path before anything is sent. An actor `L` does not list is refused, naming the actors it does. It answers only the fields it set, as the actor then holds them, in the tuning's order, and the actor's refusal as the actor words it. It does not answer the whole tuning with its schema and every field's doc, as `list_tuning` does: an agent that sets one field would read the whole tuning back on every call, and what it set would be lost among the rest. `list_tuning` reads the whole.
- **The tool is in `aethereal-tools`,** beside `LogsTool`.

## Consequences

- **A program's knobs reach an agent by declaring one kind and two short handlers,** with no tool, table, or matching by name of its own. Ichor's painter keeps a `PainterTuning` and drops `scene_settings`, `set_scene_setting`, and its `Sort` table.
- **An agent learns each setting's meaning, shape, and current value in one call,** and sets several fields in one call, each checked by the schema it was read with.
- **A capability can be tuned whatever tools a program lists,** and later tools or a program's own page can use the same two kinds.
- **Tuning is a read, a merge, and a write,** each an ask, so two setters at once can each overwrite the field the other just set. One agent drives one engine, and a setter sees what was taken in each answer.
- **Only roots are listed.** An actor beneath another, an instance of a keyed type, is reached by a tool of its program's own until the tool can list them.
- **Follow-on work this leaves open:**
  - listing tunable instanced and child actors;
  - tuning kept across a restart, in the store (ADR-0039);
  - the program's configuration as it was read, listed beside its tuning, so an agent knows which flag a respawn would change;
  - existing capabilities' knobs moved to tuning, the pacer's pace first, where a tool of their own adds nothing.

## Alternatives considered

- **A registry root every tunable actor registers with as it begins.** It would list any actor, instanced or not, but every tunable capability would then require that root in every program, as a typed path needs its root listed first; a program with no tuning tool could not use the renderer.
- **Settings set one field at a time, by name, as Ichor's are.** Each actor would match names to fields by hand, which is what the schema already does, and several fields set together would be several asks, each drawn between.
- **The tuning is the actor's section, read once and then mail.** One kind for both, but a field set as the program starts and never again would read as tunable.
- **The tool sends the JSON overlay to the actor, which merges it.** It would close the race of two setters, but every tunable actor would carry the JSON machinery that today only tools and the host need.
- **Named live settings, `LiveSettings`.** "Settings" already names what a program is set up with, and "live" alone did not keep the two apart in speech; tuning says what is done with it.
