# ADR-0008: Actor paths and references

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Mail goes to an actor, so an actor needs a name, and a sender needs something to hold that says which actor it means.

The earlier engine named an actor by where it stood: its parent's name, then its own type and key. That held up, and this ADR keeps it. What did not hold up was the number of things that could answer "does this actor exist": a registry, a watch index, a parent's child table, a route table, and others, each changing at a different moment when an actor closed. A reader that asked the one lagging behind decided wrongly.

Two rules about who makes an actor bear on names. An actor is created only beneath the actor that asked for it, and the engine alone creates and closes actors. So every part of the tree of names has one owner, and one place knows what exists.

## Decision

**An actor is named by its kind, and by a key where a place holds many of its type.**

```rust
pub trait TypedKey: Sized {
    fn to_name(&self) -> KeyName;
    fn from_name(name: &KeyName) -> Option<Self>;    // none: not a key of this type
}

pub trait ActorKey {
    fn name(&self) -> Option<KeyName>;               // none: a singleton
}
```

- An actor's type is named by a `KindName` (ADR-0006), such as `aether.window.surface`. There is no second sort of name.
- **An actor's type names the type of its key** (ADR-0009). A type whose key is the unit type `()` is a **singleton**: a place holds at most one, and it has no key. Any other key type is a `TypedKey` and makes the actor **instanced**: a place holds many, each told apart by its key. The bound an actor's key meets is `ActorKey`, which holds for `()` and for every `TypedKey`. It answers with the key's name, or with none for a singleton. It is the engine's, and no author implements it.
- **A key is a typed value, and its type says what keys there are.** A key type turns each of its values into a `KeyName`, a different one for each, and reads a `KeyName` back into the value that made it. Going there and back gives the same value, and a name that reads as a value is the one that value writes, so a path still has one spelling. A path holds the `KeyName`, and an actor's code holds the typed value. A type with two values allows two children and no third.
- **A `u64` is a key, written in decimal.** `7` is the key `7` and zero is `0`, with no sign and no leading zero, so each number has one name and `007` reads back as no value. A parent that makes a child per request keeps the next number in its own state and counts upward, so it never names a closed path. There is one width, since a second would say nothing more.
- A `KeyName` is 1 to 64 bytes: a lowercase ASCII letter or a digit, then lowercase ASCII letters, digits, `_`, `-`, and `.`. Its first byte is never `.` or `-`, so none reads as a relative path or a flag where a person types it.
- The key follows from the actor's type and is not chosen beside it. A singleton and an instanced actor are spawned by separate effects (ADR-0009): the one for a singleton carries no key, and the one for an instanced type carries a `KeyName`. So a singleton with a key, and an instanced actor with none, cannot be written, and one place never holds both `surface` and `surface:left`.
- `KindName` and `KeyName` are checked when built: a constant that does not conform stops the build, and the decoder refuses text that does not conform. Neither hands out its text.

**An actor's path is where it stands.**

```rust
pub struct ActorPath { .. }
```

```text
aether.window
aether.window/aether.window.surface:left
```

- A path is a root segment and up to 31 child segments, so it is never empty and never deeper than 32. Each segment is the name of an actor's kind, and a `KeyName` where the actor is instanced. It is written `name:key` for an instanced actor and `name` alone for a singleton. A segment asked for its key answers with the name or with none.
- The first segment is a **root**. Each later segment is a child of the actor the path before it names.
- A path is plain data (ADR-0004) with the shape of a string: its bytes are its text. Every path has exactly one spelling and every spelling that conforms is exactly one path, so the text is the stored form. `ActorPath::new` checks a constant and stops the build on one that does not conform, and the decoder refuses text that does not conform or is deeper than 32, as `KindName` does (ADR-0006).
- A path is a `Key` (ADR-0004): it may be the key of a map, in the order of its text. Everything beneath a path sorts together in that order.
- A path does not hand out its text. It answers with its parts: its root, its parent, its last segment, and whether it stands beneath another.
- A path names an actor within one engine. It crosses every boundary as data, and an engine that receives one from another engine reads it as naming an actor in the engine it came from.
- **A path names one actor for the whole life of its engine.** Two actors never have the same path, and when an actor closes its path is closed for good: nothing is spawned there again. Only an actor's parent spawns at its path, so this is the parent's to keep, below, and the engine remembers no closed path of its own. A log line, a trace, and a path held outside the engine therefore always mean the same actor, and an actor's life has one beginning and one end.

**An actor owns the paths directly beneath it.** Only an actor asks for a child beneath itself, and no actor creates anything in another actor's branch.

**Only the chassis makes a root.** The chassis is the host program that builds the engine. No actor asks for a root, and no mail creates one: everything an actor brings into being stands beneath it. The chassis makes its roots as it builds the engine.

**A reference names an actor the engine has established exists.**

```rust
pub struct ErasedActorRef { .. }
```

- Only the engine makes a reference. An actor comes to hold one in three ways: it is handed one in mail, it receives mail and is told the sender, or it asks the engine to resolve a path and the engine finds the actor. **A fourth way to send, added by ADR-0022, is a departure:** an actor names a capability, a protocol one singleton root answers, and the engine finds that root at each send, so the actor holds no reference to it and was handed nothing. It reaches the same actor a resolve of the root's path would, through a named protocol.
- There is no presumptive reference: none is built from a path without the engine. An actor that knows only a path asks.
- A reference is a bound value (ADR-0004). It crosses between actors in one engine and may be kept in an actor's state. It does not cross between engines: there a path crosses, and the receiver asks its own engine to resolve it.
- **A reference is proven once, when it is made, and valid for as long as it exists.** In bytes it is its actor's id and seat, written only by a reference, so the reader at `Actors` and at `Within` reads them back as they were written and asks the table nothing. A type that holds a reference derives `Bound`, which writes the codec `Plain` writes and does not state `Plain` (ADR-0004).
- A reference says the actor existed when the reference was made. It does not say the actor is still there. Mail sent through a reference to a closed actor is not delivered. How undelivered mail ends is decided with the envelope, in a later ADR.
- The engine has one table of actors, and it alone writes it: the table is a view the engine folds from spawns and endings (ADR-0011). A reference, a resolve, and a delivery all read that table.
- **The table is keyed by an actor's id.** A reference, the sender of mail, and the `by` of an effect each name an actor by its id alone, and an id cannot be turned back into a path. An actor's entry holds the id of the actor above it and its own segment. A live actor's path is kept with it where it is seated, so it does not depend on the actors above it being live, and a path resolves by its last segment and its parent's id, with no entry above read.
- **An entry has one source for each of its two changes.** It is made by the spawn its parent asked for, or for a root by the chassis, and it is taken out by the actor's own ending and by nothing else (ADR-0009). A parent's ending does not take out its children's entries: a child whose parent has ended is still live (ADR-0009).
- **Who stands beneath an actor is a second view**, keyed by the parent's id and folded from the same spawns. It is built when something reads it.
- **The table holds the live actors.** An actor is written once, when it goes live, and taken out once, when it ends, so the table is as large as the actors live at once and not as all the actors the engine has made.
- **An actor a reference names that is not in the table has ended.** A reference says its actor existed, so for whoever holds one, an actor that is not there is one that has gone.
- **A turn reads the table as it stood when its step began** (ADR-0011), so nothing a turn reads goes from live to closed while it runs.

**An actor's id is a short reference to its path.**

```rust
pub struct ActorId(u64);
```

- An `ActorId` is 64 bits computed from the path's segments. The path stays the actor's identity. The id stands for it where a path's text would be carried many times over, as in each effect of a turn (ADR-0009).
- **Only the engine computes an id.** No guest, tool, or other engine computes one, and none is written as a constant. The function is therefore the engine's to change.
- **An id is a hash folded along the path, one segment at a time.**

  ```text
  id(root)  = fold(basis, hash(segment))
  id(child) = fold(id(parent), hash(segment))
  ```

  `hash(segment)` is FNV-1a with ADR-0006's basis and prime over the segment as the path spells it: the kind's name, then `:` and the key's name where there is one. `fold` continues FNV-1a from the first number over the eight bytes of the second, least significant first. A kind's name is hashed when the program is built, so a child's id is reached from its parent's id and its segment's hash with no path text at hand.
- A segment's hash is of the kind's name and not its `KindId`. A `KindId` changes with the kind's layout (ADR-0006), and an actor's id does not change when its code does.
- `ActorId` is a type of the data layer, beside the path. The function that computes one enters with the engine, and nothing before the engine makes an id.
- An id is good for the life of the engine that made it. Another engine, or a later run, may give the same path a different id. What is kept longer than the engine carries paths.
- An id is plain data (ADR-0004), and one that was decoded is a claim like any other.
- **An id is not an address.** Nothing sends to one and no reference is made from one. Who an actor may send to is unchanged.
- **An id is built down from the root**, each from its parent's, so it stands for every segment above it. An id directly beneath an actor's enters the table only when that actor spawns, since only it spawns there.
- **Whether a child's key is free is its parent's own knowledge.** A spawn is refused when its kind and key have been used beneath that parent, live or closed. A child whose `begin` failed was never live and does not use its key (ADR-0009). No other actor's spawn bears on the answer, so a parent decides it within its turn.
- **A parent keeps the keys used beneath it where it is seated**, each noted as the child it made is seated, and they go when its seat does. A spawn is refused by what the parent's seat keeps, by the children live in the table, and by those stood up earlier in its step, so a parent decides it from its own state and the table, in its turn. Nothing is spawned beneath an actor that has ended, since only it spawns there, so its children's paths stay closed with no one remembering them. The table keeps the keys of the chassis's roots, for the engine's life. The parent's keys are kept in its seat and not in the table because they are only ever read by the parent, in its own turn: kept in the table, every spawn wrote a second record to the parent's entry, and measured on `fib` a piece of mail cost about 19 percent more than with every ended entry kept; kept in the seat, the table writes nothing for them.
- **A reference is an id the engine has checked.** An `ErasedActorRef` holds an `ActorId` and the seat its actor sits in. It says nothing of what the actor handles: any actor can be sent any mail, and the reference typed by its actor's type is `ActorRef<T>`, below. Making one is the check that the id is in the table, and nothing checks it again: a reference is sixteen bytes in mail and in an actor's state, read back as written. Bytes written by hand for an id that was never made are not caught where they are read, and mail sent through the reference they make is not delivered.
- A reference gives up its path by a read of the table while its actor is live, since the id alone does not say where the actor stands. Once the actor has ended it gives up none: what must name an actor after it has gone, a log line or a trace, keeps the path itself.

**An actor holds an `ActorRef<T>`: a reference typed by its actor's type.**

```rust
pub struct ActorRef<T> { .. }
```

- Only the engine makes one, when it makes the actor: a spawn of a `T` answers with an `ActorRef<T>`. Its type is therefore proven for as long as it exists, and a send through it is not checked against the receiver's declaration (ADR-0013).
- It crosses what an `ErasedActorRef` crosses, the same way: between actors in one engine, and into an actor's own state, never between engines. In a kind, the field's type is the reference's type.
- It erases to an `ErasedActorRef`. The erased reference is for the few places that cannot name the type: the sender of mail, an ending, a resolve, and an actor's reference to itself. A send through one is checked against the declaration.
- A codec written by hand could put any id where a reference is read. Nothing stops it, since stopping it would cost every reference read: kinds that carry a reference derive their codecs.

## Consequences

- Who may hold a reference to an actor follows from who was told about it. A reference spreads only by mail, by the sender of mail being known, and by a resolve the engine answers.
- Depth is capped so that what walks a path or a branch has a bound it can state: a path can arrive as bytes, and a branch can be read from the table. Nothing that walks either recurses.
- How a path is paired with the engine it belongs to when it crosses between engines is decided with that boundary.
- A path sorts by its text, so children keyed by number do not sort in the order they were made: `12` comes before `7`. Nothing here reads meaning from that order.
- The engine keeps no entry for an actor that has ended. A parent that makes a child per request still holds one key for each child it has made, in its seat, for as long as it lives. How a parent's keys are kept smaller, such as by the count that a parent numbering its children already holds, is not decided here.
- A running engine gains no root. A system that needs a new one, a server taking an upgrade among them, waits for the engine to be built again. Whether the chassis may add a root while the engine runs is left until something needs it.
- A root is a singleton or instanced like any other actor, so the path allows several roots of one type. Which key types the engine supplies beside `u64` is not decided here.
- A shorter spelling for a person typing a path, one that leaves out a type the parent makes obvious, is not part of this ADR. It needs the table of which child types a parent declares and enters with the first boundary where a person types a path.
- An id does not say where its actor stands. A reader gets the path from the engine's table, or from the spawns in a record that began with the engine. Whether a reader outside the engine may ask for an id's path is not decided.
- A reference made up from the id and seat of a known path is read like any other. It reaches nothing a resolve of that path would not, since any actor may ask the engine to resolve a path it knows.
- Two paths are taken to have two ids. By accident they do: a million paths share an id with odds of about one in forty million. On purpose they need not, since FNV-1a can be run backwards and a parent picks its children's keys. **When an actor is made under an id the table already holds, the engine stops.** The table holds only live actors, so a collision with an actor that has ended is not caught: a reference to the ended actor then carries the newcomer's id, and reaches it only if the newcomer sits in the same seat. It panics as the step's effects are folded. Nothing reasonable follows a collision: the path can never be spawned at, and a refusal would give every spawn a way to fail that is handled nowhere else and never taken. A parent refuses a key it has itself used before the spawn is asked for, so what reaches the fold is two paths with one id, or a fault in the engine. Until ids are keyed, a guest that picks a colliding key stops its engine. The remedy is an id keyed with a secret the engine holds, which is possible because only the engine computes an id.
- A reference typed by a protocol rather than an actor's type, and typed paths, which say at build time what mail the actor on the other end handles, stand on these types and are a later ADR.

## Alternatives considered

- **A written key on every segment.** One spelling. A type of which there is one would then carry a key its author had to invent and every user had to repeat.
- **One key type with a case for the only actor of its type**, `Only` beside `Named`. Every segment then has a key and one shape. The case says a place holds one such actor, which a key cannot enforce: nothing stopped `surface` beside `surface:left`. The actor's type can say it, and then the case has nothing left to say.
- **A reserved key text, such as the empty string, for a singleton.** One type for a key. `KeyName` would then have a value no author may choose, and an instanced type could be handed it.
- **A path stored as its parts**, a struct of segments. The schema then shows the parts, and a JSON view of mail spells a one-line path as nested objects wherever one appears. A struct cannot be a map key (ADR-0002).
- **An opaque number as the actor's identity, with the path as a label.** Smaller to send. A number says nothing about where the actor stands, so who owns it would have to be looked up.
- **An id anyone may compute**, with its bytes stated as ADR-0006 states a kind's. A guest could then name actors by id in what it returns. The function could never change, a keyed id would be ruled out, and the engine would check every id a guest wrote.
- **One hash over the path's whole text.** One pass, with no segment hashed apart. A child's id then needs its segment's text each time it is made, and a later change to how ids are combined would change how names are hashed with it.
- **Each id made from the ids beneath it**, as a hash tree is. An actor's id would then change with every child it gained or lost.
- **An id handed out by the engine as each actor is made.** Dense, and free of collisions. A turn does not write the engine (ADR-0009), so a child stood up in a turn would have no id until the turn was over.
- **A reference built from any path, checked when mail is sent.** No resolve step. Every send then carries a claim the engine has not checked, and an actor can address what it was never told about.
- **Numbering every change to the table from one count, with a reader holding one number.** Every reader that holds a number then has the same steady picture. A number may be taken by a reader only once every change before it is written, so writers either wait for one another in order or each keep note of what is still being written. Measured on 32 threads, the first fell below one thread's rate and the second cost more code than the table itself. A step's number is the table's position with no count of changes (ADR-0011).
- **The table keeping every actor that has ended**, marked ended. One place then answers every question about any actor that ever lived, a closed path among them. The table grows with every actor the engine makes, and what reads it slows as it grows. Measured over twelve runs of `fib` in one engine, about 1.8 million ended entries, a piece of mail cost about 17 percent more in the last runs than in the first: the same instructions, with more cache and page-table misses. With ended entries taken out, about 2 percent.
- **Reusing a path after its actor closes, with each reference meaning one particular birth.** The engine would remember nothing about closed actors, and a stale reference would still fail. A path would then name different actors at different times: a log could not be read by path, and a path held outside the engine would silently come to mean a newcomer.
