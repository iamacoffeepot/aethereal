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

### A typed path: the actor named by its type, its ancestors filled in

```rust
path::<Renderer>(())                  // the root Renderer
path::<Lens>(())                      // the Lens beneath the Camera beneath the Host
path::<Squad>((3, Name("scout")))     // World, then Army 3, then Hud, then the Squad "scout"
path::<Banner>(3)                     // the Banner beneath Army 3: one key, alone
```

- **A path is named by its last type alone.** The engine fills in the rest from each type's `Parent`, up to a root: `path::<Lens>` is the path `Host`, `Camera`, `Lens`. It builds only when every type on the way has one type as its `Parent`, or `Top` at the root; a type that is `Recursive` or `Anywhere` has no one path, and is reached from a reference held (below). A path is at most as deep as any path (ADR-0008: 32 segments), and takes at most eight keys. **The refusal says why:** `ChildOf` carries `#[diagnostic::on_unimplemented]`, so `path::<Timer>` reads "`Anywhere` is not one actor type that declares where it stands", noting that a type that is `Recursive` or `Anywhere` has no one path and is reached from a reference held. It names the parent that is not one place, as the bound unmet is the parent's.
- **Its keys are its keyed segments' keys, root first, and nothing for a singleton.** One key is passed alone and several as a tuple; a path of singletons takes `()`. The path's type says what its keys are: a key missing, extra, of the wrong type, or in the wrong place does not build.
- **`path::<L>(keys)` answers a `Path<L>`**, which holds the path its names and keys spell: no id, as only the engine computes one (ADR-0008: `root_id`, `child_id`), and `path` runs in the actor's code, a guest's among it. The actor never handles an id or a seat.
- **`send_to`, `ask_to`, and `watch` take a typed path wherever they take a reference**, `send_to_all` excepted: a send to many writes its bytes once for references, which need no lookup. At the send, the table resolves the path, making its id as it does when it seats each actor, and **the actor there must be of `L`'s kind.** A reference was proven when it was made, by the spawn, cast, or resolve that saw its actor's kind; a path proves nothing, so it is checked at each send, as `turn.child` checks. A place holding an actor of another kind (one of the same name, so the same id, and another layout) is treated as empty. Two types of one name and one layout are one kind (ADR-0006), and are not told apart.
- A typed path names a place, not an actor that is live. Mail sent to one that holds no live actor is dropped, and an ask is answered `Unanswered::Ended`, as for a reference to an actor that has ended (ADR-0008, ADR-0017). A watch at one is shown the ending at once, `How::Earlier`.
- **The table answers a path in one read, `Table::reach(path, kind)`:** the actor there and its seat when it is live and of the kind, and otherwise the place's id in `SeatNumber::NOWHERE`, a seat no actor sits in. Mail to that seat goes through the engine's way for mail to an actor that has gone, which drops it, answers a request `Ended`, and shows a watch the ending, so an empty place needs nothing of its own. A guest's table holds no other actor's type, so its host answers the read in one call out, the kind check with it.

### What a turn may say at a typed path is declared, and the declaration is the dependency

```rust
impl Carries<CreateMesh> for Renderer { type Answers = (Meshed,); }   // what asking a Renderer for a mesh gets

type Sends = (Asks<Path<Renderer>, CreateMesh, Made>, Tells<Path<Renderer>, Release>);

turn.ask_to(path::<Renderer>(()), &CreateMesh { .. }, &Made::Ground)?;
turn.send_to(path::<Renderer>(()), &Release::Mesh(mesh))?;
```

- **An ask at a typed path is declared with the path**, `Asks<Path<L>, K, C>`, and **the answer is read from the path's type `L`**: it says what it answers each kind it can be asked with `Carries<K>`, as a named protocol does (ADR-0016), beside its handler. Its handler's declared replies must be the answer it carries, checked in the marker and not at the call. An ask is answered with one kind, so the check is `RepliesWith<A>`: each reply its handler declares is `A`. This departs from checking it as a protocol's are (`AnsweredBy`), whose place in the answers is inferred, and a `DeclaredBy` impl has no parameter for it to be inferred into. A type carries only the kinds it can be asked, one by one, so no list of its kinds is kept beside `Handles`. A kind a type does not carry cannot be asked of it at a typed path.
- **This departs from ADR-0016**, where only a named protocol carries kinds: an actor's type may carry them too, for the asks made of it by its type. Named protocols stay for asking any actor that handles a set of kinds, through a `ProtocolRef`.
- **A send at a typed path is declared with the path**, `Tells<Path<L>, K>`, the counterpart of `Asks`, and builds only when `L` handles `K`.
- Both are checked at build time as every send is (`Allows`), and an ask only when the asker handles the answer (`DeclaredBy<Self>`).
- **Their `Sending` targets the path, `Target::Path(segments)`, so a type's declaration holds every path it reaches.** No dependency is declared beside the sends: a declared send at a typed path is one.

**A spawn is refused, before the child's `begin` runs, when a typed path its type reaches holds no live actor.**

- The refusal is `Refused::Missing(KindName)`, holding the name of the path's type `L`.
- It is checked for every path of singletons. A path with a key is a place many actors fill and empty while the program runs, so it is not checked at the spawn, and a send to one that holds no live actor is dropped.
- The check reads the child's whole declaration: its `begin`, every handler, and its `end`, which may send at a typed path as it may send to anyone.
- **A root is checked as it is made** (`Engine::add_root`, beside its check of stages). So the chassis makes a root before any root that reaches it.
- **An actor counts as live for the children its own `begin` spawns**, though it is not yet in the table: a root whose `begin` spawns a helper that reaches it back is not refused.
- **When a child would be refused for both, `Unstaged` is the refusal**: stages are checked first.
- **The check covers one level.** A root whose `begin` spawns a child that reaches a root made later has that child refused, and its `begin` decides what that means; one that fails is refused with `Refused::Began` and its fault, not `Missing`.
- A path checked at a spawn may hold no live actor later, when its actor ends. Nothing about a send to it changes: it is dropped, and an ask answered `Ended`.
- **A host's first roots are also checked as the program is built.** `Engine::new` hands out a `Setup<S>`, whose `S` lists the roots made so far, `()` and then `(R, S)`; `Setup::add_root` makes one and adds it to the list, and `ready()` hands out the `Engine`, building only when each root reaches at a typed path only roots made before it. So a chassis whose root reaches one made after it fails `cargo check`, and the error names the root missing.
  - **The check is at `ready()`, not at each `add_root`**: a set's membership is proven through an index the compiler infers, which needs a type parameter of its own, and `add_root::<R>` names its root with a turbofish, which would have to name the index too (`add_root::<R, _>`). `ready` has no turbofish, so it carries the index.
  - **It proves only a path whose one segment is a singleton root.** That a root was made says nothing of the children its `begin` spawns, so a deeper path is checked where its actor is made, as is a keyed one.
  - Each `add_root` changes the setup's type, so roots of one instanced type made in a loop are made by `add_roots`, one entry in the list for them all.
  - `ActorKey` gains `Keying`, `Singleton` or `Instanced`, `KEYED` as a type, which the check reads.
  - The check as each root is made stays, for a root made once the engine runs, a child spawned in a turn, and a guest's section, none of which a setup's types see.
- **Roots may be added between steps while the program runs**, as `add_root` allows today; the check reads the table as it stands, and nothing here needs the set of roots to be fixed.
- **An actor moved onto a newer blob of its kind (ADR-0009) is checked against the new blob's declaration** as a child of it would be, and the move is refused when that would be.
- The check reads the table a spawn's turn reads, as it reads which stages run (`Table::runs_stage`), through the read a send at a path makes, `Table::reach`: a place is live when it answers a seat. It is answered natively from the table and for a guest in a call out (ADR-0015). It runs for every child, whatever its code, as the check of stages does.
- **A guest's turn checks a child before it asks its host to begin it**, as it checks its stages, so no host refuses a guest's spawn for a path. `Refused::Missing` crosses the call out that answers a guest's spawn (`records::refusal`) as a code of its own, without the name, as `Unstaged` crosses without its stage. **This departs from the name crossing with it**, which nothing would read.
- **A declaration's path segment holds its type's kind as well as its name**, in a guest's section as natively, so the check, and a host's check of a guest's send at a path, compare kinds and not names: a place holding an actor of another layout under the same name is empty to both.
- A guest's module that skipped the check makes an actor whose sends to a path that holds nothing are dropped, as for one whose actor ended; its host checks nothing more.

### `turn.parent()` and `turn.child`: reaching what is not static

- **A turn is typed by its actor's type**, `Turn<'_, Self, S>`, `Beginning<'_, Self, S>`, `Ending<'_, Self, S>`, so what a turn offers can be read from where its actor's type stands. **This departs from ADR-0009 and ADR-0013**, whose turn was typed only by what its handler may send, which names no actor: nothing in a handler's signature led to its type's `Parent`. Every handler, `begin`, and `end` names `Self` in its turn, and a helper that takes a turn takes its actor's type as a parameter.
- **`turn.parent()`** is the actor directly above, inferred from `Self::Parent` (`Parented`): an `ActorRef<P>` when it is the one type `P`; an `Option<ActorRef<Self>>` when it is `Recursive`, none for a root; and an `Option<ErasedActorRef>` when it is `Anywhere`, none for a root, as which it stands beneath is a fact of the run. A root's type has no `parent()`: it does not build. **This departs from an erased parent for `Recursive`**: its parent is of its own type when it has one, so it is typed. It is the actor that spawned this one, live or not, as a reference is valid while it exists (ADR-0008): mail to one that has ended is dropped. `turn.send_to(turn.parent(), &mail)` replaces `turn.send(To::Parent, &mail)` where the parent is typed, and is checked as the program is built.
- **`turn.child(parent, &key)`** finds a child by its type beneath a reference held: `let unit: Option<ActorRef<Unit>> = turn.child(army, &7)`. It builds only when the child's `Parent` admits the parent's type: an `ActorRef<P>` whose type `C` may stand beneath, or an `ErasedActorRef` for a `C` that stands `Anywhere` (`ParentOf<C>`). The child's id is made from the parent's and the child's segment, and the answer is the reference when the table holds a live actor there of `C`'s kind, and nothing otherwise. `C` is named where the reference goes, or at the call, `turn.child::<Unit>(army, &7)`, as for a spawn. A turn looks beneath its own actor with `turn.me()`, an `ActorRef<Self>`: the turn is typed by its actor, so the reference to it is typed too.
- A child is found while it is live, whether or not its parent is: a child outlives its parent (ADR-0009).
- Both see what a resolve sees (ADR-0011): the table as the step began, and in a fused turn's run what that run stood up and ended (ADR-0013). A child this turn stood up is found, as a send by `To::Child` finds it. The same holds for a typed path.
- A guest reads each in one call out. For `turn.child` the host is handed the parent, the child's segment, and its kind, `Table::reach_child`, and answers with the child and its seat when one of the kind is live there, as `Table::reach` answers for a path. A child of the turn's own actor its host does not hold takes a second call, for the id to look it up among those the turn stood up. **This departs from the host answering with the kind** for the guest to compare: the host compares, as it does for a path.

### References

**A fourth way to reach an actor is added to the three of ADR-0008: a typed path, named by the type of its last segment.** **This departs from ADR-0008**, which is edited to say so. It is not a reference the actor holds: the engine finds the actor there at each send, as it finds the sender's parent for `To::Parent`. It reaches the same actor a resolve of its path would, through the types that path is made of, which a resolve's cast cannot give (ADR-0016).

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
