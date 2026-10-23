# ADR-0017: Requests and their answers

- **Status:** Proposed
- **Date:** 2026-10-19

## Context

An actor that asks another for something needs two things the engine does not give it yet: a way to tell which answer is to which question, and an answer that comes even when the answerer cannot give one. ADR-0009 left both open: "how a request and its answer are written over mail" and "the answer each request gets that an ended actor left unanswered".

ADR-0012 tells an actor to keep what it sent mail for under the chain's root and the depth it sent at (`Mail::root`, `Mail::depth`, `Turn::chain`). That cannot tell answers apart in two cases that matter:

- Two questions sent from one turn have the same root and the same depth. When both go to one actor and are answered with one kind, nothing tells the answers apart.
- An answer given in a later turn, after the answerer did some work or asked someone else, is sent on that later turn's chain. Its root is not the question's.

Writing a number into every question kind and keeping a table of them is the other way: the asker makes the number, writes it in, files it, and checks it when an answer comes. Every step is the author's to forget, and a number made and not filed makes a true answer look stale. An earlier engine started there and moved the number into the engine, where no author handles it.

What comes back is already typed. A named protocol says what each kind is answered with (ADR-0016: `Carries<K>::Answers`), and a sender through it handles those kinds, checked as a bound (`DeclaredBy`). What is not decided is how often an answer comes, and what the asker is told when none can.

## Decision

**A request is mail that asks for one answer, and the engine ties the answer to it.** The asker asks through a named protocol that answers the kind with one kind, and hands the engine a context to keep until the answer comes:

```rust
impl Carries<WhichTile> for Tiles { type Answers = (TileIs,); }      // one answer: a kind that can be asked

turn.to(tiles).ask(&WhichTile { x, y }, &Looking { at: x })?;        // the request, and what to keep until it is answered
```

- **The request's id is its mail's id,** its sender and that sender's count (ADR-0012: `MailId`). No other number is made, and the asker writes none.
- **The engine keeps the context beside the asker,** in its seat, as it keeps what the asker owns (ADR-0010: `Keeping`), under the request's count. The context is a kind that crosses `Within`, so it is kept as a `Written`, the same for native code and a guest's: its bytes, and a pin on each blob they refer to, which go when the context does (ADR-0014). An actor that has never asked holds no more for it than a pointer's width.
- **An ask (`turn.to(target).ask`, ADR-0016) builds only for a protocol that answers the kind with one kind, and only when the asker handles that answer.** Its turn's sends list the ask, `Asks<P, K>`, as they list any send, and the bound that checks a protocol's answers (`DeclaredBy`) checks this one. A tuple of kinds answers nothing, so nothing can be asked through one.
- **A `begin` asks as a turn does,** its `BeginSends` listing the ask, and the answer comes to the actor in a turn after it. A child's `begin` runs in its parent's turn, so the engine notes beside each request who asked it, and files it beside that actor as it is seated: a root's beside the root, a child's beside the child. A `begin` that fails asks nothing, its requests dropped with its mail. A closing turn asks nothing: the actor is gone before an answer could reach it, so its target (`Toward<.., Leaves>`) has no `ask`.

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
    fn handle(&mut self, (answered, looking): (Answered<TileIs>, Looking), turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
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
- `Unreplied`: the turn handled the request and neither replied nor held it, or the receiver held it and let it go without answering it.

What the engine says is mail too, of the engine's kind `Unanswered`, from the receiver to the asker. It is tied to the request as only the engine ties mail, so no actor can say it in the engine's place, and it is handed to the handler of the answer the asker asked for, whose kind the engine keeps with the context. `Failed` and `Unreplied` come after what the turn sent, as the receiver's next piece of mail. `Ended`, and `Unreplied` for a held request let go of, come after all the receiver sent.

An answer kind is a kind like any other, with nothing to implement for the case where no answer comes. The engine keeps only the request's count and the context, and needs nothing of the asker's code to say a request went unanswered, so it does the same for a guest. What it hands the asker comes from the receiver, the actor the request was sent to, which existed when it was sent (ADR-0009), and belongs to the request's chain. A caller's handler therefore always runs, and what it kept always comes back to it.

**An answer that comes later is held.** A receiver that cannot answer in the turn of the request holds it, and answers from any later turn:

```rust
let held: Held<Fetched> = turn.hold()?;          // in the turn of the request
self.waiting.push((path, held));                  // kept in the actor's state
turn.answer(held, &Fetched::Got(blob))?;          // in a later turn
```

- `Held<A>` names one request this actor owes an answer of kind `A`. It crosses `Within` and no other boundary, so it is kept in the actor's state, and cannot be in mail. It is neither `Clone` nor `Copy`, and `answer` takes it by value, so it answers once, which the build checks.
- The engine keeps the debt beside the receiver, in its seat. `answer` confirms the ticket names a debt this actor owes, of kind `A`, as `open` confirms a handle (ADR-0010), and fails the turn when it does not.
- The receiver's closing turn may answer what it holds, as a later turn does, from the `Held`s its state keeps. When the receiver ends, the engine tells the asker of every debt it still owes `Answered::No(Ended)`, after its closing turn: what that turn left unanswered, and what it answered when it failed, as its mail is dropped. A closing turn that answers a debt the actor does not hold fails.
- A `Held` dropped without an answer is answered `Unreplied` in the step that dropped it, once the turns of its part have run. The engine counts the values that hold each debt, as it counts the pins on a blob (ADR-0014): a `Held` noted as it is made and dropped, and bytes it is written into, such as what the actor keeps for a request of its own, counted until they are read back into a `Held`, which takes the debt over. So a debt handed through such bytes is not taken for one let go of. Bytes written and never read back hold their debt until the actor ends. A guest's library counts its own and hands its host what each call changed of them, as it hands its pins (ADR-0015), and its host takes the changes to the debts of its instance's actors and passes over the rest. A debt is counted where its holder runs, so a `Held` let go of in another part than its holder's, or in a guest of other code, holds its debt until the holder ends.
- **Not decided here:** giving a `Held` to another actor, so a child answers what its parent was asked.

**An answer belongs to the chain of the turn that sends it.** A reply in the turn of the request is on the request's chain. A held answer is on the chain of the later turn that gives it. What the engine says of an unanswered request is on the request's chain. Requests and answers are mail, so they fuse as mail does (ADR-0013).

**A stream is a request held open, with parts sent to the asker before its answer.** Nothing in the engine is a stream. The asker asks, the receiver holds the request (above) and sends the asker parts as plain mail, and its answer ends the stream. ADR-0009's promise for one sender's mail keeps every part ahead of the answer, and the held request makes a stream end exactly once: answered, or `Answered::No(Ended)` or `Unreplied` when the receiver ends or lets it go.

What the engine does not give a stream is the protocol's: which open stream a part belongs to, how fast parts come, and stopping early. Every stream writes these one way, through the stream library in `aethereal-actor`:

```rust
pub trait Stream: 'static {
    type Open;                      // what the asker opens a stream with
    type Part;                      // what each part carries
    type Done: Kind;                // the answer that ends it
    const OPEN: KindName;           // the name of its request, `Open<Self>`
    const PART: KindName;           // the name of its parts, `Part<Self>`
}

pub struct Streamed<S> { .. }       // the protocol: `Open<S>` answered with `S::Done`, `More`, and `Enough`

pub struct Open<S> { .. }           // asker to receiver: the stream's number, and what it opens with
pub struct Part<S> { .. }           // receiver to asker: the stream's number, and one value
pub struct More { .. }              // asker to receiver: how many more parts it takes
pub struct Enough { .. }            // asker to receiver: it takes no more

pub struct Streams<S, C> { .. }     // the asker's: the streams it opened, with what it keeps for each
pub struct Serving<S> { .. }        // the receiver's: the streams it serves
pub struct Streaming<S> { .. }      // one stream it serves
```

```rust
let number = self.files.open(turn, library.protocol(), Read { path }, Loading { tile })?;     // asker
let (key, read) = self.serving.open(turn, open)?;                                             // receiver, on `Open<Files>`
self.serving.get_mut(key).map(|stream| stream.send(turn, chunk));                             // as credit allows
self.serving.take(key).map(|stream| stream.finish(turn, &ReadDone::Ok));                      // the answer
```

- **Which stream** is a number the asker's `Streams` gives each stream it opens, unique among that asker's streams of one protocol. The request carries it, `Open<S>`, so the receiver has it as the stream opens, and keys its streams by the asker and the number. A `Part`, a `More`, and an `Enough` each carry it. It names a stream and proves nothing, since who sent a piece of mail is proven already. Its fields are private, so no author writes it: only `Streams` makes an `Open`, and only `Streaming` a `Part`. A `More` and an `Enough` carry the id of the protocol's `Open` kind beside the number, which the library reads with `KindId::of`, and `Serving<S>` takes only those of its own protocol. An actor handles each answer kind with one kind of context (above), so it keeps one `Streams` for each protocol, and two of its streams never meet under one key: streams of two protocols differ by their `Open`, and streams of one protocol take their numbers from one count. A part names its protocol by its own kind and carries the number alone. Two `Streams` of one protocol in one actor would share their numbers, and their answers one handler that cannot tell them apart; the library's documentation says so, and a `Streams` takes a part only from the stream's receiver.
- **What the engine keeps for a stream's request is its number**, `Number`, so an asker handles `Answer<S::Done, Number>`, and what it keeps for the stream, the `C`, stays in its `Streams` until `answered(number)` hands it back. Opening a stream builds only when the asker handles the stream's parts and that answer.
- **Pace is credit.** The asker grants the receiver a number of parts with `More`, and each part spends one. A `Streams` keeps each stream's receiver at most its window ahead: it grants the window as the stream opens, sent after the request so it arrives after it, and one more for each part the asker takes. `Streaming::send` with none left is refused, `Unsent::NoCredit`, which hands the value back, and the receiver keeps what it would have sent where it came from. Mail is never refused and nothing blocks an actor (ADR-0001), so the receiver of the parts says how many it will take, as a TCP window and Reactive Streams' `request(n)` do.
- **Stopping** is an `Enough`. The asker's `Streams` passes over parts that arrive after it, granting nothing for them, as an asker passes over an answer it no longer waits for (below). The receiver's `Serving::stop` takes the stream out, and the receiver ends it with its answer.
- **Ending** is `Streaming::finish(self, &done)`, which takes the stream by value and answers the request it holds. A stream is reached to send through `Serving::get_mut` and taken out to finish, so no part is sent after the answer, which the build checks. A `Streaming` dropped without finishing drops its `Held`, and the engine answers `Unreplied`.
- **A receiver watches each asker** as its first stream opens, and `Serving::ended` hands back the streams of one that has ended. Their parts would reach no one, and dropping them lets go of their requests, where a receiver would otherwise keep them, and whatever it keeps beside them, for as long as it lives.
- **A stream's request and parts are named by the stream.** A kind's id is its name and its layout with every name inside removed (ADR-0006), so one name for every `Part` would give the parts of two streams of one layout one id, and a file's chunk could be read as a log's line. A stream names its own in `Stream::OPEN` and `Stream::PART`, and `Open<S>` and `Part<S>` take them: `impl<S: Stream> Kind for Part<S>`, written by hand, the longhand ADR-0006 keeps for where the derive cannot write a kind, as a name taken from a type parameter. The stop is `Enough`, as the lifecycle's last stage is already `Stop` (ADR-0020).

**What an asker keeps for an answer is kept by the engine; what it keeps for other mail stays its own.** ADR-0012's root and depth still name a piece of mail's chain, for tracing and for mail that is not a request.

## Consequences

- A request and its answer cost the asker no number to make or check, and an answer cannot be lost: the caller's handler runs once for each ask, with what it kept.
- Every answer handler handles both cases. A request to an actor that can end can always go unanswered, so that is the handler's to say, and it is told why.
- Asking needs a named protocol. An actor asked through a typed reference is asked through a protocol it conforms to (ADR-0016: `ActorRef::protocol`).
- An asker's seat holds what it is waiting for, and an answerer's what it owes. Both are dropped with the actor: an asker that ends drops its contexts, and the answers to it are dropped as mail to an ended actor is.
- Mail says whether it asks, and which request it answers, in a tie of eight bytes that every piece carries (`Tie`): a request is marked, and an answer names its request by the count, which its receiver sent; what the engine says of an unanswered request is marked as the engine's. Every turn reads it, to find an answer's context and what it owes its source. Measured in steady state with fused hops, against the engine without it, a piece of mail cost about 10 percent more in `count`, 6 to 8 percent in `pingpong`, 4 percent in `big`, and 1 to 3 percent in `world`, `churn-wide`, and `fib`. About 4 points of `count` are the envelope's eight bytes, and the rest the reads each turn makes. Carrying the tie in the mail's payload, so mail that is no request stores nothing more, would take back most of the first; it is left until a load is bound by how fast mail goes.
- A guest asks, replies, holds, and answers in records its host reads, and its host holds it to what it claims, as it does for resources (ADR-0015).
- The reply through a tuple of kinds that nothing checks (ADR-0016) is unchanged for plain replies. A request cannot be sent through a tuple, so it cannot reach an asker that does not handle its answer.
- A request that times out is its asker's to decide: the engine sets no deadline, as it sets none on a job, and the asker measures time with a `Sleep` job and `turn.now()` (ADR-0018).
- **A request is not cancelled.** An asker that stops waiting notes the request in its own state and passes over the answer when it comes. Every request is answered exactly once, so the note goes when the answer does. The context, and the pins on the blobs it refers to, stay in the asker's seat until then: until the receiver answers, ends, or lets the request go. A receiver that should stop the work it was asked for is told so by mail of a kind its protocol carries, as it is told anything else.
- A stream costs the engine nothing new. Its parts are mail, so they fuse (ADR-0013) and join a run of sends as any mail does, and a held request keeps no stage but `Stop` from finishing (ADR-0020), so a long stream holds back no frame.
- Every stream is paced, and an asker that grants no credit stalls its own stream. Whoever opened it decides when to give up, as with any request.
- **Not decided here:** giving a held request to another actor.

## Alternatives considered

- **A number in every request kind, kept by the asker.** No engine change. Every ask has steps to forget, a number made and not filed turns a true answer stale, and nothing answers a request whose answerer ended.
- **Matching by root and depth (ADR-0012).** Nothing new. It cannot tell two asks of one turn apart, or follow an answer given in a later turn.
- **The context kept in the asker's own state, keyed by the request's id.** No engine table. The asker files and finds it by hand, and the engine cannot answer for a receiver that ended without running the asker's code.
- **Parts tied to the request by the engine**, each carrying the request's tie and handed to the asker with a copy of its context, as the answer is. The receiver's held request would then be the stream, with no number and no cast. It needs a case of the tie, a handler for parts, parts in a protocol's declaration and in a guest's records, and contexts found without being taken. And the asker still keeps in its own state whatever a stream builds up, since what the engine keeps is handed out as a copy, so it still needs something to find that state by.
- **A number in each protocol's own kinds**, a `Token` only the library makes. No trait for naming parts, but every author writes the field again, and nothing checks that a streamed kind has one.
- **The request's own id as the number.** It is unique already. But `ask` would have to hand it back, and since the context is passed in the same call, the answer would have to carry the id too: a change to `Answer` for one use.
- **One count for every `Streams` an actor keeps.** Its numbers are then unique across protocols, but every stream opened has to be handed the count, and nothing stops an author handing it another.
- **A `More` and an `Enough` for each protocol**, `More<S>` and `Enough<S>`. They would need names of their own, as a `Part` does, two more for each protocol, for what eight bytes of a computed id say.
- **Parts kept by the library when credit runs out.** Easier to write against, but nothing bounds what it keeps, and the pile only moves from the asker to the receiver.
- **An answer of any kind the protocol lists.** Requests answered by a status and then a result. The engine could not say which answer ends the request, nor which kind to answer with when none comes.
- **Answers only when the receiver gives one.** Less engine work. A caller whose answerer ended or failed waits forever, and what it kept is never freed.
- **Cancelling a request, so the engine drops its context before an answer comes.** The asker's seat lets go of what it kept sooner. It saves the receiver no work, since the receiver is not told. An answer already on its way when the cancel is applied must still be dropped as it arrives, the same check the asker makes when it passes over one. And a request could then end without its handler running, or with a case more, `Cancelled`, that every answer handler handles: a second way for a request to end beside answered-exactly-once, and the state to track it, to save the memory a context holds until its answer comes. A job is cancelled because its work runs on the host, which mail does not reach (ADR-0018); a receiver is an actor, and mail reaches it.
- **An answer kind's own `unanswered()`, encoded by the asker as it asks.** The handler sees only answers. Every answer kind needs a case and an implementation for it, every ask encodes it ahead, and the asker cannot tell an answerer that ended from one that failed or forgot.
