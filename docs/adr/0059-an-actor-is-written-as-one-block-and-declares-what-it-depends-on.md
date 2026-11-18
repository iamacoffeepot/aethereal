# ADR-0059: An actor is written as one block, and declares what it depends on

- **Status:** Proposed
- **Date:** 2026-11-18

## Context

An actor is written as several trait impls (ADR-0009, ADR-0013, ADR-0022, ADR-0058): one `impl ChildOf`; one `impl Actor`, with its `Config`, its `Key`, a `Handles` tuple, `BeginSends`, `EndSends`, `begin`, and `end`; one `impl Handles<K>` for each kind of mail it takes and one `impl Receives<H>` for each answer, stage, ending, and fallback; and an `impl Carries<K>` for each kind it is asked by its type. Each handler states `type Sends`, a tuple of markers (`ToSender`, `ToType`, `ToAny`, `Tells`, `Asks`), and each handler of mail its guard, `type From`.

Most of it restates a default. Across `crates/` at 3cac9d5 there are 385 `impl Actor`. Of the 392 `end` bodies, nearly all (about 380) are only `Ok(())`; 374 actors say `EndSends = ()` and 252 `BeginSends = ()`; 302 say `Key = ()` and about 250 `Parent = Top`. Of 558 `impl Handles`, 536 say `From = Anyone`. Beside them stand about 380 `impl Receives` and 187 `impl Carries`.

A sends list repeats what the body's calls already say. `Simulator` (`aethereal-chassis-browser/examples/compute/page.rs`) states, for its `On<Tick>`, the target, kind, and context of each ask it makes:

```rust
impl Receives<On<Tick>> for Simulator {
    type Sends = (Asks<Path<Renderer>, ReadStorage, Nothing>, Asks<Path<Renderer>, TimeFrame, Nothing>);

    fn handle(&mut self, _: Tick, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        // ..
        turn.to(path::<Renderer>(())).ask(&ReadStorage { storage }, &Nothing)?.ask(&TimeFrame, &Nothing)?;
        // ..
    }
}
```

What reads the declared sends:

1. **The send check at compile time.** A send builds only when its marker is in its handler's list (`Allows<M, I>`, `declared/sends.rs`), and the marker's `DeclaredBy` impl checks the receiver: that `T` handles `K` for `ToType<T, K>` and `Tells<Path<L>, K>`, that the sender handles what the receiver's handler declares it replies (`RepliesTo`, `declared/replies.rs`), and for `Asks` that the asker handles the answer with its context (`ask.rs`). A conversion to a named protocol reads the same replies (`AnsweredBy`, `declared/protocols.rs`).
2. **The guest host.** It checks each send a guest makes against the declared send of the handler that ran (`aethereal-actor/src/turn/hosted.rs`: `allows`).
3. **The program's root list.** A root the program draws must declare that it sends the renderer a draw list (`aethereal-substrate/src/program/root_list.rs`, through `Declared::may_send`; ADR-0055).
4. **The setup's order check.** A root that sends at a typed path to a singleton root is added after that root (`aethereal-actor/src/path/roots.rs`: `NeedsIn`, `MadeAfter`, `Ordered`; ADR-0022). The spawn check reads the same paths, refusing a child whose declared path of singletons holds no live actor (`Declared::missing`, `Refused::Missing`).

What does not read them:

- **Fusing.** `hand` in `aethereal-substrate/src/engine/mod.rs` decides by where the receiver is seated and by order alone (ADR-0013). The domains that ADR-0013 gave as the reason to declare sends were never built.
- **`found`.** The order check does not see a root found by its typed path in `begin` (#596), and that is how the HTTP stack's `begin` finds its layers (`aethereal-http/src/stack/walks.rs`, `last.rs`). Main's history shows no bug the order check caught; it came in with #184, as a follow-up of the design.

The `Handles` tuple has a gap. An `impl Handles<K>` whose `K` the tuple leaves out still builds; a send of `K` to the type still builds, as `ToType`'s `DeclaredBy` asks `T: Handles<K>` and not that the tuple lists `K`; and the first such mail stops the engine in `HandledList::dispatch` (`unhandled`). This was read from the code, not reproduced.

## Decision

**An actor is written as one block, `#[actor]` on `impl Name { … }`, which writes its `ChildOf`, its `Actor`, and every handler's impl. No handler declares what it sends: every send is checked at its call, its receiver's guard among it. The actor declares the types it reaches by typed path, `depends(…)`, as an import list, and the types it spawns, `spawns(…)`. The program walks its tree from each root through `spawns` and checks that it holds the root of everything each type depends on.**

```rust
// Before: Ponger in aethereal-chassis-headless/examples/savina.rs
impl ChildOf for Ponger {
    type Parent = Pinger;
}

impl Actor for Ponger {
    type Config = Number;
    type Key = u64;
    type Handles = (Ping, Ready);
    type BeginSends = ();
    type EndSends = ();

    fn begin(_: Number, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Ping> for Ponger {
    type Sends = (ToSender<Pong>,);
    type From = Anyone;

    fn handle(&mut self, Ping { left }: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong { left })?;

        Ok(())
    }
}

// and `impl Handles<Ready> for Ponger`, written by a macro, which replies `Ready`
```

```rust
// After
#[actor(key = u64, parent = Pinger)]
impl Ponger {
    fn begin(_: Number) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[tell]
    fn ping(&mut self, Ping { left }: Ping, turn: &mut Turn<'_, Self, ActorRef<Pinger>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Pong { left })?;

        Ok(())
    }

    #[tell]
    fn ready(&mut self, _: Ready, turn: &mut Turn<'_, Self, ActorRef<Pinger>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Ready)?;

        Ok(())
    }
}
```

### One block

- **`#[actor]` on `impl Name { … }` writes `ChildOf`, `Actor`, and one trait impl for each handler.** Each handler's body moves into its generated impl unchanged.
- **An actor has exactly one `#[actor]` block, and it lists every handler its author writes.** An actor whose logic spans files keeps its one block, and each handler's body calls ordinary methods, `impl Name { … }` in the files of each concept. The block is the actor's whole surface, read in one place, with a library's handlers named by its `with(…)` (below). There is no second macro for a block split across files.
- **Defaults.** Its arguments are `key = K`, `()` when left out; `parent = P`, `Top` when left out; and `depends(…)`, `spawns(…)`, and `with(…)` (below), each empty when left out. `end` may be left out, and is then `Ok(())`.
- **`Config` is the type of `begin`'s first parameter.**
- **A method that takes no turn is valid** wherever nothing needs one: `begin(config)`, or a handler that only changes the actor's state.

### A handler's role is a word, and its kind is its parameter's type

Each handler carries one word, which says what the item is:

| Word | Writes | Handed |
|---|---|---|
| `#[request]` | `Handles<K>`, for mail of a kind that is answered | the kind `K` |
| `#[tell]` | `Handles<K>`, for mail of a kind that is not answered | the kind `K` |
| `#[answer]` | `Receives<Answer<R, C>>` | `(Answered<R>, C)` |
| `#[on]` | `Receives<On<S>>` | the stage's item `S` |
| `#[ended]` | `Receives<Ended>` | `Ended` |
| `#[others]` | `Receives<Others>` | `Mail<'_>` |
| `#[refused]` | `refused` in `Handles<K>`, for mail of `K` its handler's guard refused | the kind `K` |

- **The role words are `#[request]`, `#[tell]`, `#[answer]`, `#[on]`, `#[ended]`, `#[others]`, and `#[refused]`.** There are no others.
- **The kind is the type of the handler's second parameter**, after `&mut self`. The macro reads `K`, `R` and `C`, or `S` from it.
- **The words are split so a reader classifies a data flow at a glance:** what is asked and must be answered, what is told, what comes back, what the engine brings. The macro checks each word against its return type (below), so a word that does not say what the handler does does not build.
- **`Handles` is collected from the methods, in order.** It cannot miss a handler, nor list a kind with none, so the gap above is closed: mail of a kind its type has a handler for is always dispatched to it.

### A request returns its answer, and the macro sends it

```rust
#[request]
fn which(&mut self, at: WhichTile) -> Result<TileIs, Fault> { /* answers now */ }

#[request]
fn fetch(&mut self, fetch: Fetch, turn: &mut Turn<'_, Self>) -> Result<Later<Fetched>, Fault> {
    let (held, later) = turn.hold();
    self.waiting.push((fetch.path, held));

    Ok(later)  // answered later, with turn.answer(held, &fetched)
}

#[request]
fn inbound(&mut self, inbound: Inbound<Seen>, turn: &mut Turn<'_, Self, FromStack>) -> Result<Reply<Inward<Caller>>, Fault> {
    /* Reply::Now(inward), or Reply::Later(later) after a hold: decided in the body */
}
```

- **A `#[request]` returns one of three forms:** `Result<A, Fault>` answers now; `Result<Later<A>, Fault>` answers later; `Result<Reply<A>, Fault>` is `Reply::Now(a)` or `Reply::Later(later)`, as the body decides. On `Ok` the macro sends the answer, or sends nothing for a `Later`.
- **`turn.hold()` answers `(Held<A>, Later<A>)`, and cannot fail.** A `Later<A>` is made only by a hold, so returning one proves the request was held. The `Held<A>` is kept, as before, and answered in a later turn with `turn.answer(held, &a)` (ADR-0017).
- **A `#[tell]` returns `Result<(), Fault>`.** A `#[request]` that returns `()`, and a `#[tell]` that returns anything else, do not build.
- **The request's return type is the actor's answer to its kind:** the macro writes `Carries<K>` for it, which an asker reads by type (ADR-0022) and a stack reads to answer through (ADR-0035). There is no `impl Carries` to write by hand for an actor's own kinds.
- **The turn has no `reply`.** A helper that replied returns the answer to its handler instead. A tell that talks back to its sender states a typed guard, such as `ActorRef<Pinger>` above (`guard/library.rs`), and sends to the sender it is handed, `*turn.from()`, a send checked at its call as any is. A tell whose senders can be anything states the protocol of what it sends back, `Speaks<(Pong,)>`.
- **Beneath the macro, the form is `Handles<K>::Returns`,** `type Returns: Answering`, and `handle` returns `Result<Self::Returns, Fault>`. `Answering` is implemented for `()`, for every kind, for `Later<A>`, and for `Reply<A>`, and says what the kind is answered with, `Answers`: `()` for none, `(A,)` for one kind. It is sealed, and sends through a token only the actor crate makes, so nothing but the engine sends a handler's answer. One trait cannot be written in the actor crate for both `()` and every kind, as the data crate could make `()` a kind, so the data crate, which owns `Kind`, writes `IntoKinds`: a kind is `(K,)`, `()` is `()`, and `Answering` is written over it.
- **An answer to mail that asked for none is dropped.** A request's kind may still be told, with `send`; its handler's answer then reaches no one, as nothing waits for it. A sender that needs the answer asks.
- **`Carries<K>` for an actor's type is one blanket impl over its handler,** `impl<T: Handles<K>, K: Kind> Carries<K> for T`, whose `Answers` is the handler's. A named protocol has no handler, and writes its own, as a struct with `#[protocol(…)]` will. The two never meet under coherence: a protocol type is local to its crate and has no handler, and no other crate can write one for it, so the compiler knows the blanket impl does not reach it. `Carries::Answers` is bound by `Answers`, so a protocol too answers a kind with one kind or none.
- **Holding is proven by the type, as far as stable Rust allows.** A `Later<A>` has a private field, so only `turn.hold()` makes one, and it is neither `Clone`, `Send`, nor written to bytes, so no actor state, which is `Send` and written, nor context of an ask, can keep it past the turn that made it; a turn handles one item, so the `Later` its handler returns is its own request's. Holding twice in one turn is not a build error: `hold` cannot fail, and a turn that held what it did not owe, its item no request or one it has held or answered, fails with `NotOwed` once its handler returns, the `Held` it was handed naming no debt. A handler that holds and then returns `Reply::Now` fails its turn the same way, its answer refused.
- **A run-time cast checks the kinds a protocol lists, not their answers** (ADR-0016). An actor asked through a protocol cast to at run time, whose handler of the kind answers nothing, leaves the request unanswered, and the engine answers `Unreplied` (ADR-0017).

### A refusal is a handler of its kind, `#[refused]`

```rust
#[request]
fn fetch(&mut self, fetch: Fetch, turn: &mut Turn<'_, Self, Peer>) -> Result<Fetched, Fault> { .. }

#[refused]
fn fetch_refused(&mut self, fetch: Fetch, turn: &mut Turn<'_, Self>) -> Result<Fetched, Fault> {
    // turn.from() is the sender, as an ErasedActorRef
    Ok(Fetched::NotYours)
}
```

- **`#[refused]` is keyed by its kind, the type of its second parameter,** and pairs with the `#[request]` or `#[tell]` of the same kind in the block. The macro writes it as `fn refused` in that kind's generated `Handles<K>` impl (ADR-0058).
- **It does not build** when the block has no `#[request]` or `#[tell]` of its kind, or holds two `#[refused]` for one kind.
- **A refused request returns that request's answer type,** `Result<A, Fault>`, and the macro replies with it, as it does for a request. A refused tell returns `Result<(), Fault>`. Beneath the macro, `Handles::refused` returns `Result<Refusal<Self::Returns>, Fault>`: `Refusal::Answers(r)`, in the handler's form, which the engine sends as it sends the handler's, or `Refusal::Logged`, the default (ADR-0058). The macro wraps what a `#[refused]` returns in `Refusal::Answers`. The default is a case of its own, as no form could stand for it: the engine cannot make an `A` to answer now.
- **Its turn has no guard,** `Turn<'_, Self>`, as the guard admitted nothing to hand on: `turn.from()` is the sender as an `ErasedActorRef`.
- **A kind with no `#[refused]` keeps the default:** the refusal is logged, a request answered `Unanswered::Unreadable`, and a tell dropped (ADR-0058).

### Named protocols are one attribute

`#[protocol(…)]` on a struct writes its `Protocol` and one `Carries` for each kind it lists. A bare kind is a tell, and `Kind(answers = A)` is asked and answered with `A`:

```rust
#[protocol(Exited)]
pub struct Exits;                                          // tells only

#[protocol(Gather(answers = Gathered))]
pub struct Gathering;                                      // one request

#[protocol(Write(answers = Wrote), Finish, Shut)]
pub struct Writing;                                        // mixed

#[protocol(Open<S>(answers = S::Done), More, Enough)]
pub struct Streamed<S: Stream>(PhantomData<fn() -> S>);    // generic
```

The macro copies the struct's generics onto each impl it writes, so a protocol's bound moves onto the struct, as `S: Stream` does here. What uses a protocol is unchanged: `ProtocolRef<P>`, `.protocol()` and `Conforms`, `Speaks<P>`, `Covers<Q>`, and tuples of kinds such as `(Pong,)` (ADR-0016, ADR-0058). `Conforms` reads each kind's answer from the actor's `Carries<K>`, the return type of its request, and an actor that answers a kind of a named protocol otherwise than the protocol carries it does not conform. A tuple of kinds is told, so it reads no answer.

### No sends are declared; `depends(…)` imports the types an actor reaches

**Every send is checked at its call.** A send builds when:

- **the target handles the kind:** an `ActorRef<T>` or a typed path to `T` when `T` has a handler for `K`, and a `ProtocolRef<P>` when `K` is one of `P`'s kinds (ADR-0016);
- **for an ask, the asker has an `#[answer]` for the receiver's answer, with the context it passes:** the answer read from the receiver type's `Carries<K>`, or from the named protocol's (ADR-0017);
- **for a target that names its receiver's type, the receiver's guard admits the sender's type** (below);
- **for a typed path, its root is the root of a type in the actor's `depends`.**

**`#[actor(depends(Renderer, Lens))]` names the actor types the actor reaches by typed path, and their roots are derived.** `depends(T)` takes any actor type. Its chain to the top comes from `ChildOf` (ADR-0022): `path::<Lens>(())` runs `Host`, then `Camera`, then `Lens`, so `depends(Lens)` has the root `Host`. `turn.to(path::<L>(keys))` and `turn.found(&path::<L>(keys))` build when `L`'s root is the root of something the actor depends on: `depends(Lens)` admits `path::<Camera>(())` too, and `depends(Windows)` admits `path::<Window>(id)`, as `Window` is a keyed child of the singleton root `Windows`. The list is kept on the actor's type, as a tuple, and each type in it is found as `Lists` finds a kind (`declared/lists.rs`), by its root: two types of one root are one entry, as a path beneath that root could be found at either, so a type that depends on two of them does not build. A path whose root is in none of its chains fails as an import left out does, with a diagnostic written for it: "`Simulator` reaches `Lens` beneath `Host`, the root of nothing it depends on".

- **A type parameter that is a list counts as the types it lists**, nested as deep as `ToolList` nests: `#[actor(depends(L))] impl<L: TunableList> TuningTool<L>` depends on each root `L` lists (ADR-0050). Written as the actor's associated type, the entry is `Each<L>`: a list is one actor type, `()`, a tuple of lists, or `Each<L>`, and each step of what reads one goes by which it is. The generic code that reaches a type `L` lists names none of them, so it reaches it through `L` itself: a tuning tool's lookup states that its actor's `Depends` is `Each<Self>`, which makes the list's own tuple the one its roots are found in, and a toolbox's walk of its nested list carries the path into `L` to where it stands (`Pick`, `Through`), from which the next step's path follows.

```rust
#[actor(depends(Renderer))]
impl Simulator {
    fn begin(shows: Shows, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        turn.subscribe::<Start>();
        turn.subscribe::<Tick>();

        Ok(Self::Making { window: shows.window })
    }

    #[on]
    fn tick(&mut self, _: Tick, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        // ..
        turn.to(path::<Renderer>(())).ask(&ReadStorage { storage }, &Nothing)?.ask(&TimeFrame, &Nothing)?;
        // ..
    }

    #[answer]
    fn read(&mut self, (answered, _): (Answered<StorageContents>, Nothing), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        // ..
    }

    // its other #[on] and #[answer] handlers
}
```

- **The program checks, as it is built, that each chain's root is in the program**, for every type it walks (below). An instanced root type counts through its `add_roots` entry, as the setup's list holds it as one entry however many keys it makes it under (ADR-0022, ADR-0029). A path whose types cannot fit the program, such as one beneath a root it does not have, is a topology mismatch, and does not build.
- **It checks the order too, reading `depends`:** a root is added after each singleton root type its `depends` names. `depends` is a property of the type, covering its `begin`, every handler, and its `end` alike, so the check is exactly as strict as the one it replaces, which read the sends of all three. `found` in `begin` is a reach by typed path like any other, so the order check now covers it.
- **A spawn is refused, before the child's `begin` runs, when a chain in its `depends` with no keyed segment holds no live actor** (ADR-0022: `Refused::Missing`), as `Declared::missing` checks a path of singletons today. It reads the child's `depends` where it read its declared paths.
- **Only a keyed actor could exist and may not.** Its place is filled and emptied as the program runs, so neither the build nor a spawn can say it is there. `turn.to(path)` stays allowed for any path into a tree the actor depends on, and behaves as it does today: an empty place drops a tell and answers an ask as gone (ADR-0022). `turn.found(&path)`, which answers `Option`, is for code that needs to know.
- **A reference needs no entry.** An `ActorRef`, a `ProtocolRef`, and the sender a guard hands on were proven when they were made (ADR-0016, ADR-0058).
- **A dependency is never optional.** An actor depends on what it reaches outright, or does not reach it: `depends` has no optional entry, and no root is looked up by its name to reach one some programs lack. What only some programs need is an actor of its own, which depends on both sides and is listed only where both stand, and which registers itself with the actor it serves; that actor keeps whatever registers, and does without when nothing has. So a publisher never names its listeners. The dev overlay keeps the `ChatRelay` that tells it `Relays`, and depends on no link; `Engines` tells each actor that asks to `HearTrusted` every engine the hub trusts, and in a hub that runs sessions, `ChatToSessions` points each at `Sessions` (ADR-0057).

### A parent declares what it spawns, and the program walks the tree

```rust
#[actor(spawns(L), depends(R))]
impl<L: ToolList, R: Holder> Toolbox<L, R> {
    // its begin spawns each tool of L beneath itself, and offers it to R
}
```

- **`#[actor(spawns(…))]` lists the types an actor spawns.** A type parameter that is a list counts as the types it lists, as in `depends`.
- **`turn.spawn::<C>` builds only when `C` is in the spawner's `spawns`**, found with `Lists`, beside the check that `C` may stand beneath the spawner's type (ADR-0022). A spawn is a method of the trait `Spawn<Me, I>`, which the turn and the `begin` implement: `C` is named at the call and its place `I` in the list inferred, which a method's own type parameters cannot do while one of them is named, so `Spawn` is brought into scope where an actor spawns.
- **The program's build check starts at each root and follows `spawns`, recursively**, checking that every type's `depends` lead to roots the program has. It is a recursive trait check of the same kind as `Ordered`, its index inferred. So a child's reach is checked when the program is built, however deep it is spawned: a tool's `depends` is checked through its toolbox's `spawns` (ADR-0037).
- **A parent repeats nothing from its children.** Its `depends` is what it reaches itself. The engine this repository succeeds declares spawned children the same way, `#[actor(spawns(..))]`.
- **The walk does not follow a `Recursive` type from a list, nor one that stands at the `Top`.** Only its own type spawns a `Recursive` type (ADR-0022: `Admits`), so every actor of one stands beneath a root of its type, where the walk starts and checks its `depends`; its own type in its own list is already checked, and following it would lead the walk back into itself, a trait check that does not end. A `Top` type is spawned by nothing. The walk follows every other type, its children among them, so a `Recursive` type's other children are checked too. Where a type stands says which (`Placement::Walk`: `Follow` or `Skip`), so no step compares two types. A type that stands `Anywhere` and lists its own type, or two such types that list each other, would still lead the walk round: the compiler refuses that program, as a check past its recursion limit.
- **It costs one recursive trait check per root at build,** and its compile time grows with the tree's depth.

### A guard is checked at the call

```rust
/// A guard `G` of the actor type `A` admits a sender of type `Me`, found at `I`.
pub trait Admits<A, Me, I> {}

impl<A, Me, I, G: Guard<A>> Admits<A, Me, I> for G where G::Sent: SentBy<Me, I> {}

/// The checked sender a guard hands on can be an actor of type `Me`, found at `I`.
pub trait SentBy<Me, I> {}

impl<Me> SentBy<Me, ()> for ErasedActorRef {}                            // any sender
impl<T> SentBy<T, ()> for ActorRef<T> {}                                 // an actor of type `T`
impl<Me: Conforms<P>, P: Listed> SentBy<Me, ()> for ProtocolRef<P> {}    // an actor whose type handles every kind of `P`
```

- **`send` and `ask` on a target that names its receiver's type gain the bound `<T as Handles<K>>::From: Admits<T, Me, I>`**, where `Me` is the sending actor's type: an `ActorRef<T>`, a `Path<L>`, and a place beneath the sender, `child::<D>(keys)`, whose last type is its receiver's. Every turn sends through the same target, so the bound holds for a `Turn`, a `Doing`, a `Beginning`, an `Ending`, and a send to many references at once. A guard that refuses the sender's type is reported where the send is written, with a diagnostic written for it: "`Producer` may not send this mail: the receiver's guard admits only `ActorRef<Counter>`", which rustc follows with the receiver and kind the send names.
- **No guard author writes anything new.** `Admits` has one blanket impl, over the guard's `Sent` type, which every guard already names (ADR-0058), so the check reads the type `admits` answers and cannot drift from it. Each library guard is checked through its `Sent`: `Anyone` through `ErasedActorRef`, so it admits any sender; `Itself` through `ActorRef<A>`, its own type; `Parent` through `ActorRef<A::Parent>`; `Child<T>` and `ActorRef<T>` through `ActorRef<T>`; and `Speaks<P>` through `ProtocolRef<P>`, an actor whose type handles every kind of `P` and answers each as a named protocol says, as an `ActorRef` is made a `ProtocolRef` (ADR-0016).
- **A guard of a handler's own is checked through its `Sent`:** `ItsProcess` hands on an `ActorRef<Process>`, so only a `Process` may send `Spawned` its `Exited`.
- **The index is `()` for each of the library's `Sent` types.** A protocol's kinds are found by `Conforms`, which reads the type's handlers and needs no index, so generic code that sends to a type of its own parameter states `<T as Handles<K>>::From: Admits<T, Me, ()>` and threads none, as the HTTP stack does of its layers and handlers. A `Holder` takes `Offer` and `Withdraw` from `Anyone`, as a toolbox is generic over its list and no guard that names a type could admit every toolbox. The index is there for an enum guard, whose derive writes `SentBy<Me, I>` holding when `Me` is one of its cases' types, found at `I` as `Lists<Me, I>` finds a kind (ADR-0058). No guard yet needs one, so the derive is built with `aethereal-actor-derive`, beside `#[actor]`.
- **A guarded handler runs code written for any sender through `turn.unguarded(|turn| …)`,** the same turn with its guard set aside, as a screen's library takes a drop in from `Speaks<(DropAnswered,)>` and calls the screen's methods, each written for a turn from `Anyone`. What the code sends and answers is counted on the guarded turn.
- **A sender that is not an actor is checked as the type it is.** A harness's sends are the outside's, whose type, `Outside`, handles nothing (ADR-0036): `Anyone` admits it, and a test that hands a typed guard its mail stands up the type the guard admits. The engine's own send from the outside names a seat and no type, as a `ProtocolRef` does, so delivery alone checks it. A bootstrap sends nothing; it hands over references (below). A guest's code is built against the same targets, its own type as `Me`, and its host repeats the check at delivery (below).
- **Still checked only at delivery** (ADR-0058): a send through a `ProtocolRef` or to the sender's `parent::<P>()`, which do not name their receiver's type; and the part of a guard that reads the seat or the mail, such as whether the sender is the parent's seat, or the process this spawn holds.
- **`Admits` is the guard's name alone.** The spawn's placement check, which reads where a type stands (ADR-0022), is `PlacesBeneath<A, C>`: a child `C`'s parent places it beneath an actor of type `A`.

### A library writes handlers of its own: `with(…)`

```rust
#[actor(with(aethereal_ui::screen), depends(Game))]
impl Inventory {
    fn begin(..) -> Result<Self, Fault> { .. }
}
```

- **A library module, such as `aethereal_ui::screen`, provides two macros, `handles!(collected…)` and `depends!(own…)`.** `#[actor(with(aethereal_ui::screen))]` emits `type Handles = aethereal_ui::screen::handles!(<collected kinds>);`, the block's kinds with the library's, and merges the library's `depends` with the actor's own: a screen reaches `Renderer` and `Windows`, and `Inventory` adds `Game`.
- **The library keeps writing its own handler impls,** as `screen!` does today through `view!` (ADR-0056).
- **The one-block rule applies to what the author writes.** The library's generated impls were never part of the author's block.
- **`sends!` and `Controls::Sends` go away** with every other sends list.

### What the guest host checks

- **A guest's section carries its `depends` in place of each handler's sends, and its `spawns`, as the kind of each type it lists** (ADR-0015).
- **The guest host checks each spawn a guest's turn makes against its spawner's `spawns`,** beside where the child's type stands (ADR-0022), as a native spawn is checked where it is built: a spawn of a type the list leaves out fails the turn, as a send past the declaration does.
- **With no declared sends, the guest host checks three things of each send a guest makes:** that its receiver lists the kind, or the guest's turn fails; the receiver's guard, at delivery, as for any sender (ADR-0058); and that a send at a typed path reaches a root of the guest's own `depends`. A guest reaches a typed path through its host's lookup of it, which names no sender, so the host checks the path there, against the `depends` of the types the guest's code makes, and the call fails where none holds its root.
- **These are the checks a native actor's build makes, repeated at run time,** as a guest may not have been compiled with the library. They are not a security boundary: access control between actors is out of scope for this ADR.

### A bootstrap hands over references

```rust
// a dependent's config holds the InitialWindow its bootstrap was handed: the window's id and its ActorRef<Window>
turn.to(shows.window.actor()).send(&window::Subscribe)?;   // no path, no Option, no depends entry
```

- **`Bootstrapping::begin` answers `Result<ActorRef<A>, Unbuilt>`,** the reference to the root where the engine seated it, which `begin` dropped as an `ActorId` before (ADR-0029). `begin_from` answers the same for a root a guest runs.
- **A bootstrap has `cx.found(&path)`,** the lookup a turn has, for what its root spawned in `begin`. It builds only for a path whose root is the root `A` the bootstrap makes: `L::Root: FoundBy<L, A>`, which holds only when the two are one type, as `RootIs` checks a turn's path against its `depends`. A bootstrap finds nothing another root made; that root's bootstrap hands it over.
- **What a root's `begin` spawned is in the table when its bootstrap's `found` runs.** The engine applies a root's `begin` and what it asked, the children it stood up among it, before `cx.begin` answers, so `found` runs after them with no change to when `begin` runs: the window root's bootstrap finds each window `Windows` made.
- **So `InitialWindows` carries `InitialWindow`s where it carried `WindowId`s:** each is a window's `WindowId`, which the renderer names its frames by (`Frame::Main`, ADR-0025), and the `ActorRef<Window>` of its actor, made together, only by the window root's bootstrap, so the two cannot name two windows. A dependent's config holds the `InitialWindow` it is handed, and sends to the window through its reference; one that never sends to the window holds only the id (ADR-0024).
- **`path::<Window>(id)` stays for a window an actor was not handed:** one opened while the program runs (ADR-0027), as the desktop windows example's scenes are, and one named in mail by its id, as the renderer reaches the window of a probe and the input tool the window it performs in. Those actors keep `Windows` in their `depends`.

### A turn is typed by its actor and its guard

**`Turn<'a, Me, G = Anyone>`.** The sends parameter goes, and the guard is the only type argument. It stays in the signature because it sets the type `turn.from()` answers (ADR-0058), so a guarded handler names it even when it reads nothing else of the turn. `Beginning<'a, Me>` and `Ending<'a, Me>` lose their sends parameter too.

```rust
#[actor]
impl Gate {
    fn begin(_: GateSettings) -> Result<Self, Fault> {
        Ok(Self { held: Vec::new() })
    }

    #[request]
    fn inbound(
        &mut self,
        Inbound { head, attached: seen }: Inbound<Seen>,
        turn: &mut Turn<'_, Self, FromStack>,
    ) -> Result<Reply<Inward<Caller>>, Fault> {
        let inward = match &head.credentials {
            Credentials::Bearer { token } if token == "open" => {
                Inward::Pass(Caller { request: seen.request, user: String::from("open") })
            }
            Credentials::Bearer { token } if token == "wait" => {
                let (held, later) = turn.hold();
                self.held.push(Waiting { held, seen });
                return Ok(Reply::Later(later));
            }
            _ => Inward::Answer(Response::challenge("Bearer", "gate")),
        };

        Ok(Reply::Now(inward))
    }

    #[request]
    fn outbound(&mut self, outbound: Outbound, _: &mut Turn<'_, Self, FromStack>) -> Result<Outward, Fault> {
        let mut response = outbound.response;
        response.headers.retain(|header| header.name != "x-gate");
        response.headers.push(Header::new("x-gate", "passed"));

        Ok(Outward::Pass(response))
    }

    #[tell]
    fn order(&mut self, order: GateOrder, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        // answers each held request with turn.answer, or closes
    }
}
```

`Gate` (`aethereal-chassis-headless/tests/http_stack.rs`) loses its `impl ChildOf`, its `impl Actor` with its `begin` and `end`, two `impl Carries`, and the three `Sends` lists of its handlers, and each `turn.reply(&inward)` becomes the value returned. `Echo` in the same file holds its request and returns the `Later`:

```rust
#[request]
fn request(&mut self, request: Request<Seen, ()>, turn: &mut Turn<'_, Self, FromStack>) -> Result<Later<Response>, Fault> {
    self.asked += 1;
    let (held, later) = turn.hold();
    let echoing = Echoing { held, request: request.attached.request };
    turn.to(request.body.gatherer).ask(&request.body.whole(self.most_bytes), &echoing)?;

    Ok(later)
}

#[answer]
fn gathered(
    &mut self,
    (gathered, Echoing { held, request }): (Answered<Gathered>, Echoing),
    turn: &mut Turn<'_, Self>,
) -> Result<(), Fault> {
    // builds the response
    Ok(turn.answer(held, &response)?)
}
```

## Consequences

- An actor is read in one block: where it stands, its key, what it depends on, what it spawns, and every handler with its role, kind, guard, and answer in its signature. What was restated by default is left out.
- A send is checked where it is written, against what it names, and nothing restates it. A send that does not build points at the call, and a send whose receiver's guard refuses the sender's type is one.
- An actor type's `Handles` always matches its handlers, so a send of a kind its receiver has a handler for never stops the engine at dispatch.
- A request always answers, now or later, as its return type says: a native handler cannot leave a request unreplied except by letting a `Held` go (ADR-0017). A refused request answers too, with the default or its `#[refused]`.
- What an actor needs of the program is one list of types, whose roots are checked when the program is built, for every type the walk through `spawns` reaches: a program that lacks a root an actor reaches does not build, which saves a run that would only find it missing. The order check covers `found` in `begin`.
- A parent names the types it spawns and nothing of what they reach, so a generic parent, `Toolbox<L, R>` or `TuningTool<L>`, declares its children by its list.
- The engine reads no declared sends: the guest host repeats a native build's checks of a guest's sends, the setup and the spawn check read `depends` (ADR-0022), and the root list checks only that each of its entries is a root (ADR-0055).
- Placement learns nothing from a declaration about who talks to whom (ADR-0013); that is left to traffic (Later).
- Every actor is rewritten into its block, and every handler that replied returns its answer. The change is mechanical, and a site missed does not build.

## Alternatives considered

- **Keep the per-handler sends.** They cannot be derived from the body: a macro sees only tokens, about a thousand calls outside tests hand the turn to a helper, and stable Rust cannot collect facts from function bodies. So they stay written by hand, restating each call.
- **Rename the markers** (`Replies`, `Tells`, `Asks`). The markers go.
- **One `sends_to(…)` list for each actor.** It catches no bug that a check at the call and `depends` do not.
- **Declare nothing at all.** It loses the check, as the program is built, that it holds the roots an actor needs, which saves a run.
- **Derive the sends with a nightly `rustc_private` driver.** It ties every build to nightly and to the compiler's internals.
- **`depends` naming only root types.** A type in it says where in the tree the actor reaches, and its root is derived from its chain, so a root type is one case of it.
- **An optional dependency,** `maybe(…)` in `depends`, or a root looked up by its name and cast to what is sent. A program could then lack what an actor reaches with nothing checked at build, and the dependency would point from a publisher to its listeners. An actor of its own, listed where both sides stand, keeps every dependency checked.
- **Every path into a tree through `found`.** Only a keyed actor could exist and may not, and a tell to an empty place is already dropped and an ask answered as gone, so an `Option` at every call says nothing the send does not.
- **A parent's `depends` covering its children's.** Every parent would restate its subtree's reach, and a generic parent, such as a toolbox of any tools, could not write what its children reach.
- **The answer on the kind,** `#[kind(answers = …)]`. An actor layer answers `Inbound<In>` with `Inward<its Out>` (ADR-0035), so the kind cannot fix its answer.
- **One word, `#[handles]`, with the return type deciding the role.** Split words read faster: the role is seen without reading the return type.
- **The guard in the attribute,** `#[request(from = …)]`. The signature should show the type `turn.from()` answers, which the turn's guard argument does.
- **A list in `#[actor(handles(…))]`, with handlers in `#[handlers]` blocks.** Two places to keep in step; one block lists every handler once.
- **`Speaks<P>` checked at the call by `Lists`,** each of `P`'s kinds found at its place in the sender's list, as delivery reads a cast. Its index is a place for each kind, which every generic sender would thread through its bounds; `Conforms` reads the sender's handlers, needs no index, and is what makes an `ActorRef` a `ProtocolRef<P>`, so the reference a guard hands on is one the sender could have made of itself.
- **Code written for any sender generic over the guard,** a screen's methods taking `Turn<'_, Self, G>`. Every method of every screen would name a guard for the sake of the one handler that takes a drop; `unguarded` lends that handler's turn as a turn from `Anyone`.

## Later

- **Who talks to whom, learned from traffic.** Sharding or placement can count sender and receiver types as mail is routed, as Orleans 8.2's activation repartitioning does, in place of reading it from declarations (ADR-0013). It is not built now.
- **The HTTP stack's forwarding handlers and their send lists** (ADR-0035) are left as they are here; how the stack is written is decided apart. `with(…)` is available to it.
