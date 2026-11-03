# ADR-0041: Requests carry a deadline

- **Status:** Proposed
- **Date:** 2026-11-02

## Context

ADR-0017 left a request's deadline to its asker: "the engine sets no deadline", and an asker that stops waiting passes over the answer when it comes. That holds for an asker. It leaves every actor in between holding forever.

A tool called through the hub is held at four hops: HTTP's exchange asks `Mcp`, which asks `EngineTools`, which asks `Engines`, whose `Engine:k` writes a `Call` frame to an engine, whose `Link` asks the tool (ADR-0035, ADR-0037, ADR-0038). Each hop keeps the request in a bounded table until the hop after it answers (`Mcp`'s `waiting`, `EngineTools`' `calls`, `Engine:k`'s `calls`, `Link`'s `calls`). The exchange has a deadline of its own, `answer_within_millis`, run as a `Sleep` (`exchange::Deadline`), and answers the client `504` when it passes. Nothing tells the hops behind it. A tool that stays alive and never answers, or an engine that hangs, keeps one slot at every hop for every call, and each table refuses every call once it is full (#355).

A `Sleep` in each hop would free its slot, with a setting in each, and the next actor that holds a request it forwards would need the same. The leak is in the shape, not in one actor: whatever holds a request while it asks another keeps it until that other answers.

## Decision

**A request carries the time its asker stops waiting, and the engine answers it `Expired` then.**

```rust
// aethereal-actor
pub struct Asking { pub count: u64, pub answer: KindId, pub context: Written, pub lent: Vec<BlobRef>, pub due: Due }

/// When a request stops being waited for.
pub enum Due {
    /// Never: it waits until it is answered, as every request did before this record.
    Never,
    /// At this time on the engine's clock.
    At(Time),
}

pub enum Unanswered { Ended, Failed, Unreplied, Faulted, Expired }

impl Held<A> { pub fn due(&self) -> Due; }                        // the request's deadline, for the holder that needs it

turn.to(target).within(due).ask(&kind, &context)?;               // a deadline stated
turn.to(target).ask(&kind, &context)?;                           // the deadline inherited, or `Never`
```

- **At its deadline, the engine answers the asker `Answered::No(Unanswered::Expired)`** and the request is no longer filed under its count: its context is handed to the asker's handler with `Expired`, and goes after that turn, as with every other reason (ADR-0014). An answer that comes later finds no request filed, and is dropped, as an answer to an ended asker is. A request is still answered exactly once. The notice comes from the asker to itself, first of a chain of its own, as a job's answer does: the engine keeps no receiver or chain beside a request, and expiry needs neither.
- **An ask made in a turn that handles a request inherits that request's `Due`,** unless it states its own with `within`. So one deadline, set where a request enters the engine, bounds every request made on its behalf, at every hop, with no code in the hops. A turn that handles a request it held earlier states it, `within(held.due())`. An ask from a `begin`, or from a turn that handles mail that is not a request, is due `Never` unless it states one.
- **Jobs carry no deadline.** A job is cancelled by its asker (ADR-0018: `turn.cancel`), and one run in a turn that handles a request does not inherit its `Due`.
- **The engine wakes for the earliest deadline**, as it wakes for a `Sleep` (ADR-0010: `Engine::with_wake`), and expires what is due before the step's turns run. A request due `Never` costs nothing more than it did.
- **A holder is not told that what it holds has expired.** Its own asks made for it inherited the same `Due`, so they expire at the same moment, and each comes back to it as `Answered::No`, which its handler already takes. It frees its slot there and answers what it holds, which the engine drops. Only a holder that waits on something other than an ask needs the deadline itself, through `Held::due`.
- **A guest's ask record carries its `Due`,** and the guest library inherits, as native code does: its inbox hands a guest each item's deadline, which `Held::due` needs too, and its host files the deadline the record carries (ADR-0015).

**HTTP sets the deadline.** The exchange asks its handler `within` `answer_within_millis` of when the request is whole, as its answering phase was measured before, and writes `504` when its ask is answered `Expired`. Its own `Sleep` for the answering phase goes; the deadlines of its other phases stay (ADR-0035).

**Across the link, a call carries the time it has left.** Two engines' clocks are their own, so a deadline crosses as a span.

```rust
pub enum Within { Unbounded, Millis(u64) }                       // aethereal-link
Down::Call { call: u64, revision: u64, tool: String, input: Vec<u8>, within: Within }
```

- **`Engine:k` writes the time left before its held call's `Due`,** and runs one `Sleep` to that `Due`, which frees the call's slot when it fires. This is the one hop that waits on a frame, not an ask.
- **`Link` asks the tool `within` that span of the frame's arrival,** so a tool that never answers frees the engine's slot too, and its call is answered refused.
- `Down::Call` changes in a way an older engine would misread, and the store keeps builds from before the change, so `REVISION` is `2` (ADR-0040).

**`Mcp`, `EngineTools`, and `Engines` change nothing.** Each asks in the turn that handles the request it holds, so its ask inherits the deadline, and its existing `Answered::No` arm frees the slot.

## Consequences

- **One setting bounds a call end to end:** HTTP's `answer_within_millis`. Every hop's slot is freed at the same moment the client is answered `504`, which closes #355 at every hop it reaches.
- **An answer handler takes `Expired` in the arm it has for `Answered::No`.** ADR-0017 refused to cancel requests partly because every answer handler would take one case more. A handler already takes `No` and its reason, so `Expired` is one more reason, not one more case.
- **This replaces ADR-0017's consequence that "a request that times out is its asker's to decide: the engine sets no deadline".** An asker still decides: `within` states its deadline, and an ask that states none and inherits none waits as before.
- **A request that expires is not cancelled at its receiver.** The receiver is not told, and finishes or drops its work; what it answers is dropped. ADR-0017's reasons for not cancelling still hold.
- The engine keeps the requests that have a deadline in a heap by when they are due, and checks the earliest each step. One answered early stays in the heap until its time comes and is passed over then, so a hub keeps an entry of a few dozen bytes for each request of the last `answer_within_millis`.
- A request's deadline rides in the mail's tie word, which for a request only repeated the mail's own count, so an envelope does not grow; a deadline past some 292 years from the host's first moment is kept as that time.
- An ask record of a guest grows by a deadline, and the guest library and every host read it.
- `Within` is an enum, not an optional span, so an unbounded call says so.

## Alternatives considered

- **A `Sleep` in `Mcp` alone,** as #355 was filed. Small, but `EngineTools`, `Engine:k`, and `Link` leak the same way.
- **A `Sleep` and a setting in every hop that holds a request.** Each hop's own deadline, which may disagree with the one before it, and every holder written later must remember its own.
- **The engine tells a holder that what it holds expired.** It would free a holder that asks nothing, but a holder that forwards learns it already, from its own ask, and one that waits on a frame has `Held::due`.
- **A deadline as a span everywhere.** Spans inherited hop by hop shrink by guesswork; a time on one clock does not. Within one engine a deadline is a time, and it crosses the link, between two clocks, as a span.
- **Deadlines on jobs too.** A job is the asker's own work, cancelled by its asker already, and a job that a deadline ended would leave its host's work running.
