# ADR-0058: A handler states who may send it a kind

- **Status:** Proposed
- **Date:** 2026-11-17

## Context

A handler knows the sender of what it handles only as an `ErasedActorRef` (`Turn::sender`). Its type cannot be read from it, and a cast proves only which kinds the sender's type lists (ADR-0016: `Turn::cast`). So a handler that takes a kind only from some senders checks the sender by hand, in its body. The code holds about twenty such checks, in four shapes:

- **Mail the host brings.** The engine records the actor itself as the sender of what a source brings it (ADR-0010), of a job's answer (ADR-0018), and of a lifecycle stage (ADR-0020): each is mailed `by` and `to` the same actor (`Mailed`). An actor that acts on what its host says compares the sender with `turn.me()`, as `Pacer` does for `Paced`.
- **Mail from its parent.** `Spawned` takes `Stop` only from `Engines`, and `Window` takes `Event` only from `Windows`, each comparing the sender with `turn.parent()`.
- **A subscription.** `Window`, `Tools`, `Controls`, `Link`, and `Renderer` take a subscribe from anyone, cast its sender to the protocol they will send it, and log and pass over one whose type does not list it.
- **A check against the receiver's state or the mail.** `Spawned` takes `Exited` only from the process it holds, `Engines` takes `Runs` only from a spawn it keeps a record of, and `Windows` takes `ShowCursor` only from the window the mail names.

Each is a few lines that the handler's signature does not show. A reader learns whom a handler takes a kind from by reading its body, a sender that is passed over is told nothing, and the next handler of the same shape is written again by hand. HTTP needs the same: a handler takes a request with its principal only from an HTTP stack (ADR-0035).

What the engine already has:

- **Each item has a source the engine writes** (ADR-0009: `Item`; `Turning::source`).
- **Each actor is seated with its type's declaration** (ADR-0013: `Declared`; `Table::declared`), so where mail is delivered the engine can read the sender's type: natively from the table's entry, and in a guest's table by asking its host, as a cast does (`Table::lists`).
- **Dispatch picks the handler** (`declared/handles.rs`: `HandledList::dispatch`, which calls `hand::<A, H>`), and takes the first listing whose role matches the item, so an actor type lists each kind once.

## Decision

**A mail handler states who may send it its kind: a guard, its turn's type argument, which its `#[actor]` block writes as `type From` (ADR-0059). Dispatch asks the guard before the handler runs, hands the handler the sender the guard checked, and hands what the guard refuses to the handler's `refused`. A native send that names its receiver's type is checked against the guard's `Sent` type at its call.**

```rust
#[tell]   // in Spawned's #[actor] block; its turn names who may send it this
fn stop(&mut self, stop: Stop, turn: &mut Turn<'_, Self, ActorRef<Engines>>) -> Result<(), Fault> { /* … */ }
```

### A guard is a type that admits a sender, and hands it on checked

```rust
pub trait Handles<K: Kind>: Actor {
    type From: Checks<Self, K>;
    type Returns: Answering;                                    // how it answers (ADR-0059)

    fn handle(&mut self, mail: K, turn: &mut Turn<'_, Self, Self::From>) -> Result<Self::Returns, Fault>;

    /// Mail of `K` from a sender `From` refused: by default logged, naming the sender, and a request answered
    /// `Unanswered::Unreadable`; other mail is dropped. Its turn holds the sender as an `ErasedActorRef`, as the guard
    /// checked nothing to hand on.
    fn refused(&mut self, _: K, _: &mut Turn<'_, Self>) -> Result<Refusal<Self::Returns>, Fault> {
        Ok(Refusal::Logged)
    }
}

pub trait Guard<A: Actor>: 'static {
    /// What the handler is handed for a sender it admits.
    type Sent: Sent;
}

/// The guard's check of mail of `K`, which may read the mail.
pub trait Checks<A: Actor, K>: Guard<A> {
    /// The sender, checked, when `actor` takes `mail` from it; `None` when it does not, and the mail goes to `refused`.
    fn admits(actor: &A, mail: &K, sender: &Sender<'_>) -> Option<Self::Sent>;
}

/// A checked sender: the types of actor it can be, and how it is taken from the sender, so one type states both.
pub trait Sent: Sized + 'static {
    const TYPES: SenderTypes;

    /// The sender as this type, when it is one; `None` when it is not.
    fn from_sender(sender: &Sender<'_>) -> Option<Self>;
}

impl<T: Actor> Sent for ActorRef<T> { /* Types([T]); sender.actor::<T>() */ }
impl<P: Listed> Sent for ProtocolRef<P> { /* Speaking(P's kinds); sender.speaking::<P>() */ }
impl Sent for ErasedActorRef { /* Every; Some(sender.reference()) */ }

/// The source of the item being handled, as the receiver's turn reads it: nothing is read until a guard asks.
pub struct Sender<'t> { .. }

impl Sender<'_> {
    pub fn reference(&self) -> ErasedActorRef;                                    // who it is
    pub fn actor<T: Actor>(&self) -> Option<ActorRef<T>>;                         // it, when it is an actor of type `T`
    pub fn speaking<P: Listed>(&self) -> Option<ProtocolRef<P>>;                  // it, when its type lists every kind of `P`, as a cast reads it
    pub fn receiver<A: Actor>(&self) -> Option<ActorRef<A>>;                      // it, when it is the receiver's own seat
    pub fn parent<P: Actor>(&self) -> Option<ActorRef<P>>;                        // it, when it is the actor directly above the receiver
    pub fn child<T: ChildOf>(&self) -> Option<ActorRef<T>>;                       // it, when it is of type `T`, directly beneath the receiver
    pub fn at<L: ChildOf + PathTo>(&self, path: &Path<L>) -> Option<ActorRef<L>>; // it, when it is the live actor the typed path names
}

/// Which types of sender a guard may admit, as its `Sent` type says.
pub enum SenderTypes {
    Every,                                  // any sender
    Types(&'static [KindId]),               // an actor of one of these types
    Speaking(&'static [KindId]),            // an actor whose type lists every one of these kinds
    AnyOf(&'static [SenderTypes]),          // what any of these admits: an enum's cases
}
```

A `Sent` type's `TYPES` and its `from_sender` are written in one impl, of one type: `from_sender` can answer an `ActorRef<T>` only through `sender.actor::<T>()`, which proves the sender is a `T`, so the types it states and what delivery admits are the same types.

A guard is handed the receiver, the mail read as its kind, and the sender, decides from any of them, and answers the sender as its `Sent`, or `None`. What a guard hands on depends on the actor (`Itself` hands an `ActorRef<A>`, `Parent` an `ActorRef<A::Parent>`, the stream library's parts guard a reference generic over its stream `S`), never on the mail's kind, so `Sent` sits on `Guard<A>`, which the turn can name, and the check that reads the mail on `Checks<A, K>`. A guard that admits several kinds writes one `Guard` impl and one `Checks` impl for each kind. There is one call: the forms below are types the library writes impls of, and a handler writes its own as it writes any impl. Each of `Sender`'s checks answers the reference a guard hands on, typed, so a guard never casts; a seat's check reads the type too, so `receiver` of another type than the receiver's answers `None`.

### The handler reads the sender its guard checked

Dispatch keeps what `admits` answered on the turn, `Turn<'a, Me, G: Guard<Me> = Anyone>`, read with `turn.from()`, which answers `&G::Sent`. A guarded handler names its guard in its turn, `Turn<'_, Self, G>`, from which its `#[actor]` block writes `type From = G` (ADR-0059). A handler with `Anyone` writes `Turn<'_, Self>`, as `G` is defaulted, and `Anyone`'s `Sent` is the `ErasedActorRef`.

### The library's guards

| `From` | Admits | Hands the handler (`Sent`) |
|---|---|---|
| `Anyone` | every sender | `ErasedActorRef` |
| `ActorRef<T>` | an actor of type `T` | `ActorRef<T>` |
| `Parent` | the actor directly above the receiver | `ActorRef<A::Parent>` |
| `Child<T>` | an actor of type `T` directly beneath the receiver | `ActorRef<T>` |
| `Itself` | the receiver's own seat | `ActorRef<A>` |
| `Speaks<P>` | an actor whose type lists every kind of `P`, a tuple of kinds or a named protocol | `ProtocolRef<P>` |

```rust
impl<A: Actor> Guard<A> for Itself {
    type Sent = ActorRef<A>;
}

impl<A: Actor, K> Checks<A, K> for Itself {
    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> Option<ActorRef<A>> {
        sender.receiver::<A>()
    }
}

impl<A: Actor, K, T: Actor> Checks<A, K> for ActorRef<T> {
    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> Option<ActorRef<T>> {
        ActorRef::<T>::from_sender(sender)
    }
}
```

`ActorRef<T>` and `Speaks<P>` admit what their `Sent` takes from the sender, each calling its `Sent`'s `from_sender`; `Parent`, `Child<T>`, and `Itself` compare seats, which a `Sent` type alone does not know.

- **`Itself` is mail the actor's own seat sent: what its host brought it, or what it sent itself.** The engine mails what a source brings (`aethereal-substrate/src/engine/bring/mod.rs:123`), a job's answer (`engine/jobs/mod.rs:384`), and a lifecycle stage (`engine/lifecycle/mod.rs:437`, its `by` and `to` at `:438` and `:439`) with the actor as both sender and receiver. The seat is compared, not the type, so it is exact for a keyed type as for a singleton: a sibling of the same type sits in another seat.
- **`Parent` and `Child<T>` compare seats too,** so on a keyed parent's child `Parent` is its one parent, where `ActorRef<P>` is any actor of the parent's type.
- **`Speaks<P>` is the form of a subscription:** a publisher that will send `Event` to whoever subscribes takes `Subscribe` from `Speaks<(Event,)>`, and is handed the subscriber as the `ProtocolRef<(Event,)>` it keeps, with no cast to miss and nothing to log.

### Several senders are a named enum, derived

A handler that takes its kind from senders of more than one type states a named enum of them as its guard, written with `#[derive(Guard)]`. Each case holds one field, of a `Sent` type. The guard sorts the sender once, and the handler matches over what it was handed:

```rust
/// Who may send `Server` a request: an exchange, or a client.
#[derive(Guard)]
pub enum Peer {
    Exchange(ActorRef<Exchange>),
    Client(ActorRef<Client>),
}
```

The derive writes the guard for every actor, its check for every kind, and its `Sent` impl, each read from the cases:

```rust
impl<A: Actor> Guard<A> for Peer {
    type Sent = Peer;
}

impl<A: Actor, K> Checks<A, K> for Peer {
    fn admits(_: &A, _: &K, sender: &Sender<'_>) -> Option<Peer> {
        Peer::from_sender(sender)
    }
}

impl Sent for Peer {
    const TYPES: SenderTypes = SenderTypes::AnyOf(&[<ActorRef<Exchange> as Sent>::TYPES, <ActorRef<Client> as Sent>::TYPES]);

    fn from_sender(sender: &Sender<'_>) -> Option<Peer> {
        ActorRef::<Exchange>::from_sender(sender)
            .map(Peer::Exchange)
            .or_else(|| ActorRef::<Client>::from_sender(sender).map(Peer::Client))
    }
}
```

The handler names it and matches:

```rust
#[tell]   // in Server's #[actor] block
fn request(&mut self, request: Request, turn: &mut Turn<'_, Self, Peer>) -> Result<(), Fault> {
    match turn.from() {
        Peer::Exchange(exchange) => { /* … */ }
        Peer::Client(client) => { /* … */ }
    }
}
```

The match is exhaustive: a third peer is a case added to `Peer`, and a match that does not take it does not build. The check and the types it states are both read from the cases, so they cannot drift: a case added is a type admitted and a type stated.

- **Cases are tried in the order written, and the first that takes the sender wins.** The order matters only when a sender fits two cases, such as an `ActorRef<Exchange>` case beside a `ProtocolRef<P>` case for a protocol `P` that `Exchange` speaks: an exchange is handed as the case written first.
- **The derive also writes `SentBy<Me, I>` for the enum,** holding when `Me` is one of its cases' types, found at `I` as `Lists<Me, I>` finds a kind, which the check at a send's call reads (below).
- **`#[derive(Sent)]` writes the `Sent` impl alone,** for an enum a guard of a handler's own hands back: one that also reads the receiver's state or the mail, and writes its own `Guard` and `Checks`.
- **The derives live in a new crate, `aethereal-actor-derive`,** beside `aethereal-actor` as `aethereal-data-derive` is beside `aethereal-data`: they derive the actor crate's traits, not the data layer's. `aethereal-actor` re-exports them beside the traits, as it re-exports `Portability` (`aethereal-actor/src/lib.rs:156`). The derives refuse, at compile time, a generic enum and a case that does not hold exactly one field.
- **There are no tuples of guards:** a named case says who sent the mail, where a position does not.

### A handler's own guard reads its state or the mail

```rust
/// The process its spawn runs: the one `Spawned` holds, once it has one.
pub struct ItsProcess;

impl Guard<Spawned> for ItsProcess {
    type Sent = ActorRef<Process>;
}

impl Checks<Spawned, Exited> for ItsProcess {
    fn admits(spawned: &Spawned, _: &Exited, sender: &Sender<'_>) -> Option<ActorRef<Process>> {
        let process = ActorRef::<Process>::from_sender(sender)?;
        spawned.process().filter(|held| *held == process)
    }
}

#[tell]   // in Spawned's #[actor] block
fn exited(&mut self, exited: Exited, turn: &mut Turn<'_, Self, ItsProcess>) -> Result<(), Fault> { /* … */ }
```

It takes the sender as its `Sent` type, as the library's guards do, and hands it on when it is the process it holds. Its `Sent` is `ActorRef<Process>`, so the types it states are `ActorRef<Process>`'s: a send of `Exited` to `Spawned` builds only from a `Process` (below), and delivery checks that the sender is the process this spawn holds.

### Only mail has a guard

An answer is bound by the engine to the ask it answers (ADR-0017), an ending and a stage are made by the engine (ADR-0008, ADR-0020), and a fallback takes mail of kinds its receiver does not list. A guard on any of them would always be `Anyone`. So `Handles<K>` is for mail of a kind `K`, and what an actor takes that is not mail of a kind it lists has a trait of its own, the same with no guard:

```rust
pub trait Receives<H: Received>: Actor {
    fn handle(&mut self, item: H::Item<'_>, turn: &mut Turn<'_, Self>) -> Result<(), Fault>;
}
```

`Received` is `Answer<A, C>`, `Ended`, `On<K>`, and `Others`. An actor's `Handles` list still names all of them, and `HandledList` reaches each through the trait for its role, as `Handled` already tells a kind from the rest (`impl<K: Kind> Handled for K` beside `impl Handled for Ended`).

### Checked where the mail is delivered

`hand::<A, K>` (`declared/handles.rs`) reads the mail as its kind, as it does today, then asks the guard:

```rust
let sender = turning.sender(declared);
let admitted = <A::From as Checks<A, K>>::admits(actor, &mail, &sender);

match admitted {
    Some(from) => {
        let mut turn = turning.declaring::<A, A::From>(declared, from);
        A::handle(actor, mail, &mut turn)?.send(&mut turn, Token::new())?;    // the answer, sent (ADR-0059)

        turn.settled()
    }
    None => refuse(actor, mail, turning, declared),          // `refused`, and its answer or the default
}
```

- **It holds for every sender:** a native actor, a guest whose host wrote its mail (ADR-0015), the outside (ADR-0036), a send through a `ProtocolRef`, and an answer.
- **`Anyone` costs nothing.** Its `admits` answers the reference the turn already holds, inlined, so the `None` arm folds away and the sender's type is not read.
- **A sender's type is read from the table.** `Table::declared` (`aethereal-actor/src/boundary.rs:39`) answers it natively from the actor's entry (`aethereal-substrate/src/table/mod.rs:65`), or from what the step stood up (`engine/since.rs:125`). A guest's table holds no other actor's type (`aethereal-actor/src/guest.rs:1632`), so `Table` gains `is_of(id, actor_type)`, which a guest's table asks its host for, as it does `lists` (`guest.rs:1622`). `Itself`, `Parent`, and `Child<T>` compare seats the turn already holds.
- **Mail that does not read is not handed to the guard.** It is answered as unread mail is (ADR-0017: `Untaken::Unread`), whoever sent it.

Whether a guest's own types, and the source the engine writes for its mail, can be relied on is decided separately; this decision reads them as the engine records them.

### A native send is checked at its call, against the receiver's guard

A native send is checked at its call (ADR-0059): its target takes the kind, an ask's asker handles the answer, a typed path's root is one its actor depends on, and, where the target names its receiver's type, the receiver's guard admits the sender's type.

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

- **`send` and `ask` on a target that names its receiver's type, `ActorRef<T>`, `Path<L>`, or a place beneath the sender, gain the bound `<T as Handles<K>>::From: Admits<T, Me, I>`**, its index inferred at the call, with a diagnostic written for it: "`Producer` may not send this mail: the receiver's guard admits only `ActorRef<Counter>`", followed by the receiver and kind the send names.
- **No guard author writes anything new.** `Admits` has one blanket impl, over the guard's `Sent` type, which every guard names already. The enum-guard derive writes `SentBy<Me, I>` for the enum (above). A guard of a handler's own, such as `ItsProcess`, is checked through its `Sent`, `ActorRef<Process>`, so the check reads the type `admits` answers and cannot drift from it.
- **`Speaks<P>` admits at the call a type that `Conforms` to `P`:** one with a handler of each of `P`'s kinds, answering each as a named protocol says, as an `ActorRef` is made a `ProtocolRef` (ADR-0016). Delivery reads the sender's listed kinds, as a cast does.
- **Still checked only at delivery:** a send through a `ProtocolRef`, which does not name its receiver's type; and the part of a guard that reads the seat or the mail, such as `Parent`'s seat, or whether the sender is the process `Spawned` holds. Delivery is the guarantee for every sender: a native actor, a send through a `ProtocolRef<P>`, an answer, and a guest.

### A refusal goes to the handler's `refused`

`refused` is handed the mail, read as its kind, and the plain turn. The sender is the turn's: `turn.from()` is its `ErasedActorRef`, so no wrapper restates it.

- **A refusal is the handler's to answer, beside the handler.** `refused` returns a `Refusal` of the handler's answer form: `Refusal::Answers(r)`, which the engine sends as it sends what the handler returns, or `Refusal::Logged`, the default below. An `#[actor]` block overrides `refused` for a kind with a `#[refused]` method of that kind, written beside its handler (ADR-0059). A refused request returns the request's own answer type, which the macro wraps in `Refusal::Answers`; a refused tell returns nothing. An override may tell whoever it reaches, each send checked at its call as any is, and the behaviour of one kind stays with its handler.
- **The default, `Refusal::Logged`,** logs a reason of its own, `Unreadable::Refused`, naming the sender's path and type, and answers a request `Unanswered::Unreadable`, as mail that did not read is answered (ADR-0017). A tell is dropped. The default is a case of its own because no answer form could stand for it: an answer given now, `A`, has no value the engine could make for every `A`.

### A kind is listed once

Dispatch takes the first listing whose role matches, so the declaration's constants already refuse a type that lists one kind twice (ADR-0013). There is no wrapper: a kind is listed as itself, and its one handler's guard decides who may send it. A kind several receivers take, such as `Exited`, which `Spawned` takes from `ItsProcess` and `Engines` from its spawns, is taken by each under its own guard.

## Migration

**Every mail handler states its guard**, as its turn's type argument, from which its `#[actor]` block writes `type From`; a turn that names none is `Anyone` (ADR-0059). Each handler of an answer, an ending, a stage, or a fallback is a `Receives<…>`, written from its `#[answer]`, `#[ended]`, `#[on]`, or `#[others]`. The check of an ask at its call names the asker's handler of the answer as a `Receives`.

**Each check by hand becomes a guard, and leaves the body.** A guarded handler's turn names its guard, `Turn<'_, Self, G>`, and the handler reads what the guard checked from `turn.from()` in place of comparing or casting `turn.sender()`. At 9204d99:

- **The parent.** `aethereal-link/src/engines/engine.rs:451`: `Engine` takes `Shut` from `ActorRef<Engines>`. `aethereal-link/src/engines/spawned.rs:229` and `:250`: `Spawned` takes `LinkedAs` and `Stop` from `ActorRef<Engines>`. `aethereal-window/src/window.rs:226`: `Window` takes `Event` from `ActorRef<Windows>`. Each is handed its parent as that `ActorRef`. Each parent is a singleton, so `Parent` would say the same.
- **What the host brings.** `aethereal-window/src/windows/mod.rs:448`: `Windows` takes `Heard` from `Itself`. `aethereal-chassis-browser/src/controls/mod.rs:138`: `Controls` takes `Control` from `Itself`. `aethereal-pace/src/pacer/mod.rs:233`: `Pacer` takes `Paced` from `Itself`. `aethereal-render/src/renderer/mod.rs:1083` and `:1165`: `Renderer` takes `Mapped` and `Surfaced` from `Itself`. Each is handed its own `ActorRef`.
- **A subscription.** `aethereal-window/src/window.rs:193` and `:280`: `Window` takes `Subscribe` from `Speaks<(Event,)>` and `SubscribeProbes` from `Speaks<(Probed,)>`. `aethereal-chassis-browser/src/controls/mod.rs:104`: `Controls` takes `Subscribe` from `Speaks<(Control,)>`. `aethereal-tools/src/tools/mod.rs:164`: `Tools` takes `Subscribe` from `Speaks<(Sheets,)>`. `aethereal-link/src/link/chat.rs:108`: `Link` takes `Listen` from `Speaks<Listening>`. `aethereal-render/src/renderer/watching/mod.rs:176` and `:205`: `Renderer` takes `SubscribeFrameStats` from `Speaks<(FrameStats,)>` and `SubscribeGpuTimes` from `Speaks<(GpuTimes,)>`. `aethereal-link/src/engines/hearing.rs`: `Engines` takes `HearTrusted` from `Speaks<Hearing>`. Each is handed the subscriber as the `ProtocolRef` it keeps, so the cast, and the log for a cast that missed, leave the body, and `Subscribers::subscribe` takes a turn of any guard. A screen's library takes `Dropped` from `Speaks<(DropAnswered,)>` (`aethereal-ui/src/lib.rs`, `screen!`), and answers its source through the reference its guard handed on.
- **The receiver's state.** `aethereal-link/src/engines/spawned.rs:207`: `Spawned` takes `Exited` from `ItsProcess`, handed its process as an `ActorRef<Process>`. `aethereal-link/src/engines/spawning.rs:125`, `:147`, and `:165`: `Engines` takes `Runs`, `Spawn`, and `Exited` from `ItsSpawn`, a spawn it keeps a record of, handed the spawn as an `ActorRef<Spawned>`. `aethereal-link/src/engines/mod.rs:371`: `Engines` takes `Shown` from `ItsEngine`, an engine it has linked, handed as an `ActorRef<Engine>`. `aethereal-window/src/window.rs:298` and `:323`: `Window` takes `Picked` and `KeysListed` from `Prober`, an actor subscribed to its probes, handed as a `ProtocolRef<(Probed,)>`; `aethereal-window` cannot name `Renderer`, which depends on it. `aethereal-actor/src/stream/streams.rs:116` and `:132`: a stream's opener takes `Part<S>` from the library's guard for parts, which reads the actor's `Streams<S, C>`, admits the receiver the part's stream was opened with, and hands it on as its `ProtocolRef<Streamed<S>>`.
- **The mail.** `aethereal-window/src/windows/mod.rs:425`: `Windows` takes `ShowCursor` from `ItsWindow`, the `Window` keyed by the mail's `window`, handed as an `ActorRef<Window>`. `aethereal-render/src/renderer/mod.rs:1063`: `Renderer` takes `Probed` from `ProbingWindow`, the actor at `path::<Window>` of the mail's `window`, handed as an `ActorRef<Window>`. `aethereal-sessions/src/sessions/chat/mod.rs:211`: `Sessions` takes `Chatted` from `JoinedAs`, the actor that joined as the mail's `engine`, handed as the `ProtocolRef<Showing>` it joined with; `aethereal-sessions` cannot name `Engine`, as `aethereal-link` depends on it.
- **No check today.** `aethereal-sessions/src/sessions/chat/mod.rs:173`: `Sessions` takes `Joined` from `Speaks<Showing>`, an actor it can show sessions to, handed as a `ProtocolRef<Showing>`.

A check that passed the mail over silently, or logged a warning of its own, now goes to the default `refused`.

**HTTP (ADR-0035).** A handler takes `Request<A, C>`, and an actor layer `Inbound<In>` and `Outbound`, from `FromStack`, `Speaks<Gathering>`, an actor whose type answers a request's body whole, which is what a stack is; each named as its turn's guard.

**Untouched.** `Engines` answers `TakeToken` (`aethereal-link/src/engines/mod.rs:462`) for any asker, and links only an engine of its own: that is what the token is for, not who may ask. The renderer keys what it makes by its maker (`renderer/mod.rs:1379`, `:1442`, `:1502`, `:1679`, `:1720`; `renderer/text/mod.rs:697`, `:742`), and casts its sender for what it will tell it (`CreateDispatch` at `:1567`, `DrawList` at `:1611`), each taking from anyone. An unsubscribe removes its own sender, and a store, a connection, or HTTP holds whoever asked.

## Consequences

- Whom a handler takes its kind from is in its signature, and the engine holds it for every sender.
- A handler is handed the sender its guard checked, typed as the guard found it, and never casts it; a handler that tells its senders apart matches over an enum guard's cases, exhaustively.
- A refused sender is logged, and a refused request answered, where a check by hand often did neither.
- Delivery checks every sender against the receiver's guard: a native one, a send through a protocol reference, an answer, and a guest. A native send whose target names its receiver's type is also checked against the guard's `Sent` type at its call, so a sender the guard refuses by type does not build.
- The types a guard admits are stated once, by its `Sent` type, which also takes them from the sender, and cannot disagree with `admits`; an enum of senders is derived from its cases, so its check and its types cannot drift.
- `Handles<K>` gains `From`, `Returns`, and a default `refused`, and is for mail; `Receives<H>` takes answers, endings, stages, and fallbacks. `Guard<A>` names a `Sent` and `Checks<A, K>` its check, `Admits` and `SentBy` read the `Sent` at a send's call, `Turn` takes its guard as its type argument after its actor, defaulted (ADR-0059), and `Table` gains `is_of`.
- A test that hands a guarded handler its mail from the outside (ADR-0036), whose type, `Outside`, handles nothing and is admitted at the call by `Anyone` alone, stands up the type the guard admits.

## Alternatives considered

- **The rule on the protocol,** `Carries<K>::Senders`. Protocols are structural: any type whose handlers cover a protocol's kinds conforms (`declared/protocols.rs:158`), so a sender reaches the same handler through another protocol, a tuple of kinds, or an `ActorRef`, and passes no rule a protocol states.
- **The rule on the kind,** `#[kind(senders = …)]`. A kind is data in the data layer and names no actor, and a kind several receivers take from different senders, `Shut`, `Stop`, `Event`, `Control`, would have to be split.
- **A `SentBy<S, K>` wrapper in the actor's list, with senders restating it** (`ToType<T, K, Only<S>>`). The rule is written in two places, and every sender's declaration changes when a receiver adds one.
- **A `Senders` trait whose forms each have a check of their own.** Every form is a type that implements `Guard`, checked by one call; a form the library lacks is an impl a handler writes, and an enum of forms is derived.
- **`Sent` on the keyed trait,** one `Guard<A, K>` holding both. The turn knows its actor and guard but not the kind, so to name `F::Sent` it would have to carry the mail's kind as a parameter too, on every guarded handler's signature, for a type that never depends on it.
- **`admits` answers `bool`.** The guard has already found which sender it is, and `bool` throws that away: the handler asks again and casts by hand, and no match over senders is exhaustive.
- **`TYPES` on each guard,** a constant beside `admits`. It states the rule a second time, and the two statements can disagree; a handler's own guard left it `Read` and went unchecked when built.
- **Tuples of guards,** `(G1, G2)` admitting what either does, handing back a positional enum (`OneOf2::First`) or a tuple of `Option`s. A reader landing on `Peer::Exchange(exchange)` sees who sent the mail; a positional case or an `(Option, Option)` hides it, cannot be matched exhaustively, and may fill more than one slot.
- **A hand-written `Sent` impl for an enum.** It restates the cases, and a case added without it drifts from the check; the derive reads them.
- **`From` on every `Handles` impl,** answers, endings, stages, and fallbacks included: 348 more lines that each say `Anyone`, of items no sender chooses.
- **The build check as a trait bound,** a bound on `ToType`'s `DeclaredBy<A>` that `<T as Handles<K>>::From` admits `A`. It holds for one type, but `Speaks<P>` and an enum guard need an index, the places of `P`'s kinds or the case whose type matches, which an impl cannot leave unconstrained. At the send, where `Allows` infers its index, `Toward` would have to carry the sender's type, and a send declared and never made would go unchecked.
- **The check at the send alone.** It sees no guest, no reply, and no send through a protocol reference, which is how an HTTP stack reaches its handler.
- **The check at delivery alone.** It holds, but a native sender learns only from a log at run time what its build could have told it.
- **Rust privacy on a kind's constructor.** A native sender must build a value to send it, but a guest writes bytes, whatever kind they claim.
