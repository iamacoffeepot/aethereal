# ADR-0017: Requests and their answers

- **Status:** Proposed
- **Date:** 2026-10-19

## Context

An actor that asks another for something needs two things the engine does not give it yet: a way to tell which answer is to which question, and an answer that comes even when the answerer cannot give one. ADR-0009 left both open: "how a request and its answer are written over mail" and "the answer each request gets that an ended actor left unanswered".

ADR-0012 tells an actor to keep what it sent mail for under the chain's root and the depth it sent at (`Mail::root`, `Mail::depth`, `Turn::chain`). That cannot tell answers apart in two cases that matter:

- Two questions sent from one turn have the same root and the same depth. When both go to one actor and are answered with one kind, nothing tells the answers apart.
- An answer given in a later turn, after the answerer did some work or asked someone else, is sent on that later turn's chain. Its root is not the question's.

Writing a number into every question kind and keeping a table of them is the other way: the asker makes the number, writes it in, files it, and checks it when an answer comes. Every step is the author's to forget, and a number made and not filed makes a true answer look stale. An earlier engine started there and moved the number into the engine, where no author handles it.

What comes back is already typed. A named protocol says what each kind is answered with (ADR-0016: `Carries<K>::Answers`), and a sender through it handles those kinds, checked as a bound (`BackTo`). What is not decided is how often an answer comes, and what the asker is told when none can.

## Decision

**A request is mail that asks for one answer, and the engine ties the answer to it.** The asker asks through a named protocol that answers the kind with one kind, and hands the engine a context to keep until the answer comes:

```rust
impl Carries<WhichTile> for Tiles { type Answers = (TileIs,); }      // one answer: a kind that can be asked

turn.ask_to(tiles, &WhichTile { x, y }, &Looking { at: x })?;         // the request, and what to keep until it is answered
```

- **The request's id is its mail's id,** its sender and that sender's count (ADR-0012: `MailId`). No other number is made, and the asker writes none.
- **The engine keeps the context beside the asker,** in its seat, as it keeps what the asker owns (ADR-0010: `Keeping`), under the request's count. The context is a kind that crosses `Within`, so it is kept as bytes, the same for native code and a guest's. An actor that has never asked holds no more for it than a pointer's width.
- **`ask_to` builds only for a protocol that answers the kind with one kind, and only when the asker handles that answer.** Its turn's sends list the ask, `Asks<P, K>`, as they list any send, and the bound that checks a protocol's answers (`BackTo`) checks this one. A tuple of kinds answers nothing, so nothing can be asked through one.

**An answer is the receiver's reply to a request.** The receiver replies as it replies to any mail, `turn.reply(&answer)`, and when the mail it handles is a request, the reply answers it: the engine writes in the reply which request it answers. A request is answered once. A second reply to it in the same turn is refused, as a send its declaration does not allow is.

**The asker handles what became of its request, with its context.** It is handed to the handler of `Answer<A, C>`: whether the request was answered, and the context the engine kept, which leaves the engine with it:

```rust
pub enum Answered<A> {
    Yes(A),                     // the receiver's answer
    No(Unanswered),             // why none came
}
pub enum Unanswered { Ended, Failed, Unreplied }

impl Handles<Answer<TileIs, Looking>> for Pointer {
    type Sends = ();
    fn handle(&mut self, (answered, looking): (Answered<TileIs>, Looking), turn: &mut Turn<'_, ()>) -> Result<(), Fault> {
        match answered {
            Answered::Yes(tile) => { .. }
            Answered::No(why) => { .. }
        }
    }
}
```

- An actor handles each answer kind with one kind of context. Asks that keep different things for one answer kind keep an enum.
- An answer is told apart from other mail of its kind by what the engine wrote in it, so an actor may also handle `TileIs` as plain mail, and each goes to its own handler.
- A context that does not decode as the handler's kind fails the turn, as mail that does not decode as its kind does.

**Every request is answered exactly once.** When the receiver does not answer, the engine tells the asker why, as `Answered::No`:

- `Ended`: the receiver had ended, or was gone, when the request was to be handed to it, or it held the request and ended before answering it;
- `Failed`: the turn that handled the request failed;
- `Unreplied`: the turn handled the request and neither replied nor held it.

What the engine says is mail too, of the engine's kind `Unanswered`, from the receiver to the asker. It is tied to the request as only the engine ties mail, so no actor can say it in the engine's place, and it is handed to the handler of the answer the asker asked for, whose kind the engine keeps with the context. `Failed` and `Unreplied` come after what the turn sent, as the receiver's next piece of mail. `Ended` comes after all the receiver sent, from where the engine would have handed the request over.

An answer kind is a kind like any other, with nothing to implement for the case where no answer comes. The engine keeps only the request's count and the context, and needs nothing of the asker's code to say a request went unanswered, so it does the same for a guest. What it hands the asker comes from the receiver, the actor the request was sent to, which existed when it was sent (ADR-0009), and belongs to the request's chain. A caller's handler therefore always runs, and what it kept always comes back to it.

**An answer that comes later is held.** A receiver that cannot answer in the turn of the request holds it, and answers from any later turn:

```rust
let held: Held<Fetched> = turn.hold()?;          // in the turn of the request
self.waiting.push((path, held));                  // kept in the actor's state
turn.answer(held, &Fetched::Got(blob))?;          // in a later turn
```

- `Held<A>` names one request this actor owes an answer of kind `A`. It crosses `Within` and no other boundary, so it is kept in the actor's state, and cannot be in mail. It is neither `Clone` nor `Copy`, and `answer` takes it by value, so it answers once, which the build checks.
- The engine keeps the debt beside the receiver, in its seat. `answer` confirms the ticket names a debt this actor owes, of kind `A`, as `open` confirms a handle (ADR-0010), and fails the turn when it does not.
- When the receiver ends, the engine tells the asker of every debt it still owes `Answered::No(Ended)`, after its closing turn. A `Held` dropped from the actor's state without an answer is answered when the actor ends.
- **Not decided here:** giving a `Held` to another actor, so a child answers what its parent was asked.

**An answer belongs to the chain of the turn that sends it.** A reply in the turn of the request is on the request's chain. A held answer is on the chain of the later turn that gives it. What the engine says of an unanswered request is on the request's chain. Requests and answers are mail, so they fuse as mail does (ADR-0013).

**What an asker keeps for an answer is kept by the engine; what it keeps for other mail stays its own.** ADR-0012's root and depth still name a piece of mail's chain, for tracing and for mail that is not a request.

## Consequences

- A request and its answer cost the asker no number to make or check, and an answer cannot be lost: the caller's handler runs once for each ask, with what it kept.
- Every answer handler handles both cases. A request to an actor that can end can always go unanswered, so that is the handler's to say, and it is told why.
- Asking needs a named protocol. An actor asked through a typed reference is asked through a protocol it conforms to (ADR-0016: `ActorRef::protocol`).
- An asker's seat holds what it is waiting for, and an answerer's what it owes. Both are dropped with the actor: an asker that ends drops its contexts, and the answers to it are dropped as mail to an ended actor is.
- Mail says whether it asks, and which request it answers, in a tie of eight bytes that every piece carries (`Tie`): a request is marked, and an answer names its request by the count, which its receiver sent; what the engine says of an unanswered request is marked as the engine's. Every turn reads it, to find an answer's context and what it owes its source. Measured in steady state with fused hops, against the engine without it, a piece of mail cost about 10 percent more in `count`, 6 to 8 percent in `pingpong`, 4 percent in `big`, and 1 to 3 percent in `world`, `churn-wide`, and `fib`. About 4 points of `count` are the envelope's eight bytes, and the rest the reads each turn makes. Carrying the tie in the mail's payload, so mail that is no request stores nothing more, would take back most of the first; it is left until a load is bound by how fast mail goes.
- A guest asks, replies, holds, and answers in records its host reads, and its host holds it to what it claims, as it does for resources (ADR-0015).
- The reply through a tuple of kinds that nothing checks (ADR-0016) is unchanged for plain replies. A request cannot be sent through a tuple, so it cannot reach an asker that does not handle its answer.
- **Not decided here:** a request that times out, which needs time in a turn; giving a held request to another actor; and a request answered more than once, such as a status and then a result, which stays plain mail through a protocol that answers with more than one kind.

## Alternatives considered

- **A number in every request kind, kept by the asker.** No engine change. Every ask has steps to forget, a number made and not filed turns a true answer stale, and nothing answers a request whose answerer ended.
- **Matching by root and depth (ADR-0012).** Nothing new. It cannot tell two asks of one turn apart, or follow an answer given in a later turn.
- **The context kept in the asker's own state, keyed by the request's id.** No engine table. The asker files and finds it by hand, and the engine cannot answer for a receiver that ended without running the asker's code.
- **An answer of any kind the protocol lists.** Requests answered by a status and then a result. The engine could not say which answer ends the request, nor which kind to answer with when none comes.
- **Answers only when the receiver gives one.** Less engine work. A caller whose answerer ended or failed waits forever, and what it kept is never freed.
- **An answer kind's own `unanswered()`, encoded by the asker as it asks.** The handler sees only answers. Every answer kind needs a case and an implementation for it, every ask encodes it ahead, and the asker cannot tell an answerer that ended from one that failed or forgot.
