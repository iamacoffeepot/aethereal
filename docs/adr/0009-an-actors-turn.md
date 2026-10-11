# ADR-0009: An actor's turn

- **Status:** Proposed
- **Date:** 2026-10-11

## Context

An actor handles one piece of mail at a time, and while it does it wants things of the engine: mail sent, a child created, itself closed. This ADR decides how it asks.

In the earlier engine an actor acted as it ran. A send left during the handler, a spawn registered a child during it, and each such call was a function the host offered. Three costs followed.

- What happens to a thing in flight when the actor at the other end changes state was answered separately at each call site, and the answers differed. Mail could be destroyed silently, a request left unanswered, a child registered whose parent then failed.
- A handler that failed halfway had already sent some of its mail.
- A guest paid for a call across the guest boundary per operation. In a browser that is the costly part of running a guest.

## Decision

**A turn takes one piece of mail and returns effects.**

```rust
pub trait Actor: Sized {
    fn handle(&mut self, mail: Mail<'_>, turn: &mut Turn<'_>) -> Result<(), Fault>;
}
```

An **effect** is one thing the actor asks of the engine. The first effects are:

```rust
pub enum Effect {
    Send { to: To, kind: KindId, bytes: Vec<u8> },
    Spawn { of: TypeName, key: ChildKey, config: Vec<u8> },
    CloseChild { of: TypeName, key: ChildKey },
    Close,
}
```

- A handler changes its own state and nothing else. Everything it wants of the engine it asks for through `turn`, which adds an effect to the turn's list.
- **The engine alone applies effects**, in the order the turn asked for them, after the turn returns `Ok`.
- **Nothing a turn asks for happens during the turn.** Mail it sends is delivered afterwards, so no actor runs inside another actor's turn.
- `Spawn` creates a child beneath the asking actor and nowhere else. `CloseChild` closes one of its own children. `Close` closes the actor itself. Closing an actor closes everything beneath it first.
- An effect is plain data, and so is the list.

**A turn reads the engine; it does not write it.** `turn` answers questions from the engine's state as it stood when the turn began: the sender of the mail, and whether a path names an actor (ADR-0008's resolve). A read is not an effect.

**Who a turn may send to is a closed set.**

```rust
pub enum To {
    Parent,
    Child(TypeName, ChildKey),
    Sender,
    Held(ActorRef),
}
```

`Child` names an actor by key because the sender owns that part of the tree (ADR-0008); a turn may spawn a child and send to it in the same list. Every other case is an actor the engine has already established exists. There is no send to a bare path.

**What a send carries crosses between actors.** `turn.send(to, &kind)` requires `K: Kind + Encode<Actors>` (ADR-0004, ADR-0006), encodes the kind, and adds the `Send`. A kind that holds a value bound to its own actor is refused when the program is built.

**A turn that fails changes nothing outside its actor.** When `handle` returns a `Fault`, or a guest traps, the engine drops the turn's effects, closes the actor, and tells its parent. The actor's state is not rolled back, and it is not used again.

**An actor keeps its own state between turns.** The engine does not hold it and does not copy it per turn.

**A guest's state is a kind.**

```rust
pub trait Stateful: Actor {
    type State: Kind + Encode<Within> + Decode<Within>;

    fn state(&self) -> Self::State;
    fn from_state(state: Self::State) -> Self;
}
```

The engine takes a guest's state out to replace its code, and puts it back into the new code. The door that runs guests requires `Stateful`. A native actor may offer it and is not required to: a socket, a GPU object, and an open file are not data.

## Consequences

- The rules about mail in flight have one place to live. The engine is the only thing that moves mail and creates or closes an actor, and it sees every request to do so as data.
- A guest is one call per turn: the mail's bytes in, the effects' bytes out, both through the codec of ADR-0004.
- A turn can be recorded and compared: the same state and mail give the same effects, as far as the actor's own code is deterministic.
- A handler cannot learn within a turn whether a send was delivered or a spawn succeeded. It is told by mail in a later turn.
- A read sees the engine as it was when the turn began, so it can be one turn out of date.
- Data that can be rebuilt from the state (an index, a cache) is rebuilt in `from_state`. An author keeps it apart from the state kind.
- **Not decided here, and due before the piece that needs each:** what a piece of mail carries beside its kind and bytes, and how a request and its answer are tied (the next ADR; it adds effects and no second way to act); what the `Actors` and `Within` boundaries offer; when applied mail is delivered and in what order actors run; logging, large payloads, and a value an actor publishes for others to read, each a further effect; and the mail by which a parent learns a child was born, failed, or closed.

## Alternatives considered

- **An actor acts as it runs, through calls the host offers.** Simple to write against, and the three costs under Context.
- **The engine holds each actor's state, and a turn returns the new state.** A failed turn then costs nothing to undo and a guest instance holds nothing. Every turn decodes and encodes the whole state, which falls hardest on an actor with large state that runs every frame. `Stateful` leaves room for an actor type that wants this.
- **A failed turn leaves the actor running.** The actor then continues from a state no handler was written to produce.
- **A send to any path, checked when the effect is applied.** One case in `To`, and every send is a claim the engine has not checked (ADR-0008).
