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
pub trait Actor: Kind + Encode<Within> + Decode<Within> {
    type Config: Kind + Encode<Actors> + Decode<Actors>;

    fn begin(config: Self::Config, turn: &mut Turn<'_>) -> Result<Self, Fault>;
    fn handle(&mut self, mail: Mail<'_>, turn: &mut Turn<'_>) -> Result<(), Fault>;
    fn end(self, turn: &mut Ending<'_>);
}
```

An actor's life is three sorts of turn. `begin` is the turn in which it is born, from the config its `Spawn` carried. Each piece of mail after that is a `handle`. `end` is its closing turn, and it runs once, whichever way the actor came to close.

An **effect** is one thing the actor asks of the engine. The first effects are:

```rust
pub enum Effect {
    Send { to: To, kind: KindId, bytes: Vec<u8> },
    Spawn { of: KindName, key: ChildKey, config: Vec<u8> },
    Close,
}
```

- A handler changes its own state and nothing else. Everything it wants of the engine it asks for through `turn`, which adds an effect to the turn's list.
- **The engine alone applies effects**, in the order the turn asked for them, after the turn returns `Ok`.
- **Nothing a turn asks for happens during the turn.** Mail it sends is delivered afterwards, so no actor runs inside another actor's turn.
- `Spawn` creates a child beneath the asking actor and nowhere else.
- **An actor's end is its own decision or its parent's end, and nothing else.** `Close` closes the actor that asks for it, and no effect closes another actor. A parent that wants one child gone while it carries on sends that child mail saying so, and the child closes itself.
- **A closing actor signals its children.** A child does not outlive the actor it stands beneath. When an actor closes, by its own `Close` or because its turn failed, the engine carries that to every actor beneath it, deepest first. Each is handed its closing turn, `end`, in which it answers what it owes and sends its last mail. It does not choose whether or when.
- **The closing turn cannot fail and cannot be repeated.** `end` takes the actor by value and returns nothing, so there is no actor left to hand a second one. Its `Ending` offers what a last turn may ask for, sending and giving, and neither `Spawn` nor `Close`. The engine applies its effects, then drops the actor and what it owned (ADR-0010). An actor whose turn failed is still handed `end`; a guest that trapped is not, since its memory can no longer be trusted, and the engine closes it without one.
- An effect is plain data, and so is the list.

**A turn reads the engine; it does not write it.** `turn` answers questions from the engine's state as it stood when the turn began: the sender of the mail, and whether a path names an actor (ADR-0008's resolve). A read is not an effect.

**Who a turn may send to is a closed set.**

```rust
pub enum To {
    Parent,
    Child(KindName, ChildKey),
    Sender,
    Held(ActorRef),
}
```

`Child` names an actor by key because the sender owns that part of the tree (ADR-0008); a turn may spawn a child and send to it in the same list. Every other case is an actor the engine has already established exists. There is no send to a bare path.

**What a send or a spawn carries crosses between actors.** `turn.send(to, &kind)` requires `K: Kind + Encode<Actors>` (ADR-0004, ADR-0006), encodes the kind, and adds the `Send`. `turn.spawn::<C>(key, &config)` requires the same of the child's config. A kind that holds a value bound to its own actor is refused when the program is built. The bytes in an effect are always those of a kind that passed this check.

**A turn that fails changes nothing outside its actor.** When `begin` or `handle` returns a `Fault`, or a guest traps, the engine drops the turn's effects, closes the actor, and tells its parent. The actor's state is not rolled back, and it is not used again.

**An actor is a kind.** The type that implements `Actor` is the actor's state, and it is a kind: it has a name, a schema, and a codec for `Within`.

- Every field of an actor crosses `Within`. What an actor holds that is not data, it holds as a handle (ADR-0010). This is the same for a native actor and a guest.
- The engine can take any actor's state out, by encoding the actor, and put it back by decoding it. It does so to replace the actor's code, and may do so to show the state or record a turn.
- An actor's name is its kind's name. It is the name in a path (ADR-0008) and in `Spawn`.
- New code takes over an actor's state as it is when its kind has the same name and the same layout (ADR-0006). A different layout is a different kind, and the new code says how it reads the old one.
- An actor lives decoded, where its runtime keeps it: a native actor as a value the engine owns, a guest in its own memory. The engine does not hold its bytes and does not copy it per turn.

## Consequences

- The rules about mail in flight have one place to live. The engine is the only thing that moves mail and creates or closes an actor, and it sees every request to do so as data.
- A guest is one call per turn: the mail's bytes in, the effects' bytes out, both through the codec of ADR-0004.
- A turn can be recorded and compared: the same state and mail give the same effects, as far as the actor's own code is deterministic.
- A handler cannot learn within a turn whether a send was delivered or a spawn succeeded. It is told by mail in a later turn.
- A read sees the engine as it was when the turn began, so it can be one turn out of date.
- An actor's working fields are limited to what a kind can hold: no `Box`, no recursive type, no `usize`, nothing from a foreign crate. A tree is an arena with indices. A native actor may put a foreign value behind a handle as a resource (ADR-0010); a guest cannot, so a guest's working data fits the vocabulary entirely.
- Where the engine keeps actors is its own choice and not part of this decision. It may keep the native actors of one type side by side. An actor whose fields are all of fixed width has the same bytes in memory as encoded on a little-endian machine (ADR-0002), so such actors could be kept as one stretch of bytes per type, which makes taking their state out, or keeping a copy from before a turn, a copy of that stretch. An actor with a string, a sequence, or a map cannot be changed in place in ADR-0002's bytes, since a field's position depends on the lengths before it.
- **Not decided here, and due before the piece that needs each:** what a piece of mail carries beside its kind and bytes, and how a request and its answer are tied (the next ADR; it adds effects and no second way to act); what the `Actors` and `Within` boundaries offer; when applied mail is delivered and in what order actors run; logging, large payloads, and a value an actor publishes for others to read, each a further effect; and the mail by which a parent learns a child was born, failed, or closed.

## Alternatives considered

- **An actor acts as it runs, through calls the host offers.** Simple to write against, and the three costs under Context.
- **The engine holds each actor's state, and a turn returns the new state.** A failed turn then costs nothing to undo and a guest instance holds nothing. Every turn decodes and encodes the whole state, which falls hardest on an actor with large state that runs every frame.
- **An actor beside a separate state kind it converts to and from.** The actor may then work on any structure it likes. It is a second type and two conversions per actor, nothing checks that the conversion keeps everything, and a native actor could decline to offer it.
- **A failed turn leaves the actor running.** The actor then continues from a state no handler was written to produce.
- **An effect by which a parent closes its child.** The parent owns the path, so it could. The child would then end without running its own close, at a moment it did not choose.
- **A send to any path, checked when the effect is applied.** One case in `To`, and every send is a claim the engine has not checked (ADR-0008).
