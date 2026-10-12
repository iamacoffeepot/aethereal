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

    fn begin(config: Self::Config, turn: &mut Beginning<'_>) -> Result<Self, Fault>;
    fn handle(&mut self, mail: Mail<'_>, turn: &mut Turn<'_>) -> Result<(), Fault>;
    fn end(self, turn: &mut Ending<'_>) -> Result<(), Fault>;
}
```

These three are what the engine calls. How an author writes the handling of each kind of mail, and how the kinds an actor handles are declared, is a later ADR; whatever it decides arrives at the engine through `handle`.

An actor's life has three parts. `begin` makes it from a config, and runs inside the turn of the parent that spawns it; a root's runs when the chassis makes it. Each piece of mail after that is a turn of its own, a `handle`. `end` is its closing turn, and it runs once, whichever way the actor came to close.

An **effect** is one thing an actor asks of the engine, and it says which actor asked. An effect exists only for what the engine itself must do: an actor that could do a thing with its own state and mail gets no effect for it. The first effects are:

```rust
pub struct Effect {
    by: ActorId,
    what: What,
}

pub enum What {
    Send { to: To, kind: KindId, bytes: Vec<u8> },
    Spawn { of: KindName, key: ChildKey, from: Code },
    Embed { of: KindName, key: ChildKey, from: Code },
    Faulted,
    Publish { kind: KindName, head: Head },
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

- **`by` is the id of the actor that asked** (ADR-0008's `ActorId`). A turn's list holds the effects of the actor whose turn it is, of every child stood up in that turn, and of the embedded actors it holds, so each effect names its own actor. `Parent` in a `Send`, a `Close`, and a `Watch` are each read against the effect's `by`.
- A handler changes its own state and nothing else. Everything it wants of the engine it asks for through `turn`, which adds an effect to the turn's list.
- **The engine alone applies effects**, in the order the turn asked for them, after the turn returns `Ok`.
- **Nothing a turn asks for happens during the turn.** Mail it sends is delivered afterwards, so no actor runs inside another actor's turn.
- `Spawn` creates a child beneath the asking actor and nowhere else.
- **An actor stands up its own children, in its own turn.** `turn.spawn` names the child's kind, its code, its key, and its config, and runs the child's `begin` then and there. It returns a reference to the child, or the reason there is none: the key is taken or closed, the path would be too deep (ADR-0008), the engine holds no such blob or the blob makes no actor of that kind, or `begin` returned a `Fault`. The parent handles a child that failed to begin in the handler that asked for it.
- **A child is spawned or embedded.** `turn.spawn` makes a child that takes its own mail. `turn.embed` makes a child whose mail is handed to its parent. Each has its own effect, `Spawn` and `Embed`, and no field says where a child's mail goes, so that cannot be written wrong. In everything else the two are the same: the child stands beneath the actor that asked, has a path and an id, has its `begin` run in the parent's turn, and is refused for the same reasons. What this ADR says of a spawn it says of an embed.
- **Embedding changes where mail is routed and nothing else.** An embedded child is a virtual actor its parent contains. It has its own path, id, and references, and what it asks for is an effect by the child, not by its parent. Mail addressed to it is handed to its parent, and the mail says which actor it is for. The parent decides what to do with it.
- **The engine hands an embedded child's mail to the first actor above it that takes its own mail.** A child embedded in an embedded parent is contained in that parent, which is contained in its own, so the mail reaches each through the one that holds it.
- **The `Spawn` effect records a child that is already stood up.** The turn holds the child aside. It adds the `Spawn`, by the parent, and then the effects the child's `begin` asked for, each by the child. The child's id is its parent's continued over the child's segment (ADR-0008), which the turn works out without the engine. When the engine applies the `Spawn` the child is live. Until then it exists for no one but its parent: its path resolves for nobody else, and the reference the parent holds can leave the turn only inside a later effect of the same list.
- **A `begin` that returns a `Fault` leaves `Faulted`.** The turn adds it, by the child, after whatever that `begin` asked for. It is the one entry in a list that reports and asks for nothing. The engine does not make that child live. It applies the child's `Log` effects, kept under the path that never went live, and drops the child's other effects and every effect by an actor that child stood up. A spawn refused before `begin` ran leaves nothing in the list.
- **Only the parent changes its branch**, so the turn knows whether a key is free from the engine as it stood when the turn began and from the turn's own spawns.
- **If the parent's turn fails, the children it stood up are dropped** with what they owned (ADR-0010), having never been live. They are handed no `end`.
- **An actor's end is its own decision or its parent's end, and nothing else.** `Close` closes the actor that asks for it, and no effect closes another actor. A parent that wants one child gone while it carries on sends that child mail saying so, and the child closes itself.
- **A closing actor signals its children.** A child does not outlive the actor it stands beneath. When an actor closes, by its own `Close` or because its turn failed, the engine carries that to every actor beneath it, deepest first. Each is handed its closing turn, `end`, in which it answers what it owes and sends its last mail. It does not choose whether or when.
- **Children end first.** The closing actor's own `end` runs after the `end` of every actor beneath it. Mail a child sends its parent from `end` finds a parent that handles no more mail.
- **`begin` is a first turn, and its `Beginning` offers less than a turn.** It offers no `To::Sender`, since a `begin` has no mail of its own and a root's has none behind it at all; a parent that wants its child to answer someone puts that reference in the child's config. It offers no `Close`: an actor does not end in the turn that makes it.
- **The closing turn runs once, and the actor ends whatever it returns.** `end` takes the actor by value, so there is no actor left to hand a second one. Its `Ending` offers what a last turn may ask for: sending, giving, and logging. It offers no `Spawn`, `Embed`, `Close`, or `Watch`. Asking for an effect can fail, so `end` returns a `Result`: on `Ok` the engine applies its effects, and on a `Fault` it applies the `Log` effects and drops the rest. Either way the engine then drops the actor and what it owned (ADR-0010) and tells its parent how it ended.
- **`end` runs whenever the actor's code can still be run and trusted.** That is after its own `Close`, when its parent ends, and after a turn of its own returned a `Fault`. It does not run for a guest that trapped, since its memory can no longer be trusted, nor for a child that was never live. When the host itself goes away, nothing runs.
- **`end` is told why it runs.**

  ```rust
  pub enum Why {
      Closed,
      ParentEnded,
      Faulted,
  }
  ```

  `Ending` answers with one of these. After a `Fault` the actor's state is whatever the failed handler left, so an `end` asked `Faulted` leaves out what depends on that state being sound.
- **No guarantee rests on `end`.** What must be true once an actor has ended is the engine's to make true, however the actor ended: its resources are dropped (ADR-0010), its children are closed, and its parent and its watchers are told. `end` is the actor's last words: a better answer than the engine's own, or something its parent should know. Nothing is broken when it does not run.
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

**A turn that fails changes nothing outside its actor.** When `begin` or `handle` returns a `Fault`, or a guest traps, the engine closes the actor and tells its parent. The actor handles no more mail. Its state is not rolled back, so its `end` sees that state as the failed turn left it.

- On a `Fault` the engine applies the turn's `Log` effects, in order, and drops every other effect. A failed turn is the one whose lines are most wanted.
- A guest that traps returns no list, so the lines of that turn are lost with its other effects. The engine logs the trap itself.

**Code is a blob the engine holds, and a library says which blob a kind is.** A guest's code is a **blob**: bytes the engine holds, named by their hash, and never changed. A blob can make certain kinds of actor. Making an actor leaves the blob as it was, so one blob makes any number of actors. A native actor's code is the program's own.

```rust
pub enum Code {
    Program,          // a native actor: its code is in the program
    Blob(BlobId),     // a guest: its code is this blob
}
```

- **A spawn of a guest says which blob, and which actor in it.** `Spawn` and `Embed` each carry the child's `Code` beside its kind and key. The child runs that blob from then on.
- **A library is a root, and it alone says which blob a kind is.** A library registers a namespace, and a namespace is registered once. The kinds named under it are the library's: `aether.chat.room` is `aether.chat`'s. Only the library puts a blob for one of its kinds into the engine, and only it says which blob is that kind's now. That is the kind's **head**.
- **A library moves a head with `Publish`.**

  ```rust
  pub enum Head {
      At(BlobId),       // the kind is this blob
      Withdrawn,        // the kind has no blob now
  }
  ```

  The effect names one of the library's kinds and where its head now points. The kind is under the namespace the library registered, which the library knows, so that is checked where the effect is asked for and applying it has nothing to refuse. A blob that makes several kinds takes one `Publish` for each. The engine applies a `Publish` and keeps the heads.
- **A kind that a newer blob no longer makes is withdrawn, not dropped.** The library publishes `Withdrawn` for it. That is what the head says and no more: the engine does nothing else with it. An actor that looks sees the kind has no current blob. Actors of that kind already running stay on their blob and end as any actor does, and a blob the engine still holds can still be spawned from.
- **Everyone else sees a library's heads and does not hold them.** A library stores its code and makes known where each head points. What the others get is a view of those heads that is brought up to date, or an alert that a head moved. It is not mail that one actor sends another. An actor spawns with the head it last saw.
- **The child stands beneath the actor that spawned it**, as every child does (ADR-0008). A library holds code. The actors made from it stand elsewhere.
- **Nothing moves a running actor.** An actor stays on the blob it was made from. It sees that a head moved later, in a turn of its own, and whether and when it moves onto the new blob is its own choice.
- **No actor changes code that is not its own.** Changing which blob a kind is belongs to the library that registered the kind's namespace, and to nothing else in the engine.

- Whichever runtime runs the parent stands the child up, inside the parent's turn. A native child's `begin` is called directly. A guest child's is called by its host, and a guest parent's spawn is a call out to that host.
- The engine runs no actor's code during another actor's turn. It applies the `Spawn` afterwards.
- **A guest computes no id.** Only the engine does (ADR-0008). A guest names an actor by an id it was handed: its own, the reference a spawn or an embed returned, or the one its mail says the mail is for. Its host confirms that each `by` is the guest's own actor, a child it stood up in the turn, or an actor embedded beneath it.

**An actor is a kind.** The type that implements `Actor` is the actor's state, and it is a kind: it has a name, a schema, and a codec for `Within`.

- Every field of an actor crosses `Within`. What an actor holds that is not data, it holds as a handle (ADR-0010). This is the same for a native actor and a guest.
- The engine can take any actor's state out, by encoding the actor, and put it back by decoding it. It does so to replace the actor's code, and may do so to show the state or record a turn.
- An actor's name is its kind's name. It is the name in a path (ADR-0008) and in `Spawn`.
- Whether new code can take over an actor's state as it is has a test: its kind has the same name and the same layout (ADR-0006). Replacing an actor's code is a later ADR.
- An actor lives decoded, where its runtime keeps it: a native actor as a value the engine owns, a guest in its own memory. The engine does not hold its bytes and does not copy it per turn.

## Consequences

- The rules about mail in flight have one place to live. The engine is the only thing that moves mail and creates or closes an actor, and it sees every request to do so as data.
- A guest is one call per turn: the mail's bytes in, the effects' bytes out, both through the codec of ADR-0004. A spawn is one more call, out to its host. The host must be able to make an instance without waiting.
- A turn can be recorded and compared: the same state and mail give the same effects, as far as the actor's own code is deterministic.
- A list of effects can be read one entry at a time: each says who asked, and no entry depends on the ones around it to say so. The lists of many turns join into one without being rewritten, for as long as the engine that made the ids lives.
- An effect is something to be done, and it can be dropped or go undelivered. What the engine did with each effect is a separate record and a later ADR; the word *event* is kept for that record's entries.
- An actor cannot count on `end` to save its state or release what it holds. Release belongs to the resource itself, which the engine drops (ADR-0010). Saving is the engine's, which can take any actor's state out.
- An actor can spawn a library's kind only once it has seen that kind's head. One that must do so in its `begin` is handed the blob in its config.
- Two actors of one kind can be running different blobs at the same time, each until it chooses to move. So can two kinds of one library. Nothing is left without its code, since the engine keeps a blob while an actor runs on it. What can stop working between them is mail: a kind of mail whose layout changed has another id (ADR-0006), so it goes unhandled and is never read as something else.
- A handler learns within the turn whether a spawn succeeded. It cannot learn whether a send was delivered; it is told by mail in a later turn.
- A child's `begin` runs in its parent's turn and is counted against it. A `begin` that spawns runs its children's the same way, to the depth a path allows.
- A resolve sees the engine as it was when the turn began, so it can be one turn out of date.
- An actor's working fields are limited to what a kind can hold: no `Box`, no recursive type, no `usize`, nothing from a foreign crate. A tree is an arena with indices. A native actor may put a foreign value behind a handle as a resource (ADR-0010); a guest cannot, so a guest's working data fits the vocabulary entirely.
- Where the engine keeps actors is its own choice and not part of this decision. It may keep the native actors of one type side by side. An actor whose fields are all of fixed width has the same bytes in memory as encoded on a little-endian machine (ADR-0002), so such actors could be kept as one stretch of bytes per type, which makes taking their state out, or keeping a copy from before a turn, a copy of that stretch. An actor with a string, a sequence, or a map cannot be changed in place in ADR-0002's bytes, since a field's position depends on the lengths before it.
- **Not decided here, and due before the piece that needs each:** what a piece of mail carries beside its kind and bytes, and how a request and its answer are tied (the next ADR; it adds effects and no second way to act); what the `Actors` and `Within` boundaries offer; when applied mail is delivered and in what order actors run; large payloads, a further effect; how a library registers a namespace and puts a blob into the engine, what is checked when it does, and whether one namespace may sit inside another; how an actor sees a library's heads and learns that one moved; how a library is told of new code, and what it does when two changes reach it; how an actor asks to move onto another blob; when the engine lets go of a blob; the mail by which a parent learns a child closed, faulted, or trapped, and what an actor's `end` is told of how its children ended; how mail says which actor it is for, which comes with the rest of what mail carries; and the engine answering each request an ended actor left unanswered, which comes with the tie between a request and its answer.

## Alternatives considered

- **An actor acts as it runs, through calls the host offers.** Simple to write against, and the three costs under Context.
- **The engine holds each actor's state, and a turn returns the new state.** A failed turn then costs nothing to undo and a guest instance holds nothing. Every turn decodes and encodes the whole state, which falls hardest on an actor with large state that runs every frame.
- **An actor beside a separate state kind it converts to and from.** The actor may then work on any structure it likes. It is a second type and two conversions per actor, nothing checks that the conversion keeps everything, and a native actor could decline to offer it.
- **A failed turn leaves the actor running.** The actor then continues from a state no handler was written to produce.
- **No `end` after a failed turn.** `end` then only ever sees a sound state. An actor that returned a `Fault` could say nothing as it went, though its code can still run, and the fault is the ending its parent most wants explained.
- **The state put back as it was before the failed turn, and then `end`.** `end` sees a sound state every time. The engine keeps a copy of every actor from before each turn, which is the cost that ruled out the engine holding the state.
- **The engine binds each kind's name to a blob, and a spawn names only the kind.** A spawn needs nothing in hand, and works in any turn. Something outside the actors must then hold the right to change a binding, each kind of a library is bound apart from the others, and moving running actors is done to them.
- **A second effect that withdraws a head.** Two effects that each do one thing. A head is then moved by one and removed by another, where one effect with a named case says both.
- **Running actors move when their kind's blob changes.** No actor is left on old code. An actor's code would change under it at a moment it did not choose, as with a parent closing its child.
- **The engine stands a child up when it applies the `Spawn`.** The parent then needs none of the child's code and a guest's turn stays one call. Between the `Spawn` and the child's `begin` the path is taken and not live, mail for it waits, and the parent learns of a failed `begin` a turn later, by mail it must match to the spawn.
- **Marks in the list around a child's effects**: `Spawn` opens the child's part and a closing entry ends it, so who asked is read from where an effect sits. No id on each effect. The list then holds entries that are not asks, an effect cannot be read without the ones before it, and two lists cannot be joined without wrapping each.
- **A number on each effect that is good for one turn**: 0 for the turn's own actor, and n for the nth child it stood up. Smaller than an id. It means nothing outside its turn, so a record of many turns needs a heading on each to say who 0 was.
- **The actor's path on each effect.** No id to define. A path's text is carried on every effect, and that falls hardest on a guest's turn.
- **One spawn, with a field saying where the child's mail goes.** One effect. As an id the field can name an actor that is not allowed, which is a check and one more refusal. As a value with two cases it cannot be wrong, and is then a flag choosing between two behaviours where two names say the same.
- **An effect by which a parent closes its child.** The parent owns the path, so it could. The child would then end without running its own close, at a moment it did not choose.
- **An effect that publishes a value other actors read without being sent it.** A reader could then sample a value that changes often, such as a pointer's position, on its own turn and not be woken by each change. The read is a second input to a turn beside its mail, and the mailbox does not record its order: two actors handling the same mail could see the value before or after a third actor's turn, by scheduling. Fixing the read to the mail being handled settles that and makes the engine keep old values while older mail is queued. Mail to subscribers covers the case until a reader needs the saving.
- **Logging as a call the host offers.** A line then leaves at once, so a trapped guest's lines survive. It is a second way to act, and a call across the guest boundary per line.
- **A send to any path, checked when the effect is applied.** One case in `To`, and every send is a claim the engine has not checked (ADR-0008).
