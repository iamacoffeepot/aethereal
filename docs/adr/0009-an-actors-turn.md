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
    fn end(self, turn: &mut Ending<'_>) -> Result<(), Fault>;
}
```

These three are what the engine calls. How an author writes the handling of each kind of mail, and how the kinds an actor handles are declared, is a later ADR; whatever it decides arrives at the engine through `handle`.

An actor's life is three sorts of turn. `begin` is the turn in which it is born, from the config its `Spawn` carried. Each piece of mail after that is a `handle`. `end` is its closing turn, and it runs once, whichever way the actor came to close.

An **effect** is one thing the actor asks of the engine. An effect exists only for what the engine itself must do: an actor that could do a thing with its own state and mail gets no effect for it. The first effects are:

```rust
pub enum Effect {
    Send { to: To, kind: KindId, bytes: Vec<u8> },
    Spawn { of: KindName, key: ChildKey },
    Close,
    Watch { of: ActorRef },
    Log { level: Level, text: String },
}

pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}
```

- A handler changes its own state and nothing else. Everything it wants of the engine it asks for through `turn`, which adds an effect to the turn's list.
- **The engine alone applies effects**, in the order the turn asked for them, after the turn returns `Ok`.
- **Nothing a turn asks for happens during the turn.** Mail it sends is delivered afterwards, so no actor runs inside another actor's turn.
- `Spawn` creates a child beneath the asking actor and nowhere else.
- **An actor stands up its own children, in its own turn.** `turn.spawn` names the child's kind, its key, and its config, and runs the child's `begin` then and there. It returns a reference to the child, or the reason there is none: the key is taken or closed, the path would be too deep (ADR-0008), no code is known by that name, or `begin` returned a `Fault`. The parent handles a child that failed to begin in the handler that asked for it.
- **The `Spawn` effect records a child that is already stood up.** The turn holds the child aside, and adds its `Spawn` and then the effects its `begin` asked for, as the child's. When the engine applies the `Spawn` the child is live. Until then it exists for no one but its parent: its path resolves for nobody else, and the reference the parent holds can leave the turn only inside a later effect of the same list.
- **Only the parent changes its branch**, so the turn knows whether a key is free from the engine as it stood when the turn began and from the turn's own spawns.
- **If the parent's turn fails, the children it stood up are dropped** with what they owned (ADR-0010), having never been live. They are handed no `end`.
- **An actor's end is its own decision or its parent's end, and nothing else.** `Close` closes the actor that asks for it, and no effect closes another actor. A parent that wants one child gone while it carries on sends that child mail saying so, and the child closes itself.
- **A closing actor signals its children.** A child does not outlive the actor it stands beneath. When an actor closes, by its own `Close` or because its turn failed, the engine carries that to every actor beneath it, deepest first. Each is handed its closing turn, `end`, in which it answers what it owes and sends its last mail. It does not choose whether or when.
- **The closing turn runs once, and the actor ends whatever it returns.** `end` takes the actor by value, so there is no actor left to hand a second one. Its `Ending` offers what a last turn may ask for: sending, giving, and logging. It offers no `Spawn`, `Close`, or `Watch`. Asking for an effect can fail, so `end` returns a `Result`: on `Ok` the engine applies its effects, and on a `Fault` it applies the `Log` effects and drops the rest. Either way the engine then drops the actor and what it owned (ADR-0010) and tells its parent how it ended. An actor whose earlier turn failed is still handed `end`; a guest that trapped is not, since its memory can no longer be trusted.
- **A panic in a native actor is a bug in the program, not a failed turn.** The engine does not catch it: a browser host cannot unwind (ADR-0001). Every failure an actor can meet is a `Fault` it returns.
- **`Watch` asks to be told when another actor ends.** Only the engine knows that an actor has ended, so an actor cannot learn it any other way. The watcher names the actor by a reference it holds, and is sent one piece of mail when that actor ends, however it ends. If the actor has already ended when the effect is applied, the mail is sent then. Watching an actor already watched changes nothing, so the watcher is told once. A watch ends with its watcher. A parent is told of its child's end without asking.
- **`Log` writes a line to the actor's log**, which the engine keeps. It is an effect so that the engine decides what happens to a line when the turn fails, and so that a guest's lines leave it with the rest of the turn.
- An effect is plain data, and so is the list.

**A turn reads the engine; it does not write it.** `turn` answers whether a path names a live actor (ADR-0008's resolve) from the engine's state as it stood when the turn began. A read is not an effect.

- A resolve never finds an actor whose `Spawn` has not been applied, so a reference is never to an actor whose `begin` failed.
- Mail caused by a spawn finds the child: a parent that spawns and then sends has made the child live before that mail is queued.
- A resolve made while handling unrelated mail can miss an actor that is about to be live. An actor that must know waits to be told.

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

**What a send or a spawn carries crosses between actors.** `turn.send(to, &kind)` requires `K: Kind + Encode<Actors>` (ADR-0004, ADR-0006), encodes the kind, and adds the `Send`. `turn.spawn` requires the same of the child's config. A kind that holds a value bound to its own actor is refused when the program is built. The bytes in an effect are always those of a kind that passed this check.

**A turn that fails changes nothing outside its actor.** When `begin` or `handle` returns a `Fault`, or a guest traps, the engine closes the actor and tells its parent. The actor's state is not rolled back, and it is not used again.

- On a `Fault` the engine applies the turn's `Log` effects, in order, and drops every other effect. A failed turn is the one whose lines are most wanted.
- A guest that traps returns no list, so the lines of that turn are lost with its other effects. The engine logs the trap itself.

**Code is a prototype an actor is made from, and a turn reads it.** The engine keeps a repository of code. Each entry is a blob, named by what it holds, and the kinds of actor it can make. Making an actor from an entry leaves the entry as it was, so one entry makes any number of actors.

- A turn sees the repository as it stood when the turn began. The repository changes only when the engine applies an effect, so what a turn sees does not change under it.
- Whichever runtime runs the parent stands the child up. A native parent calls the child's `begin`. A guest's host makes the child's instance from the repository's entry and calls its `begin`, inside the parent's turn; a guest spawning a kind from its own module does so itself.
- The engine runs no actor's code during another actor's turn. It applies the `Spawn` afterwards.
- How code enters the repository, the effect that adds it, and what is checked when it does are a later ADR.

**An actor is a kind.** The type that implements `Actor` is the actor's state, and it is a kind: it has a name, a schema, and a codec for `Within`.

- Every field of an actor crosses `Within`. What an actor holds that is not data, it holds as a handle (ADR-0010). This is the same for a native actor and a guest.
- The engine can take any actor's state out, by encoding the actor, and put it back by decoding it. It does so to replace the actor's code, and may do so to show the state or record a turn.
- An actor's name is its kind's name. It is the name in a path (ADR-0008) and in `Spawn`.
- Whether new code can take over an actor's state as it is has a test: its kind has the same name and the same layout (ADR-0006). Replacing an actor's code is a later ADR.
- An actor lives decoded, where its runtime keeps it: a native actor as a value the engine owns, a guest in its own memory. The engine does not hold its bytes and does not copy it per turn.

## Consequences

- The rules about mail in flight have one place to live. The engine is the only thing that moves mail and creates or closes an actor, and it sees every request to do so as data.
- A guest is one call per turn: the mail's bytes in, the effects' bytes out, both through the codec of ADR-0004. A spawn of code from the repository is one more call, out to its host. The host must be able to make an instance without waiting.
- A turn can be recorded and compared: the same state and mail give the same effects, as far as the actor's own code is deterministic.
- A handler learns within the turn whether a spawn succeeded. It cannot learn whether a send was delivered; it is told by mail in a later turn.
- A child's `begin` runs in its parent's turn and is counted against it. A `begin` that spawns runs its children's the same way, to the depth a path allows.
- A resolve sees the engine as it was when the turn began, so it can be one turn out of date.
- An actor's working fields are limited to what a kind can hold: no `Box`, no recursive type, no `usize`, nothing from a foreign crate. A tree is an arena with indices. A native actor may put a foreign value behind a handle as a resource (ADR-0010); a guest cannot, so a guest's working data fits the vocabulary entirely.
- Where the engine keeps actors is its own choice and not part of this decision. It may keep the native actors of one type side by side. An actor whose fields are all of fixed width has the same bytes in memory as encoded on a little-endian machine (ADR-0002), so such actors could be kept as one stretch of bytes per type, which makes taking their state out, or keeping a copy from before a turn, a copy of that stretch. An actor with a string, a sequence, or a map cannot be changed in place in ADR-0002's bytes, since a field's position depends on the lengths before it.
- **Not decided here, and due before the piece that needs each:** what a piece of mail carries beside its kind and bytes, and how a request and its answer are tied (the next ADR; it adds effects and no second way to act); what the `Actors` and `Within` boundaries offer; when applied mail is delivered and in what order actors run; large payloads, a further effect; the repository of code; and the mail by which a parent learns a child failed or closed.

## Alternatives considered

- **An actor acts as it runs, through calls the host offers.** Simple to write against, and the three costs under Context.
- **The engine holds each actor's state, and a turn returns the new state.** A failed turn then costs nothing to undo and a guest instance holds nothing. Every turn decodes and encodes the whole state, which falls hardest on an actor with large state that runs every frame.
- **An actor beside a separate state kind it converts to and from.** The actor may then work on any structure it likes. It is a second type and two conversions per actor, nothing checks that the conversion keeps everything, and a native actor could decline to offer it.
- **A failed turn leaves the actor running.** The actor then continues from a state no handler was written to produce.
- **The engine stands a child up when it applies the `Spawn`.** The parent then needs none of the child's code and a guest's turn stays one call. Between the `Spawn` and the child's `begin` the path is taken and not live, mail for it waits, and the parent learns of a failed `begin` a turn later, by mail it must match to the spawn.
- **An effect by which a parent closes its child.** The parent owns the path, so it could. The child would then end without running its own close, at a moment it did not choose.
- **An effect that publishes a value other actors read without being sent it.** A reader could then sample a value that changes often, such as a pointer's position, on its own turn and not be woken by each change. The read is a second input to a turn beside its mail, and the mailbox does not record its order: two actors handling the same mail could see the value before or after a third actor's turn, by scheduling. Fixing the read to the mail being handled settles that and makes the engine keep old values while older mail is queued. Mail to subscribers covers the case until a reader needs the saving.
- **Logging as a call the host offers.** A line then leaves at once, so a trapped guest's lines survive. It is a second way to act, and a call across the guest boundary per line.
- **A send to any path, checked when the effect is applied.** One case in `To`, and every send is a claim the engine has not checked (ADR-0008).
