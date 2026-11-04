# ADR-0050: Live settings, and the tool that reads and sets them

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

**An actor's live settings are one settings kind it keeps, read and replaced whole by two kinds of mail any actor may send. One tool, `SettingsTool<L>`, lists the live settings of the roots `L` and sets any of their fields, by JSON checked against the kind's schema.**

### Live settings

```rust
/// An actor whose settings change while the program runs: one settings kind, read whole and set whole.
pub trait LiveSettings: Actor + Handles<ReadLive> + Handles<SetLive> {
    type Live: Settings;
    /// Its live settings as they stand.
    fn live(&self) -> Self::Live;
    /// Takes `live` in place of what it held, or refuses it, saying why: a value past what it can draw.
    fn set_live(&mut self, live: Self::Live) -> Result<(), String>;
}

#[kind(name = "aether.settings.read_live")]
pub struct ReadLive;                                   // answered with Live
#[kind(name = "aether.settings.live")]
pub struct Live { pub bytes: Bytes }                   // the actor's `Live`, as plain data

#[kind(name = "aether.settings.set_live")]
pub struct SetLive { pub bytes: Bytes }                // a whole `Live`, as plain data
#[kind(name = "aether.settings.live_set")]
pub enum LiveSet { Set(Live), Refused(String), Unread }
```

- **The live settings are a settings kind,** `#[settings]`, as a section is: each field's doc comment is what an agent reads of it, and the derive already keeps it. A range a number keeps to is said in its doc comment and held by `set_live`, as Ichor's `Sort::Number` slider was.
- **They are set whole.** `SetLive` carries a whole value, which the actor decodes as its `Live` (`Unread` when it is not one) and hands to `set_live`. Its answer is the value as it now stands, so the setter sees what was taken. A field-by-field patch is the tool's, below; the actor sees only a typed value.
- **The handlers are a line each,** `live::read(self, turn)` and `live::set(self, set, turn)`, as no blanket impl of `Handles` may be written for every `LiveSettings` actor. They live in `aethereal-actor`, beside `Carries`, with the trait and the kinds, so a capability gives live settings without depending on the tools, and a program's own actors (a settings page, a test) can read and set them too.
- **Live settings are their own kind, apart from the actor's section.** A value read once as the program starts (what is baked, what a window opens at) and one taken while it draws differ, and a field in both would say it can be changed live when it cannot. An actor whose live settings begin from its section copies them in its `begin`.

### `SettingsTool<L>`

```rust
#[kind(name = "aether.settings.list_settings")]
pub struct ListSettings;
#[kind(name = "aether.settings.listed")]
pub struct Listed { pub actors: Vec<Tunable> }
pub struct Tunable {
    pub actor: ActorPath,      // the root, as `actor_logs` names it
    pub values: Json,          // its `Live` as it stands
    pub schema: Json,          // its `Live`'s JSON Schema
    pub fields: Vec<About>,    // each field and its doc comment
}
pub struct About { pub name: String, pub doc: String }

#[kind(name = "aether.settings.set_settings")]
pub struct SetSettings { pub actor: ActorPath, pub values: Json }   // the fields to set
// answered with LiveSet, or refused naming the field and why
```

- **`L` is a tuple of roots that give live settings,** `SettingsTool<(Painter, Renderer)>`, listed in the program's `Toolbox` as any tool is. It asks each at `path::<A>(())`, so a program that lists a root it does not have does not build, and the tool knows each `A::Live`'s schema and fields from its type.
- **`list_settings` reads each root,** answering its values as JSON, the kind's JSON Schema (`json_schema`), and each field's doc comment, in the order `L` lists them.
- **`set_settings` sets only the fields it is given.** It reads the actor's value, lays `values` over it field by field, as ADR-0028 lays a layer over the defaults, decodes the result with `A::Live`'s schema, and sends it whole as `SetLive`. A field the kind does not have, or a value of the wrong shape, is refused with its path before anything is sent. An actor `L` does not list is refused, naming the actors it does.
- **The tool is in `aethereal-tools`,** beside `LogsTool`.

## Consequences

- **A program's knobs reach an agent by declaring one kind and two short handlers,** with no tool, table, or matching by name of its own. Ichor's painter keeps a `PainterLive` and drops `scene_settings`, `set_scene_setting`, and its `Sort` table.
- **An agent learns each setting's meaning, shape, and current value in one call,** and sets several fields in one call, each checked by the schema it was read with.
- **A capability can give live settings whatever tools a program lists,** and later tools or a program's own page can use the same two kinds.
- **Setting is a read, a merge, and a write,** each an ask, so two setters at once can each overwrite the field the other just set. One agent drives one engine, and a setter sees what was taken in each answer.
- **Only roots are listed.** An actor beneath another, an instance of a keyed type, is reached by a tool of its program's own until the tool can list them.
- **Follow-on work this leaves open:**
  - listing instanced and child actors that give live settings;
  - live settings kept across a restart, in the store (ADR-0039);
  - the program's configuration as it was read, listed beside the live settings, so an agent knows which flag a respawn would change;
  - existing capabilities' knobs moved to live settings, the pacer's pace first, where a tool of their own adds nothing.

## Alternatives considered

- **A registry root every actor with live settings registers with as it begins.** It would list any actor, instanced or not, but every capability with live settings would then require that root in every program, as a typed path needs its root listed first; a program with no settings tool could not use the renderer.
- **Settings set one field at a time, by name, as Ichor's are.** Each actor would match names to fields by hand, which is what the schema already does, and several fields set together would be several asks, each drawn between.
- **The live settings are the actor's section, read once and then mail.** One kind for both, but a field set as the program starts and never again would read as live.
- **The tool sends the JSON overlay to the actor, which merges it.** It would close the race of two setters, but every actor with live settings would carry the JSON machinery that today only tools and the host need.
