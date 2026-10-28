# ADR-0036: The outside seat, through which a test asks and is asked

- **Status:** Proposed
- **Date:** 2026-10-28

## Context

A test of an actor wants what a caller of that actor gets: send it something, step the engine, and read back the typed answer. The engine gives a test no way to do that.

What it gives instead is two one-way roads, both for a chassis rather than a test:

- **In:** a source (ADR-0010: `Engine::source`, `Feed`, `Engine::bring`). The chassis pushes events onto a feed, and the engine brings each in between steps as mail from the source's owner to itself. Nothing comes back along it.
- **Out:** a controller (ADR-0019: `Controller`, `Requests`). The actor that owns one pushes requests the host takes between steps. They are values on a queue, not mail: nothing can answer them, and they are not typed by any protocol.

So every test so far writes an actor of its own to stand in for the caller, and has that actor record what happened somewhere the test can read it after stepping. Somewhere is a static: a `thread_local!` or `Mutex` log of lines (`SAID`, `say`, `said` in `aethereal-substrate/tests/streams.rs` and about twenty more files), or atomics (`PORT`, `DONE` in `aethereal-chassis-headless/tests/http.rs`). That has three costs.

- What the test asserts on is text the stand-in formatted, not the value the engine delivered. A stand-in that formats wrongly, or two that write the same line, pass or fail for the wrong reason.
- A static is shared by every test on its thread or process, so each test must drain it first, and the order in which tests run can leak lines between them.
- Every answer a test wants needs a handler written for it (`Handles<Answer<O, C>>`) on the stand-in, so a test grows a new actor for each thing it asks.

The obvious fix, one harness actor that runs whatever a test orders, fails on that last point. An actor handles each answer kind through its own `Handles<Answer<O, C>>` impl, fixed when it is compiled, so one generic actor cannot receive whatever answer kind a test asks for.

A test also needs to be asked. HTTP hands each request to a handler named by a `ProtocolRef<Handling>` (ADR-0035: `Serve::handler`), and a test of it wants to be that handler: take the request, decide, and answer with a typed `Response`.

## Decision

**The engine seats one actor for the host to speak through, the outside.** It is a root like any other, made by the host as the engine is set up. Its code is the engine's own: a `Runs` (ADR-0013) that takes every piece of mail it is handed, whatever its kind, and pushes it, still written, onto a queue the host takes from between steps, as a controller's requests are taken (ADR-0019: `Requests::take`). It runs no handler and sends nothing in its turns.

Because it sits in a seat (`Holding::Here`), everything the engine keeps for an actor, it keeps for the outside unchanged: its life (`Lives`), its stream, the requests it asked and is waiting on (ADR-0017: `Held::waiting`), the requests it holds (`Held::debts`), and its watches. No view learns of a second kind of actor.

**The host sends from the outside between steps,** as it brings a source's events (`Engine::bring`): written into the arena for mail sent between steps, with the outside as the sender, each the root of a chain of its own (ADR-0012). The engine counts them as the outside's sends, so a request it asks has a mail id like any other (ADR-0017: `MailId`), and its answer routes back to the outside's seat, where the outside's turn puts it on the queue.

**A harness is a host and its outside, which a test owns and steps.**

```rust
let mut harness = Harness::of(Headless::program().with_cycle::<(Tick,)>().root::<Tcp>().root::<Http>())?;
let http = harness.root::<Http>();                                    // ActorRef<Http>, proven when the program was built
let handler = harness.protocol::<Handling>();                         // ProtocolRef<Handling> naming the outside

let serving = harness.ask(http, &Serve { port: 0, handler })?;        // Asked<Served>: a ticket typed by the answer
let Answered::Yes(Served::Bound { port, .. }) = harness.answer(serving)? else { panic!("bound") };

let request = harness.asked::<Request>()?;                            // Held<Request>: a request asked of the outside
harness.reply(request, &Response::new(Status::Ok, "text/plain", b"hi".to_vec()))?;
```

- **Asking.** `Harness::ask(target, &kind)` builds only when the target's protocol answers the kind with one kind (ADR-0016: `Carries<K>::Answers`, `DeclaredBy`), the same check a turn's ask makes. It answers with `Asked<O>`, which holds the request's mail id and is typed by the answer. `Harness::answer(asked)` steps until that request's answer is on the queue and decodes it as `Answered<O>`, so a test sees `Unanswered::Ended`, `Failed`, `Unreplied`, and `Faulted` as the asker would. `Harness::tell` sends mail that asks for nothing.
- **Being asked.** `Harness::protocol::<P>()` makes a `ProtocolRef<P>` to the outside. The outside takes every kind, so the reference is as true for it as `ActorRef::protocol` is for a type that handles each of `P`'s kinds. `Harness::asked::<K>()` steps until mail of kind `K` asked of the outside is on the queue and answers with `Held<K>`: the request, and what `Harness::reply` answers it with. A held request the test never answers waits, since the outside never ends.
- **Hearing.** A test's own actor that must report what it did takes a `ProtocolRef<P>` to the outside in its config and tells it, and `Harness::heard::<K>()` takes what it told, decoded. That is the road that replaces every static log.
- **Reading the queue.** The queue keeps what arrived in order. `asked`, `answer`, and `heard` each take the first entry that matches what they ask for and leave the rest, and `Harness::rest` answers with what is left, so a test can assert that nothing else came. Mail of a kind other than the one asked for is a typed refusal naming both kinds, never a panic inside the harness.
- **Stepping.** `Harness::step` runs one step. Every verb that waits steps until what it waits for has arrived or its budget is spent, and refuses with where it stood. Over a bare `Engine` the budget is a count of steps and the clock is one the test moves (`Harness::advance`), so a run is the same every time. Over a host whose own threads do work the test waits on, such as the headless host's sockets and jobs, the budget is real time, as the tests' `waiting::until` reads it today.

**The harness lives in `aethereal-substrate`, behind a `harness` feature.** The outside is engine code, and the substrate's own tests use the harness under `cfg(test)`. Any other crate turns the feature on in its dev-dependencies. A host the harness steps implements one trait there, which gives the harness its engine between steps and runs a step; the headless host implements it.

## Consequences

- Tests assert on the values the engine delivered, decoded by the type the test asked for, not on lines a stand-in formatted.
- No test keeps a static. Each harness owns its engine and its queue, so tests are independent of the thread they run on and of each other.
- A test needs an actor of its own only for the behaviour under test. One that only stood in for a caller goes away.
- HTTP's unanswered cases become ordinary assertions: the harness, as the handler, can end, fail, or stay silent, and the test reads the status the client got.
- The engine grows one seat it makes for itself, and between-steps sends from it on the path `bring` already takes. Nothing a program runs can reach the outside except through a reference the host handed in, and a program not run by a harness has no outside.
- `ProtocolRef` gains a second maker outside a turn, `Harness::protocol`. It is the one reference in the engine whose truth rests on the receiver taking every kind rather than on its declared handlers.
- Follow-on work, in order: the outside seat and the harness, then the tests replaced crate by crate (`aethereal-substrate`, `aethereal-actor`, `aethereal-chassis-headless`, `aethereal-chassis-browser`, `aethereal-window`), adding the HTTP cases no test covers today, and then MCP.

## Alternatives considered

- **One harness actor that runs the test's orders.** An actor handles each answer kind through an impl fixed at compile time, so a generic one cannot receive whatever a test asks for without an erased handler hook in the engine, which is the outside under another name and with a handler table in the way.
- **A source in and a controller out.** A controller's requests are plain values, not mail, so nothing could answer them and no protocol types them; the test would still read what an actor chose to push.
- **A stand-in actor per test, reporting through a channel instead of a static.** It removes the sharing between tests but keeps the formatted lines and the new actor for every answer kind.
- **An outside that is not seated, its mail taken off the stream before it is run.** Every view would need to know of an actor that is not in the table, and the requests it asks would have nowhere to be kept.
