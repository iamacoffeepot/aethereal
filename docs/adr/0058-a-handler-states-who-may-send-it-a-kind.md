# ADR-0058: A handler states who may send it a kind

- **Status:** Proposed
- **Date:** 2026-11-17

## Context

A handler knows the sender of what it handles only as an `ErasedActorRef` (`Turn::sender`). Its type cannot be read from it, and a cast proves only which kinds the sender's type lists (ADR-0016: `Turn::cast`). So a handler that takes a kind only from some senders checks the sender by hand, in its body. The code holds about twenty such checks, in four shapes:

- **Mail the host brings.** The engine records the actor itself as the sender of what a source brings it (ADR-0010), of a job's answer (ADR-0018), and of a lifecycle stage (ADR-0020): each is mailed `by` and `to` the same actor (`Mailed`). An actor that acts on what its host says compares the sender with `turn.me()`, as `Pacer` does for `Paced`.
- **Mail from its parent.** `Spawned` takes `Stop` only from `Engines`, and `Window` takes `Event` only from `Windows`, each comparing the sender with `turn.parent()`.
- **A subscription.** `Window`, `Tools`, `Controls`, `Link`, and `Renderer` take a subscribe from anyone, cast its sender to the protocol they will send it, and log and pass over one whose type does not list it.
- **A check against the receiver's state or the mail.** `Spawned` takes `Exited` only from the process it holds, `Engines` takes `Runs` only from a spawn it keeps a record of, and `Windows` takes `ShowCursor` only from the window the mail names.

Each is a few lines that the handler's signature does not show. A reader learns whom a handler takes a kind from by reading its body, a sender that is passed over is told nothing, and the next handler of the same shape is written again by hand. HTTP needs the same: a handler takes a request with its principal only from an exchange (ADR-0035).

What the engine already has:

- **Each item has a source the engine writes** (ADR-0009: `Item`; `Turning::source`).
- **Each actor is seated with its type's declaration** (ADR-0013: `Declared`; `Table::declared`), so where mail is delivered the engine can read the sender's type: natively from the table's entry, and in a guest's table by asking its host, as a cast does (`Table::lists`).
- **Dispatch picks the handler** (`declared/handles.rs`: `HandledList::dispatch`, which calls `hand::<A, H>`), and takes the first listing whose role matches the item, so an actor type lists each kind once.

## Decision

**A mail handler states, beside what it may send, who may send it its kind: `type From`, a guard. Dispatch asks the guard before the handler runs, and hands what the guard refuses to the handler's `refused`.**

```rust
impl Handles<Stop> for Spawned {
    type Sends = (ToType<Process, Stop>,);   // who it may send to
    type From = ActorRef<Engines>;           // who may send it this

    fn handle(&mut self, stop: Stop, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> { /* … */ }
}
```

### A guard is a type that admits or refuses a sender

```rust
pub trait Handles<K: Kind>: Actor {
    type Sends: Sends + DeclaredBy<Self>;
    type From: Guard<Self, K>;

    fn handle(&mut self, mail: K, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault>;

    /// Mail of `K` from a sender `From` refused: logged, naming the sender, and a request answered
    /// `Unanswered::Unreadable`; other mail is dropped.
    fn refused(&mut self, refused: Refused<K>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.refuse(&refused)
    }
}

pub trait Guard<A: Actor, K>: 'static {
    /// The types of sender it may admit, as far as types alone decide: what a native send that names `A` is checked
    /// against when the program is built. A guard that reads the receiver or the mail leaves it `Read`.
    const TYPES: SenderTypes = SenderTypes::Read;

    /// Whether `actor` takes `mail` from `sender`.
    fn admits(actor: &A, mail: &K, sender: &Sender<'_>) -> bool;
}

/// The source of the item being handled, as the receiver's turn reads it: nothing is read until a guard asks.
pub struct Sender<'t> { .. }

impl Sender<'_> {
    pub fn reference(&self) -> ErasedActorRef;           // who it is
    pub fn is<T: Actor>(&self) -> bool;                   // it is an actor of type `T`
    pub fn speaks<P: Listed>(&self) -> bool;              // its type lists every kind of `P`, as a cast reads it
    pub fn is_receiver(&self) -> bool;                    // it is the receiver's own seat
    pub fn is_parent(&self) -> bool;                      // it is the actor directly above the receiver
    pub fn is_child<T: ChildOf>(&self) -> bool;           // it is of type `T`, directly beneath the receiver
    pub fn is_at<L: ChildOf + PathTo>(&self, path: &Path<L>) -> bool;   // it is the live actor the typed path names
}

/// Which types of sender a guard may admit, as its type alone says.
pub enum SenderTypes {
    Every,                                  // any sender
    Types(&'static [KindId]),               // an actor of one of these types
    Speaking(&'static [KindId]),            // an actor whose type lists every one of these kinds
    AnyOf(&'static [SenderTypes]),          // what any of these admits: a tuple of guards
    Read,                                   // decided from the receiver or the mail, when the mail is delivered
}
```

A guard is handed the receiver, the mail read as its kind, and the sender, and decides from any of them. There is one trait and one call: the forms below are types the library writes impls of, and a handler writes its own as it writes any impl.

### The library's guards

| `From` | Admits | `TYPES` |
|---|---|---|
| `Anyone` | every sender | `Every` |
| `ActorRef<T>` | an actor of type `T` | `Types([T])` |
| `(G1, G2, …)` | what any of its guards admits | `AnyOf([G1::TYPES, G2::TYPES, …])` |
| `Parent` | the actor directly above the receiver | `Types([A::Parent])` |
| `Child<T>` | an actor of type `T` directly beneath the receiver | `Types([T])` |
| `Itself` | the receiver's own seat | `Types([A])` |
| `Speaks<P>` | an actor whose type lists every kind of `P`, a tuple of kinds or a named protocol | `Speaking(P's kinds)` |

```rust
impl<A: Actor, K> Guard<A, K> for Itself {
    const TYPES: SenderTypes = SenderTypes::Types(&[KindId::of::<A>()]);

    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> bool {
        sender.is_receiver()
    }
}

impl<A: Actor, K, G1: Guard<A, K>, G2: Guard<A, K>> Guard<A, K> for (G1, G2) {
    const TYPES: SenderTypes = SenderTypes::AnyOf(&[G1::TYPES, G2::TYPES]);

    fn admits(actor: &A, mail: &K, sender: &Sender<'_>) -> bool {
        G1::admits(actor, mail, sender) || G2::admits(actor, mail, sender)
    }
}
```

- **`Itself` is mail the actor's own seat sent: what its host brought it, or what it sent itself.** The engine mails what a source brings (`aethereal-substrate/src/engine/bring/mod.rs:123`), a job's answer (`engine/jobs/mod.rs:384`), and a lifecycle stage (`engine/lifecycle/mod.rs:437`, its `by` and `to` at `:438` and `:439`) with the actor as both sender and receiver. The seat is compared, not the type, so it is exact for a keyed type as for a singleton: a sibling of the same type sits in another seat.
- **`Parent` and `Child<T>` compare seats too,** so on a keyed parent's child `Parent` is its one parent, where `ActorRef<P>` is any actor of the parent's type.
- **`Speaks<P>` is the form of a subscription:** a publisher that will send `Event` to whoever subscribes takes `Subscribe` from `Speaks<(Event,)>`.
- **The tuples run to the width `Sends` tuples do,** written by the same macro.

### A handler's own guard reads its state or the mail

```rust
/// The process its spawn runs: the one `Spawned` holds, once it has one.
pub struct ItsProcess;

impl Guard<Spawned, Exited> for ItsProcess {
    fn admits(spawned: &Spawned, _: &Exited, sender: &Sender<'_>) -> bool {
        spawned.process().is_some_and(|process| sender.reference() == process.erase())
    }
}

impl Handles<Exited> for Spawned {
    type Sends = (ToType<Engines, Spawn>, ToType<Engines, Exited>);
    type From = ItsProcess;
    // ..
}
```

Its `TYPES` is left `Read`: what it admits is known only when the mail is delivered. A tuple admits what either of its guards admits, never what both do, so a guard that also wants a type checks it in its own body, `sender.is::<Process>() && …`.

### Only mail has a guard

An answer is bound by the engine to the ask it answers (ADR-0017), an ending and a stage are made by the engine (ADR-0008, ADR-0020), and a fallback takes mail of kinds its receiver does not list. A guard on any of them would always be `Anyone`. So `Handles<K>` is for mail of a kind `K`, and what an actor takes that is not mail of a kind it lists has a trait of its own, the same with no guard:

```rust
pub trait Receives<H: Received>: Actor {
    type Sends: Sends + DeclaredBy<Self>;

    fn handle(&mut self, item: H::Item<'_>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault>;
}
```

`Received` is `Answer<A, C>`, `Ended`, `On<K>`, and `Others`. An actor's `Handles` list still names all of them, and `HandledList` reaches each through the trait for its role, as `Handled` already tells a kind from the rest (`impl<K: Kind> Handled for K` beside `impl Handled for Ended`).

### Checked where the mail is delivered

`hand::<A, K>` (`declared/handles.rs`) reads the mail as its kind, as it does today, then asks the guard:

```rust
let sender = turning.sender(declared);

if <A::From as Guard<A, K>>::admits(actor, &mail, &sender) {
    A::handle(actor, mail, &mut turning.declaring::<A, A::Sends>(declared))
} else {
    A::refused(actor, Refused { sender: sender.reference(), mail }, &mut turning.declaring::<A, A::Sends>(declared))
}
```

- **It holds for every sender:** a native actor, a guest whose host wrote its mail (ADR-0015), the outside (ADR-0036), a send through a `ProtocolRef`, and a reply.
- **`Anyone` costs nothing.** Its `admits` is `true`, inlined, and the branch folds away; the sender is not read.
- **A sender's type is read from the table.** `Table::declared` (`aethereal-actor/src/boundary.rs:39`) answers it natively from the actor's entry (`aethereal-substrate/src/table/mod.rs:65`), or from what the step stood up (`engine/since.rs:125`). A guest's table holds no other actor's type (`aethereal-actor/src/guest.rs:1632`), so `Table` gains `is_of(id, actor_type)`, which a guest's table asks its host for, as it does `lists` (`guest.rs:1622`). `Itself`, `Parent`, and `Child<T>` compare seats the turn already holds.
- **Mail that does not read is not handed to the guard.** It is answered as unread mail is (ADR-0017: `Untaken::Unread`), whoever sent it.

Whether a guest's own types, and the source the engine writes for its mail, can be relied on is decided separately; this decision reads them as the engine records them.

### Checked when the program is built, where the receiver's type is named

A native send that names its receiver's type declares it: `ToType<T, K>` (`declared/sends.rs:44`) for a send through an `ActorRef<T>` or to `turn.me()`, and `Tells<Path<L>, K>` (`declared/sends.rs:30`) and `Asks<Path<L>, K, C>` (`aethereal-actor/src/ask.rs:501`) for a typed path. Each already requires that its receiver handles `K`, and each marker's `SENDING` now carries the receiver's guard's `TYPES`: `Target::Type` and `Target::Path` gain a `SenderTypes`, read from `<<T as Handles<K>>::From as Guard<T, K>>::TYPES`.

`Declared::checked` (`declared/mod.rs:281`), which already refuses a type that handles one kind twice, then checks each send the type declares, as it begins, in each handler, and as it ends, against its receiver's `SenderTypes`: `Types` must hold the declaring type's id, and the declaring type must list each of `Speaking`'s kinds; `Every` passes, and so does anything that holds `Read`, which only delivery decides. A send that cannot be admitted panics in the constant, which stops the build and names the declaring type.

- **Nothing is inferred at a call site.** The check reads the declaration, where both types are known, so no index is written or inferred, and a send declared and never made is checked too.
- **What the build checks:** `Anyone`, `ActorRef<T>`, `Parent`, `Child<T>`, `Itself`, `Speaks<P>`, and tuples of them. `Parent`, `Child<T>`, and `Itself` are checked by type when built, and by seat at delivery. A handler's own guard, and a tuple that holds one, is checked at delivery only.
- **What only delivery checks:** a send through a `ProtocolRef<P>` (`ToAny<P, K>`), which names no receiver type; a reply (`ToSender<K>`), whose receiver is whoever sent the item; and every send a guest makes.

### A refusal goes to the handler's `refused`

```rust
pub struct Refused<K> {
    pub sender: ErasedActorRef,                             // who sent it: a sender its handler's guard refused
    pub mail: K,                                            // what it sent, read as its kind
}
```

- **A refusal is the handler's to answer, beside the handler.** It shares the handler's `Sends`, so an override may answer a request with a kind of its own, or tell whoever the handler may, and the behaviour of one kind stays in one impl.
- **The default** logs a reason of its own, `Unreadable::Refused`, naming the sender's path and type, and answers a request `Unanswered::Unreadable`, as mail that did not read is answered (ADR-0017). A tell is dropped.

### A kind is listed once

Dispatch takes the first listing whose role matches, so the declaration's constants already refuse a type that lists one kind twice (ADR-0013). There is no wrapper: a kind is listed as itself, and its one handler's guard decides who may send it. A kind several receivers take, such as `Exited`, which `Spawned` takes from `ItsProcess` and `Engines` from its spawns, is taken by each under its own guard.

### Not decided here: what `admits` answers

- **`bool`,** as above. `handle` is unchanged, and a handler that sends to its sender casts it, as it does today: for `Speaks<P>` the cast cannot miss. A handler that tells its senders apart asks `Sender::is` in its guard, and again in its body.
- **The checked sender.** `Guard` gains `type Sent`, and `admits` answers it, or why not (`Option<Self::Sent>`, or a `Result`). Dispatch keeps it on the turn, `Turn<'a, Me, S, F = Anyone>`, read with `turn.from()`, and a guarded handler names its guard in its turn, `Turn<'_, Self, Self::Sends, Self::From>`; a handler with `Anyone` keeps its signature, as the parameter is defaulted, and is handed the `ErasedActorRef`. `ActorRef<T>` hands an `ActorRef<T>`, `Speaks<P>` a `ProtocolRef<P>`, an enum of references that implements `Guard`, `enum Peer { Exchange(ActorRef<Exchange>), Client(ActorRef<Client>) }`, tells its senders apart in one match, and a handler's own guard hands what it found, such as the spawn record its sender is.

## Migration

**Every mail handler states its guard.** Rust 1.97.1 (`rust-toolchain.toml`) has no defaults for associated types, so each `Handles<K>` impl for a kind gains `type From = Anyone;`: 465 at 9204d99, 20 of them in the bodies of `macro_rules!`, and 34 more in doc examples. Each impl for an answer, an ending, a stage, or a fallback is written `Receives<…>` in place of `Handles<…>`: 262 of `Answer`, 28 of `Ended`, 56 of `On`, and 2 of `Others`, and 7 in doc examples. A bound that names a handler of an answer (`Asks`'s `DeclaredBy`) reads `Receives`.

**Each check by hand becomes a guard, and leaves the body.** At 9204d99:

- **The parent.** `aethereal-link/src/engines/engine.rs:451`: `Engine` takes `Shut` from `ActorRef<Engines>`. `aethereal-link/src/engines/spawned.rs:229` and `:250`: `Spawned` takes `LinkedAs` and `Stop` from `ActorRef<Engines>`. `aethereal-window/src/window.rs:226`: `Window` takes `Event` from `ActorRef<Windows>`. Each parent is a singleton, so `Parent` would say the same.
- **What the host brings.** `aethereal-window/src/windows/mod.rs:448`: `Windows` takes `Heard` from `Itself`. `aethereal-chassis-browser/src/controls/mod.rs:138`: `Controls` takes `Control` from `Itself`. `aethereal-pace/src/pacer/mod.rs:233`: `Pacer` takes `Paced` from `Itself`. `aethereal-render/src/renderer/mod.rs:1083` and `:1165`: `Renderer` takes `Mapped` and `Surfaced` from `Itself`.
- **A subscription.** `aethereal-window/src/window.rs:193` and `:280`: `Window` takes `Subscribe` from `Speaks<(Event,)>` and `SubscribeProbes` from `Speaks<(Probed,)>`. `aethereal-chassis-browser/src/controls/mod.rs:104`: `Controls` takes `Subscribe` from `Speaks<(Control,)>`. `aethereal-tools/src/tools/mod.rs:164`: `Tools` takes `Subscribe` from `Speaks<(Sheets,)>`. `aethereal-link/src/link/chat.rs:108`: `Link` takes `Listen` from `Speaks<Listening>`. `aethereal-render/src/renderer/watching/mod.rs:176` and `:205`: `Renderer` takes `SubscribeFrameStats` from `Speaks<(FrameStats,)>` and `SubscribeGpuTimes` from `Speaks<(GpuTimes,)>`.
- **The receiver's state.** `aethereal-link/src/engines/spawned.rs:207`: `Spawned` takes `Exited` from `ItsProcess`. `aethereal-link/src/engines/spawning.rs:125`, `:147`, and `:165`: `Engines` takes `Runs`, `Spawn`, and `Exited` from `ItsSpawn`, a spawn it keeps a record of. `aethereal-link/src/engines/mod.rs:371`: `Engines` takes `Shown` from `ItsEngine`, an engine it has linked. `aethereal-window/src/window.rs:298` and `:323`: `Window` takes `Picked` and `KeysListed` from `Prober`, an actor subscribed to its probes; `aethereal-window` cannot name `Renderer`, which depends on it. `aethereal-actor/src/stream/streams.rs:116` and `:132`: a stream's opener takes `Part<S>` from the library's guard for parts, which reads the actor's `Streams<S, C>` and admits the receiver the part's stream was opened with.
- **The mail.** `aethereal-window/src/windows/mod.rs:425`: `Windows` takes `ShowCursor` from `ItsWindow`, the `Window` keyed by the mail's `window`. `aethereal-render/src/renderer/mod.rs:1063`: `Renderer` takes `Probed` from `ProbingWindow`, the actor at `path::<Window>` of the mail's `window`. `aethereal-sessions/src/sessions/chat/mod.rs:211`: `Sessions` takes `Chatted` from `JoinedAs`, the actor that joined as the mail's `engine`; `aethereal-sessions` cannot name `Engine`, as `aethereal-link` depends on it.
- **No check today.** `aethereal-sessions/src/sessions/chat/mod.rs:173`: `Sessions` takes `Joined` from `Speaks<Showing>`, an actor it can show sessions to.

A check that passed the mail over silently, or logged a warning of its own, now goes to the default `refused`.

**HTTP (ADR-0035).** A handler takes `Routed<S>`, and an authenticator `Authenticate`, from `ActorRef<Exchange>`, written as `type From` in each one's impl.

**Untouched.** `Engines` answers `TakeToken` (`aethereal-link/src/engines/mod.rs:462`) for any asker, and links only an engine of its own: that is what the token is for, not who may ask. The renderer keys what it makes by its maker (`renderer/mod.rs:1379`, `:1442`, `:1502`, `:1679`, `:1720`; `renderer/text/mod.rs:697`, `:742`), and casts its sender for what it will tell it (`CreateDispatch` at `:1567`, `DrawList` at `:1611`), each taking from anyone. An unsubscribe removes its own sender, and a store, a connection, or HTTP holds whoever asked.

## Consequences

- Whom a handler takes its kind from is in its impl, beside what it sends, and the engine holds it for every sender.
- A refused sender is logged, and a refused request answered, where a check by hand often did neither.
- A native send to a type whose guard is decided by type does not build when it would be refused. A send through a protocol reference, a reply, and a guest are checked at delivery.
- `Handles<K>` gains `From` and a default `refused`, and is for mail; `Receives<H>` takes answers, endings, stages, and fallbacks. `Sending`'s typed targets carry `SenderTypes`, and `Table` gains `is_of`.
- A test that hands a guarded handler its mail from the outside (ADR-0036), whose type is its own, stands up the type the guard admits.
- **Not decided here:** what `admits` answers, `bool` or the checked sender.

## Alternatives considered

- **The rule on the protocol,** `Carries<K>::Senders`. Protocols are structural: any type whose handlers cover a protocol's kinds conforms (`declared/protocols.rs:158`), so a sender reaches the same handler through another protocol, a tuple of kinds, or an `ActorRef`, and passes no rule a protocol states.
- **The rule on the kind,** `#[kind(senders = …)]`. A kind is data in the data layer and names no actor, and a kind several receivers take from different senders, `Shut`, `Stop`, `Event`, `Control`, would have to be split.
- **A `SentBy<S, K>` wrapper in the actor's list, with senders restating it** (`ToType<T, K, Only<S>>`). The rule is written in two places, and every sender's declaration changes when a receiver adds one.
- **A `Senders` trait whose forms each have a check of their own,** with a derive for enums. Every form is a type that implements `Guard`, checked by one call; a form the library lacks is an impl a handler writes.
- **`From` on every `Handles` impl,** answers, endings, stages, and fallbacks included: 348 more lines that each say `Anyone`, of items no sender chooses.
- **The build check as a trait bound,** `<T as Handles<K>>::From: Admits<A>` on `ToType`'s `DeclaredBy<A>`. It holds for one type, but a tuple and `Speaks<P>` need an index, a place in the tuple or the places of `P`'s kinds, which an impl cannot leave unconstrained. At the send, where `Allows` infers its index, `Toward` would have to carry the sender's type, and a send declared and never made would go unchecked.
- **The check at the send alone.** It sees no guest, no reply, and no send through a protocol reference, which is how HTTP's exchange reaches its handler.
- **The check at delivery alone.** It holds, but a native sender learns only from a log at run time what its build could have told it.
- **Rust privacy on a kind's constructor.** A native sender must build a value to send it, but a guest writes bytes, whatever kind they claim.
