# ADR-0019: Resources the host makes while the engine runs

- **Status:** Proposed
- **Date:** 2026-10-19

## Context

A window, a GPU device, a canvas, and an audio output are resources (ADR-0010): values that are not data, each with one owner. An actor cannot make them itself, and a job cannot either (ADR-0018):

- A native window can be made only inside the windowing library's own callbacks, on the thread that runs its event loop. In winit that is the `ActiveEventLoop` it lends each callback.
- A browser's GPU adapter and device come from promises, and its audio output plays only once the page has seen a user's gesture.
- In a browser, most of these values are bound to the page's one thread, and cannot be moved to another.

So the host makes them, on its own thread, when it can. ADR-0010 gives it two ways to bring things in, and neither carries a resource made while the engine runs. A chassis hands a root its resources at boot, in its config (`Engine::hand`). And what a source brings in reaches its owner as mail between steps (`Feed::push`, `Engine::bring`), but an event that holds a resource is dropped. Nothing carries a request out to the host either: an actor that wants a second window has no way to say so.

The earlier engine put one window actor on the thread that runs the event loop, the only actor there, which owned every native window and translated every event, with the chassis only composing it (its ADR-0164). It later gave each window an addressable actor of its own for control, beneath that manager, while the manager kept the native handles (its ADR-0167). Its render actor took native handles only on that same thread.

## Decision

**Terms.** Each is used here in this one meaning.

- A **host system** is something outside the engine that the host drives: the windowing library, the GPU, the audio output.
- A **capability** is the root actor that fronts one host system for every other actor. Actors ask it for what they want, by mail.
- A **controller** is a resource the capability owns, through which it asks the host for things. Its host end is `Requests<K>`.
- A **source** is a resource the capability owns, through which the host brings things in. Its host end is a `Feed<K>` (ADR-0010).

**The chassis makes a capability with a controller and a source.**

```rust
pub struct Controller<K> { .. }                     // a resource: requests to the host, of kind K
pub struct Requests<K> { .. }                       // the host's end of it

impl Engine {
    pub fn controller<K>(&mut self) -> (Given<Controller<K>>, Requests<K>);   // for a root's config, and the host
}
impl<K> Controller<K> {
    pub fn push(&self, request: K);                 // in a turn, through the handle the capability opens
}
impl<K> Requests<K> {
    pub fn take(&self) -> Vec<K>;                   // between steps, on the host's thread
    pub fn is_dropped(&self) -> bool;               // once the capability has dropped it: stop what was made
}
```

- `engine.controller()` makes the controller as `engine.source()` makes a source: kept, answered with the `Given` for a root's config and with the host's end.
- The capability opens its controller in a turn and pushes a request, as native code opens any resource it owns (ADR-0010: `turn.open`). The host takes what was pushed between steps, on its own thread, where it can make what was asked for.
- **A host system whose order of use can be observed has one controller**, so one actor's turns order everything asked of it. Two windows opened, or a window closed and another opened, land in the order the capability's turns asked for them. Two controllers to one such system would be pushed to in an order the interleaving of workers decides, and the same turns would not always leave the same windows. A host system whose requests are independent, such as a pool, needs no such rule.

**The host makes the resource, and the source brings it to the capability.** Between steps, the chassis takes the requests, makes what each asks for, hands it to the engine with `engine.hand`, and pushes the `Given` onto the source's feed in an event:

```rust
// between steps, on the thread that runs the event loop
for request in requests.take() {
    match request {
        Make::Window { id, title } => {
            let window = event_loop.create_window(Window::default_attributes().with_title(title))?;
            feed.push(Made::Window { id, window: engine.hand(Native(window)) })?;
        }
    }
}
engine.bring();
```

**This changes one rule of ADR-0010.** It said an event that holds a resource is dropped. Now an event may hold a `Given` that `engine.hand` made and that no other event or config holds. `bring` writes it as the writer at `Actors` writes a gift, and the resource is on its way to the source's owner, as a resource is in mail; the owner takes it in the turn that handles the event, or it is dropped and recorded in the owner's log, as any `Given` that is not taken is. Any other `Given` in an event is still dropped. The rule was there so that the engine always knows where a resource is (ADR-0010: "a resource is always somewhere"), and that holds: from `hand` until `bring`, the resource is in the engine's keeping, and from `bring`, in transit to an actor. What `hand` made and no event carried at a `bring` stays in the engine's keeping, for a config of a root to come.

**The capability answers whoever asked.** An actor asks the capability for a window, and the capability holds the request until the host has made it (ADR-0017: `turn.hold`, `turn.answer`):

```rust
impl Handles<OpenWindow> for Windows {
    fn handle(&mut self, open: OpenWindow, turn: &mut Turn<'_, ..>) -> Result<(), Fault> {
        let id = self.next();
        self.waiting.push((id, turn.hold()?));
        turn.open(&mut self.control)?.push(Make::Window { id, title: open.title });
        Ok(())
    }
}

impl Handles<Made> for Windows {               // brought from the source: from this actor to itself
    fn handle(&mut self, made: Made, turn: &mut Turn<'_, ..>) -> Result<(), Fault> {
        let Made::Window { id, window } = made;
        let taken = turn.take(window)?;
        let window = turn.give(taken)?;
        turn.answer(self.held(id)?, &Opened { window })
    }
}
```

What the capability gives, and to whom, is the capability's to decide: it may hand the window to the actor that asked, or keep it and answer with a reference to an actor that fronts that one window, as the earlier engine came to.

**A resource is `Send`, so a value that must stay on the host's thread stays with the chassis.** A native window and a GPU device can be moved between threads, and are resources as they are. A value that cannot, such as an audio stream natively, is kept by the chassis, and the resource it hands in is a front for it that can be moved: the producer's end of a buffer that the stream on the host's thread reads. In a browser, the engine runs on the page's one thread, in the pumped mode (ADR-0001), and a build for `wasm32` without atomics has no second thread to move anything to; the browser's values are put in a wrapper that is `Send` in that build alone, `target_arch` used for what it means, a difference in the machine (ADR-0001 rule 1).

**A requested resource that the host cannot make yet is waited on by the host, never by a turn.** The capability holds the request, and the chassis pushes what it made when it has it: once a browser's GPU promise resolves, or once the page sees a gesture and its audio output can play. A host that cannot make it at all pushes an event that says so, and the capability answers with it.

## Consequences

- A window, a device, or an audio output can be asked for at any time a program runs, not only at boot, and arrives in a turn of its capability like any mail.
- The engine adds one resource type, the controller, and changes one rule of what a feed carries. The rest, holding a request and answering it, giving and taking, is ADR-0010's and ADR-0017's.
- A capability is native: a guest cannot open a resource (ADR-0010), so it cannot push to a controller.
- What a capability pushes to its controller acts on the world outside its effects. A turn that pushed and then failed is not undone, and the host still makes what was asked for, as any use of a resource is not undone (ADR-0010). A capability that wants a request to go only when its turn succeeds pushes last.
- A turn that pushes to a controller cannot be run again from its record without pushing again: a replay gives the capability a controller whose requests go nowhere, and feeds it what its source brought from the record.
- The thread that runs the windowing library's event loop is the thread that steps the engine, natively: requests are taken and windows made there, between steps, inside the library's callbacks. The workers run turns on other threads, as before (ADR-0011).
- `Engine::hand` is no longer only for boot. A chassis that hands a value and never puts it in an event or a config leaves it in the engine's keeping until the engine is dropped.
- **Not decided here:** the window capability's own mail: which requests and events it has, and whether each window gets an actor of its own; and the render capability's.

## Alternatives considered

- **Jobs that make resources.** One way in for everything. A job is not an actor, so it cannot be an owner, and it is the factory ADR-0010 rejects; a native window can only be made in the event loop's callbacks; and most browser values cannot cross to a worker (ADR-0018).
- **Requests as an effect the engine hands the chassis at the fold.** A failed turn would ask for nothing, and a record would hold the requests themselves. It is a second road out of the engine beside mail, with a kind of effect of its own, for what a resource that the capability owns already does.
- **A host actor, a mailbox on the host's thread that requests are mailed to.** Requests would be mail. An actor would then run outside the steps, on a thread of the host's, which ADR-0011 has no place for.
- **Every resource made at boot, in a root's config.** Nothing to add. A second window, a device lost and made again, or audio started after a gesture could never be had.
- **Dropping `Send` from `Resource`.** The browser's values would fit as they are. A native host runs an owner on any worker, and a resource goes with it (ADR-0010), so every resource would then have to be kept to one thread.
