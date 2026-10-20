# ADR-0022: Topology: what an actor spawns, the capabilities it reaches, and how it finds a child

- **Status:** Proposed
- **Date:** 2026-10-20

## Context

An actor's type declares what each of its handlers may do: the kinds it sends and to whom, the stages it subscribes to, the actors it watches, the jobs it runs (ADR-0013: `Marker`, `Sending`, `Target`; ADR-0018; ADR-0020). It declares nothing of where actors stand. Any actor may spawn a child of any type (`Turn::spawn::<C>` has no bound), and the chassis may make a root of any type.

So nothing typed reaches an actor the asker did not make or was not handed. An actor comes to hold a reference in three ways (ADR-0008): it is handed one in mail, it is told the sender of mail, or it resolves a path and is answered with an `ErasedActorRef`. An erased reference casts at runtime only to a tuple protocol, and asking needs a named protocol, which only a typed `ActorRef<R>` converts to (ADR-0016: `ActorRef::protocol`; ADR-0017). So an actor cannot ask a capability, a root that fronts a host system for every other actor (ADR-0019), unless it was handed a reference to it: the renderer (ADR-0021) cannot be asked for a mesh. A guest cannot name the renderer's type at all, since that type holds wgpu values and does not build into a guest. And reaching a child of another actor means writing its path out of names and casting what it resolves to.

The earlier engine came to the same needs. It declared a dependency on a root singleton, refused a spawn before `init` when one was not live, and minted a typed reference to it in a turn (its ADR-0230: `depends(R)`, `actor_ref::<R>()`). It declared placement on types, `Root` and `ChildOf<P>` (its ADR-0166), and resolved singletons by type from a scope (its ADR-0098). Its spawner declared the children it spawns (`#[actor(spawns(..))]`).

## Decision

**What an actor may do to the tree is declared as the rest of what it may do is: per handler, beside its sends** (`BeginSends`, `Handles::Sends`, `EndSends`), and read by the engine from its declaration.

### A capability is a protocol with one root

```rust
pub trait Capability: Protocol {
    /// The name of the singleton root that answers this protocol: the name of its type.
    const ROOT: KindName;
}

impl Capability for Rendering { const ROOT: KindName = Renderer::NAME; }
```

- A capability is a named protocol (ADR-0016) that one singleton root answers. The protocol, its kinds, and this constant live where a guest can build them; the root's type does not need to.
- **An actor never handles the root's name.** The constant is part of the capability's declaration, as a kind's `NAME` is part of the kind's (ADR-0006), and only the engine reads it.
- A singleton root's id is a function of its name alone (ADR-0008: `root_id`), so the engine finds the root from the constant: it computes the id, as only it does, and reads the root's seat from the table.

### An actor reaches a capability by naming it, and the declaration of the send is the dependency

```rust
impl Handles<Start> for Game {
    type Sends = (Asks<Rendering, CreateMesh, Made>, ToAny<Rendering, Release>);

    fn handle(&mut self, _: Start, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.ask_to(Rendering, &CreateMesh { .. }, &Made::Ground)?;
        turn.send_to(Rendering, &Release::Mesh(mesh))?;
        Ok(())
    }
}
```

- **`ask_to` and `send_to` take a capability where they take a reference**: the unit value of its protocol's type. The protocol is named at the call, as a value, so nothing is written as a turbofish and every other parameter is inferred.
- **What the actor may say to the capability is checked at build time, as every send is:** the call builds only when the turn's sends list the ask or the send (`Allows`), and an ask only when the actor handles the answer (`BackTo<Self>`).
- **That the capability's root exists is checked at run time, as early as it can be.** Each declared ask or send through a capability is a `Sending` whose target is the capability's root, `Target::Root(&'static KindName)`. Its declaration therefore holds every capability a type reaches, and nothing is declared twice.
  - **A spawn is refused, before the child's `begin` runs, when a capability its type reaches has no live root**, or its root's type does not handle every kind of the protocol, as a cast checks (ADR-0016). The refusal is `Refused::Missing(KindName)`, the capability's root. The check reads the child's whole declaration: its `begin`, every handler, and its `end`, which may send to a capability as it may send to anyone.
  - **A root is checked as it is made** (`Engine::add_root`, beside its check of stages), against the roots made before it. So the chassis makes a capability before what reaches it.
  - **A root counts as live for the children its own `begin` spawns**, though it is not yet in the table: a capability whose `begin` spawns a helper that asks it back is not refused.
  - **When a child is refused for both, `Unstaged` is the refusal**: stages are checked first.
- **The check covers one level.** A root whose `begin` spawns a child that reaches a capability made later has that child refused, and its `begin` decides what that means; most fail, and the root is refused with `Refused::Began` and the fault, not `Missing`.
- **A capability's root may end after the spawn that checked it.** A send to it is then dropped, and an ask is answered `Unanswered::Ended` (ADR-0017), as for any actor that has ended (ADR-0008). Nothing about the call changes.
- **Roots may be added between steps while the program runs**, as `add_root` allows today; the check reads the table as it stands, and nothing here needs the set of roots to be fixed.
- The check reads which roots are live through the table a spawn's turn reads, as it reads which stages run (`Table::runs_stage`), so the `Table` trait gains a read of a root by name, answered natively from the table and for a guest in a call out (ADR-0015). It runs for every child, whatever its code, as the check of stages does.
- **A guest declares its capabilities in its section**, as it declares the rest (ADR-0015), and asks through one in its turn as natively. Its host finds the root as it finds the target of any send it checks. A guest whose module skipped the check at spawn and asks a capability with no root has its ask answered `Ended` and its send dropped, as for a root that ended, and its host checks nothing more.
- `Refused::Missing` crosses the call out that answers a guest's spawn (`records::refusal`) as the other refusals do, holding the name.

### `Spawns<C>`: a turn spawns only what it declares

- `turn.spawn` and `turn.spawn_from` build only when the turn's sends list `Spawns<C>`, as `subscribe` builds only when they list `Subscribes<K>`.
- **`C` is inferred from where the child goes**, as every type here is: `let unit: ActorRef<Unit> = turn.spawn(&7, config)?`, or the field the reference is kept in. A spawn whose child's type nothing names does not build.
- Its `Sending` is `C`'s kind with `Target::Spawned`, so the engine's declaration of each type holds the children it may spawn.
- `Spawns<C>` in `EndSends` does not build: a closing turn spawns nothing.
- A guest's section carries it. A guest that spawns a type it did not declare has broken its declaration, and its host fails the turn that did, as for a send past it (ADR-0013); a child of other code it asked for in a call out is not begun.
- **An actor moved onto a newer blob of its kind (ADR-0009) is checked against the new blob's declaration** as a child of it would be, and the move is refused when it would be: a new blob may not reach a capability with no root.

### `turn.child`: a child found by its type, beneath a reference held

```rust
let camera: Option<ActorRef<Camera>> = turn.child(host, &());
let unit: Option<ActorRef<Unit>> = turn.child(army, &7);
```

- The parent is any reference the turn holds (`impl Into<ErasedActorRef>`). The child's id is made from the parent's and the child's segment, `C`'s name and `key` (ADR-0008: `child_id`).
- The answer is the reference when the table holds a live actor there and its kind is `C`'s, and nothing otherwise. The kind is checked against the table, so the reference is as sound as one a spawn makes. Nothing checks at build time that the parent's type spawns `C`: a pair that never meets finds nothing.
- A child is found while it is live, whether or not its parent is: a child outlives its parent (ADR-0009).
- It sees what a resolve sees (ADR-0011): the table as the step began, and in a fused turn's run what that run stood up and ended (ADR-0013). A child this turn stood up is found, as a send by `To::Child` finds it.
- A guest asks in one call out, which answers the child's seat and kind from the parent's id and the child's segment.

### References

**A fourth way to hold something to send to is added to the three of ADR-0008: a capability, named by its type.** **This departs from ADR-0008**, which is edited to say so. It is not a reference the actor holds: the engine finds the root from the capability's declaration at each send, as it finds the sender's parent for `To::Parent`. It reaches the same actor a resolve of the root's path would, through a named protocol, which a resolve's cast cannot give (ADR-0016).

## Consequences

- An actor reaches a capability by its protocol, at any depth, with nothing handed down through configs, and a guest reaches it as a native actor does.
- A spawn that cannot reach what it declares is refused where it is asked, before any of the child's code runs, and names the capability that is missing.
- Each type's declaration holds the children it may spawn and the capabilities it reaches. What lists every declaration, so a tool can walk them and print the tree's shape, is left until a tool needs it: native declarations are constants of their types today, and a guest's are its section.
- Every type that spawns declares what it spawns. Each of the 81 spawn sites in the code today lists a `Spawns<C>` in its turn's sends and names its child's type where the reference goes, in place of a turbofish. The change is mechanical, and a site that is missed does not build.
- A turn's sends are a tuple of at most 12 markers (`Allows`), and the markers a spawner and a capability's caller add count toward it. A handler that would declare more is split, or the tuples grow, when one does.
- The chassis makes roots in the order they reach one another. One made too early is refused at boot with the capability it is missing.
- An instanced root is not a capability. One reached by many, a window for each key, is reached as any instanced actor is: by a reference handed over, or by `turn.child` from one held.
- Sends to a child by name, `To::Child(KindName)` and `To::ChildInstanced`, are unchanged.
- A send through a capability costs a computed id and a table read for the root's seat, which may be kept once found; a spawn's check is one pass over a declaration it already reads for stages, and `turn.child` one table read.

## Alternatives considered

- **`add_root` answers an `ActorRef<A>`, and the chassis hands it to the roots that use it, in their configs.** Explicit, and needs nothing new. Every actor beneath a root that reaches a capability would be handed it through each config on the way down, a guest could not hold the type, and nothing would refuse a spawn whose capability is missing.
- **A marker of its own for a dependency, `Needs<R>`, and `turn.root::<R>()` for a reference.** The ask through the capability already says the actor depends on it; a second marker would say it twice. It named the root's actor type, which a guest cannot build.
- **A trait binding a root's type to the protocol it answers, checked at build time.** Proves the binding where both types are known, but a root is the one actor at its name, the capability's crate defines both, and the spawn's check of the root's declaration finds a wrong one at boot.
- **The protocol named by a turbofish, `turn.ask::<Rendering>(..)`.** Rust has every type parameter named once one is, and the index a send's declaration is found at is inferred, so the call would be `ask::<Rendering, _, _, _>`.
- **Placement declared on the child, `ChildOf<P>`, as the earlier engine did.** Would check at build time that `P` may hold `C` in `turn.child`. Every type would list the types it may stand beneath, a reusable child would name every parent it is used under, and the parent's `Spawns` already says the same. The table's kind check already makes the reference sound.
- **The set of roots as a type, so a capability's existence is checked at build time.** Every chassis would build its engine from a typed list of roots, for a check the spawn already makes at boot.
