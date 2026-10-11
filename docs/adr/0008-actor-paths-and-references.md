# ADR-0008: Actor paths and references

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Mail goes to an actor, so an actor needs a name, and a sender needs something to hold that says which actor it means.

The earlier engine named an actor by where it stood: its parent's name, then its own type and key. That held up, and this ADR keeps it. What did not hold up was the number of things that could answer "does this actor exist": a registry, a watch index, a parent's child table, a route table, and others, each changing at a different moment when an actor closed. A reader that asked the one lagging behind decided wrongly.

Two rules from the design of the turn (ADR-0009) bear on names. An actor is created only beneath the actor that asked for it, and the engine alone creates and closes actors. So every part of the tree of names has one owner, and one place knows what exists.

## Decision

**An actor type has a name, and every actor has a key.**

```rust
pub struct TypeName(..);       // "aether.window.surface"
pub struct ChildKey(..);       // "main"
```

- A `TypeName` follows the rule of a `KindName` (ADR-0006): segments of lowercase ASCII letters, digits, and `_`, separated by `.`, each starting with a letter. It is a separate type, so the name of an actor type cannot be passed where the name of a kind is wanted.
- A `ChildKey` is what a parent calls one actor among those of a type beneath it: 1 to 64 bytes of lowercase ASCII letters, digits, `_`, `-`, and `.`.
- Both are checked when built: a constant that does not conform stops the build, and the decoder refuses text that does not conform. Neither hands out its text.

**An actor's path is where it stands.**

```rust
pub struct ActorPath { .. }
```

```text
aether.window:main
aether.window:main/aether.window.surface:left
```

- A path is one or more segments. Each segment is a type and a key, written `type:key`. There is no segment without a key.
- The first segment is a **root**. Each later segment is a child of the actor the path before it names.
- A path is plain data (ADR-0004): a sequence of segments, each a struct of the type's name and the key. It crosses every boundary and means the same thing in every engine, though it names an actor in only one.
- Two actors in one engine never have the same path, and a path is not used again while its engine lives. A closed actor's path stays closed.

**An actor owns the paths directly beneath it.** Only an actor asks for a child beneath itself, and only it or the engine closes that child. No actor creates or closes anything in another actor's branch. Roots are asked for from outside the engine.

**A reference is a path the engine has established exists.**

```rust
pub struct ActorRef { .. }
```

- Only the engine makes a reference. An actor comes to hold one in three ways: it is handed one in mail, it receives mail and is told the sender, or it asks the engine to resolve a path and the engine finds the actor.
- There is no presumptive reference: none is built from a path without the engine. An actor that knows only a path asks.
- A reference is a bound value (ADR-0004). It crosses between actors in one engine and may be kept in an actor's state. It does not cross between engines: there a path crosses, and the receiver asks its own engine to resolve it.
- A reference says the actor existed when the reference was made. It does not say the actor is still there. Mail sent through a reference to a closed actor ends as ADR-0009 says undelivered mail ends.
- The engine has one table of actors, keyed by path, and it alone writes it. A reference, a resolve, and a delivery all read that table.

## Consequences

- Who may hold a reference to an actor follows from who was told about it. A reference spreads only by mail, by the sender of mail being known, and by a resolve the engine answers.
- Whether more than one root of a type may exist follows from the path: roots have keys, so the path allows it. Whether an actor, and not only the outside, may ask for a root is not decided here.
- A shorter spelling for a person typing a path, one that leaves out a type the parent makes obvious, is not part of this ADR. It needs the table of which child types a parent declares and enters with the first boundary where a person types a path.
- A type of which there is only ever one still has a key. Its author chooses one and its users write it.
- Typed references and typed paths, which say at build time what mail the actor on the other end handles, stand on these types and are a later ADR.

## Alternatives considered

- **A segment without a key for a type of which there is one.** Shorter paths for shared services, and two forms of segment with a rule for which applies where.
- **An opaque number as the actor's identity, with the path as a label.** Smaller to send. A number says nothing about where the actor stands, so who owns it would have to be looked up.
- **A reference built from any path, checked when mail is sent.** No resolve step. Every send then carries a claim the engine has not checked, and an actor can address what it was never told about.
- **Reusing a path after its actor closes.** Simpler keys for an actor that is closed and made again. A reference to the old actor would then reach the new one.
