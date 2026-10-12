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
```

- An actor's type is named by a `KindName` (ADR-0006), such as `aether.window.surface`. There is no second sort of name.
- **An actor's type names the type of its key** (ADR-0009). A type whose key is the unit type `()` is a **singleton**: a place holds at most one, and it has no key. Any other key type is a `TypedKey` and makes the actor **instanced**: a place holds many, each told apart by its key.
- **A key is a typed value, and its type says what keys there are.** A key type turns each of its values into a `KeyName`, a different one for each, and reads a `KeyName` back into the value that made it. Going there and back gives the same value, and a name that reads as a value is the one that value writes, so a path still has one spelling. A path holds the `KeyName`, and an actor's code holds the typed value. A type with two values allows two children and no third.
- **A `u64` is a key, written in decimal.** `7` is the key `7`, with no sign and no leading zero, so each number has one name and `007` reads as no key. A parent that makes a child per request keeps the next number in its own state and counts upward, so it never names a closed path. There is one width, since a second would say nothing more.
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
- **A path names one actor for the whole life of its engine.** Two actors never have the same path, and when an actor closes its path is closed for good: nothing is spawned there again. A log line, a trace, and a path held outside the engine therefore always mean the same actor, and an actor's life has one beginning and one end.

**An actor owns the paths directly beneath it.** Only an actor asks for a child beneath itself, and no actor creates anything in another actor's branch.

**Only the chassis makes a root.** The chassis is the host program that builds the engine. No actor asks for a root, and no mail creates one: everything an actor brings into being stands beneath it.

**A reference is a path the engine has established exists.**

```rust
pub struct ActorRef { .. }
```

- Only the engine makes a reference. An actor comes to hold one in three ways: it is handed one in mail, it receives mail and is told the sender, or it asks the engine to resolve a path and the engine finds the actor.
- There is no presumptive reference: none is built from a path without the engine. An actor that knows only a path asks.
- A reference is a bound value (ADR-0004). It crosses between actors in one engine and may be kept in an actor's state. It does not cross between engines: there a path crosses, and the receiver asks its own engine to resolve it.
- A reference says the actor existed when the reference was made. It does not say the actor is still there. Mail sent through a reference to a closed actor is not delivered. How undelivered mail ends is decided with the envelope, in a later ADR.
- The engine has one table of actors, keyed by path, and it alone writes it. A reference, a resolve, and a delivery all read that table.

**An actor's id is a short reference to its path.**

```rust
pub struct ActorId(u64);
```

- An `ActorId` is 64 bits computed from the path's text. The path stays the actor's identity. The id stands for it where a path's text would be carried many times over, as in each effect of a turn (ADR-0009).
- **Only the engine computes an id.** No guest, tool, or other engine computes one, and none is written as a constant. The function is therefore the engine's to change.
- The function is FNV-1a with ADR-0006's basis and prime, over the bytes of the path's text. A child's id is its parent's id continued over the mark `/` and the child's segment as the path spells it. That gives the same number as hashing the child's whole text, so the engine reaches a child's id from its parent's without the text.
- An id is good for the life of the engine that made it. Another engine, or a later run, may give the same path a different id. What is kept longer than the engine carries paths.
- An id is plain data (ADR-0004), and one that was decoded is a claim like any other.
- **An id is not an address.** Nothing sends to one and no reference is made from one. Who an actor may send to is unchanged.
- **An id is built down from the root**, each from its parent's, so it stands for every segment above it. Only a parent makes the ids directly beneath its own, since only it spawns there.
- **Whether a child's key is free is its parent's own knowledge.** A spawn is refused when its kind and key have been used beneath that parent, live or closed. No other actor's spawn bears on the answer, so a parent decides it within its turn.
- The table keeps each path's id beside it, live or closed.
- **A reference is an id the engine has checked.** An `ActorRef` holds an `ActorId` and nothing else. Making one is the check that the id is in the table, and wherever a reference is read at a boundary the reader makes that check again. So a reference is eight bytes in mail and in an actor's state, and bytes that name no actor are refused.
- A reference gives up its path by a read of the table, since the id alone does not say where the actor stands.

## Consequences

- Who may hold a reference to an actor follows from who was told about it. A reference spreads only by mail, by the sender of mail being known, and by a resolve the engine answers.
- Depth is capped so that what walks a path or a branch has a bound it can state: a path can arrive as bytes, and closing an actor reaches everything beneath it. Nothing that walks either recurses.
- How a path is paired with the engine it belongs to when it crosses between engines is decided with that boundary.
- A path sorts by its text, so children keyed by number do not sort in the order they were made: `12` comes before `7`. Nothing here reads meaning from that order.
- The engine remembers every closed path, so that a spawn there is refused. A parent that makes a child per request leaves one behind for each. The cost is kept small by what is stored: a type and a key are each a number in a table, and so is a path.
- A root is a singleton or instanced like any other actor, so the path allows several roots of one type. Which key types the engine supplies beside `u64` is not decided here.
- A shorter spelling for a person typing a path, one that leaves out a type the parent makes obvious, is not part of this ADR. It needs the table of which child types a parent declares and enters with the first boundary where a person types a path.
- An id does not say where its actor stands. A reader gets the path from the engine's table, or from the spawns in a record that began with the engine. Whether a reader outside the engine may ask for an id's path is not decided.
- A reference made up from the id of a known path passes the check. It reaches nothing a resolve of that path would not, since any actor may ask the engine to resolve a path it knows.
- Two paths are taken to have two ids. By accident they do: a million paths share an id with odds of about one in forty million. On purpose they need not, since FNV-1a can be run backwards and a parent picks its children's keys. What the engine does when two paths share an id is not decided here. The remedy is an id keyed with a secret the engine holds, which is possible because only the engine computes an id.
- Typed references and typed paths, which say at build time what mail the actor on the other end handles, stand on these types and are a later ADR.

## Alternatives considered

- **A written key on every segment.** One spelling. A type of which there is one would then carry a key its author had to invent and every user had to repeat.
- **One key type with a case for the only actor of its type**, `Only` beside `Named`. Every segment then has a key and one shape. The case says a place holds one such actor, which a key cannot enforce: nothing stopped `surface` beside `surface:left`. The actor's type can say it, and then the case has nothing left to say.
- **A reserved key text, such as the empty string, for a singleton.** One type for a key. `KeyName` would then have a value no author may choose, and an instanced type could be handed it.
- **A path stored as its parts**, a struct of segments. The schema then shows the parts, and a JSON view of mail spells a one-line path as nested objects wherever one appears. A struct cannot be a map key (ADR-0002).
- **An opaque number as the actor's identity, with the path as a label.** Smaller to send. A number says nothing about where the actor stands, so who owns it would have to be looked up.
- **An id anyone may compute**, with its bytes stated as ADR-0006 states a kind's. A guest could then name actors by id in what it returns. The function could never change, a keyed id would be ruled out, and the engine would check every id a guest wrote.
- **An id handed out by the engine as each actor is made.** Dense, and free of collisions. A turn does not write the engine (ADR-0009), so a child stood up in a turn would have no id until the turn was over.
- **A reference built from any path, checked when mail is sent.** No resolve step. Every send then carries a claim the engine has not checked, and an actor can address what it was never told about.
- **Reusing a path after its actor closes, with each reference meaning one particular birth.** The engine would remember nothing about closed actors, and a stale reference would still fail. A path would then name different actors at different times: a log could not be read by path, and a path held outside the engine would silently come to mean a newcomer.
