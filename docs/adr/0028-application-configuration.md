# ADR-0028: Application configuration

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

Nothing about a program is configured from outside it. Every knob is a literal in the code that sets the program up:
- the engine's sizes, `Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 20, 0)`, in every example;
- the workers;
- the time `Stop` is given (`Headless::run`, `Desktop`);
- the rate a headless cycle runs at;
- each window's title and size (`Screen::open_window`);
- a canvas's density (`canvas::listen`).

Changing any of them means changing code and building again. A person running the program cannot, and neither can a tool or a page that links to it.

A root is configured already, by its config kind, handed to its `begin` (ADR-0009). But those configs mix two things: data a person could type, and values the chassis makes and hands in. `RendererConfig` holds a `Given<Gpu>`, a `Given<Front>`, and a `Vec<WindowFormat>` side by side (ADR-0021).

The earlier engine configured itself through about fifty environment variables, then layered them (its ADR-0090). Four later records patched what that shape cost:
- **Ambient, string-keyed names.** A misspelt `export` was indistinguishable from a working one, so every boot swept the environment for unknown names. A forked child inherited its parent's knobs, so the hub kept a hand list of every child's knobs, and at last cleared the environment at every fork (its ADR-0162).
- **Config and wiring in one struct,** until a wide migration split the two (its ADR-0156).
- **A different machine for each channel:** one crate for the environment and file, another for the command line, hand-written command-line roots kept in step with them by tests, and records for the knobs that fitted neither.
- **A grammar per field** (`windowed:WxH`), each with its own parser, as the stored form.

What this engine already has:
- A kind has a schema, and JSON produced and read through it, with no Rust type in the loop (ADR-0003, ADR-0007: `to_json`, `from_json`). A field that does not exist and a value of the wrong shape are refused there, with the path to them (`JsonPath`).
- A root's config is a kind (ADR-0009).

## Decision

**A program's configuration is plain data, a kind, and it is only what a person could type.**

- Configuration is read once, as the program is set up. Changing a setting while the program runs is mail to the actor that owns it, not configuration.
- A value the host makes is never configuration: a device, a source, a controller, a feed, a `Given`. The chassis makes it, and builds each root's config from the configuration and the values it made. **So a root's config and the program's configuration are two things**: the first may hold handles, the second never does. This is the line the earlier engine drew late, drawn first.

**The configuration is the composite of its roots' sections, and the host's beside them.**

```json
{
  "engine":   { "workers": 4, "parts": 4, "read_limit": 1024, "mail_bytes": 1048576, "blob_bytes": 1048576, "hops": 0 },
  "running":  { "every_millis": 16, "stop_within_millis": 1000 },
  "windows":  [{ "title": "aethereal", "size": [800, 600] }],
  "game":     { "difficulty": "hard" }
}
```

- **Each root that has settings brings a section,** the windows, each capability, and the program's own roots alike, and the host brings its own: the engine and the way the program is run. A section belongs to what reads it; how a root reads its section as it is stood up is ADR-0029.
  - A section is a config kind, under a short name its root's bootstrap declares (`windows`, `game`); the host's are `engine` and `running`.
  - Two roots that declare one name are refused as the program is put together, naming both.
- **A section's defaults are the program's, given where the section is listed, or else its kind's `Default`.** The host's sections take theirs with the engine's other options, `.with_engine(EngineSettings { workers, ..Default::default() })` and `.with_running(..)`; a root's take theirs where it is listed, `.root_with::<Counter>(Counters { children, idle, shape })`. A root listed with `.root::<A>()` starts from its kind's `Default`. A kind with no honest default leaves `Default` off, and every program that lists its root gives a value with `root_with`. Each is a whole typed value, a struct literal checked when the program is built.
- **The configuration's schema is the composite**, so it says exactly what this program, on this host, can be configured with: no more, no less. A host that composes no windows has no `windows`.

**Layers are overlays on one document, decoded once.**

| host | layers, the first winning |
|---|---|
| native | the command line, the environment, a file (`--config path`), the program's defaults, the kinds' `Default`s |
| browser | the page (a JSON object, or the page's URL query), the program's defaults, the kinds' `Default`s |

- **The program's defaults are the bottom layer, above only the kinds' `Default`s.** Defaults sit with what they are for, the engine's beside its options and a root's beside the root, and code never overrides the person running the program: what someone types, exports, or writes in the file always wins over what the program was built with.
- Each layer is an overlay: the fields it sets, at their paths. Overlays are merged field by field: a structure by its fields, a sequence whole or one element by its index. The result is decoded once, with the composite's schema.
- **A field that does not exist, or a value of the wrong shape, is an error that stops the program as it is set up**, naming the field's path and the layer that set it. **This departs from the earlier engine, which warned on an unknown name and went on**: a setting that does nothing is the failure configuration exists to prevent, and one decode over one schema leaves no reason to guess.
- The file is JSON, in the form ADR-0007 gives every kind. A field that holds a time or a size of memory spells its unit in its name (`stop_within_millis`, `mail_bytes`), as every name in this repository does, and holds a number.

**People use flags. The flags are a layer in front of the document, never the document.**

```
--workers 4  -w 4  --size 1280x720  --fullscreen  --stop-within-millis 500  --present capped:60  --set renderer.present=uncapped
```

- **A section's author suggests the flags for its fields**, beside the field, as they declare the field's doc comment. **The program decides**: it takes a suggestion, renames it, hides it, or adds a flag of its own for any field of any section.
  ```rust
  #[derive(Plain, Default)] #[kind(name = "aether.engine.settings")]
  pub struct EngineSettings {
      /// The threads that run a step.
      #[flag(long = "workers", short = 'w')]
      pub workers: NonZeroUsize,
      ..
  }

  Flags::of(&parts)
      .rename(EngineSettings::WORKERS, "threads")
      .flag("size", 's', WindowsSettings::FIRST_SIZE)
  ```
- **A field is named by a handle the derive emits**, `EngineSettings::WORKERS: Field<EngineSettings, NonZeroUsize>`, beside the schema it already emits. So a program's flags name fields that exist, checked when it is built.
- **A field of one element of a sequence is named by a handle made from its element's,** `WindowSettings::SIZE.in_element(0)`, which names `windows[0].size`. The section is then the sequence itself, `WindowsSettings(Vec<WindowSettings>)`, written as the sequence alone, so its path names it once; the derive marks such settings, and settings of named fields, so the element's type and the sequence's are checked when the program is built. The desktop's windows give it as `WindowsSettings::FIRST_SIZE`, and a program offers it as `--size`. An element the section does not have refuses what a layer sets there, naming its path, as any index past the end is.
- **Two flags of one name, left so by the program, are an error as the program is put together**, naming both fields. Neither wins.
- **A field's type picks how its flag's value is read:**
  - a size is `1280x720`;
  - an enum is its variant's name in kebab case, and a variant of one field its name, a colon, and the field's value read by the field's form: `screen`, `at-most:2`, `for:3`. A variant of several fields is set by JSON;
  - a `bool` is a switch, and `--no-<name>` turns it off;
  - a number is a number, and a string is itself.

  Each of these is written once and shared. They exist only on the command line: what a flag sets is the typed value, in the document.
- **Every field is reachable without a flag**, by `--set <path>=<value>`, its value read as the flag of that field's type would read it, and JSON for a structure.
- **`--help` lists the flags with each field's doc comment**, and `--describe` prints the composite's schema with its defaults, both made from the declarations, so nothing is kept in step by hand. The parser is `clap`, the one Rust programs' users already know: short and long flags, `--flag=value` and `--flag value`, subcommands, and the nearest flag suggested for a typo.
- **The program's own arguments**, a file to open or a subcommand, are fields of the section of the root that reads them, and flags or positionals like any other.

**The environment is a layer that reads the same flags.**

- Each flag's long name, under the program's own prefix, is an environment variable: `--workers` is `<PROGRAM>_WORKERS`, in capitals, with `-` written `_`. A program named `mygame` reads `MYGAME_WORKERS`.
- **A variable with the program's prefix that names no flag is an error**, as an unknown field is.
- **A host that starts another process does not pass the program's variables on.** Configuration meant for a child is given on its command line. The earlier engine came to this after inheriting (its ADR-0162); here it is the rule from the first child.
- A page has no environment, so the browser has no such layer.

**In the browser, the page is the command line.**

- A page hands the chassis a JSON object, or its URL query, read with the same flags: `?workers=4&size=1280x720`. A link to a program set up a certain way reads as its command line would.
- The browser bootstraps the roots it can. A field of a section it does not compose, such as a native window's size, is not in its schema, and a page that sets one is refused as any unknown field is.

**Where it lives.**

- The overlays, their merge, and the decode are in `aethereal-data`, beside the JSON they work on, behind its `json` feature.
- The `Field` handles and `#[flag]` are emitted by the `Plain` and `Bound` derives.
- The flags, the command line, the environment, and the file are the native host's, in `aethereal-chassis-headless`, which the desktop chassis builds on.
- The page's layer is the browser chassis's.

## Consequences

- **A person, a script, or a link configures a program without building it again**, in the form tools already use. And a program can say exactly what it can be configured with.
- The literals in the examples and the site's demos become defaults, or settings of the program's own roots.
- **Each root builds its config from its section and what the host made, as it is stood up** (ADR-0029). The configuration is read first, then the roots.
- Each root's settings are a kind, declared beside the root that reads them, as the earlier engine's were; there is no central list. It has a `Default` when it has an honest one, and otherwise each program gives its defaults.
- A flag's value forms are a small fixed set, and a setting that needs another form is set by JSON through `--set` or the file until one is added.
- `clap` is a dependency of the native host, and nothing else's.
- **Left for later:**
  - settings changed while the program runs, as mail;
  - secrets, which are never a flag, since a command line is read by anyone on the machine, and need a record of their own;
  - a configuration in a format other than JSON.

## Alternatives considered

- **A third-party configuration crate**, as the earlier engine used. Its own derive, its own idea of a field, and a second machine beside the schema every kind already has. One decode over the schema validates, merges, and describes with what exists.
- **Flags declared only by the sections' authors.** They would choose names for every program that uses them. Two sections that each want `--size` could not both be used, and a program could not rename either.
- **Flags declared only by the program.** Every program would write the same table again, and a section's settings would have no flag until someone mapped them.
- **Dotted paths as the command line** (`--engine.workers=4`). Exact, and what `--set` is, but no one types it. A command line should read like any other tool's.
- **No environment layer.** It would remove the class of problems the earlier engine patched four times. But containers, CI, and shells configure by environment, and names derived from the flags, under one prefix, refused when unknown, and never passed to a child, keep its convenience without a second registry of names.
- **Settings set in code, above every layer**, field by field through the flags' handles (`.set_engine(EngineSettings::WORKERS.to(w))`). A value the program fixes would win over what a person typed, so a flag would do nothing for that field, which is the failure this record exists to prevent; and a section written one field at a time through handles is verbose where a struct literal says it whole. A value a program means to fix is its own, not a setting.
- **Warning on an unknown setting**, as the earlier engine did. It keeps a program running with a setting that does nothing, which is the failure this record exists to prevent.
