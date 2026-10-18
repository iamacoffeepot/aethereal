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

- the send is one that the handler it was sent from declares, as every send is;
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

**Every actor type declares what it handles, and each of its handlers declares what it may send.** An actor type lists what it handles: kinds of mail, the ending of an actor it watches, and, when it wants one, a fallback for mail of any kind it does not list. Each has a handler of its own, and each handler, the type's `begin`, and its `end` lists what it may send: a kind, and where it goes. Where it goes is one of three things:

- **a type**: an actor of that type, which handles the kind;
- **the sender**: the source of the item being handled;
- **whoever handles some kinds**: any actor whose type lists all of them. This is what a reference that names a protocol, and not a type, resolves to. A fallback does not count as listing a kind.

A handler, `begin`, or `end` that watches an actor lists that too, as `Watches`, beside its sends: a watch is the other thing a turn asks for that reaches another actor.

```rust
impl Actor for Ponger {
    type Handles = (Ping, Ended, Others);   // Ended when it watches, Others for its fallback
    type BeginSends = ();
    type EndSends = (ToType<Pinger, Done>,);
    // begin, end
}

impl Handles<Ping> for Ponger {
    type Sends = (ToSender<Pong>, Watches);   // it may watch whoever pinged it
    fn handle(&mut self, ping: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        turn.reply(&Pong)
    }
}
```

**The declaration is the handler's own type, so the two cannot disagree.** A handler is handed a turn typed by what it declares, and that turn offers only those sends: a send to a type its handler does not list for that kind, a reply it does not list, a send to a protocol it does not list, or a watch it does not list does not build. A type that lists a watch anywhere and does not handle endings does not build either: its declaration, made when the program is built, is checked there, as its watcher could never be handed an ending. The check runs on each declaration the program uses, so it stops a build, not a check of types alone, and passes over a type nothing runs. A declaration written beside the code it describes, as a list the engine reads apart from the handler, would be a claim nothing ties to that code. The engine reads the same declaration as plain data, made from those types when the program is built: for each handler, what it handles and what it may send. There is no undeclared type, since a type is written as its handlers, and the declaration is the one way an actor is written.

**What the types cannot see, the engine checks at the send.** A reference names an actor whose type is known only at run time, so a send to a type checks that the receiver is of that type, and a send to whoever handles some kinds checks that the receiver's type lists them. Mail is handed to the handler of its kind, or to the fallback when the receiver lists no handler for it. A send that breaks the declaration, and mail its receiver neither lists nor falls back on, is a false declaration. In a native actor it is a bug in the program, and the engine panics, naming the actor's type, its handler, and the send. A declaration is a promise the engine relies on, as an id is (ADR-0008), and a run built on a false one would not leave the views the program's turns imply. A false watch does not build. A guest's host holds its records to the same declaration, so a guest that sends or watches past it, which only one not built from the guest library can, fails that turn (ADR-0015).

**A guest declares the same way.** Its handlers are generated from the same impls in the guest's code, and its module carries the declaration as data the engine reads when it loads the module. The engine hands an item to a guest's handler by its number, through the guest's one export, and checks each send when it reads the effects the guest's turn wrote (ADR-0015). A guest is code the engine loaded and may not trust, so a false declaration by a guest fails the turn with a fault, and does not stop the engine.

**What the declarations make possible: domains, worked out each step.** When a type is first seated, the engine numbers each of its handlers and works out once which handlers each one can reach by following declared sends. When a step is planned, each item waiting is a starting point, the handler of its receiver's type for its kind. Starting points whose reach shares an actor type belong to one domain, since a type's handlers share its actors' state. A domain is the set of actors whose turns in that step can affect one another. Domains are worked out again every step from what is waiting, so two domains that a burst of traffic joined split again once it stops. Fusing does not need them: the rules above are enough for it. They are what planning by domain needs, and what domains with steps of their own would stand on. The engine works them out once something in it uses them.

## Consequences

- A chain of mail between actors in one part costs about one turn for each hop, up to the budget, and about one step for each budget's worth of hops.
- A run is invisible to the rest of the engine until the step is folded. A turn outside the run sees nothing of it during the step, as it saw nothing of other turns in the step before (ADR-0011).
- What a fused turn sees differs from what it would have seen in a step of its own in one way: it does not see what unrelated actors did earlier in the step. ADR-0009 already says a resolve made while handling unrelated mail can miss an actor about to be live, and an actor that must know waits to be told.
- Whether a chain fuses depends on placement. A chain between actors in different parts runs one step for each hop, on any number of workers. The 712 to 125 ns measured above was across parts on one worker, so it is what this gives for a chain whose actors share a part.
- The chains a part's waiting items begin share its budget, so many chains in one part each move a few hops in a step, where one chain alone would move many. An actor that sends a burst into its own part fuses nothing more until the next step.
- Fusing saves steps, and leaves the work of each piece of mail as it was: a fused hop costs about what mail does, without its share of a step. Where a step holds a great deal of mail, that share is small, so fewer steps take about as long; where it holds little, as for a lone chain, the share is most of what mail costs.
- A part keeps up to eight seats in other parts set aside, unused until it spawns past its room.
- Every actor type declares its handlers, and every send is checked. The types catch most false declarations when the program is built; the rest stop the engine, or fail a guest's turn, at the send.
- A handler is handed its kind already read, so the handler of a kind cannot be handed mail of another. Mail of a kind the receiver neither lists nor falls back on stops the engine at delivery, where a send to the sender or a reference could not be checked before.
- The reach of each handler is worked out again whenever a new type is first seated, which happens between steps.
- A reference that names a protocol reaches every type that handles that protocol's kinds, so domains are as precise as the declarations are, and a reference whose target type is unknown makes them coarse.
- **Not decided here:** seating a child at once in its parent's part, in the step that stood it up, so a run could mail it fused; domains with steps of their own; using the time a step would spend waiting at its end, and not a fixed count, as the budget; following where references go kind by kind, from what each send declares it carries; and references that can be used only by the actor they are sent to and passed no further, which would make that following unnecessary for them.

## Alternatives considered

- **Fusing across parts when a step runs on one worker.** What the spike did. The views at a given step then depend on how many workers ran it.
- **A declared group of actor types that send only to one another, checked actor by actor.** Two instances of the group's types mailing one actor is ordinary, and a check that one actor was mailed only by its own run stopped correct programs. Order between two sources is promised nothing (ADR-0009), so mail from outside a run breaks nothing.
- **One set of table changes for each part.** Whether a turn found an actor would then depend on which runs shared its part, and placement would change what a program does.
- **Stopping a run at the first spawn or ending.** Simple, and correct, but the chains of real work spawn and close, and would stop fusing where they matter.
- **A declaration beside one `handle` that takes every item.** The engine would read a list of what the actor sends, and nothing would tie that list to the code that sends. Every false declaration would be found only at a send, at run time.
- **A type that declares nothing, whose sends are never checked or fused.** Two ways to write an actor, and a type that opts out of the check is a type the engine can plan nothing about.
- **The host handing the engine its groups between steps.** The engine has no step where it learns its types, so one would have to be added, and what a host hands over can disagree with the code it describes.
- **Running ahead and undoing what proves wrong.** It needs every view to be undone, for a gain the rules above already take.
