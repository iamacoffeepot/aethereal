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
    pub tie: Tie,
    pub kind: KindId,
    pub bytes: Vec<u8>,
}

pub enum Tie {
    Tell,                          // nothing is owed for it
    Ask,                           // an answer is owed
    Part { request: MailId },      // one part of a stream that request opened
    Answer { request: MailId },    // the answer to that request
}
```

- **The cause** is the mail whose turn sent this one.
- **The root** is the first mail of the chain. Mail sent while a piece of mail is handled takes that mail's root. Mail with no cause is its own root.
- A chain is every piece of mail with one root.

**A chain begins where something from outside the engine comes in.** What a resource brings in reaches its owner as mail from its owner (ADR-0010). That mail has no cause, and it is the root of whatever follows from it.

**Mail sent from a `begin` belongs to the turn that stood the actor up.** A child's `begin` runs inside its parent's turn (ADR-0009), so its mail has the cause and the root of the mail the parent was handling.

**An ending belongs to the chain of the mail that brought it about.** When an actor closes or fails while handling a piece of mail, the mail that tells its parent and its watchers has that piece of mail as its cause, and its root. So does mail sent from its closing turn (ADR-0009's `end`). A child that closes because its parent told it to closes while handling that mail, so its ending is in the same chain. A chain has therefore not finished until the endings it led to have been handled.

**A root's `begin` starts a chain of its own.** Nothing caused it and it is handling no mail. The mail it sends has no cause, and its root is the id made of that root actor and a count of zero, which no piece of sent mail has.

**Asking and answering are effects of their own, beside `Send`.**

```rust
pub enum What {
    // ... ADR-0009's effects, and:
    Ask { to: To, kind: KindId, bytes: Vec<u8> },
    Answer { request: MailId, kind: KindId, bytes: Vec<u8> },
}

pub trait Request: Kind {
    type Answer: Kind;
}
```

- An `Ask` is mail that is owed an answer. A `Send` is mail that is owed nothing.
- **An answer names its request by the request's id.** The id is already unique to the asker, so the tie needs no number of its own. An `Answer` is delivered to the request's sender.
- A kind that is asked says what answers it: `Request::Answer`.

**The asker keeps a context for each request it has out.** `turn.ask(to, &request, context)` stores the context in the asker's own state under the request's count, and the handler of the answer is given it. A context is a kind, so it is kept with the rest of the actor's state. It is never dropped while its request is unanswered.

**A request is answered once.** The handler of a request returns its answer, and the engine adds the `Answer`. Or it keeps a hold and answers in a later turn.

**A hold is a promise to answer.**

```rust
pub struct Held<A: Kind> { .. }      // I owe this answer
```

- A hold is typed by the answer it owes. It is kept in the actor's state, crosses `Within` and no other boundary (ADR-0004), and is neither `Clone` nor `Copy`.
- `turn.answer(held, &answer)` takes the hold by value, so a request cannot be answered twice. That is checked when the program is built.

**A stream is a request that is sent parts before its answer.**

```rust
pub trait Stream: Request {
    type Part: Kind;
}

// beside `Ask` and `Answer`:
Part { request: MailId, kind: KindId, bytes: Vec<u8> }
```

- The asker opens a stream with an `Ask`, and the actor that takes it keeps the hold.
- While it holds, it may send any number of parts. `turn.part(&held, &part)` borrows the hold, and a part names its request as an answer does.
- **The answer says the stream is over.** It may carry a last value, such as a count or a reason. A request is answered once, so a stream ends once.
- Parts and the answer are mail from one actor to one actor, so the parts arrive in the order they were sent and the answer after them (ADR-0009).
- The asker's context lasts as long as the stream. The handler of a part is lent it, and the handler of the answer is given it.
- A kind that opens a stream says what its parts are: `Stream::Part`. An actor that asks for one has a handler for its parts and for its answer.

**The engine answers in an actor's place** when a request could not be delivered, and when an actor ends still owing an answer (ADR-0009). The answer given in its place says which of the two happened. So an asker always gets one answer, and its context is always given back to it. A stream whose producer ends is told it is over the same way.

**The program is checked for both ends when it is built.** A handler of a request returns `Request::Answer` or a hold of it. An actor that asks has a handler for that answer.

**Mail that is owed nothing and is not delivered ends there.** Its sender learns nothing of it. An actor that must know another has ended watches it (ADR-0009).

## Consequences

- Whether a chain has finished is counted by root: mail sent, less mail handled (ADR-0011).
- What happened can be followed back from any piece of mail by its causes, to the resource that began it.
- Every piece of mail carries three ids, of sixteen bytes each.
- Every chain but a root's first begins at a resource, so the engine can say for any piece of mail what from outside set it off.
- An actor's requests out, and the holds it owes, are part of its state: they are shown with it and carried onto new code with it.
- A hold that is lost, by overwriting the state that held it, is answered only when its actor ends.
- **Not decided here:** how a live asker stops a stream early; how a producer that is too fast is slowed; the chain of an ending that nothing inside the engine brought about, such as the host shutting down; large payloads; and whether the cause is kept on every piece of mail or only in the record of a run.

## Alternatives considered

- **An id from one count the engine keeps.** One number, eight bytes. Every send by every worker takes from the same count, and which mail gets which id then depends on scheduling.
- **The root alone, with no cause.** Sixteen bytes less on each piece of mail. The order within a chain is then read from the record of a run and not from the mail.
- **No chain on mail, and chains tracked beside it.** Mail stays small. A second structure then has to be kept in step with every send, which is what a view over effects replaces.
