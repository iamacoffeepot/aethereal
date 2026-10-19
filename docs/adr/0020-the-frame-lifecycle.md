# ADR-0020: The frame lifecycle, run by the engine

- **Status:** Proposed
- **Date:** 2026-10-19

## Context

A program that draws runs in frames. In each frame its actors update what they hold, then draw it, then the drawing is shown, and every actor wants to know that the others have finished the stage before it begins the next: a camera that has not moved yet draws last frame's view. Before the first frame, actors set up; after the last, they put things away.

An actor is a state machine, and stages are how the program keeps every actor at the same point of the frame. An actor in two stages at once, talking to actors that may or may not be in the same stage, cannot tell which frame what it is told is about. So a stage begins only once the one before it has finished: every turn it set off has run, however far its mail went. The actors a stage is sent to cannot say that alone, since each knows only the mail it sent, not what that mail set off in actors it never named.

What "finished" means is ADR-0011's planned view, "whether a chain of mail has finished", which is not built. ADR-0012 counts it by root: mail sent, less mail handled. It says what belongs to a chain: mail sent while handling a piece of mail takes its root (`Envelope::root`), a closing turn's mail and what the watchers of an ending send belong to the chain that brought the ending about, and what a source brings in is the root of a chain of its own.

The earlier engine let an application declare its stages as a graph, with a driver actor that broadcast each and advanced when the stage's chain settled (its ADR-0082). Custom stages never found a use; every program ran start, then tick, render, and present repeated, then shutdown. Its `Tick` came to carry the frame's delta and the time elapsed, and every other stage nothing: what a frame's actors need of one another rides their own mail. Its desktop driver limited each frame's delta, so a long stall slowed the program's time and skipped nothing.

## Decision

**The engine runs the program's lifecycle, a fixed set of stages it defines.**

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
- The lifecycle is the engine's, between steps, and not an actor's: no actor waits on a chain, and the engine is the only reader of whether one has finished.

**An actor subscribes to a stage in a turn, and the engine sends it the stage.**

```rust
turn.subscribe::<Tick>()?;       // builds only when the actor handles Tick
turn.unsubscribe::<Tick>()?;

impl Handles<Tick> for Camera { .. }
```

- Subscribing and unsubscribing are effects, folded into a view of each stage's subscribers: many actors write one stage's set, and a set is the same in whatever order its records arrive (ADR-0011). A `begin` may subscribe. An actor that ends leaves every set.
- A stage is sent to each subscriber as mail from that actor to itself, as what a source brings in is (ADR-0010), between steps, and handled in the next step. Subscribers are sent it in the order of their ids.
- Every piece of one stage's mail has one root, a `MailId` the engine makes of an id of its own that no actor has, and the stage's number. So a stage is one chain, whatever its subscribers do.
- A guest subscribes in records its host reads, as it asks (ADR-0015).

**A stage begins only once the one before it has finished.** After each fold, the engine reads whether the chain of the stage under way has finished, and when it has, sends the next stage before the next step.

- A chain has finished when no mail of it is in flight and no turn of it is left to run: everything sent in it has been handled or dropped, and every closing turn it brought about has run (ADR-0012).
- A held request does not keep a stage from finishing: its answer is sent from a later turn, on that turn's chain (ADR-0017). Nor does a job, whose answer starts a chain of its own (ADR-0018), nor what a source brings in.
- **The view counts only the chains of stages.** A turn whose item belongs to a stage's chain, which its root says, records what it changed of the chain's count: the mail it sent, less the item it handled. A sum is the same in whatever order its records arrive, so many actors' turns write one chain's count through the fold (ADR-0011). Mail dropped undelivered takes one away where it is dropped, and an ending to come keeps its chain open until its closing turn has run. A turn that sends exactly one piece changes nothing and writes nothing, and a turn of any other chain writes nothing at all.

**The host says when a frame is due, and when to stop.**

```rust
impl Engine {
    pub fn frame(&mut self);              // a frame is due: on an animation frame, a display's refresh, a timer
    pub fn quit(&mut self);               // the window closed, the process was told to end, the page was hidden
    pub fn lifecycle(&self) -> Lifecycle; // Lifecycle::Waiting | Running | Stopping | Stopped
}
```

- **Start is sent at the first `frame`**, to every subscriber folded by then. A root's `begin` is folded as `add_root` makes it, so a chassis that makes its roots before the first frame has every root, and every child their `begin`s stood up, subscribed. An actor made after `Start` was sent is not sent it: its `begin` is its start. `Tick` follows once `Start` has finished.
- **A `frame` while a frame is under way is not queued.** The next `Tick` covers the time since the last one, however many frames were due between. A frame that runs long makes the frames fewer, never late in a queue.
- `Tick`'s delta is measured on the steps' time (ADR-0018: the time the host passes in with each step), so a run replays with the same deltas. The engine limits each delta to a most the host gives, so a long stall slows the program's time and skips none of it.
- **An actor may ask to stop**, with `turn.quit()`, an effect, as the host does with `engine.quit()`. The engine lets a frame under way finish, sends `Stop`, and once its chain has finished, its lifecycle is `Stopped`, and the host drops the engine. Frames due after a quit are not run.

## Consequences

- Every subscriber sees stages in one order, and none is sent a stage before every turn of the stage before it has run, in whatever actors its mail reached.
- A frame takes at least one step a stage: the engine sends the next stage between the step whose fold finished the last and the step after. With three stages, a frame is at least three steps.
- A stage waits for its whole chain. An actor that keeps mail going around within a stage, forever, holds every frame after it. Work that outlives a frame leaves the stage's chain: as a job, or a request held and answered later, each of which starts a chain of its own.
- An actor that wants to know when its own work is done asks for it and counts the answers (ADR-0017). No actor can wait on a whole chain, so none waits on actors it never named.
- Only turns of a stage's chain that do not send exactly one piece write to the view of chains. Other chains pay a test of their root on each turn. What that costs is measured before the view enters, steady and fusing, as every view is (ADR-0011).
- A capability outside the engine cannot make a barrier of its own. A program that shows a need for one, or for a stage the set does not cover, is a reason to grow the set, or to give it a graph.
- A browser hides a page with little time left to run, so a browser's `Stop` is best effort: what does not run before the page goes is not run.
- Any actor may stop the program. A program that wants fewer to is written so they do not.

## Alternatives considered

- **Stages declared by each program, as a graph.** What the earlier engine had. No program used a stage of its own, and the graph was a builder, a check of its edges, and a driver to keep for nothing.
- **A lifecycle actor that watches each stage's chain**, with a verb any actor could use to be told when its own chain has finished. The lifecycle would be one more capability. Every actor could then wait on a chain, so on actors it never named and did not know were in it: one more actor forwarding a stage's mail would hold back everyone watching. The actor also costs two steps a stage, one for its notice and one for the next stage's mail. The engine needs no turn to send a stage.
- **Stages that do not wait for one another.** A frame would take fewer steps. An actor could be sent `Render` while turns of its `Tick` were still to run, and the stages would order nothing.
- **A barrier that counts held requests and jobs in flight.** A stage would wait for every fetch it began. Work that takes time would hold back the frame, which nothing that takes time may do.
- **Each subscriber answering its stage, as a request.** No view of chains. A stage would end when its subscribers had handled it, not when what they set off had: the renderer could be sent `Render` before the camera's view reached it.
- **Every chain counted.** Any chain could be asked about later. Every turn that does not send exactly one piece would write a record, for chains nothing reads.
- **Every due frame queued as a `Tick`.** No frame skipped. A program slower than the display falls further behind every frame, and catches up by running stale frames back to back.
