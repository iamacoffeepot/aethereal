# ADR-0020: The frame lifecycle, and knowing when a chain has finished

- **Status:** Proposed
- **Date:** 2026-10-19

## Context

A program that draws runs in frames. In each frame its actors update what they hold, then draw it, then the drawing is shown, and every actor wants to know that the others have finished the stage before it begins the next: a camera that has not moved yet draws last frame's view. Before the first frame, actors set up; after the last, they put things away.

An actor is a state machine, and stages are how the program keeps every actor at the same point of the frame. An actor in two stages at once, talking to actors that may or may not be in the same stage, cannot tell which frame what it is told is about. So a stage begins only once the one before it has finished.

What "finished" means is ADR-0011's planned view, "whether a chain of mail has finished", which is not built. ADR-0012 counts it by root: mail sent, less mail handled. It says what belongs to a chain: mail sent while handling a piece of mail takes its root (`Envelope::root`), a closing turn's mail and what the watchers of an ending send belong to the chain that brought the ending about, and what a source brings in is the root of a chain of its own. Nothing yet tells an actor that a chain has finished.

The earlier engine let an application declare its stages as a graph, with a driver that broadcast each and advanced when the stage's chain settled (its ADR-0082). Custom stages never found a use; every program ran start, then tick, render, and present repeated, then shutdown. Its `Tick` came to carry the frame's delta and the time elapsed, and every other stage nothing: what a frame's actors need of one another rides their own mail. Its desktop driver limited each frame's delta, so a long stall slowed the program's time and skipped nothing.

## Decision

**An actor may be told when its own chain has finished.**

```rust
turn.watch_chain(&Stage::Tick)?;                    // the chain of the item this turn handles

impl Handles<Finished<Stage>> for Lifecycle { .. }  // handed once the chain has nothing left to run
```

- A chain has finished when no mail of it is in flight and no turn of it is left to run: everything sent in it has been handled or dropped, and every closing turn it brought about has run (ADR-0012).
- A held request does not keep a chain from finishing: its answer is sent from a later turn, on that turn's chain (ADR-0017). Nor does a job, whose answer starts a chain of its own (ADR-0018).
- An actor watches only the chain of the item its turn handles, so the turn that asks is itself part of the chain, and the chain cannot have finished before the watch is in place.
- The notice is handed to the handler of `Finished<C>` with the context the watch kept, kept as an ask's is (ADR-0017). It is mail from the actor to itself, and the root of a chain of its own, as what a source brings in is: so a stage begun from it is a chain of its own, which the next watch follows.

**Whether a chain has finished is a view**, the one ADR-0011 planned, kept under the chain's root. Each turn's record says what it changed of its chain's count: the mail it sent, less the item it handled. Many actors' turns change one chain's count, and a sum is the same in whatever order its records arrive, so this is a key with many writers that the fold makes whole (ADR-0011). Mail dropped undelivered takes one away where it is dropped. An ending to come keeps its chain open until its closing turn has run. A turn that sends exactly one piece and handles one changes nothing, so a chain of hops writes no record. When the fold leaves a chain at zero, the watches of it are handed on, and the chain is forgotten.

**The program's lifecycle is a fixed set of stages the engine defines.**

```text
Start → ( Tick → Render → Present )* → Stop
```

```rust
pub struct Start;
pub struct Tick { pub delta_nanos: u64, pub elapsed_nanos: u64 }
pub struct Render;
pub struct Present;
pub struct Stop;
```

- No program declares its own stages. A program that needs a phase inside a stage orders it with its own mail, within the stage's chain.
- `Tick` carries the frame's time: how long since the last `Tick`, and the time elapsed, the sum of every delta. Every other stage carries nothing. What one actor's work in a stage means to another rides its own mail, as a camera mails its view to the renderer.

**A lifecycle capability runs them**, a root the chassis makes (ADR-0019), with a source the host brings frames in through and a controller it tells the host it has stopped through.

- Actors subscribe to the stages they take, each by mail to the capability, which keeps them as `Subscribers` (ADR-0016) for each stage.
- **Each stage is sent to its subscribers, and the next begins only once its chain has finished.** The capability sends a stage to every subscriber of it, watches the chain, and sends the next stage from the notice. So every turn a stage set off has run before any subscriber is sent the next.
- **The host says when a frame is due**, by pushing `Frame` onto the source: on a browser's animation frame, on the display's refresh natively, on a timer headless. A `Frame` that arrives while a frame is still running is not queued: the next `Tick` covers the time since the last one, however many `Frame`s came between. A frame that runs long therefore makes the frames fewer, never late in a queue.
- `Tick`'s delta is measured on the steps' time (ADR-0018: `turn.now()`), so a run replays with the same deltas. The capability limits each delta to a most its config gives, so a long stall slows the program's time and skips none of it.
- **Start is sent on the first frame.** The chassis makes its roots, steps the engine until nothing is left to run (`Engine::is_idle`), so every root's `begin` and what it set off has subscribed, and only then pushes the first `Frame`. An actor made after `Start` was sent is not sent it: its `begin` is its start.
- **Stop is asked for by the host or by an actor.** The host pushes `Quit` onto the source when its window closes, the process is told to end, or the page is hidden. An actor sends `Quit` to the capability. The capability lets a frame that is running finish, sends `Stop`, watches its chain, and when it has finished pushes `Stopped` onto its controller, and the host drops the engine. `Frame`s brought after `Quit` are dropped.

## Consequences

- Every actor sees stages in one order, and no actor is sent a stage before every turn of the stage before it has run.
- A frame takes at least two steps a stage: one in which the stage's subscribers run, and one in which its notice is handled and the next stage sent. With three stages, a frame is at least seven steps, which at microseconds a step is far below a frame's time.
- A stage waits for its whole chain. An actor that keeps mail going around within a stage, forever, holds every frame after it. Work that outlives a frame leaves the stage's chain: as a job, or a request held and answered later, each of which starts a chain of its own.
- Every turn that does not send exactly one piece writes a record to the view of chains: a turn that sends none takes one away, and one that sends many adds. That is a cost on the path every piece of mail takes, measured before the view enters, steady and fusing, as every view is (ADR-0011). Every chain with mail in flight is kept in the view until it finishes.
- The view is the first an actor's turn can ask to be told of, and the first whose records come from nearly every turn. Watches of chains are kept under the chain's root, with what they wait for, as watches of actors are kept under the actor watched (ADR-0011).
- A browser hides a page with little time left to run, so a browser's `Stop` is best effort: what does not run before the page goes is not run.
- Who may send `Quit` is whoever holds a reference to the capability. A program that wants fewer to hold it keeps the reference from them.
- A fixed set can grow a graph later, if a program shows a need for a stage these do not cover.

## Alternatives considered

- **Stages declared by each program, as a graph.** What the earlier engine had. No program used a stage of its own, and the graph was a builder, a check of its edges, and a driver to keep for nothing.
- **Stages that do not wait for one another.** A frame would take fewer steps. An actor could be sent `Render` while turns of its `Tick` were still to run, and the stages would order nothing.
- **A barrier that counts held requests and jobs in flight.** A stage would wait for every fetch it began. Work that takes time would hold back the frame, which nothing that takes time may do.
- **Finished told to every actor of the chain, or polled from the view by a turn.** Nothing to ask for. A turn runs only when it has an item, so a finished chain must hand someone an item, and only its watcher wants it.
- **Every `Frame` the host pushes queued as a `Tick`.** No frame skipped. A program slower than the display falls further behind every frame, and catches up by running stale frames back to back.
- **Start sent as soon as the capability is made.** No waiting on the chassis. The roots made after it would subscribe too late to be sent it.
