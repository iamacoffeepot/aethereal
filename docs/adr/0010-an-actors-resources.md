# ADR-0010: An actor's resources

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

Some of what an actor works with is not data: a socket, an open file, a GPU object. Such a value has no bytes that mean anything elsewhere, so it cannot be a field of a kind (ADR-0004), and an effect is data (ADR-0009), so it cannot ride in a `Spawn` or a `Send` as itself.

Left there, three things go wrong. An actor that holds a socket could not be a kind, and an actor is one (ADR-0009). A parent cannot hand a child the file it opened for it. And the place such values do live is up to each actor, so nothing closes them when the actor ends unless its author remembered to.

Letting two actors hold the same socket does not help. One mail at a time protects only what one actor owns. A value two actors can both open needs a lock as soon as turns run side by side, every use by the second holder is a round trip to wherever the value lives, and no moment is the one at which it should be dropped.

## Decision

**A resource is a value that is not data, and it has exactly one owner.** The owner is an actor. The engine keeps each actor's resources beside that actor, and the actor holds a handle to each.

```rust
pub trait Resource: Any + Send {}          // a value that is not data
pub struct Handle<R: Resource> { .. }                           // mine
pub struct Given<R: Resource> { .. }                            // on its way to a new owner
```

**A type is a resource because its author says so.** `Resource` has no methods. Implementing it for a type states that the type's values are not data: nothing a kind's fields can spell holds what the value is. A socket, an open file, and a GPU object are resources. An actor's own working state is not, and where it is a tree it is an arena with indices (ADR-0009). A resource is `Send`: a native host runs its owner on any of its workers, and the resource goes with it. The engine tells resource types apart by Rust's own type id, and records one dropped on its way by its type's name.

**A `Handle<R>` is the owner's own.**

- Its shape is one unsigned 64-bit integer, the resource's ticket, which no other resource of the engine has had: the writer whose turn kept it, in its high bits, and that writer's count of what its turns have kept, as a blob's id is made (ADR-0014). A resource keeps its ticket from when it is kept until it is dropped, whoever owns it. A kind may hold one, so an actor may.
- It crosses `Within` and no other boundary (ADR-0004). It cannot be in mail or a config: a kind that holds one has no codec for `Actors`, and the send does not compile.
- It is neither `Clone` nor `Copy`. There is one handle to a resource.
- Native code opens it, in the owner's turn: `turn.open(&mut handle)?` gives the `R`. Access is direct, because nobody else can hold the handle. A guest may hold a handle and cannot open one. A closing turn may open what its actor keeps, for a last use.
- A native actor takes a value it made into its keeping with `turn.keep(value)?`, which returns the handle. `keep` takes a `Resource` and nothing else. The actor makes the value with its own code and `keep` moves its ownership to the actor: there is no factory the engine runs. Making, keeping, and opening change only what the actor owns, so none is an effect (ADR-0009).
- A `begin` may take what its config gives it and keep what it makes. If it then fails, the child was never live and what it held is dropped with it.

**Giving is its own act, with its own type.**

```rust
impl Turn<'_> {
    pub fn give<R>(&mut self, handle: Handle<R>) -> Result<Given<R>, Fault>;   // I no longer have it
    pub fn take<R>(&mut self, given: Given<R>) -> Result<Handle<R>, Fault>;    // now it is mine
}
```

- `give` takes the handle by value, so the giver no longer holds it once it has given. That is checked when the program is built. A closing turn may give, so a resource outlives its owner in the owner's last mail.
- A `Given<R>` has the same shape as a handle, and the same ticket. It crosses `Actors` and no other boundary: it may be in mail and in a config, and it cannot be a field of an actor.
- The engine moves the resource to its new owner when it applies the effect that carries the `Given`. The receiver calls `take` in the turn that handles the mail, or in its `begin` for a config, and the resource is the receiver's under the ticket it came with, so taking needs nothing answered by whoever holds it. One `Given` goes with one piece of mail: a send that writes it into a second, or into a send to many, is refused.
- A `Given` that no effect carries by the end of its turn, or that its receiver does not take in the turn it arrives, is dropped, and its resource with it.

**The engine checks every ticket where it is used.** `open`, `give`, and `take` each confirm that the ticket names a resource of type `R` that the actor owns, or that came to it from the sender of the mail it handles, or from the actor above it for a config; each answers a `Fault` when it does not, which `?` makes the turn's (ADR-0009). The writer at `Actors` confirms that a `Given` it writes is one the turn gave and no other mail carries, and refuses the send otherwise. A guest holds tickets and no values, so what it gives and takes is a claim its host holds it to as it hands the turn back, and a false claim fails the turn with the same fault (ADR-0015). So bytes a guest makes up are refused, though Rust did not check that guest. The bytes themselves are not checked where they are read: mail can be read more than once, and only `take` can say that a resource is taken once.

**A resource ends with its owner or with its mail.** When an actor closes, the engine drops its resources after its closing turn. When mail that carries a `Given` is not delivered, the engine drops the resource with the mail and records it: in the log of the actor that gave it when no mail carried it, when its turn failed, or when its receiver had gone or ended before handling the mail, and in the receiver's log when the receiver handled the mail and did not take it. What a `begin` that fails took or gave is dropped with its actor, which was never live.

**What a resource brings in from outside the engine reaches its owner as mail, and its owner is the sender.** A key press, bytes on a socket, a read that finished: the host hands each to the engine between two steps, naming the resource. The engine delivers it to the actor that owns that resource, as mail from that same actor which says which handle it is for, and the owner handles it in the next step (ADR-0011). Mail from outside the engine therefore has an actor as its sender, as all mail does (ADR-0009), and what one resource brings in keeps its order. Whoever else should hear of it is mailed by the owner.

**The chassis hands a root its resources the same way.** It puts them in the root's config as `Given` values, so a root is born as any actor is. The chassis has no turn, so it hands each value to the engine as it builds it, with `engine.hand(value)`, and the engine keeps the value and returns the `Given` that names it. The engine gives the root's `begin` the ones its config holds, and what that `begin` does not take is dropped and recorded in the root's log.

**A resource is always somewhere.** It is in its owner's keeping, or in the engine's while a `Given` carries it to its new owner. The engine makes a `Given` and moves it, and it never holds one that is going nowhere.

## Consequences

- Every actor can be a kind, native ones included: what an actor holds that is not data, it holds as handles.
- A parent opens what a child needs and gives it in the child's config. Nothing travels as a live Rust value between actors.
- An actor that wants the use of another's resource asks its owner by mail. There is no shared handle.
- Nothing checks that a type implementing `Resource` could not have been data: Rust has no bound that says what a type is not. A native actor's author can put ordinary state behind a handle by implementing `Resource` for it, and the engine can then neither show that state nor carry it onto new code. Each such type is one line that names it, and that line is judged by reading it. A guest cannot do this at all, since it opens no handle and makes nothing to keep: it holds, gives, and takes what native code made.
- Nothing an actor owns is left open when it ends, whether or not its author wrote a close for it.
- An actor that holds a handle cannot be written to a file or sent to another engine: a handle does not cross `Bare` or `Engines`. Replacing an actor's code keeps its resources, because its state crosses `Within`.
- `Within` offers the owner's resources and `Actors` offers the ones in transit. These are the first offers either boundary makes (ADR-0004).
- Mail that carries a resource is never fused (ADR-0013): the resource reaches its receiver as the receiver's part is folded, after the mail's step, so a turn that gives sends all its mail as mail.
- An actor that has never kept a resource holds no more for it than a pointer's width, beside it in its seat.
- A turn that makes or uses a resource acts on the world outside its effects. A failed turn does not undo that, and such a turn cannot be run again from its record: a turn repeats only as far as its actor's own code does (ADR-0009).
- **Not decided here:** what kind of mail each type of resource brings its owner, and how an actor asks a resource for work that takes time. A turn does not block, and a browser host cannot.
- Two kinds that differ only in what their handles hold have the same layout (ADR-0006), since each handle is one integer. The engine's check of the ticket is what tells them apart.

## Alternatives considered

- **A handle that can be copied into mail, shared by whoever holds it.** One type. The second holder's every use becomes a round trip, parallel turns need a lock, and nothing says when the value is dropped.
- **One handle type that moves when sent, enforced when the engine applies the send.** One type fewer. The giver's handle would still look usable, and using it would fail in a later turn where a build error is possible.
- **A native parent passing a live value to a native child at spawn.** No table and no ticket. The effect is then not data, a guest cannot do the same, and a turn that spawns can no longer be recorded.
- **A factory the actor hands the engine, which the engine runs to make the value.** The engine could then choose when and where the value is made. It is a second way to bring a value in, beside handing one over, which is what `keep` does and what the chassis does at the engine.
- **`keep` taking any value.** Nothing to implement. An author whose state does not fit a kind then moves it behind a handle in one call, and nothing in the program says it was done.
- **Resources only at roots, built by the chassis.** Nothing to add. An actor beneath a root could never own a file or a socket of its own.
