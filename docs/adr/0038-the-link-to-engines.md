# ADR-0038: The link to engines

- **Status:** Proposed
- **Date:** 2026-11-02

## Context

ADR-0037 put an MCP server inside a program, whose tools reach that program's own actors, and left "the link to engines" to a later ADR. An agent drives a hub, and the work it wants done is in engines: other processes, running other programs, built with kinds the hub was not built with. This ADR is the second slice of the hub stack: how an engine's tools reach the agent through its hub. Starting, stopping, and listing engines' processes is the next slice.

What is already decided:

- **The engine dials its hub** (ADR-0033's context). The hub listens, and the link reads `LengthPrefixed` frames (ADR-0033: `Framing::LengthPrefixed`, `Connect`, `Listen`, `Reading`, `Writing`).
- **What crosses to a tool is a schema and bytes** (ADR-0037: `Tooling`, `Call`, `Called`, `ToolSheet`). `Tooling` was made the seam for "the link's proxy for an engine" (ADR-0037).
- **A tool is a concrete actor in the crate it belongs to, and a program lists its tools** (ADR-0037: `Tool`, `Toolbox`, `Offer`). In an engine, `path::<Mcp>` holds nothing, so a toolbox there has nothing to offer to (ADR-0037, consequences).
- **A client reads the tool list when it chooses.** MCP `2026-07-28` with whole HTTP responses has no `listChanged` (ADR-0037), and clients commonly read `tools/list` once.
- **A held request is answered `No(Ended)` when its holder ends** (ADR-0017), and **a watched actor's end is told to its watcher** (`turn.watch`, `Ended`, as `Mcp` watches its workers in ADR-0037).

The earlier engine linked its hub by a general wire: any mail to any actor path, with a kind's id and bytes, path resolution on the far side, and an error vocabulary for each way that could fail. It also moved its tunnel and protocol crates back and forth four times.

## Decision

**An engine offers tools to its root `Tools`, and its root `Link` dials the hub and hands the hub those tools. The hub's `Engines` root keeps one actor for each engine linked, and serves the agent three fixed tools that list engines, describe an engine's tools, and call one. Only tools cross the link.**

```
engine                                                                 hub
Toolbox<L, Tools> ──Offer──> Tools ──Sheets──> Link ══ Hello, Sheets ══> Engines ── Engine:n (its sheets)
                                                                          ▲
tool <──Call── Tools <──CallTool── Link <══ Call { call, tool, input } ══ Engine:n <── CallTool <── Mcp <── agent
     ──Called─>     ──answered───>      ══ Called { call, called } ══>                (tools/call "call_tool")
```

### The tools vocabulary moves to its own crate

`Tooling`, `Call`, `Called`, `ToolSheet`, `Offer`, `Offered`, `Withdraw`, `Tool`, `ToolList`, and `Toolbox` move from `aethereal-mcp` to a new crate, `aethereal-tools`, which has no HTTP and no TCP. So does the checking of an offer and the table of what is held (`Table`): the name rules, `Taken`, `Full`, `NotAnObject`, and `Unwritable`, which needs only `aethereal-data`'s JSON. `aethereal-mcp` depends on it, and so does a capability's `tools` feature, which no longer brings in HTTP.

**A toolbox names the root it offers to:**

```rust
// aethereal-tools
pub trait Holder: Actor<Key = ()> + ChildOf<Parent = Top> + Carries<Offer, Answers = (Offered,)> + Handles<Withdraw> {}

pub struct Toolbox<L, H> { /* .. */ }   // Asks<Path<H>, Offer, Offering>; Tells<Path<H>, Withdraw> as it ends

// a hub                                        // an engine
.root::<Mcp>()                                  .root::<Tools>()
.root::<Toolbox<(ReadTool,), Mcp>>()            .root::<Toolbox<(ReadTool, SceneTool), Tools>>()
                                                .root::<Link>()
```

- `Mcp` is the hub's `Holder`, and `Tools` an engine's: a root every engine lists, linked or not (ADR-0053). `Link` is no `Holder`. A tool does not change: `ReadTool` is listed in a hub or an engine the same way.
- The build check of ADR-0037 holds for either: a toolbox listed before its holder, or with none, does not build.

### The engine's `Link`

```rust
// aethereal-link
#[settings]
pub struct LinkSettings {
    pub hub: String,                 // the hub's host
    pub port: u16,                   // the hub's link port
    pub name: String,                // what the hub shows the agent; not an identity
    pub token: Vec<u8>,              // `--hub-token`, the token the hub minted for this engine; required (ADR-0040)
    pub most_bytes: u32,             // a frame's limit, each way
}
```

- **`Link` holds no tools; `Tools` does** (ADR-0053). `Tools` checks each offer with the shared `Table`, answers `Offered` in the turn that asks, and watches each worker, as `Mcp` does. It keeps the table, its revision, and the setting `most_tools`. A tool's offer never waits on the hub.
- **`Link` dials once, in `begin`** (`Connect` with `LengthPrefixed { most_bytes }`). Once open, it writes `Hello`, then `Sheets` with every tool `Tools` holds: it subscribes to `Tools`, and writes each `Sheets` it is sent, with `Tools`'s revision. After each offer, withdrawal, or worker's end, `Tools` sends the whole set again, never a change, so the hub never holds a set that missed one. A set is bounded by the table's `most_tools`.
- **Each set has a revision,** a count `Tools` keeps and raises with every change to what it holds. A `Call` names the revision of the set the hub converted it with, and `Tools` refuses a call of a revision it has changed since, saying the tools have changed, without asking the worker. So a tool replaced by one of another schema is never called with bytes written for the first, even where the new schema would read them.
- **A call is asked of `Tools`** as `CallTool`, with the call's revision, within the call's time (ADR-0041), keyed by the call's number; `Tools` asks the worker that offered the tool (`Asks<Tooling, Call, _>`), and its answer is written back as `Called`. A worker that ends first, or a call not answered in time, is answered `No`, which `Link` writes as refused. A `Called` that would not fit in a frame is written as refused saying so, so an answer too large refuses the call and ends nothing.
- **`Link` ends its program when its connection ends,** when its dial is refused, or when the hub refuses its `Hello` (ADR-0040: `turn.quit`). Its token is spent, so it could never link again, and no engine outlives its link.

### The hub's `Engines`

```
Engines              root, asked at path::<Engines>(()), listens on the link port
 └─ Engine:n         one engine linked: keeps its connection, name, and sheets; keyed by the root's count
```

- **`Engines` listens** (`Listen { port, framing: LengthPrefixed }`, loopback by default, as ADR-0033's listener) and, for each connection accepted, spawns an `Engine` with it. Its key `n` is the engine's id for the agent. A name is shown beside it and is not checked: two engines may share one.
- **An `Engine` reads `Hello` first.** A revision the hub does not speak is written `Refused { revision }`, and the engine shuts its connection. It then keeps the latest `Sheets`, as it comes.
- **A call is asked of the `Engine`** (`Carries<CallEngine>`), which holds it, writes `Call` with a number of its own, and answers the held request when `Called` comes back. Past its setting `most_calls`, it refuses at once.
- **An engine's end is its connection's end.** When its reader's stream ends (`HungUp`), the `Engine` ends, the engine tells each held call's asker `No(Ended)` (ADR-0017), and `Engines`, which watches each, drops it from its list.

### The frames

Each frame is one plain kind, its bytes in a `LengthPrefixed` frame. The two directions are two closed enums:

```rust
pub enum Up {                                        // engine to hub
    Hello { revision: u32, name: String, token: Token },   // `token`: the one the hub minted for it (ADR-0040)
    Sheets { revision: u64, tools: Vec<ToolSheet> },  // each sheet with its `Output` (ADR-0043)
    Called { call: u64, called: Outcome },
}

pub enum Outcome { Output(Vec<u8>), Image { png: Vec<u8>, text: String }, Refused(String) }   // a `Called`, framed

pub enum Down {                                      // hub to engine
    Refused(Refusal),                                // Revision { revision }, the one the hub speaks, or Token (ADR-0040)
    Call { call: u64, revision: u64, tool: String, input: Vec<u8>, within: Within },
}                                                    // `revision`: the set it was converted with; `within`: the time the call has left (ADR-0041)
```

- **The link speaks only `Tooling`.** Anything else an engine shows an agent (its logs, its actors, a frame it drew) is a tool, written in the crate it belongs to, and crosses as one.
- **A frame that does not decode ends the connection.** On the hub, that ends the `Engine`; on the engine, it ends the link, and `Link` ends its program. A frame past `most_bytes` ends it the same way (ADR-0033: `HungUp::TooLarge`), which the writer's own check keeps from happening between a hub and engine that agree on the limit.
- **The revision is `3`.** A change to `Up` or `Down` that an older side would misread is the next revision.

### The agent's three tools

The engines' tools are not offered to `Mcp` one by one. The hub's tool list does not change as engines come and go, so a client that read it once has every tool it needs:

```rust
// aethereal-link, behind its `tools` feature; a hub lists them in its toolbox
list_engines {}                                          -> { engines: [{ engine: u32, name: String, tools: u32 }] }
describe_engine { engine: u32, show: "Names" }           -> { tools: { Names: [{ name, description }] } }
describe_engine { engine: u32, show: { Sheets: [String] } }
                                                         -> { tools: { Sheets: [{ name, description, input: Json, output: Json }] } }
call_tool { engine: u32, tool: String, input: Json }     -> { output: Json }, or the image and text the tool answered (ADR-0043: `ValueOrImage`)
```

- **An engine's id is a `u32`,** as `Engines` counts them, and an engine's key is that id. A 64-bit integer's JSON form is a string (ADR-0007), and an agent writes the `3` it was listed, not `"3"`. Once every id is given, `Engines` shuts each new connection.

- **`describe_engine` writes each tool's input and output as JSON Schema**, as `tools/list` does for the hub's own (ADR-0037).
- **`describe_engine` shows names first, and sheets only when asked.** `show` is required. `"Names"` lists every tool by its name and the first sentence of its description: up to and including the first `.` that whitespace or the end of the text follows, or the whole text when there is none. `{"Sheets": [...]}` answers the full sheets of the tools it names, in the order the engine lists them, and refuses a name the engine has no tool of, as `call_tool` does. An agent asks for names, then for the sheets of the tools it will call: an agent that was answered every schema spent 6–8k tokens a call on 23 tools' schemas. The answer is one object whose `tools` is either form, so its output schema stays an object, as MCP asks of one.
- **`call_tool` turns `input` into bytes of the tool's input schema** (`from_json`, ADR-0007), with the sheet the `Engine` keeps, and the output bytes back into JSON (`to_json`). Input that does not fit the schema is refused before anything is sent. So the engine receives only bytes its schema reads, and the hub needs no kind of the engine's.
- **Tools come and go while an engine runs, and the agent sees them as data.** `call_tool` finds its tool in the latest set when it is called, so one offered a moment ago is called at once, and one withdrawn is refused as not found. The agent asks `describe_engine` again when a call is refused because the tools changed. The agent never passes a revision: the hub writes the one it converted with.
- **`Json` is a new shape in `aethereal-data`:** a JSON value, carried as its text. Its JSON Schema is `{}`, any value. `from_json` checks the value is JSON and keeps its text, and `to_json` writes the text as it is. It is plain, and its bytes are a string's (ADR-0002). It is what lets one tool take or answer a value whose schema is known only when it runs.

### Who dialed

Every engine links with a token the hub minted for it, spawned or started by hand, and passed in the engine's arguments (ADR-0040), and `Hello` brings it. A `Hello` with none cannot be read. The hub refuses a token it did not mint, or one already used. A token is good for one `Hello`, so reading it from the process list after the engine has linked gains nothing. That is also how the hub ties a connection to the process it started.

## Consequences

- **Tools are written once, and listed in a hub or an engine.** Neither the tool nor its crate knows which.
- **A capability's `tools` feature gets lighter:** it depends on `aethereal-tools`, not on HTTP and TCP.
- **The hub never needs an engine's kinds.** Schemas and bytes cross, and the hub converts JSON with the sheets it was sent.
- **An agent calls an engine's tool by `call_tool`, not by its name.** It reads the schemas with `describe_engine` first. That is one call more than a tool listed by name, and it is what keeps the list fixed.
- **Every engine ends when its link ends** (ADR-0040), and one started by hand is started again with a fresh token after its hub goes.
- **A hung engine holds a call until the call's deadline** (ADR-0041). Nothing here pings.
- **An answer too large refuses a call and ends nothing,** on either side, as each writer checks it against the frame limit. In-process, `Mcp` still has #357.
- `Link` and `Engines` live in one crate, `aethereal-link`, as the two ends of one protocol. A program lists the end it is.

## Alternatives considered

- **The hub dials the engine,** which listens on a port it writes to a file the hub names. The earlier engine ended there, so the hub knows which engine it reached. Rejected: the engine dials, as ADR-0033 decided, so an engine started by hand finds its hub. The hub's knowing who dialed is the token's job.
- **Any mail to any actor path over the link.** General, but it erases types on the wire, and it needs path resolution, settlement, and an error vocabulary of its own on both sides. Everything an agent needs from an engine can be a tool.
- **Each engine's tools offered to `Mcp` under a prefix** (`e3.read_file`). The agent would see each one's schema in `tools/list`. But the list would change with every engine, and a client that read it once would not see them.
- **Offers and withdrawals sent one at a time.** It needs an answer on the wire for each, and the hub's view drifts from the engine's when one is lost to a hang-up. The whole set is small, so it is sent whole.
- **The token in a file or on stdin.** It keeps the token out of the process list. A token good for one `Hello` is worth nothing once used, so its arguments carry it.
- **`call_tool`'s `input` as a JSON string.** It needs no new shape, but the agent would write JSON escaped inside JSON, and the hub would have to parse a string that its schema says nothing about.
