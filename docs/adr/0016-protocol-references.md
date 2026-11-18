# ADR-0016: References typed by a protocol

- **Status:** Proposed
- **Date:** 2026-10-18

## Context

A reference proves what it reaches. `ActorRef<T>` proves its actor is a `T`, so a send through it is not checked again (ADR-0008, ADR-0013: `Seat::write`). That proof names one type. A list of `ActorRef<T>` holds actors of one type, so the fan-out send, `turn.to(&references).send(&mail)`, reaches only actors of one type. A publisher whose listeners are a camera, a picker and a menu cannot hold them in one list.

What reaches actors of different types today is a send to "whoever handles some kinds": `ToAny<P, K>`, where `P` is a tuple of kinds (the `Kinds` trait), through a `To` that names the receiver. The send is checked at every send against the receiver's declaration (`Seat::check`, `Target::Handling`), and a receiver that does not list `P`'s kinds stops the engine. ADR-0008 left the reference that carries that proof to a later ADR: "a reference typed by a protocol rather than an actor's type … stand[s] on these types and [is] a later ADR".

There is a second gap: what comes back. A receiver replies to the sender of what it handles (`Turn::reply`, `ToSender<K>`). The send check skips a reply (`Target::Sender` returns early in `Seat::check`). So nothing confirms the sender handles the reply's kind. When it does not, dispatch reaches `unhandled` and the engine panics. In a browser host, wasm cannot unwind, so the page dies. A protocol that says what each request is answered with can close that gap where it is used.

## Decision

**A protocol is a set of kinds an actor takes, with what it answers each of them with.** It comes in two forms:
- a tuple of kinds, which answers none of them;
- a named protocol, which says per kind what comes back.

**A tuple of kinds is a protocol whose kinds are all tells.** `(PointerMoved, KeyPressed)` is the protocol of an actor that handles both, and nothing comes back. It is declared where it is used and needs nothing more.

**A named protocol is composed like an actor's handlers: a list of its kinds, and one implementation for each.**

```rust
pub trait Protocol { type Kinds: Kinds; }          // the list, as an actor lists `type Handles`
pub trait Carries<K: Kind> { type Answers: Kinds; } // what one kind is answered with: `()` for a tell

#[protocol(WhichTile(answers = TileIs), Highlight)]   // the list, and one `Carries` for each kind (ADR-0059)
pub struct Tiles;
```

`#[protocol(…)]` writes `impl Protocol for Tiles { type Kinds = (WhichTile, Highlight); }`, `impl Carries<WhichTile> for Tiles { type Answers = (TileIs,); }`, and `impl Carries<Highlight> for Tiles { type Answers = (); }`: a bare kind is a tell.

- **The list is needed.** Rust cannot list a type's trait implementations, so what goes over every kind of a protocol reads the list. That means the conversion, the cast, and the declaration the engine reads. The build fails when a listed kind has no `Carries` implementation, as it fails when a kind in `Handles` has no handler.
- **`Answers` is a tuple of kinds,** so a request can be answered with more than one kind, such as a status and then a result. `()` is a tell.
- **Tuples get no `Carries`.** A blanket implementation for tuples would overlap for a tuple that names one kind twice, so a tuple's kinds carry no answers by construction.

**A `ProtocolRef<P>` is a reference proven to reach an actor whose type takes every kind of `P`, with its answers.**

```rust
pub struct ProtocolRef<P> { .. }    // an ActorId and a seat, as every reference is
```

It has the shape of an `ActorRef`: sixteen bytes, `Copy`, crossing what a reference crosses (in mail and in an actor's own state, never between engines). It erases to an `ErasedActorRef`. Like an `ActorRef`, it is proven when it is made, and nothing checks it again. There are three ways to make one:

1. **From a typed reference, at build time.** `reference.protocol()` turns an `ActorRef<T>` into a `ProtocolRef<P>` when `T` conforms to `P`:
   - `T` handles every kind of `P`;
   - for a named `P`, the answer `T`'s handler for a kind `K` returns, `T`'s own `Carries<K>` (ADR-0059), is a kind of `P`'s `Carries<K>::Answers`.

   The compiler checks this, as a trait bound, and nothing is read at runtime. Each answer's place among the protocol's answers is inferred, as a narrowing's places are, which a `From` impl cannot do, so the conversion is a method, the one way to make a protocol reference from a typed one, for a tuple as for a named protocol. A handler need not answer with every kind the protocol lists: what a sender through the protocol must handle is the answers, and what keeps an answer from reaching a sender that does not take it is that the receiver answers with nothing else.
2. **From an erased reference, at runtime, once.** `turn.cast::<P>(reference)` answers with a `ProtocolRef<P>` when the declaration of the actor's type lists every kind of `P`, and with nothing otherwise. It is a lookup in the table, as a send's check is today. Answers are not read, because the engine's declaration holds what a type handles but not what a protocol promises. So a cast is to a tuple, a protocol of tells; a named protocol is reached by conversion.
3. **By narrowing.** `reference.narrow::<Q>()` turns a `ProtocolRef<P>` into a `ProtocolRef<Q>` when every kind of `Q` is one of `P`'s, in any order, each found at its place, which the compiler infers (`Covers`). Narrowing a named protocol to a tuple drops what it answers.

**`ProtocolRef<(A, B)>` and `ProtocolRef<(B, A)>` are two types to the compiler, and one protocol to the engine.** Stable Rust cannot sort types. The engine reads a protocol as a set of kind ids (`Target::Handling`), and a guest's section holds the same set (`section::To::Handling`). The two spellings narrow into each other at no cost. A list holds one spelling, so its owner chooses it, and what is added to the list is narrowed to fit.

**A turn's target takes either reference.** `turn.to(target)` answers a `Toward`, the target found once, and its `send` and `ask` each answer it again, so they chain on the one target or it is bound and sent to many times:

```rust
turn.to(listener).send(&moved)?;                               // one actor
turn.to(&self.listeners).send(&moved)?;                        // several references, the bytes written once
turn.to(renderer).ask(&mesh, &Made::Ground)?.ask(&texture, &Made::Sky)?;
```

`to` is generic over what it takes (`Reachable`): a reference, a typed path (ADR-0022), or a slice, array, or `Vec` of references. A send through it is checked at its call (ADR-0059):
- to an `ActorRef<T>` it builds when `T` handles `K`, and its handler's guard admits the sender's type (ADR-0058);
- to a `ProtocolRef<P>` it builds when `K` is one of `P`'s kinds, and the receiver's guard is checked at delivery.

Neither is checked when it is sent: the reference is the proof. **The target comes first, rather than in flat verbs, one call naming its action, target, and mail** (`send_to(reference, &mail)`, `ask_to(reference, &mail, &context)`, `send_to_all(&references, &mail)`), so that what names it (a typed path's turbofish, ADR-0022) is a call of its own, and every send and ask to one target reads it once. A turn holds its target while it lives, so nothing else is asked of the turn between two sends through a target held. A send through a target answers the target, not `()`, so the last send of a handler is a statement, followed by `Ok(())`. There are no sends by a `To` name (`turn.send(To::Parent, &mail)`, `turn.send_to_any`): each name has a typed target (ADR-0013, ADR-0022). A closing turn's target sends and does not ask (ADR-0017). **A send through a `ProtocolRef<P>` builds only when `K` is one of `P`'s kinds.**

**A sender handles what its protocol sends back.** An ask through a named `P` builds only when the asker handles the answer `Carries<K>::Answers` names, with the context it passes (ADR-0017, ADR-0059). That is a bound at the call, so a type that does not handle it does not compile, and a check of types alone finds it. A tell is answered with nothing. For a send through a named protocol, an answer therefore always reaches a type that takes it.

**A send to an actor of a type gets the same check from its receiver.** An ask of an actor of type `T`, through a typed reference, at a typed path, or at a place named from the sender, reads its answer from `T`'s `Carries<K>`, the return type of `T`'s handler (ADR-0059), and builds only when the asker handles it. A tell is not answered: a receiver that talks back to its sender sends to the typed sender its guard hands it (ADR-0058), checked at that call as any send is. A fallback does not count: a bound cannot say "handles this kind, or falls back", so a type that takes an answer lists its kind.

## Consequences

- One list can hold listeners of many types: `&[ProtocolRef<P>]`, sent to with `turn.to(&listeners).send(&mail)`.
- A send through a protocol reference is checked once, when the reference is made, and never at the send.
- The reply that panics is closed. An answer reaches only an asker that handles it, and a receiver talks back to a sender only through a reference its guard proved, so a send to the sender is checked at its call as any send is (ADR-0058, ADR-0059).
- A send through a `ProtocolRef<P>` of a `K` outside `P` does not build.
- A named protocol gives a set of kinds an identity of its own: a guest's section, or a wire, could later name it. Nothing here gives it one beyond its kind ids.
- A reference that names a protocol reaches every type that handles its kinds (ADR-0013).

## Alternatives considered

- **Tuples only.** Nothing to declare. They cannot say what comes back, so the reply check has nowhere to come from.
- **Named protocols only.** One spelling, and no narrowing. Every ad hoc set of kinds then needs a declaration, and the 39 `send_to_any` sites in the code each name one.
- **Kinds and answers in one list of entries,** `(Ask<WhichTile, TileIs>, Tell<Highlight>)`. Each kind appears once. It is unlike how an actor's handlers are written, where each kind's meaning is a trait implementation of its own.
- **The answer on the request kind,** `impl Request for WhichTile { type Answer = TileIs; }`. One answer per kind everywhere, and every typed send checked. Two protocols could not answer one kind differently, and it puts a protocol's meaning on the kind.
- **A protocol reference cast at runtime to a named protocol,** reading answers from the receiver's handlers. The engine's declaration could hold each handler's answer, so this could be read. It would make a cast's success depend on reply declarations the receiver's author need not have meant as a promise. A conversion at build time says the same thing where the compiler can check it.
