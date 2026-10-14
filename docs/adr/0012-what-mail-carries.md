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
    pub cause: Option<MailId>,   // none: nothing inside the engine caused it, and nothing ever will have
    pub root: MailId,
    pub kind: KindId,
    pub bytes: Vec<u8>,
}
```

- **The cause** is the mail whose turn sent this one.
- **The root** is the first mail of the chain. Mail sent while a piece of mail is handled takes that mail's root. Mail with no cause is its own root.
- A chain is every piece of mail with one root.

**A chain begins where something from outside the engine comes in.** What a resource brings in reaches its owner as mail from its owner (ADR-0010). That mail has no cause, and it is the root of whatever follows from it.

**Mail sent from a `begin` belongs to the turn that stood the actor up.** A child's `begin` runs inside its parent's turn (ADR-0009), so its mail has the cause and the root of the mail the parent was handling.

**An ending belongs to the chain of the mail that brought it about.** When an actor closes or fails while handling a piece of mail, the mail sent from its closing turn (ADR-0009's `end`) has that piece of mail as its cause, and its root. A child that closes because its parent told it to closes while handling that mail, so its ending is in the same chain. A chain has therefore not finished until the closing turns it led to have run. An ending makes no mail for the actors that watch it (ADR-0009): a watcher reads the ending in a turn of its own, and what it sends there belongs to the chain of the mail that turn handles.

**A root's `begin` starts a chain of its own.** Nothing caused it and it is handling no mail. The mail it sends has no cause, and its root is the id made of that root actor and a count of zero, which no piece of sent mail has.

**Asking and answering are not the engine's.** Mail is sent, untyped, and nothing is owed for it. A request and its answer, the context an asker keeps, a promise to answer, and a stream are a layer written over mail, and that layer is a later ADR's. The engine holds none of it.

**Mail that is not delivered ends there.** Its sender learns nothing of it. An actor that must know another has ended watches it (ADR-0009).

## Consequences

- Whether a chain has finished is counted by root: mail sent, less mail handled (ADR-0011).
- What happened can be followed back from any piece of mail by its causes, to the resource that began it.
- Every piece of mail carries three ids, of sixteen bytes each.
- Every chain but a root's first begins at a resource, so the engine can say for any piece of mail what from outside set it off.
- **Not decided here:** how a request and its answer are written over mail; the chain of an ending that nothing inside the engine brought about, such as the host shutting down; large payloads; and whether the cause is kept on every piece of mail or only in the record of a run.

## Alternatives considered

- **An id from one count the engine keeps.** One number, eight bytes. Every send by every worker takes from the same count, and which mail gets which id then depends on scheduling.
- **The root alone, with no cause.** Sixteen bytes less on each piece of mail. The order within a chain is then read from the record of a run and not from the mail.
- **No chain on mail, and chains tracked beside it.** Mail stays small. A second structure then has to be kept in step with every send, which is what a view over effects replaces.
