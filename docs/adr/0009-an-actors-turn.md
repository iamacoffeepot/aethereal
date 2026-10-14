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
    type Key: ActorKey;                     // () for a singleton, a TypedKey otherwise

    fn begin(config: Self::Config, turn: &mut Beginning<'_>) -> Result<Self, Fault>;
    fn handle(&mut self, item: Item<'_>, turn: &mut Turn<'_>) -> Result<(), Fault>;
    fn end(self, turn: &mut Ending<'_>) -> Result<(), Fault>;
}
```

These three are what the engine calls. How an author writes the handling of each kind of mail, and how the kinds an actor handles are declared, is a later ADR; whatever it decides arrives at the engine through `handle`.

An actor's life has three parts. `begin` makes it from a config, and runs inside the turn of the parent that spawns it; a root's runs when the chassis makes it. Each piece of mail after that is a turn of its own, a `handle`. `end` is its closing turn, and it runs once, whichever way the actor came to close.

**A `Fault` is why a turn could not go on, and it carries a kind.**

```rust
pub struct Fault {
    kind: KindId,
    bytes: Vec<u8>,
}
```

- An actor defines its reasons as kinds and returns one: `Fault::new(&reason)` requires `K: Kind + Plain` (ADR-0004). A reason is plain data: it says what went wrong with names, paths, numbers, and text, and it holds no reference to an actor and no resource. So a fault is made anywhere, with no turn in hand, and can be written to a log or sent out of the engine as it is.
- Whoever is handed a fault reads it as a kind it knows: `fault.read::<K>()` answers with the reason when it is of kind `K`. A parent reacts to the reasons it knows and learns from the rest only that the child failed.
- The kind's id names the reason to a reader that cannot decode it, so the engine can log a fault by name.
- **A codec error in a handler becomes a fault with `?`.** The actor crate defines two reasons of its own: `Unreadable`, for bytes that did not decode as the kind they claimed, and `Unwritable`, for a value holding a collection too long to encode. A handler that uses `?` on a read or a send fails its turn with one of them. A handler that is to carry on past mail it cannot read matches on the error.

An **effect** is one thing an actor asks of the engine, and it says which actor asked. An effect exists only for what the engine itself must do: an actor that could do a thing with its own state and mail gets no effect for it. The first effects are:

```rust
pub struct Effect {
    by: ActorId,
    what: What,
}

pub enum What {
    Root { from: Code },
    RootInstanced { from: Code, key: KeyName },
    Send { to: To, id: MailId, kind: KindId, bytes: Vec<u8> },
    Spawn { from: Code },
    SpawnInstanced { from: Code, key: KeyName },
    Embed { from: Code },
    EmbedInstanced { from: Code, key: KeyName },
    Faulted,
    Failed(Fault),
    EndFailed(Fault),
    Ended(How),
    Publish { kind: KindName, blob: BlobId },
    Close,
    Watch { of: ErasedActorRef },
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
- A spawn creates a child beneath the asking actor and nowhere else.
- **A root enters as an effect the engine writes.** Only the chassis makes a root, and no actor asks for one (ADR-0008). A view sees only effects (ADR-0011), so for each root the chassis makes the engine writes `Root`, or `RootInstanced` for one with a key, by the root itself. The table makes the root's entry from it as it makes a child's from a spawn. A root is made between steps: its `begin` runs when the chassis asks for it, and what it asked for is applied then, as a step's effects are.
- **An actor stands up its own children, in its own turn.** `turn.spawn` names the child's kind, its code, its key, and its config, and runs the child's `begin` then and there. It returns a reference to the child, or the reason there is none: the key is taken or closed, the path would be too deep (ADR-0008), the engine holds no such blob or the blob makes no actor of that kind, the key does not fit the child's type, or `begin` returned a `Fault`. The parent handles a child that failed to begin in the handler that asked for it.
- **A child is spawned or embedded.** `turn.spawn` makes a child that takes its own mail. `turn.embed` makes a child whose mail is handed to its parent. Each has its own effects, and no field says where a child's mail goes, so that cannot be written wrong. In everything else the two are the same: the child stands beneath the actor that asked, has a path and an id, has its `begin` run in the parent's turn, and is refused for the same reasons. What this ADR says of a spawn it says of an embed.
- **A singleton and an instanced child have separate effects.** An actor's `Key` type says which it is (ADR-0008), and the plain effects are the singleton's. `Spawn` and `Embed` carry no key, and `SpawnInstanced` and `EmbedInstanced` carry a `KeyName`, so no effect holds a key that may be absent. `turn.spawn` and `turn.embed` take a value of the child's `Key` and write the effect that fits it. An author's code names a child by its typed key, and a `KeyName` appears only in an effect as data. A guest's spawn arrives as bytes, so its host checks it against the child's type before standing the child up: the effect is the one for a singleton or for an instanced type as the child is, and the key's name reads back as a value of the child's key type. A spawn that fails is refused, and the parent handles it as it does any child that did not begin. It can fail in a correct system, since a guest built against one blob may name another whose kind is keyed differently, and a kind's id does not cover its key type (ADR-0006). For native code a keyed spawn of a singleton, an unkeyed spawn of an instanced type, and a key of another type's do not build. Because the effect says which the child is, the engine may keep singletons and instanced actors apart where that serves it.
- **Embedding changes where mail is routed and nothing else.** An embedded child is a virtual actor its parent contains. It has its own path, id, and references, and what it asks for is an effect by the child, not by its parent. Mail addressed to it is handed to its parent, and the mail says which actor it is for. It says so beside its kind and bytes, as an `ActorId` the engine writes when it applies the send, from the `To` the sender gave. No kind carries its own destination, so a kind is written the same whether its receiver is embedded or not, and a parent learns whose mail it holds without decoding it. The parent decides what to do with it.
- **The engine hands an embedded child's mail to the first actor above it that takes its own mail.** A child embedded in an embedded parent is contained in that parent, which is contained in its own, so the mail reaches each through the one that holds it.
- **The spawn effect records a child that is already stood up.** The turn holds the child aside. It adds the spawn effect, by the parent, and then the effects the child's `begin` asked for, each by the child. The child's id is its parent's folded with the child's segment (ADR-0008), which the turn works out without the engine. When the engine applies the spawn effect the child is live. Until then it exists for no one but its parent: its path resolves for nobody else, and the reference the parent holds can leave the turn only inside a later effect of the same list.
- **A `begin` that returns a `Fault` leaves `Faulted`.** The turn adds it, by the child, after whatever that `begin` asked for. It reports and asks for nothing. The engine does not make that child live. It applies the child's `Log` effects, kept under the path that never went live, and drops the child's other effects and every effect by an actor that child stood up. A spawn refused before `begin` ran leaves nothing in the list. A child that failed to begin does not use its key: it was never live and never made known to anyone, so no reference or held path can mean it, and its parent may spawn there again.
- **Only the parent changes its branch**, so the actor knows whether a key is free from the table as it stood when the step began and from its own spawns since.
- **If the parent's turn fails, the children it stood up are dropped** with what they owned (ADR-0010), having never been live. They are handed no `end`.
- **An actor's end is its own, and nothing else ends it.** `Close` closes the actor that asks for it, and an actor whose turn fails is closed by that failure. No effect closes another actor, and no actor has that authority over another, a parent over its child included. A parent that wants a child gone sends that child mail saying so, and the child closes itself.
- **The engine does not end an actor's children when the actor ends.** Whether its children close with it is part of how the actor is written: it tells them by mail, and each closes itself. A child whose parent has ended is still live, and it still owns the paths beneath it.
- **An actor's closing turn runs in the next step.** At the end of the step in which an actor closes, it is marked ended, and no more mail is delivered to it (ADR-0011). In the step after, it runs its `end` as a turn among that step's other turns, in no order among them.
- **A `begin` and a closing turn send to `ToWithoutMail`**, which is `To` without `Sender`. Neither is brought about by a piece of mail it is handling, so neither names any mail, and a send to the sender from either does not build.
- **`begin` is a first turn, and its `Beginning` offers less than a turn.** It offers no `To::Sender`, since a `begin` has no mail of its own and a root's has none behind it at all; a parent that wants its child to answer someone puts that reference in the child's config. It offers no `Close`: an actor does not end in the turn that makes it.
- **The closing turn runs once, and the actor ends whatever it returns.** `end` takes the actor by value, so there is no actor left to hand a second one. Its `Ending` offers what a last turn may ask for: sending, giving, and logging. It offers no spawn, embed, `Close`, or `Watch`. Asking for an effect can fail, so `end` returns a `Result`: on `Ok` the engine applies its effects, and on a `Fault` it applies the `Log` effects and drops the rest. Either way the engine then drops the actor and what it owned (ADR-0010).
- **`end` runs whenever the actor's code can still be run and trusted.** That is after its own `Close`, and after a turn of its own returned a `Fault`. It does not run for a guest that trapped, since its memory can no longer be trusted, nor for a child that was never live. When the host itself goes away, nothing runs.
- **`end` is told why it runs.**

  ```rust
  pub enum Why {
      Closed,
      Faulted,
  }
  ```

  `Ending` answers with one of these. After a `Fault` the actor's state is whatever the failed handler left, so an `end` asked `Faulted` leaves out what depends on that state being sound.
- **No guarantee rests on `end`.** What must be true once an actor has ended is the engine's to make true, however the actor ended: its resources are dropped (ADR-0010), and the actors that watch it are shown its ending. `end` is the actor's last words: a better answer than the one given in its place, or something its parent should know. Nothing is broken when it does not run.
- **A panic in a native actor is a bug in the program, not a failed turn.** The engine does not catch it: a browser host cannot unwind (ADR-0001). Every failure an actor can meet is a `Fault` it returns.
- **`Watch` asks to be shown that another actor has ended.** Only the engine knows that an actor has ended, so an actor cannot learn it any other way. The watcher names the actor by a reference it holds. Watching an actor already watched changes nothing. A watch ends with its watcher. A parent watches its child as it would any other actor: it is shown nothing of a child it has not watched.

- **An actor reacts to items, and mail is one kind of item.** Each actor has a stream of what it has to react to, a view keyed by that actor (ADR-0011), and an actor runs in a step when its stream has items, whether or not any is mail. `handle` is given one item at a time, each a turn of its own. The kinds are a closed list the engine knows:

  ```rust
  pub enum Item<'a> {
      Mail(Mail<'a>),
      Ended(Ended),
  }

  pub struct Ended { /* the actor, and how it ended */ }

  pub enum How {
      Closed,
      Failed(Fault),
      Earlier,
  }
  ```

  `Closed` is an actor that asked to close, and `Failed` carries the fault its turn returned. `Earlier` is an actor that had already ended in a step before the one the watch was asked for in: how it ended is not kept.

- **The stream is a view that reads no other.** A turn resolves the actor its mail is for when it sends it: a child's id from its key, its parent from its own path, a held reference as it is. A send to a parent the turn cannot see, a root's or one no longer live, sends nothing. `Send` carries the id, and the mail lands in that actor's stream; an actor that is not live when its items would be handed to it is never handed them. The stream keeps who watches each actor itself, under the actor watched, so a watch and an ending meet in one part. When the step settles, an ending goes to each watcher, those whose watch came in the same step among them, with how the actor ended; a watch of an actor whose closing turn ran in an earlier step is shown an ending with `How::Earlier`.
- **No mail is made for an ending.** An ending is an item in each watcher's stream, so the engine still sends no mail of its own, and a watcher with nothing else to do is run for it.
- **The engine fixes the order.** A stream is ordered by each item's source, then by the order that source caused it. An actor's ending is the last thing it causes. When its closing turn has run, the engine adds `Ended`, by that actor, with how it ended, after what that turn sent, and the stream folds it into the stream of each actor watching it. So the ending reaches a watcher with the closing turn's mail, after it, and a watcher always handles what an actor sent it, its last words included, before it is shown that the actor ended. Items from different sources have no order between them, as mail from two senders has none. An order the actor chose would leave that to every author to get right, and the bug would show only when a sender's last mail and its ending arrive together.
- **Every item has a source**, and `To::Sender` reaches it: the sender of mail, or the actor that ended, to which a send is not delivered. The turn puts that actor in place of `To::Sender` when it makes the `Send`, since only the turn knows its item. So one `Turn` serves every kind of item. What a turn sends belongs to the item's chain: a piece of mail's own, and for an ending the chain of the mail that brought it about (ADR-0012).
- **An item holds typed values, and only mail holds bytes.** An ending is an id and how it ended, kept as it is in the stream, and encoded only when it is handed to a guest. So the way mail's bytes are kept can change without touching the other kinds.
- **A new kind of item changes the list.** Only what the engine alone knows becomes an item, so there are few. Kinds of mail are not items of their own: sections for them are the sugar, in the ADR on how handlers are written.
- **`Log` writes a line to the actor's log**, which the engine keeps as a view (ADR-0011): each actor's most recent entries, under the actor's own id. A key has one writer, so an actor's entries are in the order it made them, and an older entry leaves as a newer one arrives. The fault of a failed turn is an entry in it, taken from `Failed`, so an actor nobody watches does not fail unseen. An actor's entries stay once it has ended. The fault of a closing turn that fails is an entry too: the engine writes `EndFailed`, by that actor, which marks nothing, since the actor had already ended, and is there for the log. So every fault a turn returns can be read, whether or not anything acted on it. It is an effect so that the engine decides what happens to a line when the turn fails, and so that a guest's lines leave it with the rest of the turn.
- An effect is plain data, and so is the list.

**A turn reads the engine; it does not write it.** `turn` answers whether a path names a live actor from the table of actors as it stood when the turn's step began (ADR-0008, ADR-0011). A read is not an effect. No view changes while a step's turns run, so every read in a turn agrees with every other, and the step's number is all a record of the turn needs to give the same answers again.

- A resolve never finds an actor whose spawn effect has not been applied, so a reference is never to an actor whose `begin` failed.
- Mail caused by a spawn finds the child: the spawn is folded in the step that asked for it, and the mail is handled in a later one.
- A resolve made while handling unrelated mail can miss an actor that is about to be live. An actor that must know waits to be told.

**Mail sent to an actor is handled in the order its source sent it.** That is the one rule of order. Mail from two sources to one actor has no order between them, and none is made up. A turn's effects are applied in the order it asked for them, and a step's effects are folded before any mail sent in it is handled (ADR-0011): whoever comes to hold a reference finds its actor there, however far the reference travelled.

**Turns run in steps (ADR-0011).** Every actor that has items when a step begins runs in it, on the engine's workers. By the end of the step the effects of its turns are applied: folded into the views, and their mail put in the queues of the actors it was sent to. A browser host runs a step on one worker (ADR-0001). How many workers a step wakes follows from the work it holds.

**Who a turn may send to is a closed set.**

```rust
pub enum To {
    Parent,
    Child(KindName),
    ChildInstanced(KindName, KeyName),
    Sender,
    Held(ErasedActorRef),
}
```

`Child` and `ChildInstanced` name an actor beneath the sender, which owns that part of the tree (ADR-0008). They are split as the spawn effects are: `Child` names a singleton and carries no key, and `ChildInstanced` carries a `KeyName`. A turn may spawn a child and send to it in the same list. Every other case is an actor the engine has already established exists. There is no send to a bare path.

**What a send or a spawn carries crosses between actors.** `turn.send(to, &kind)` requires `K: Kind + Encode<Actors>` (ADR-0004, ADR-0006), encodes the kind, and adds the `Send`. `turn.spawn` requires the same of the child's config. A kind that holds a value bound to its own actor is refused when the program is built. The bytes in an effect are always those of a kind that passed this check.

**Every piece of mail has an actor as its sender, and the engine sends none of its own.** Mail from a `Send` names the actor the effect is by. An actor's ending makes no mail: its watchers read it from their turn.

**A turn that fails changes nothing outside its actor.** When `begin` or `handle` returns a `Fault`, or a guest traps, the engine closes the actor, and the actors that watch it are shown its ending. The actor handles no more mail. Its state is not rolled back, so its `end` sees that state as the failed turn left it.

- On a `Fault` the engine applies the turn's `Log` effects, in order, and drops every other effect. A failed turn is the one whose lines are most wanted.
- **The engine then adds `Failed`, by the actor whose turn failed, holding the fault.** The actor returned a `Fault` and so asked for no `Close`, and a view sees only effects (ADR-0011). `Failed` is the entry that says the actor ended, written by the engine because the actor's code could not, and the table marks the actor from it as it does from `Close`. `Faulted` is the other such entry: it says a child's `begin` failed and the child was never live.
- A guest that traps returns no list, so the lines of that turn are lost with its other effects. The engine logs the trap itself.

**Code is a blob the engine holds, and a library says which blob a kind is.** A guest's code is a **blob**: bytes the engine holds, named by their hash, and never changed. A blob can make certain kinds of actor. Making an actor leaves the blob as it was, so one blob makes any number of actors. A native actor's code is the program's own.

```rust
pub enum Code {
    Linked(KindName),           // the program's actor of this kind
    Blob(BlobId, KindName),     // a guest: this blob, and which actor in it
}
```

- **A spawn of a guest says which blob, and which actor in it.** Each spawn and embed effect carries the child's `Code`, and a `Code` holds what its case needs and nothing beside it. A guest is named by a place: the blob, and the kind of the actor in it, since one blob makes several kinds. A native actor is named by its kind alone. The kind in either case is the child's name in its path (ADR-0008). A guest child runs that blob from then on.
- **Any actor may spawn a native kind, a guest among them.** The program keeps the native kinds it can make, by name. A native parent names the child's type and a guest names its kind, and either way the child's code is the one the program was built with for that name. A program holds one such code for a name, so a spawn of a native kind has nothing further to choose: its `Code` is `Linked` and holds the kind. Which build the program is belongs to a record of the engine as a whole, stated once, and no effect carries it.
- **A library is a root, and it alone says which blob a kind is.** A library registers a namespace, and a namespace is registered once. The kinds named under it are the library's: `aether.chat.room` is `aether.chat`'s. Only the library puts a blob for one of its kinds into the engine, and only it says which blob is that kind's now. That is the kind's **head**.
- **A library moves a head with `Publish`.** The effect names one of the library's kinds and the blob its head now points at. The kind is under the namespace the library registered, which the library knows, so that is checked where the effect is asked for. A blob that makes several kinds takes one `Publish` for each. The engine applies a `Publish` and keeps the heads.
- **A head points only at code that makes its kind.** A `Publish` names a blob that makes the kind, and one naming a blob that does not is refused where it is asked for. A library does not take a kind away. A kind that a newer blob no longer makes keeps the head it had. Actors of that kind already running stay on their blob and end as any actor does, and a blob the engine still holds can still be spawned from.
- **Everyone else sees a library's heads and does not hold them.** A library stores its code and makes known where each head points. What the others get is a view of those heads, folded from `Publish` (ADR-0011). It is not mail that one actor sends another. An actor spawns with the head the view shows it.
- **The child stands beneath the actor that spawned it**, as every child does (ADR-0008). A library holds code. The actors made from it stand elsewhere.
- **Nothing moves a running actor.** An actor stays on the blob it was made from. It sees that a head moved later, in a turn of its own, and whether and when it moves onto the new blob is its own choice.
- **No actor changes code that is not its own.** Changing which blob a kind is belongs to the library that registered the kind's namespace, and to nothing else in the engine.

- Whichever runtime runs the parent stands the child up, inside the parent's turn. A native child's `begin` is called directly. A guest child's is called by its host, and a guest parent's spawn is a call out to that host.
- The engine runs no actor's code during another actor's turn. It applies the spawn effect afterwards.
- **A guest computes no id.** Only the engine does (ADR-0008). A guest names an actor by an id it was handed: its own, the reference a spawn or an embed returned, or the one its mail says the mail is for. Its host confirms that each `by` is the guest's own actor, a child it stood up in the turn, or an actor embedded beneath it.

**An actor is a kind.** The type that implements `Actor` is the actor's state, and it is a kind: it has a name, a schema, and a codec for `Within`.

- Every field of an actor crosses `Within`. What an actor holds that is not data, it holds as a handle (ADR-0010). This is the same for a native actor and a guest.
- The engine can take any actor's state out, by encoding the actor, and put it back by decoding it. It does so to replace the actor's code, and may do so to show the state or record a turn.
- An actor's name is its kind's name. It is the name in a path (ADR-0008) and in a spawn effect.
- Whether new code can take over an actor's state as it is has a test: its kind has the same name and the same layout (ADR-0006). Replacing an actor's code is a later ADR.
- An actor lives decoded, where its runtime keeps it: a native actor as a value the engine owns, a guest in its own memory. The engine does not hold its bytes and does not copy it per turn.

## Consequences

- What an actor's code names is a crate of its own, `aethereal-actor`: the turn, mail, effects, and a fault. It holds nothing of the engine. A guest is compiled apart from the engine and depends on that crate alone, and the engine depends on it to run turns and apply effects.
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
- An actor's working fields are limited to what a kind can hold: no recursive type, no `usize`, nothing from a foreign crate. A tree is an arena with indices. A native actor may put a foreign value behind a handle as a resource (ADR-0010); a guest cannot, so a guest's working data fits the vocabulary entirely.
- An actor is one stretch of memory its parent sets up when it spawns it: its queue of mail and its state. Only the worker running the actor touches it while a step's turns run, and its queue is added to when the step's mail is delivered (ADR-0011). Actors of a kind whose state has a fixed size are kept together in a slab and reached by a pointer, and everything else is allocated as it is needed.
- How the engine lays that memory out beyond this is its own choice. It may keep the native actors of one type side by side. An actor whose fields are all of fixed width has the same bytes in memory as encoded on a little-endian machine (ADR-0002), so such actors could be kept as one stretch of bytes per type, which makes taking their state out, or keeping a copy from before a turn, a copy of that stretch. An actor with a string, a sequence, or a map cannot be changed in place in ADR-0002's bytes, since a field's position depends on the lengths before it.
- **Not decided here, and due before the piece that needs each:** what a piece of mail carries beside its kind and bytes (ADR-0012); how a request and its answer are written over mail, which is a layer above the engine and adds no effect; what the `Actors` and `Within` boundaries offer; which actor with mail runs next, and what the engine does when none has any; large payloads, a further effect; how a library registers a namespace and puts a blob into the engine, what is checked when it does, and whether one namespace may sit inside another; how an actor sees a library's heads and learns that one moved; how a library is told of new code, and what it does when two changes reach it; how an actor asks to move onto another blob; when the engine lets go of a blob; the mail by which a parent learns a child closed, faulted, or trapped, and what an actor's `end` is told of how its children ended; how a child learns that its parent ended when the parent could not tell it, as after a trap; what becomes of an embedded child, whose mail is handed to its parent, when that parent ends; and the answer each request gets that an ended actor left unanswered, which comes with the tie between a request and its answer.

## Alternatives considered

- **An actor acts as it runs, through calls the host offers.** Simple to write against, and the three costs under Context.
- **The engine holds each actor's state, and a turn returns the new state.** A failed turn then costs nothing to undo and a guest instance holds nothing. Every turn decodes and encodes the whole state, which falls hardest on an actor with large state that runs every frame.
- **An actor beside a separate state kind it converts to and from.** The actor may then work on any structure it likes. It is a second type and two conversions per actor, nothing checks that the conversion keeps everything, and a native actor could decline to offer it.
- **A reason that crosses `Actors`, made by the turn.** A reason could then hold a reference or a resource being given. Only the turn can write at that boundary, so a helper with no turn in hand could not make a fault, and a resource in a fault nobody reads is one more thing for the engine to drop.
- **A `Fault` that carries nothing, or a line of text.** Nothing to encode. An actor says why in its log, and a parent can read a reason only as words, so it cannot react to one.
- **A failed turn leaves the actor running.** The actor then continues from a state no handler was written to produce.
- **No `end` after a failed turn.** `end` then only ever sees a sound state. An actor that returned a `Fault` could say nothing as it went, though its code can still run, and the fault is the ending its parent most wants explained.
- **The state put back as it was before the failed turn, and then `end`.** `end` sees a sound state every time. The engine keeps a copy of every actor from before each turn, which is the cost that ruled out the engine holding the state.
- **The engine binds each kind's name to a blob, and a spawn names only the kind.** A spawn needs nothing in hand, and works in any turn. Something outside the actors must then hold the right to change a binding, each kind of a library is bound apart from the others, and moving running actors is done to them.
- **A way for a library to say a kind has no code now**: a second effect, a head with a case for pointing at nothing, or a head pointed at an empty blob. A library could then tell everyone a kind is gone. A head would no longer always lead to code that works, and every reader of one would have to handle the case that it does not.
- **Running actors move when their kind's blob changes.** No actor is left on old code. An actor's code would change under it at a moment it did not choose, as with a parent closing its child.
- **The engine stands a child up when it applies the spawn effect.** The parent then needs none of the child's code and a guest's turn stays one call. Between the spawn effect and the child's `begin` the path is taken and not live, mail for it waits, and the parent learns of a failed `begin` a turn later, by mail it must match to the spawn.
- **Marks in the list around a child's effects**: the spawn effect opens the child's part and a closing entry ends it, so who asked is read from where an effect sits. No id on each effect. The list then holds entries that are not asks, an effect cannot be read without the ones before it, and two lists cannot be joined without wrapping each.
- **A number on each effect that is good for one turn**: 0 for the turn's own actor, and n for the nth child it stood up. Smaller than an id. It means nothing outside its turn, so a record of many turns needs a heading on each to say who 0 was.
- **The actor's path on each effect.** No id to define. A path's text is carried on every effect, and that falls hardest on a guest's turn.
- **The destination as a field of each kind.** Nothing beside the payload. A kind's author must then know how its receivers are hosted, the destination is stated in the send and again in the payload, and a parent decodes mail to learn whose it is.
- **Mail from the engine, with a sender that is no actor.** It says truly that no actor's turn sent it. Every handler that reads a sender then has a second case, and the actor the mail is about moves into each payload. An actor's ending is already its own, recorded by the engine where its code could not ask.
- **A failed `begin` closing its path.** One rule for every path that was ever asked for. A singleton whose `begin` failed for a passing reason could then never be stood up beneath that parent.
- **One spawn, with a field saying where the child's mail goes.** One effect. As an id the field can name an actor that is not allowed, which is a check and one more refusal. As a value with two cases it cannot be wrong, and is then a flag choosing between two behaviours where two names say the same.
- **An effect by which a parent closes its child.** The parent owns the path, so it could. The child would then end without running its own close, at a moment it did not choose.
- **The engine ending every actor beneath one that ends.** No child is then left running under a parent that is gone, whatever the parent's author wrote. It gives a parent's ending authority over its children, each of which ends at a moment it did not choose. One ending also becomes many: the table and every other view that follows endings must be told of each actor beneath, by something other than that actor.
- **An effect that publishes a value other actors read without being sent it.** A reader could then sample a value that changes often, such as a pointer's position, on its own turn and not be woken by each change. The engine's own views are read this way, and a step fixes what each reader sees (ADR-0011). Only the engine defines a view for now, and mail to subscribers covers the case until an actor may have one of its own.
- **Logging as a call the host offers.** A line then leaves at once, so a trapped guest's lines survive. It is a second way to act, and a call across the guest boundary per line.
- **A send to any path, checked when the effect is applied.** One case in `To`, and every send is a claim the engine has not checked (ADR-0008).
