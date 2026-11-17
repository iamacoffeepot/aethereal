# ADR-0058: A handler takes a kind only from the senders it names

- **Status:** Proposed
- **Date:** 2026-11-17

## Context

A handler knows the sender of what it handles only as an `ErasedActorRef` (`Turn::sender`). Its type cannot be read from it, and a cast proves only which kinds the sender's type lists (ADR-0016: `Turn::cast`). So a handler whose trust rests on who sent the mail, and not on what the mail says, checks the sender by hand. The code holds about seventeen such checks, in three shapes:

- **Mail the host brings.** The engine records the actor itself as the sender of what a source brings it (ADR-0010), of a job's answer (ADR-0018), of a sleep that came due (ADR-0041), and of a lifecycle stage (ADR-0020): each is mailed `by` and `to` the same actor (`Mailed`). An actor that acts on what its host says compares the sender with `turn.me()`, as `Pacer` does for `Paced`, and passes over the same kind from anyone else.
- **Mail from one other type.** `Spawned` takes `Stop` only from its parent, `Engines`, and `Window` takes `Event` only from its root, `Windows`, each comparing the sender with `turn.parent()`.
- **A subscription.** `Window`, `Tools`, `Controls`, `Link`, and `Renderer` take a subscribe from anyone, cast its sender to the protocol they will send it, and log and pass over one whose type does not list it.

Each is a few lines that the handler's signature does not show. A reader learns what a handler trusts from its body, a sender that is passed over is told nothing, and the next handler of the same shape is written again by hand. HTTP needs the same check with more at stake (ADR-0035): a handler believes the principal a request comes with only because an exchange sent it, and an authenticator must not be tried with credentials, one after another, by any actor that can name it.

Rust's privacy cannot carry the check. A kind whose constructor only one crate has can be built only there by native code, but a guest writes bytes (ADR-0015), whatever kind they claim.

What the engine already has:

- **Each item has a source the engine writes** (ADR-0009: `Item`; `Turning::source`). No sender writes it, a guest's host included.
- **Each actor is seated with its type's declaration** (ADR-0013: `Declared`, `Table::declared`), so where mail is delivered the engine can read the sender's type. A guest's table asks its host what it cannot read itself, as a cast does (`Table::lists`).
- **Dispatch takes the first listing whose role matches the item** (`HandledList::dispatch`), so an actor type lists each kind once.

## Decision

**A handled kind may name the senders it is taken from. `SentBy<S, K>` is mail of the kind `K`, taken only from an actor that `S` accepts.**

```rust
pub struct SentBy<S: Senders, K: Kind>(PhantomData<fn() -> (S, K)>);

pub struct Delivered<S: Senders, K> {
    pub sender: S::Sent,                                    // the sender, proven to be one `S` accepts
    pub mail: K,
}

impl Actor for Pacer {
    type Handles = (SentBy<ActorRef<Pacer>, Paced>, /* .. */);
    // ..
}

impl Handles<SentBy<ActorRef<Pacer>, Paced>> for Pacer {
    type Sends = ();

    fn handle(&mut self, delivered: Delivered<ActorRef<Pacer>, Paced>, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let Paced::Cycled { elapsed_nanos } = delivered.mail;
        // ..
    }
}
```

- **It has `K`'s role.** The engine reads it as `Role::Kind` of `K`'s id, so its type lists `K`: a protocol that holds `K`, a cast, and the answers `K` is replied with see a handler of `K` (ADR-0016), and mail of `K` is handed to it, read as `K`, with its sender.
- **A kind is listed once: as itself, or as one `SentBy`.** A second listing would never run, as dispatch takes the first, so the declaration's constants refuse a type that lists one kind twice, which stops the build (ADR-0013). A kind taken from two types is taken through one `Senders` that accepts both.

### `Senders` says who is accepted, and what the handler is handed of the sender

```rust
pub trait Senders: 'static {
    /// What the handler is handed of a sender it accepts.
    type Sent;

    /// The sender `sender`, whose type `of` reads, as its handler is handed it: none when its type is not accepted.
    fn sent(sender: Seated, of: &SenderType<'_>) -> Option<Self::Sent>;
}

/// The type of an item's sender, as the receiver's table reads it.
pub struct SenderType<'t> { .. }

impl SenderType<'_> {
    pub fn is(&self, actor_type: KindId) -> bool;           // it is the type of id `actor_type`
    pub fn lists(&self, kinds: &[KindId]) -> bool;          // it lists every one of `kinds`, as a cast reads it
}
```

It has four forms:

| `S` | Accepts | The handler is handed |
|---|---|---|
| `ActorRef<T>` | an actor of type `T` | `ActorRef<T>` |
| `(A, B, …)` | an actor of any of the types, each alike | `Among<(A, B, …)>` |
| an enum that derives `Senders` | an actor of the type one of its variants holds | the enum, its variant the sender's type |
| `Speaks<P>` | an actor whose type lists every kind of `P`, a tuple of kinds or a named protocol | `ProtocolRef<P>` |

- **`ActorRef<T>` accepts one type.** It reads `of.is` with `T`'s id, and hands over the sender as an `ActorRef<T>`, proven as a spawn proves one.
- **A tuple accepts several types handled alike,** with no telling them apart: what a handler that answers whoever sent does with it. It is handed `Among<(A, B)>`, a reference proven to be of one of them, which erases and compares as any reference does. The tuples run to the width `Kinds` tuples do, written by the same macro.
- **An enum accepts several types handled differently.** Each variant holds one `ActorRef<T>`, and the handler matches on it. The derive refuses a variant that holds anything else, and two variants of one type. For

  ```rust
  #[derive(Senders)]
  pub enum Peer {
      Exchange(ActorRef<Exchange>),
      Client(ActorRef<Client>),
  }
  ```

  it writes

  ```rust
  impl Senders for Peer {
      type Sent = Self;

      fn sent(sender: Seated, of: &SenderType<'_>) -> Option<Self> {
          if let Some(exchange) = <ActorRef<Exchange> as Senders>::sent(sender, of) {
              return Some(Self::Exchange(exchange));
          }

          <ActorRef<Client> as Senders>::sent(sender, of).map(Self::Client)
      }
  }

  impl Accepts<Exchange, Here> for Peer {}
  impl Accepts<Client, Here> for Peer {}
  ```

- **`Speaks<P>` accepts whatever takes a protocol.** It reads `of.lists` with `P`'s kinds, the check a cast makes, and hands over a `ProtocolRef<P>`. It is the form of a subscription: a publisher that will send `Event` to whoever subscribes takes `SentBy<Speaks<(Event,)>, Subscribe>`, and is handed the subscriber already cast.

### Checked twice: when a native sender is built, and when the mail is delivered

**Delivery is the guarantee.** Before dispatch hands the mail to the handler, `S::sent` reads the type of the item's source, which the engine wrote. So the check holds for every sender: a native actor, a guest whose host wrote its mail, the outside (ADR-0036), a send through a `ProtocolRef`, and a reply, none of which a type check sees in full. `Handled::take` is handed the sender's type beside the item, and every other listing passes it over. `Table` gains `is_of(id, actor_type)`, read from `declared` natively and asked of the host by a guest's table, as `lists` is.

**A native send is checked when it is built.** A send of `K` to a type that takes it only from `S` builds only from an actor whose type `S` accepts.

- **The sender's marker names the gate.** `ToType<T, K>`, `Tells<Path<L>, K>`, and `Asks<Path<L>, K, C>` gain a last parameter, how the receiver lists `K`, which is `Anyone` unless written. To a receiver that lists `SentBy<S, K>`, a sender declares `ToType<T, K, Only<S>>`, whose `DeclaredBy` reads the receiver's handler as `T: Handles<SentBy<S, K>>`, for the reply check it makes today (ADR-0016). A plain `ToType<T, K>` still needs `T: Handles<K>`, so a declaration that leaves the gate out does not build. The gate is written on both sides on purpose: from `T` and `K` alone Rust cannot find how `T` lists `K`, since an impl for the plain listing and one for `SentBy` overlap, so the sender names it, and its reader sees the gate it passes.
- **The send reads the sender's type.** A turn is typed by its actor (`Turn<'_, Me, S>`), and the target its `to` makes carries `Me`. Its `send` and `ask` bound the marker by `Admits<Me, J>`: always for `Anyone`, and for `Only<S>` when `S: Accepts<Me, J>`, `J` inferred as a send's place in its list is (`Allows<M, I>`). `ActorRef<T>` accepts `T`; a tuple each of its types, at its place; a derived enum each type its variants hold; and `Speaks<P>` a type that lists each kind of `P`, as itself or behind a gate. An index cannot be inferred in an impl, so this check is at the send, not the declaration. `Reaches<K>` gains the gate the same way, `Reaches<K, G>`, `G` inferred from the marker the sends list.
- **What the compiler cannot see, delivery catches:** a send through a `ProtocolRef<P>`, which names no receiver type and declares `ToAny<P, K>` with no gate; a reply, `ToSender<K>`; and a guest.

### A refusal goes to the handler's `refused`

```rust
pub trait Handles<H: Handled>: Actor {
    type Sends: Sends + DeclaredBy<Self>;

    fn handle(&mut self, item: H::Item<'_>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault>;

    /// Mail of the handled kind from a sender its listing does not accept: logged, naming the sender, and a request
    /// answered `Unanswered::Unreadable`; other mail is dropped.
    fn refused(&mut self, refused: Refused<H::Withheld>, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        turn.refuse(&refused)
    }
}

pub struct Refused<K> {
    pub sender: ErasedActorRef,                             // who sent it: a type its handler does not accept
    pub mail: K,                                            // what it sent, read as its kind, and not to be believed
}
```

- **A refusal is the handler's to answer, beside the handler.** It shares the handler's `Sends`, so an override may answer a request with a kind of its own, or tell whoever the handler may, and the behaviour of one listing stays in one impl. The default does what mail that did not read gets (ADR-0017: `Untaken::Unread`), with a reason of its own, `Unreadable::Refused`, and its log line names the sender's path and type. A tell or an event is dropped.
- **Only a gated listing is ever refused.** `Handled::Withheld` is `K` for `SentBy<S, K>`, and for a plain kind, `Ended`, and `Others` a type with no value, so their `refused` cannot be called.
- **The refused mail is untrusted.** It read as `K`, but its sender is not one the handler accepts, so nothing in it holds what a delivery would: a principal, a window's event, a cycle's timing. An override may log it, count it, or answer it, and never acts on it as if it had been accepted.

### The host's mail is sent by the actor itself

What a source brings, a job's answer, a sleep come due, and a lifecycle stage are mailed by the engine with the actor as both sender and receiver. So `SentBy<ActorRef<Self>, K>` takes only what the host brought. **That is exact for a singleton**, a root (`ChildOf<Parent = Top>`) whose `Key` is `()`, since no other actor of its type stands. **It is wrong for a keyed type:** every actor of the type is accepted, so a sibling could send it what only its host should. A keyed type that acts on what its host brings keeps comparing the sender with `turn.me()`, behind the gate.

### A type names what sent it, not which actor

`SentBy` proves the sender's type. A handler that takes a kind only from one actor of a type, its own process or one of its spawns, keeps that comparison, now behind the gate, which proves the type before the handler runs. A check against what the mail names, such as whether a resource it names is the sender's, is not a sender check, and stays as it is.

### A type's id is the engine's to give

The check compares type ids, and a kind's id is its name and its layout (ADR-0006). A guest declares the types its blob makes (ADR-0015), and the engine keys them by id (`Instances::is_own`, `BlobType::is`). A guest that declared a type of a linked type's name and layout would have that type's id, and pass every `SentBy` that names it. **A blob that declares a type with a linked type's id is refused when it is taken in.** The change that builds `SentBy` checks whether the engine refuses it today, and adds the refusal where it does not.

## Migration

Each guard on `main` (9204d99) whose trust rests on its sender, and what it becomes. A guard that passed the mail over silently, or logged a warning of its own, now goes to the default `refused`: it logs, and a request among them is answered `Unanswered::Unreadable` where it was answered `Unreplied`.

**Replaced by `SentBy`, the guard gone.** Each sender is a singleton, so the gate is the whole check.

- `aethereal-link/src/engines/engine.rs:451`: `Engine` takes `Shut` from `Engines`, its parent.
- `aethereal-link/src/engines/spawned.rs:229` and `:250`: `Spawned` takes `LinkedAs` and `Stop` from `Engines`, its parent.
- `aethereal-window/src/window.rs:225`: `Window` takes `Event` from `Windows`, its root.
- `aethereal-window/src/windows/mod.rs:447`: `Windows` takes `Heard` from itself, as the host brings it.
- `aethereal-chassis-browser/src/controls/mod.rs:137`: `Controls` takes `Control` from itself.
- `aethereal-pace/src/pacer/mod.rs:232`: `Pacer` takes `Paced` from itself.
- `aethereal-render/src/renderer/mod.rs:1083` and `:1165`: `Renderer` takes `Mapped` and `Surfaced` from itself.

**Replaced by `Speaks<P>`, the cast gone.**

- `aethereal-window/src/window.rs:193`: `Window` takes `Subscribe` from `Speaks<(Event,)>`.
- `aethereal-window/src/window.rs:280`: `Window` takes `SubscribeProbes` from `Speaks<(Probed,)>`.
- `aethereal-chassis-browser/src/controls/mod.rs:104`: `Controls` takes `Subscribe` from `Speaks<(Control,)>`.
- `aethereal-tools/src/tools/mod.rs:164`: `Tools` takes `Subscribe` from `Speaks<(Sheets,)>`.
- `aethereal-link/src/link/chat.rs:108`: `Link` takes `Listen` from `Speaks<Listening>`.
- `aethereal-render/src/renderer/watching/mod.rs:176` and `:205`: `Renderer` takes `SubscribeFrameStats` and `SubscribeGpuTimes` from `Speaks<(FrameStats,)>` and `Speaks<(GpuTimes,)>`.

**A gate in front of an identity check that stays.**

- `aethereal-link/src/engines/spawned.rs:207`: `Spawned` takes `Exited` from `ActorRef<Process>`, and checks it is its own process.
- `aethereal-link/src/engines/spawning.rs:125`, `:146`, and `:165`: `Engines` takes `Runs`, `Spawn`, and `Exited` from `ActorRef<Spawned>`, and finds which of its spawns sent it.
- `aethereal-link/src/engines/mod.rs:370`: `Engines` takes `Shown` from `ActorRef<Engine>`, and finds which of its engines sent it.
- `aethereal-window/src/windows/mod.rs:425`: `Windows` takes `ShowCursor` from `ActorRef<Window>`, and checks it is the named window's.
- `aethereal-render/src/renderer/mod.rs:1061`: `Renderer` takes `Probed` from `ActorRef<Window>`, and checks it is the named window's.
- `aethereal-window/src/window.rs:298` and `:323`: `Window` takes `Picked` and `KeysListed` from `Speaks<(Probed,)>`, and keeps checking the sender subscribed to its probes. `aethereal-window` cannot name `Renderer`, which depends on it.
- `aethereal-sessions/src/sessions/chat/mod.rs:209`: `Sessions` takes `Chatted` from `Speaks<Showing>`, and keeps checking the sender is the engine that joined. `aethereal-sessions` cannot name `Engine`, as `aethereal-link` depends on it.

**Untouched.** The renderer's checks that a resource named in mail is the sender's own, and the stream library's checks that a part comes from the stream its receiver opened, compare the sender with what the mail names, not with a type. So does `Renderer`'s `CreateDispatch` (`renderer/mod.rs:1567`), which casts its sender only for a dispatch run once, to tell it when it has run, and takes one run until released from anyone.

## Consequences

- A handler's trust is in its signature. `SentBy<ActorRef<Engines>, Shut>` says what a body's comparison said, and the engine enforces it for every sender.
- A refused sender is logged, and a refused request answered, where a hand-written guard often did neither.
- A native sender that is not accepted does not build, and one whose receiver adds a gate stops building until it declares `Only<S>`. A send through a protocol reference is checked only at delivery.
- `Handles` gains a default method; `Handled` an associated type, and the sender's type in `take`; `Table` `is_of`; and `ToType`, `Tells`, `Asks`, and `Reaches` a defaulted parameter. A target made by a turn's `to` carries the turn's actor type. Every handler written today builds unchanged.
- Delivery of a gated kind reads the sender's type once, a lookup in the table a cast already makes; a plain kind costs nothing more.
- A test cannot hand a gated handler its mail from the outside (ADR-0036), whose type is its own: it stands up the type the handler accepts, or the real sender.
- `SentBy<ActorRef<Self>, K>` is the host's mail only for a singleton. A keyed type's brought mail keeps its identity check.
- **Not decided here:** a sender of its own for what the host brings, which would make the keyed case exact; and a gate on an ending (`Ended`), whose source the engine already names.

## Alternatives considered

- **Rust privacy on a kind's constructor, alone.** It holds against native code, which must build a value to send it, and not against a guest or any sender that writes bytes.
- **A policy type parameter, `SentBy<S, K, R>`,** naming what a refusal does. It splits one listing's behaviour between its handler and a type elsewhere; a default method keeps it in the handler's impl.
- **A refusal decided by the answer kind,** each kind answering `From<Refused>`. It decides nothing for a tell, and puts a receiver's policy on a kind its senders share.
- **Positional `OneOf2` demux, or `narrow::<T>()` on a tuple's sender.** The cases are unnamed, or a match on them is not exhaustive; an enum names each, and its match is checked.
- **The check at the send alone.** It sees no guest, no reply, and no send through a protocol reference, which is how HTTP's exchange reaches its handler.
- **The check at delivery alone.** It holds, but a native sender learns only from a log at run time what its build could have told it.
