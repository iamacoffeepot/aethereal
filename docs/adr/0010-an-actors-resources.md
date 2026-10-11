# ADR-0010: An actor's resources

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Some of what an actor works with is not data: a socket, an open file, a GPU object. Such a value has no bytes that mean anything elsewhere, so it cannot be a field of a kind (ADR-0004), and an effect is data (ADR-0009), so it cannot ride in a `Spawn` or a `Send` as itself.

Left there, three things go wrong. A native actor that holds a socket has state that is not a kind. A parent cannot hand a child the file it opened for it. And the place such values do live is up to each actor, so nothing closes them when the actor ends unless its author remembered to.

Letting two actors hold the same socket does not help. One mail at a time protects only what one actor owns. A value two actors can both open needs a lock as soon as turns run side by side, every use by the second holder is a round trip to wherever the value lives, and no moment is the one at which it should be dropped.

## Decision

**A resource is a value that is not data, and it has exactly one owner.** The owner is an actor. The engine keeps each actor's resources beside that actor, and the actor's state holds a handle to each.

```rust
pub struct Handle<R> { .. }      // mine
pub struct Given<R> { .. }       // on its way to a new owner
```

**A `Handle<R>` is the owner's own.**

- Its shape is one unsigned 64-bit integer, the resource's ticket. A kind may hold one, so an actor's state may.
- It crosses `Within` and no other boundary (ADR-0004). It cannot be in mail or a config: a kind that holds one has no codec for `Actors`, and the send does not compile.
- It is neither `Clone` nor `Copy`. There is one handle to a resource.
- Native code opens it, in the owner's turn: `turn.open(&mut handle)` gives the `R`. Access is direct, because nobody else can hold the handle. A guest may hold a handle and cannot open one.
- A native actor takes a value it made into its keeping with `turn.keep(value)`, which returns the handle. Keeping and opening change only what the actor owns, so neither is an effect.

**Giving is its own act, with its own type.**

```rust
impl Turn<'_> {
    pub fn give<R>(&mut self, handle: Handle<R>) -> Given<R>;      // I no longer have it
    pub fn take<R>(&mut self, given: Given<R>) -> Handle<R>;       // now it is mine
}
```

- `give` takes the handle by value, so the giver no longer holds it once it has given. That is checked when the program is built.
- A `Given<R>` has the same shape as a handle. It crosses `Actors` and no other boundary: it may be in mail and in a config, and it cannot be kept in state.
- The engine moves the resource to its new owner when it applies the effect that carries the `Given`. The receiver calls `take` in the turn that handles the mail, or in its first turn for a config.
- A `Given` that no effect carries by the end of its turn, or that its receiver does not take in the turn it arrives, is dropped, and its resource with it.

**The engine checks every ticket.** At a boundary the reader confirms that the ticket names a resource of type `R` that the actor owns, or that was given to it. When it applies an effect it confirms the sender owned what it gave. A ticket that fails either check fails the turn (ADR-0009). So bytes a guest makes up are refused, though Rust did not check that guest.

**A resource ends with its owner or with its mail.** When an actor closes, the engine drops its resources after its closing turn. When mail that carries a `Given` is not delivered, the engine drops the resource with the mail and records it.

**The chassis hands a root its resources the same way.** It puts them in the root's config as `Given` values, so a root is born as any actor is.

## Consequences

- A native actor's state can be a kind: what it holds that is not data, it holds as handles.
- A parent opens what a child needs and gives it in the child's config. Nothing travels as a live Rust value between actors.
- An actor that wants the use of another's resource asks its owner by mail. There is no shared handle.
- Nothing an actor owns is left open when it ends, whether or not its author wrote a close for it.
- State that holds a handle cannot be written to a file or sent to another engine: a handle does not cross `Bare` or `Engines`. Replacing an actor's code keeps its resources, because its state crosses `Within`.
- `Within` offers the owner's resources and `Actors` offers the ones in transit. These are the first offers either boundary makes (ADR-0004).
- Two kinds that differ only in what their handles hold have the same layout (ADR-0006), since each handle is one integer. The engine's check of the ticket is what tells them apart.
- Whether every actor, native ones included, must now offer its state (ADR-0009's `Stateful`) is not decided here.

## Alternatives considered

- **A handle that can be copied into mail, shared by whoever holds it.** One type. The second holder's every use becomes a round trip, parallel turns need a lock, and nothing says when the value is dropped.
- **One handle type that moves when sent, enforced when the engine applies the send.** One type fewer. The giver's handle would still look usable, and using it would fail in a later turn where a build error is possible.
- **A native parent passing a live value to a native child at spawn.** No table and no ticket. The effect is then not data, a guest cannot do the same, and a turn that spawns can no longer be recorded.
- **Resources only at roots, built by the chassis.** Nothing to add. An actor beneath a root could never own a file or a socket of its own.
