# ADR-0059: An actor is written as one block, and declares only the roots it depends on

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

**An actor is written as one block, `#[actor]` on `impl Name { … }`, which writes its `ChildOf`, its `Actor`, and every handler's impl. No handler declares what it sends: every send is checked at its call. The actor declares the root types whose trees it reaches by typed path, `depends(…)`, as an import list, and the program checks that it holds them.**

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
- **An actor has exactly one `#[actor]` block, and it lists every handler.** An actor whose logic spans files keeps its one block, and each handler's body calls ordinary methods, `impl Name { … }` in the files of each concept. The block is the actor's whole surface, read in one place. There is no second macro for a block split across files.
- **Defaults.** Its arguments are `key = K`, `()` when left out, and `parent = P`, `Top` when left out. `end` may be left out, and is then `Ok(())`.
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
- **The turn has no `reply`.** A helper that replied returns the answer to its handler instead. A tell that talks back to its sender states a typed guard, such as `ActorRef<Pinger>` above (`guard/library.rs`), and sends to the sender it is handed, `*turn.from()`, a send checked at its call as any is.

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

The macro copies the struct's generics onto each impl it writes, so a protocol's bound moves onto the struct, as `S: Stream` does here. What uses a protocol is unchanged: `ProtocolRef<P>`, `.protocol()` and `Conforms`, `Speaks<P>`, `Covers<Q>`, and tuples of kinds such as `(Pong,)` (ADR-0016, ADR-0058). `Conforms` reads each kind's answer from the actor's `Carries<K>`, the return type of its request.

### No sends are declared; `depends(…)` imports root types

**Every send is checked at its call.** A send builds when:

- **the target handles the kind:** an `ActorRef<T>` or a typed path to `T` when `T` has a handler for `K`, and a `ProtocolRef<P>` when `K` is one of `P`'s kinds (ADR-0016);
- **for an ask, the asker has an `#[answer]` for the receiver's answer, with the context it passes:** the answer read from the receiver type's `Carries<K>`, or from the named protocol's (ADR-0017);
- **for a typed path, its root type is in the actor's `depends`.**

**`#[actor(depends(Renderer, Host))]` names the root types whose trees the actor reaches by typed path.** A typed path follows its `ChildOf` chain up to exactly one root type (ADR-0022): `path::<Lens>(())` runs `Host`, then `Camera`, then `Lens`, so it needs `Host`. `turn.to(path::<L>(keys))` and `turn.found(&path::<L>(keys))` build only when the actor depends on the root type at the top of `L`'s chain. The list is kept on the actor's type, as a tuple, and the actor depends on `R` when the tuple holds it, found as `Lists` finds a kind (`declared/lists.rs`). One left out fails as an import left out does, with a diagnostic written for it: "`Simulator` reaches `Lens` beneath `Host`, which it does not depend on".

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

- **The program checks, as it is built, that it holds every root type its roots depend on.** An instanced root type counts, as the setup's list holds it as one entry however many keys `add_roots` makes it under (ADR-0022, ADR-0029).
- **It checks the order too:** a root is added after each singleton root type in its `depends`, so that one its `begin` reaches is there when `begin` runs. `found` in `begin` is a reach by typed path like any other, so the order check now covers it.
- **A spawn builds only when the parent's `depends` covers the child's**: `spawn::<C>` checks that every root type in `C`'s list is in the spawner's, a subset check over the two tuples built on `Lists`. So a root's list covers its whole subtree, and the program checks only roots.
- **A spawn is refused, before the child's `begin` runs, when a singleton root type its `depends` names holds no live actor** (ADR-0022: `Refused::Missing`). It reads the child's list where it read its declared paths.
- **Not covered, and checked at run time as today:** whether a keyed instance, or an actor beneath a root, is live now. `turn.found` answers `Option`, and a send to an empty place is dropped and an ask answered as gone (ADR-0022).
- **A reference needs no entry.** An `ActorRef`, a `ProtocolRef`, and the sender a guard hands on were proven when they were made (ADR-0016, ADR-0058).
- **A guest's section carries its `depends` in place of each handler's sends** (ADR-0015), and its host checks each send a guest makes at a typed path against it, at run time, failing the turn as it fails a false send.

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

- An actor is read in one block: where it stands, its key, what it depends on, and every handler with its role, kind, guard, and answer in its signature. What was restated by default is left out.
- A send is checked where it is written, against what it names, and nothing restates it. A send that does not build points at the call.
- An actor type's `Handles` always matches its handlers, so a send of a kind its receiver has a handler for never stops the engine at dispatch.
- A request always answers, now or later, as its return type says: a native handler cannot leave a request unreplied except by letting a `Held` go (ADR-0017).
- What an actor needs of the program is one list of root types, checked when the program is built: a program that lacks a root an actor reaches does not build, which saves a run that would only find it missing. The order check covers `found` in `begin`.
- The engine reads no declared sends: the guest host checks a guest's sends at a path against its `depends`, the root list checks a drawn root's `depends` (ADR-0055), and the setup and the spawn check read `depends` (ADR-0022).
- Placement learns nothing from a declaration about who talks to whom (ADR-0013); that is left to traffic (Later).
- Every actor is rewritten into its block, and every handler that replied returns its answer. The change is mechanical, and a site missed does not build.

## Alternatives considered

- **Keep the per-handler sends.** They cannot be derived from the body: a macro sees only tokens, about a thousand calls outside tests hand the turn to a helper, and stable Rust cannot collect facts from function bodies. So they stay written by hand, restating each call.
- **Rename the markers** (`Replies`, `Tells`, `Asks`). The markers go.
- **One `sends_to(…)` list for each actor.** It catches no bug that a check at the call and `depends` do not.
- **Declare nothing at all.** It loses the check, as the program is built, that it holds the roots an actor needs, which saves a run.
- **Derive the sends with a nightly `rustc_private` driver.** It ties every build to nightly and to the compiler's internals.
- **The answer on the kind,** `#[kind(answers = …)]`. An actor layer answers `Inbound<In>` with `Inward<its Out>` (ADR-0035), so the kind cannot fix its answer.
- **One word, `#[handles]`, with the return type deciding the role.** Split words read faster: the role is seen without reading the return type.
- **The guard in the attribute,** `#[request(from = …)]`. The signature should show the type `turn.from()` answers, which the turn's guard argument does.
- **A list in `#[actor(handles(…))]`, with handlers in `#[handlers]` blocks.** Two places to keep in step; one block lists every handler once.

## Later

- **Who talks to whom, learned from traffic.** Sharding or placement can count sender and receiver types as mail is routed, as Orleans 8.2's activation repartitioning does, in place of reading it from declarations (ADR-0013). It is not built now.
- **The HTTP stack's forwarding handlers and their send lists** (ADR-0035) are left as they are here; how the stack is written is decided apart.
