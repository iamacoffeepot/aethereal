# ADR-0012: What a piece of mail carries

- **Status:** Proposed
- **Date:** 2026-10-13

## Context

ADR-0009 decided that a piece of mail says which actor it is for and who sent it, beside its kind and its bytes, and left the rest to this ADR.

Two things already decided need more than that. A step puts the mail that reaches an actor in a fixed order, by its sender and then in the order that sender sent it (ADR-0011), so mail has to say where it stands in its sender's order. And whether everything one piece of mail set in motion has finished is a view (ADR-0011), so mail has to say which chain it belongs to.

## Decision

**A piece of mail has an id: its sender, and that sender's count of what it has sent.**

```rust
pub struct MailId {
    pub sender: ActorId,
    pub count: u64,          // the sender's own count, from one
}
```

- Each actor counts only its own mail, so no count is shared between actors.
- Two runs of the same turns give every piece of mail the same id, whichever worker ran which turn.
- The id is the fixed order ADR-0011 asks for: mail that reaches an actor in one step is ordered by its id.

**A piece of mail carries its cause and its root.** The engine carries mail as an `Envelope`. A handler sees ADR-0009's `Mail`, which is the envelope's kind, sender, and bytes to read.

```rust
pub struct Envelope {
    pub id: MailId,
    pub to: ActorId,             // the actor it is for
    pub cause: Option<MailId>,   // none for mail from outside the engine, or from a root's `begin`
    pub root: MailId,
    pub depth: u32,              // how far down its chain: one more than its cause's
    pub kind: KindId,
    pub bytes: Vec<u8>,
}
```

- **The cause** is the mail whose turn sent this one.
- **The root** is the first mail of the chain. Mail sent while a piece of mail is handled takes that mail's root. Mail with no cause is its own root.
- A chain is every piece of mail with one root.
- **The depth** is how far down its chain a piece of mail is. Mail sent while a piece of mail is handled is one deeper than it. Mail with no cause is at depth zero, and the mail a root's `begin` sends is at depth one.

**A turn's effects are emitted in a context.** The context is the item the turn handled: its cause, and its root. Every effect of one turn shares it, and the engine folds each effect into the views with it (ADR-0011). A piece of mail's id is on its `Send`, the sender and that sender's count, since the turn counts what its actor sends. The rest of the envelope, its cause and its root, comes from the context, and who it is for comes from the `Send`'s target, with `To::Sender` already put in place by the turn.

**A chain begins where something from outside the engine comes in.** What a resource brings in reaches its owner as mail from its owner (ADR-0010). That mail has no cause, and it is the root of whatever follows from it.

**Mail sent from a `begin` belongs to the turn that stood the actor up.** A child's `begin` runs inside its parent's turn (ADR-0009), so its mail has the cause and the root of the mail the parent was handling.

**An ending belongs to the chain of the mail that brought it about.** When an actor closes or fails while handling a piece of mail, the mail sent from its closing turn (ADR-0009's `end`) has that piece of mail as its cause, and its root. A child that closes because its parent told it to closes while handling that mail, so its ending is in the same chain. A chain has therefore not finished until the closing turns it led to have run. An ending makes no mail for the actors that watch it (ADR-0009): a watcher reacts to the ending in a turn of its own, and what it sends there belongs to the chain of the mail that brought the ending about.

**A root's `begin` starts a chain of its own.** Nothing caused it and it is handling no mail. The mail it sends has no cause, and its root is the id made of that root actor and a count of zero, which no piece of sent mail has.

**What an actor keeps about the mail it sent is its own, kept under a chain's root and a depth.** An actor that sends mail and wants to know later what it sent it for keeps that in a field of its own, under the root of the chain and the depth of the mail it sent. A handler reads both from its mail: `mail.root()` and `mail.depth()`.

- Mail that comes back further down that chain has the same root and a greater depth. The actor finds what it kept under that root at a lesser depth, and the difference is how far back it was kept: one for mail sent in direct reply, more for mail that came by way of other actors.
- An actor that keeps a second thing further down the same chain finds both, the nearer first.
- Two pieces of mail sent from one turn are in the same chain at the same depth. What was kept for each is found by mail that follows from either, and the actor tells them apart by what the mail is and who sent it.
- This is not an effect, and the engine keeps nothing for it. What an actor keeps is its own, it is not sent, and no other actor sees it. The engine adds the one number to the mail.
- What an actor kept stays until the actor takes it out. Nothing tells an actor that a chain has finished.

**Asking and answering are not the engine's.** Mail is sent, untyped, and nothing is owed for it. A request and its answer, a promise to answer, and a stream are a layer written over mail and over what an actor keeps, and that layer is a later ADR's. The engine holds none of it.

**Mail that is not delivered ends there.** Its sender learns nothing of it. An actor that must know another has ended watches it (ADR-0009).

## Consequences

- Whether a chain has finished is counted by root: mail sent, less mail handled (ADR-0011).
- What happened can be followed back from any piece of mail by its causes, to the resource that began it.
- Every piece of mail carries three ids, of sixteen bytes each, and its depth.
- Every chain but a root's first begins at a resource, so the engine can say for any piece of mail what from outside set it off.
- **Not decided here:** how a request and its answer are written over mail; how a `begin` and a closing turn, which handle no mail, read the chain and depth they send at; the chain of an ending that nothing inside the engine brought about, such as the host shutting down; large payloads; and whether the cause is kept on every piece of mail or only in the record of a run.

## Alternatives considered

- **An id from one count the engine keeps.** One number, eight bytes. Every send by every worker takes from the same count, and which mail gets which id then depends on scheduling.
- **The root alone, with no cause.** Sixteen bytes less on each piece of mail. The order within a chain is then read from the record of a run and not from the mail.
- **No chain on mail, and chains tracked beside it.** Mail stays small. A second structure then has to be kept in step with every send, which is what a view over effects replaces.
