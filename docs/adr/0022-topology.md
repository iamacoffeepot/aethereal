# ADR-0022: Topology: where actors may stand, and typed paths to them

- **Status:** Proposed
- **Date:** 2026-10-20

## Context

An actor's type declares what each of its handlers may do: the kinds it sends and to whom, the stages it subscribes to, the actors it watches, the jobs it runs (ADR-0013: `Marker`, `Sending`, `Target`; ADR-0018; ADR-0020). It declares nothing of where actors stand. Any actor may spawn a child of any type (`Turn::spawn::<C>` has no bound), and the chassis may make a root of any type.

So nothing typed reaches an actor the asker did not make or was not handed. An actor comes to hold a reference in three ways (ADR-0008): it is handed one in mail, it is told the sender of mail, or it resolves a path and is answered with an `ErasedActorRef`. An erased reference casts at runtime only to a tuple protocol, and asking needs a named protocol, which only a typed `ActorRef<R>` converts to (ADR-0016: `ActorRef::protocol`; ADR-0017). So an actor cannot ask a root it was not handed: the renderer (ADR-0021) cannot be asked for a mesh. And reaching a child of another actor means writing its path out of names and casting what it resolves to.

Yet much of a program's tree is fixed by its types. A singleton root is the one actor of its type at its name, and a singleton beneath it is the one actor of its type there (ADR-0008). Its path, and so its id, follow from types alone; an instanced actor's follow from its types and keys.

The earlier engine declared placement on types, `Root` and `ChildOf<P>` (its ADR-0166), resolved singletons by type from a scope, `ctx.actor::<ComponentHost>().actor::<Camera>()` (its ADR-0098), declared a dependency on a root and refused a spawn before `init` when it was not live (its ADR-0230: `depends(R)`), and had a spawner declare the children it spawns (`#[actor(spawns(..))]`).

## Decision

### Where an actor may stand is declared on its type, once: `ChildOf`

```rust
impl ChildOf for Host   { type Parent = Top; }            // a root
impl ChildOf for Camera { type Parent = Host; }           // directly beneath a Host
impl ChildOf for Lens   { type Parent = Camera; }
impl ChildOf for Fib    { type Parent = Recursive; }      // a root, or beneath a Fib
impl ChildOf for Timer  { type Parent = Anywhere; }       // beneath any actor, or a root: a library's
```

- **`ChildOf` names the one place a type may stand, as its `Parent`:** `Top`, one type, `Recursive` (the top, or beneath its own type), or `Anywhere` (beneath any actor, or the top). A type declares it once; a second `impl ChildOf` does not build.
- **A type stands beneath one type, or anywhere.** A type that may stand beneath either of two types declares `Anywhere`, or is two types. A choice of two spawner types cannot be checked where it is declared: the two impls that would admit each overlap when the two are one type, and Rust refuses them (E0119); an index to tell them apart has no call to be inferred at. A check made as a constant instead is not seen by `cargo check` or an editor, nor for a type no build makes. **This is a limit to revisit**, as `Recursive` and `Anywhere` at the top are: what the tests and benches need today.
- **`Root` is a type whose `Parent` is at the top: `Top`, `Recursive`, or `Anywhere`.** It is not declared: it follows from `ChildOf`. `Engine::add_root` and `add_root_from` build only for a `Root`. A root is a singleton or instanced like any other actor (ADR-0008), so a root keyed by `round` is `path::<A>(round)`: its key is its path's first.
- It says nothing of whether an actor is live. It is a permission, which spawning uses and reaching relies on.
- A guest's section carries it, as it carries the rest of its type's declaration (ADR-0015).
- **This departs from the earlier engine**, whose `ChildOf<P>` took the parent as a type parameter, so a type could declare it many times and no path could follow. Here the parent is an associated type, declared once, so a type has one path and the engine can fill it in (below).

### A turn spawns only what may stand beneath its actor's type

- `turn.spawn` and `turn.spawn_from` build only when **`C`'s `Parent` admits the spawning actor's type** (`C::Parent: Admits<Me, C>`): the type itself, `C` itself for `Recursive`, or any type for `Anywhere`. The turn is typed by its actor, so the check is made at the call, against where `C` declares it stands, and the turn's sends list nothing for it: a spawn sends no mail. Where a type may stand is the one fact, and the spawn is checked against it.
- **`C` is named where the child goes**: `let unit: ActorRef<Unit> = turn.spawn(&7, config)?`, or the field the reference is kept in, or at the call when the reference is not kept, `turn.spawn::<Unit>(&7, config)`. **This departs from inferring every child**: with no list of children, nothing else names one, and the turbofish names only the child, with nothing padded.
- A closing turn has no `spawn`: it spawns nothing.
- Each type's declaration holds who may spawn it, as its `Parent` says: no one for `Top`, anyone for `Anywhere`, its own type for `Recursive`, and the one type otherwise (`Spawner`). A guest's section holds it with the rest of the type's declaration, and a guest's host checks a guest's spawns against the child type's: a guest that spawns a child that does not stand beneath its spawner's type has broken its declaration, and its host fails the turn that did, as for a send past it (ADR-0013); a child of other code it asked for in a call out is not begun.
- So any actor may spawn a type that stands `Anywhere`, as any may hold one.

## Consequences

- An actor reaches a root, or any actor beneath it whose place its types and keys name, by its type alone, at any depth a path may have, with nothing handed down through configs, and asks it through the protocol it declares.
- A spawn that cannot reach what it declares is refused where it is asked, before any of the child's code runs, and names what is missing.
- A type's declaration holds where it may stand, who may spawn it, and the paths it reaches: the tree's shape, as types. What lists every declaration, so a tool can walk them, is left until a tool needs it: native declarations are constants of their types today, and a guest's are its section.
- **What it costs the code today:**
  - every type made as a root or spawned declares `ChildOf`: one line for each of the types behind today's ~120 `add_root` calls and 81 spawn sites;
  - every spawn names its child's type where its reference goes, or at the call.
  
  The change is mechanical, and a site that is missed does not build.
- A turn's sends are a tuple of at most 12 markers (`Allows`), and `Asks` and `Tells` count toward it. A handler that would declare more is split, or the tuples grow, when one does.
- A type a guest reaches at a typed path must build in the guest: its name, its declaration of where it stands, and the protocol it is asked through. A root whose host state lives in resources (ADR-0010) builds there. Whether a root whose handlers' code cannot build in a guest is reachable from one is left until a guest needs one.
- Sends to a child by name, `To::Child(KindName)` and `To::ChildInstanced`, are unchanged.
- A send at a typed path costs the table's resolve of it, which hashes each of its segments, and a read of its actor's kind; a spawn's check is one pass over a declaration it already reads for stages; `turn.child` and `turn.parent` are one table read.

## Alternatives considered

- **`add_root` answers an `ActorRef<A>`, and the chassis hands it to the roots that use it, in their configs.** Explicit, and needs nothing new. Every actor beneath a root that reaches it would be handed the reference through each config on the way down, and nothing would refuse a spawn whose root is missing.
- **A marker of its own for a dependency, `Needs<R>`.** A declared ask to the root already says the actor depends on it; a second marker would say it twice.
- **A protocol named for each actor asked at a typed path, `impl Asked for Renderer { type Protocol = Rendering; }`.** A type made only to say what the actor answers, which `Carries` on the actor's own type says beside its handler.
- **A capability: a protocol named with the name of the root that answers it.** Gave a guest something to name that holds no host state. A root's type and key name it, so its type is what to name, and `Root` checks at build time what a capability's name could not.
- **`ChildOf<P>` with the parent as a type parameter, declared once for each type a type may stand beneath, as the earlier engine did.** A type with several gives no path to fill in, and no typed parent; `Anywhere` as one `Parent` says the places, without the check, and the one declaration is what lets a path be named by its last type. It also let the parent's crate place a library's type beneath its own; here, the library says where its type may stand, `Anywhere` when any actor may hold one.
- **A tuple of types as a parent, `type Parent = (Army, Fort)`, any of which a type may stand beneath.** Not checkable where it is declared (above); kept as the first thing to revisit when a type needs it.
- **A path named by every type in it, `path::<(Host, Camera, Lens)>()`.** The tuple restates what the types' `Parent`s say.
- **Two verbs, `path` for a path of singletons and one with a suffix for a path with a key.** Rust does not choose a function by how many arguments it is given; one verb that takes `()` for no keys is the one way to name a path.
- **A key only on the last segment, with a keyed segment above it reached by `turn.child`.** An actor beneath an instanced one would need a reference to its parent held first, though its place is named by types and keys alone.
- **Paths by chaining, `root::<Host>().child::<Camera>()`.** One call per segment, for what the types already say.
- **The receiver named by a turbofish on the send, `ask_to::<Renderer>(..)`.** Rust has every type parameter named once one is, and the index a send's declaration is found at is inferred, so the call would be `ask_to::<Renderer, _, _, _, _>`. A typed path is the receiver as a value.
- **Placement left undeclared, with the table's check of a child's kind alone.** Sound at runtime, but nothing at build time would say where an actor may stand, and no path could be checked or reached from types.
- **The set of roots as a type, so a path's actor is known to exist at build time.** Every chassis would build its engine from a typed list of roots, for a check the spawn already makes at boot.
- **Another name, `At`, `Site`, or `Spot`.** A path is a root segment and the child segments beneath it (ADR-0008); this is one, spelled with types where `ActorPath` is spelled with names, and both fold to the same id.
