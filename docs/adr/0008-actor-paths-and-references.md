# ADR-0008: Actor paths and references

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Mail goes to an actor, so an actor needs a name, and a sender needs something to hold that says which actor it means.

The earlier engine named an actor by where it stood: its parent's name, then its own type and key. That held up, and this ADR keeps it. What did not hold up was the number of things that could answer "does this actor exist": a registry, a watch index, a parent's child table, a route table, and others, each changing at a different moment when an actor closed. A reader that asked the one lagging behind decided wrongly.

Two rules about who makes an actor bear on names. An actor is created only beneath the actor that asked for it, and the engine alone creates and closes actors. So every part of the tree of names has one owner, and one place knows what exists.

## Decision

**An actor is named by its kind, and every actor has a key.**

```rust
pub enum ChildKey {
    Only,                      // the one actor of its type in its place
    Named(KeyName),            // "main"
}
```

- An actor's type is named by a `KindName` (ADR-0006), such as `aether.window.surface`. There is no second sort of name.
- A `ChildKey` says which actor of a type this is among those in one place. A type of which a place holds at most one uses `Only`. Otherwise the key is `Named`, and a `KeyName` is 1 to 64 bytes: a lowercase ASCII letter or a digit, then lowercase ASCII letters, digits, `_`, `-`, and `.`. A key cannot be `.`, `..`, or start with `-`, so none reads as a relative path or a flag where a person types it.
- `KindName` and `KeyName` are checked when built: a constant that does not conform stops the build, and the decoder refuses text that does not conform. Neither hands out its text.

**An actor's path is where it stands.**

```rust
pub struct ActorPath { .. }
```

```text
aether.window
aether.window/aether.window.surface:left
```

- A path is a root segment and up to 31 child segments, so it is never empty and never deeper than 32. Each segment is the name of an actor's kind and a key. It is written `name:key` for a named key and `name` alone for `Only`, so every actor has a key and the one that needs no name shows none.
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
- The table keeps each path's id beside it. A spawn whose id is already held by a different path, live or closed, is refused.

## Consequences

- Who may hold a reference to an actor follows from who was told about it. A reference spreads only by mail, by the sender of mail being known, and by a resolve the engine answers.
- Depth is capped so that what walks a path or a branch has a bound it can state: a path can arrive as bytes, and closing an actor reaches everything beneath it. Nothing that walks either recurses.
- How a path is paired with the engine it belongs to when it crosses between engines is decided with that boundary.
- The engine remembers every closed path, so that a spawn there is refused. A parent that makes a child per request leaves one behind for each. The cost is kept small by what is stored: a type and a key are each a number in a table, and so is a path.
- A root has a key like any other actor, so the path allows several roots of one type. Whether a type's actors use `Only` or named keys is declared by the type, in a later ADR.
- A shorter spelling for a person typing a path, one that leaves out a type the parent makes obvious, is not part of this ADR. It needs the table of which child types a parent declares and enters with the first boundary where a person types a path.
- An id does not say where its actor stands. A reader gets the path from the engine's table, or from the spawns in a record that began with the engine. Whether a reader outside the engine may ask for an id's path is not decided, and neither is whether an `ActorRef` carries the id.
- A collision can be made on purpose. FNV-1a can be run backwards, and a parent picks its children's keys, so an actor can make a child whose id is that of a path another actor will spawn later. That spawn is then refused, and the refusal names both paths. Mail is never delivered to the wrong actor. Closing this takes an id keyed with a secret the engine holds, which is possible because only the engine computes an id. It is left for later.
- Typed references and typed paths, which say at build time what mail the actor on the other end handles, stand on these types and are a later ADR.

## Alternatives considered

- **A written key on every segment.** One spelling. A type of which there is one would then carry a key its author had to invent and every user had to repeat.
- **Two kinds of segment, one with a key and one without.** The same spellings, and every piece of code that handles a segment handles two shapes. One key with a reserved value keeps one shape.
- **A path stored as its parts**, a struct of segments. The schema then shows the parts, and a JSON view of mail spells a one-line path as nested objects wherever one appears. A struct cannot be a map key (ADR-0002).
- **An opaque number as the actor's identity, with the path as a label.** Smaller to send. A number says nothing about where the actor stands, so who owns it would have to be looked up.
- **An id anyone may compute**, with its bytes stated as ADR-0006 states a kind's. A guest could then name actors by id in what it returns. The function could never change, a keyed id would be ruled out, and the engine would check every id a guest wrote.
- **An id handed out by the engine as each actor is made.** Dense, and free of collisions. A turn does not write the engine (ADR-0009), so a child stood up in a turn would have no id until the turn was over.
- **A reference built from any path, checked when mail is sent.** No resolve step. Every send then carries a claim the engine has not checked, and an actor can address what it was never told about.
- **Reusing a path after its actor closes, with each reference meaning one particular birth.** The engine would remember nothing about closed actors, and a stale reference would still fail. A path would then name different actors at different times: a log could not be read by path, and a path held outside the engine would silently come to mean a newcomer.
