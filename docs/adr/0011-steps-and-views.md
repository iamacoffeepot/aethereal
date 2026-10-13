# ADR-0011: Turns run in steps, and shared state is views folded from their effects

- **Status:** Proposed
- **Date:** 2026-10-13

## Context

Actors share nothing, and yet the engine has state that many of them read: which actors there are, where each kind's head points, whether everything one piece of mail set in motion has finished. Each of these is changed by many actors' turns and read by many others.

The earlier engine built a structure for each. Each needed its own layout, its own rule for who writes, its own argument for why a reader is safe, and its own tests, and the structures changed at different moments from one another (ADR-0008). Whoever needed the next piece of shared state had to invent another.

The first piece built here went the same way. The table of actors (ADR-0008's `ActorTable`) numbered its changes from one count so that a reader could hold a number. Measured on 32 threads, that count let fewer actors be spawned in a second than one thread managed alone. Two repairs were built and measured, and each was a further invention for this one table.

A turn already says everything it wants of the engine as a list of effects (ADR-0009's `What`). That list is a record of what happened.

## Decision

**There is one way to share state: turns leave effects, a view is folded from them, and a turn reads the view.** Nothing else is shared between actors, and no view is built by hand around a lock or an atomic.

**Turns run in steps.**

- Every actor that has mail when a step begins runs in that step. They run side by side on the engine's workers, and no turn in a step sees what another turn in it did.
- An actor handles the mail it had when the step began, in the order ADR-0009 gives, one turn for each piece.
- Mail sent in a step is handled in a later one.
- A step has a number, and steps are numbered upward from one count that only the engine advances, once a step.

**A view is a fold over the effects of finished turns.** Its author writes two things: what the view keeps of an effect, and how one such record changes the view's state. The author writes no lock and no atomic, and nothing about workers.

```rust
pub trait View {
    type Shard;                 // one part of the view's state
    type Record;                // what the view keeps of an effect

    /// The key and record of an effect this view folds, or nothing for one it does not.
    fn takes(effect: &Effect) -> Option<(u64, Self::Record)>;

    /// Folds one record into the part its key falls in.
    fn fold(shard: &mut Self::Shard, key: u64, record: Self::Record);
}
```

**Only the engine defines a view, for now.** The views are the ones the engine itself needs, and actors share everything else by mail. The engine's own views go through the interface above, so that a view defined outside the engine is later an addition.

**The engine keeps each view, and it does the same for every one.**

- While a step's turns run, they read the view and nothing writes it.
- When the step's turns have finished, their effects are folded into the view, and then the next step begins.
- Reading and folding never happen together, so the view needs no lock, and the program is checked for that when it is built.
- A view's keys are spread over parts, and the parts are folded side by side.

**A turn reads a view as it stood when its step began.** The view is not written while turns run, so every read in a turn agrees with every other. A view is one step behind: a turn never sees an effect of its own step.

**The step's number is the position of every view.** A record of a turn holds its step's number, and that number is all it needs to read the same views again.

**Who may write a key is limited, so that order between actors never decides a view.**

- A key has one owner, and only its owner's effects change it. The owner's records reach the fold in the order it asked for them. A child's place belongs to its parent, and a kind's head to its library (ADR-0008, ADR-0009), so these are settled where the effect is asked for.
- Or many actors write one key, through a fold that gives the same result in whatever order their records arrive: a count, a set.
- A key that many actors write and the last one wins is not allowed.

**Mail that reaches an actor in one step is put in a fixed order**: by its sender, then in the order that sender sent it. Mail from one source keeps its order, as ADR-0009 requires. Between two sources the rule promises an actor nothing, and the fixed order is there so that the same turns always leave the same result.

**The table of actors is the first view.** It is folded from spawns and endings. Where a library's heads point is a view (ADR-0009). Whether a chain of mail has finished, and who watches an actor, are views when they are built.

**When a step's turns have finished, the engine folds, then delivers, then tells.** It folds the step's effects into the views. It then delivers the step's mail against the views as just folded, and mail to an actor that has ended is not delivered. It then tells the watchers of each actor that ended. So a child spawned in a step is there for mail sent to it in that step, and a watch placed in the step its actor ends is told once.

**An actor that ends handles no more mail.** Mail it had when it ended and had not yet handled is not delivered, as mail that reaches it later is not. An actor that wants to finish what it was sent closes on a piece of mail that asks it to: everything queued ahead of that mail has been handled by then.

**How many workers a step wakes follows from the work it holds**, not from how many turns it has: the cost of its turns as measured so far, against the measured cost of waking a worker.

## Consequences

- Shared state has one shape. The next view is a fold, and it takes no new argument about threads.
- A view's author cannot get the sharing wrong, because the author never touches it.
- The same turns leave the same views and the same actors, however many workers ran them. A prototype with three views gave one result on one thread and on 32, in every run.
- A turn cannot see what happened in its own step. An actor that must know of something waits for a later step or is told by mail.
- A turn cannot test and take in one move. Two actors that both read a name as free in one step both ask for it, the fold picks one by a fixed rule, and the other is told later.
- Each view costs a fold of each record and a share of the time between steps. In a prototype that folded each record twice, three views cost nothing measurable where a turn takes tens of microseconds, 10 to 17 parts in a hundred where it takes a few, and 23 to 45 where it takes a fraction of one.
- A step ends when its slowest turn ends. One long turn holds back every actor's next step.
- A chain of mail takes one step for each actor it passes through.
- The count of changes in ADR-0008's table goes, and so does a reader holding a number: the step's number is the position.
- ADR-0009 said no worker waits for the others and there is no moment when all of them stop. Between two steps there is such a moment.
- **Not decided here:** in which step the closing turns of an ending actor and of the actors beneath it run; domains of actors that never meet, each with steps of its own, so that a slow turn in one does not hold back another; and how the cost of a wake is kept true when few workers are being woken.

## Alternatives considered

- **Numbering every change to one shared table, a reader holding a number.** One structure, built and measured. A number may be read only once every change before it is written, so writers wait for one another: on 32 threads, fewer spawns in a second than on one.
- **The same count with writers noting what they are still writing.** No writer waits. About three times the code of the count it replaced, for one table, and nothing a second view could use.
- **A count for each worker, carried in every piece of mail.** No shared count at all. Every piece of mail grows by a number for each worker, every turn merges them, and a turn sees an actor only once some mail connects it to that actor.
- **Each actor keeping its own view, sent every other actor's effects.** Nothing shared. Explored over a dozen small arrangements of actors: an actor can hold a reference before it knows the actor exists, learn of a close before the spawn, and, if spawned late, never learn that an actor closed.
- **Two copies of each view, one read while the other is folded, changing places at the step.** A fold could then run while the step's turns are still running. Each record is folded twice and each view is kept twice, and the copy being written is shared between threads with no lock to guard it. It gains time only where workers stand idle while a few long turns finish. A view's author would not see the change, so it can be taken up later.
- **No steps, each shared structure built for its use.** The fastest for any one structure. It is the earlier engine's way, and each new structure is a new invention.
