# ADR-0013: Chains of mail run within a step

- **Status:** Proposed
- **Date:** 2026-10-15

## Context

Turns run in steps, and mail sent in a step is handled in a later one (ADR-0011). A chain of mail, where each turn handles one piece and sends the next, therefore takes one step for each actor it passes through, and each of those steps pays for being a step: planning it, handing out its parts, and folding its views. Measured with two actors passing one piece of mail back and forth on one worker, with the engine in 16 parts, a piece of mail cost about 712 ns. The turns themselves were a small part of that.

A spike ran such a chain's mail inside the step. When a turn's send went to an actor that could take it, the engine handed it over at once and ran the receiver's turn in the same step, up to a budget of hops in each step. On the same program a piece of mail cost about 125 ns. With a budget of 4 hops in a step it cost about 273 ns, and with 64 about 150, so a small budget takes nearly all of the gain.

The spike broke three things this ADR keeps:

- **It fused across parts when a step ran on one worker** (`Engine::run_whole`), and could not when a step was shared. The same program then left different views at the same step depending on how many workers ran it, which ADR-0011 rules out.
- **A fused turn read the table as it stood when the step began.** A turn later in the chain could not find a child a turn earlier in the chain had spawned, though in a step of its own it would have.
- **It trusted that the actors it fused sent only to one another.** Nothing checked it.

Work that is not a benchmark spawns and closes actors in the middle of chains. So a chain has to see what it did itself, not stop at the first spawn.

## Decision

**A send may be fused: handed to its receiver at once, and the receiver's turn run in the same step.** A chain of fused sends is a run. A send is fused only when all of these hold, and otherwise goes out as mail, as today:

- the receiver sits in the same part as the sender, and is seated;
- the part has hops left in the step's budget;
- the send is the first its turn makes. A turn that sends several pieces of mail fuses at most the first, and the rest go out as mail. A chain fuses and a burst spreads, so a fan-out still reaches the workers;
- the sender has sent no mail as mail, in this step, to an actor in its part. A hop runs in the step it was made and mail in a later one, so a hop after such mail could be handed over ahead of it, and mail from one source would no longer be handled in the order it was sent (ADR-0009). Mail to an actor in another part never fuses, so it cannot be overtaken;
- the send is made by a turn handling an item: a closing turn's sends go out as mail;
- the receiver has not been cut off at its slice in the step (ADR-0011). A hop to it would be handed ahead of what it carries over; a hop made to it before it was cut off is carried over after what it carries, in the order made.

A part always runs on exactly one worker, whichever it is, so every one of these is the same however many workers run the step, but for where an actor is cut off at its slice, which happens only on more than one (ADR-0011).

**A part runs every item that was waiting before any fused hop.** Its waiting items run in the order ADR-0011 gives, then its closing turns, then its fused hops. The hops wait in a queue, each joining the back as it is made, and run from the front, so every chain a waiting item began moves on before any moves twice, and two hops from one sender to one receiver are handled in the order they were sent. Each waiting item whose send fuses begins a run of its own, so a part runs as many runs in a step as its waiting items begin, sharing its budget. Mail an actor sent in an earlier step comes before anything it fuses in this one, so mail from one source is still handled in the order that source sent it (ADR-0009).

**The hop budget is a setting of the engine**, given to `Engine::new` beside its read limit and the bytes of mail it holds. It bounds how many fused hops one part runs in one step, so a step still ends after bounded work, and one chain cannot hold back every other actor's next step.

**A run sees the table changes it made.** A fused turn reads the table as it stood when the step began, as every turn does (ADR-0011), and over it the spawns and endings that the turns of its own run have made. Its own run starts at the turn of a waiting item that made a fused send. A run is named by the first send it fused, so two runs never share what they see, even when they share a part or a chain's root. A turn of another run, or of no run, does not see them until the step is folded. Two consequences follow:

- **A child a run stood up is live to that run.** It is found by a resolve in every later turn of the run, once the turn that stood it up has finished without failing. ADR-0009's promise holds: a resolve never finds an actor whose `begin` failed. To everyone else the child is live from the next step, as today.
- **An actor that ends in a run is ended to that run.** A later turn of the run does not resolve it. An actor that has ended handles nothing more, so a fused hop to it is dropped, as mail to it would be. Its closing turn runs in the next step (ADR-0011).

The other views are not read by a turn, so their records wait for the step's fold, as now.

**A child is seated when the step that stood it up is applied**, as today, wherever its seat is, so mail to it in that step goes out as mail.

**A child is seated in its parent's part while that part has room.** Room means fewer actors than twice what an even spread would give each part, counting at least one, as the step began. A parent and the child it spawned are the likeliest partners in a chain, and this keeps them in one part without piling a large family into one.

**Each part gives the seats of the children its turns spawn, and what it gives depends on the parts alone.** Whether a send fuses depends on which part its receiver sits in, so placement must be the same however many workers run the step. Seats given out by worker, each worker numbering new seats in every few parts by its place among the workers, would put the same child in a different part on a different number of workers. So each part keeps its own seats, its free ones and its next new number, and as a step begins the engine gives each part that runs its room, and makes up to eight the seats it sets aside for that part in the other parts that have room, one in each in turn from the part after it. A child takes a seat of its parent's part while there is room, then one set aside, then one of its parent's part again. A seat set aside stays with the part it was set aside for until a child takes it. A root sits in the part with the fewest actors, and the children its `begin` stands up take that part's seats while it has room, then a new seat in the part that has numbered fewest.

**Every actor type declares what it handles, and no handler declares what it may send.** An actor type lists what it handles: kinds of mail, each once, whose handler states who may send it the kind, which the engine checks where the mail is delivered (ADR-0058); the ending of an actor it watches; and, when it wants one, a fallback for mail of any kind it does not list. Each has a handler of its own, and the list is collected from the handlers of the type's one `#[actor]` block, so it names every handler and nothing else (ADR-0059). A send is checked at its call against what it names: an actor of a type, which handles the kind; any actor whose type lists all of some kinds, which is what a reference that names a protocol, and not a type, resolves to, and a fallback does not count as listing a kind; and, at a typed path, a root the sender's type depends on (ADR-0059).

A watch is not a send: a turn may watch an actor when its actor's type handles endings, as the ending is handed to that handler, which the compiler checks at the watch (ADR-0022, which types a turn by its actor).

```rust
#[actor(parent = Pinger)]
impl Ponger {
    fn begin(_: Number) -> Result<Self, Fault> {
        Ok(Self)
    }

    fn end(self, turn: &mut Ending<'_, Self>) -> Result<(), Fault> {
        turn.to(turn.parent()).send(&Done)?;

        Ok(())
    }

    #[tell]
    fn ping(&mut self, _: Ping, turn: &mut Turn<'_, Self, ActorRef<Pinger>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&Pong)?;   // it may watch whoever pinged it, as it handles Ended

        Ok(())
    }

    #[ended]
    fn ended(&mut self, ended: Ended) -> Result<(), Fault> { .. }      // when it watches

    #[others]
    fn others(&mut self, mail: Mail<'_>) -> Result<(), Fault> { .. }   // its fallback
}
```

**The declaration is read from the handlers, so the two cannot disagree.** A handler is handed a turn typed by its actor's type and its guard (ADR-0022, ADR-0058), and a send through it builds only when its target takes the kind (ADR-0059), nor does a watch build by an actor whose type does not handle endings, as its watcher could never be handed one. The checks a declaration's constants make run on each declaration the program uses, so it stops a build, not a check of types alone, and passes over a type nothing runs. A declaration written beside the code it describes, as a list the engine reads apart from the handler, would be a claim nothing ties to that code. The engine reads the same declaration as plain data, made from those types when the program is built: for each handler, what it handles. There is no undeclared type, since a type is written as its handlers, and the declaration is the one way an actor is written.

**What the types cannot see, the engine checks where the receiver is found.** A native actor sends through a target a turn's `to` makes (ADR-0016): a reference proves its receiver's type when it is made, by the spawn, resolve, or cast that saw it, and a path or a place relative to the sender is checked as the target is made, a place that holds another type being as an empty one (ADR-0022). **Nothing is checked at the send as it runs.** A send by name would need a check at each send, its receiver's type against the kind each time, a mismatch stopping the engine; but every name such a send would take (a parent, a child, a sender, or an erased reference) has a typed target, so there is no `To` and no send by name. Mail is handed to the handler of its kind, or to the fallback when the receiver lists no handler for it. Mail its receiver neither lists nor falls back on is a false declaration. In a native actor it is a bug in the program, and the engine panics, naming the actor's type and the kind. A declaration is a promise the engine relies on, as an id is (ADR-0008), and a run built on a false one would not leave the views the program's turns imply. A false watch does not build. A guest's host holds its records to its declaration, so a guest that sends a kind its receiver does not list, sends at a typed path past its `depends`, or watches past what it handles, which only one not built from the guest library can, fails that turn (ADR-0015).

**A guest declares the same way.** Its handlers are generated from the same impls in the guest's code, and its module carries the declaration as data the engine reads when it loads the module. The engine hands an item to a guest's handler by its number, through the guest's one export, and when it reads the effects the guest's turn wrote it repeats the checks a native build makes of each send: its receiver lists the kind, and a send at a typed path reaches a root of the type's `depends`; the receiver's guard is checked at delivery, as for any sender (ADR-0015, ADR-0058). They are repeated because a guest may not have been compiled with the library, so a false declaration by a guest fails the turn with a fault, and does not stop the engine. They are not a security boundary: access control between actors is not decided here (ADR-0059).

## Consequences

- A chain of mail between actors in one part costs about one turn for each hop, up to the budget, and about one step for each budget's worth of hops.
- A run is invisible to the rest of the engine until the step is folded. A turn outside the run sees nothing of it during the step, as it saw nothing of other turns in the step before (ADR-0011).
- What a fused turn sees differs from what it would have seen in a step of its own in one way: it does not see what unrelated actors did earlier in the step. ADR-0009 already says a resolve made while handling unrelated mail can miss an actor about to be live, and an actor that must know waits to be told.
- Whether a chain fuses depends on placement. A chain between actors in different parts runs one step for each hop, on any number of workers. The 712 to 125 ns measured above was across parts on one worker, so it is what this gives for a chain whose actors share a part.
- The chains a part's waiting items begin share its budget, so many chains in one part each move a few hops in a step, where one chain alone would move many. An actor that sends a burst into its own part fuses nothing more until the next step.
- Fusing saves steps, and leaves the work of each piece of mail as it was: a fused hop costs about what mail does, without its share of a step. Where a step holds a great deal of mail, that share is small, so fewer steps take about as long; where it holds little, as for a lone chain, the share is most of what mail costs.
- A part keeps up to eight seats in other parts set aside, unused until it spawns past its room.
- Every actor type declares its handlers, and every send is checked at its call. The types catch a native actor's false send when the program is built; mail no handler takes stops the engine, and a guest's false send fails its turn, as its host checks it.
- A handler is handed its kind already read, so the handler of a kind cannot be handed mail of another. Mail of a kind the receiver neither lists nor falls back on stops the engine at delivery.
- **Not decided here:** seating a child at once in its parent's part, in the step that stood it up, so a run could mail it fused; domains, the sets of actors whose turns in a step can affect one another, learned from traffic as mail is routed and not from declarations (ADR-0059), and domains with steps of their own; using the time a step would spend waiting at its end, and not a fixed count, as the budget; and references that can be used only by the actor they are sent to and passed no further.

## Alternatives considered

- **Fusing across parts when a step runs on one worker.** What the spike did. The views at a given step then depend on how many workers ran it.
- **A declared group of actor types that send only to one another, checked actor by actor.** Two instances of the group's types mailing one actor is ordinary, and a check that one actor was mailed only by its own run stopped correct programs. Order between two sources is promised nothing (ADR-0009), so mail from outside a run breaks nothing.
- **One set of table changes for each part.** Whether a turn found an actor would then depend on which runs shared its part, and placement would change what a program does.
- **Stopping a run at the first spawn or ending.** Simple, and correct, but the chains of real work spawn and close, and would stop fusing where they matter.
- **A declaration beside one `handle` that takes every item.** The engine would read a list of what the actor handles, and nothing would tie that list to the code that handles it. Every false declaration would be found only at delivery, at run time.
- **A type that declares nothing, whose mail is never checked.** Two ways to write an actor, and a type that opts out of the check is one whose mail the engine cannot hand to a handler by its kind.
- **The host handing the engine its groups between steps.** The engine has no step where it learns its types, so one would have to be added, and what a host hands over can disagree with the code it describes.
- **Running ahead and undoing what proves wrong.** It needs every view to be undone, for a gain the rules above already take.
