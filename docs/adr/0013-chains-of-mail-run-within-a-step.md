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

- the sender's type declares its handlers, and the send is one that the handler it was sent from declares;
- the receiver sits in the same part as the sender, and is seated;
- the part has hops left in the step's budget;
- the part has no fused hop waiting to run. A turn that sends several pieces of mail fuses at most the first, and the rest go out as mail. A chain fuses and a burst spreads, so a fan-out still reaches the workers.

A part always runs on exactly one worker, whichever it is, so every one of these is the same however many workers run the step. The same turns leave the same views on one worker and on many (ADR-0011).

**A part runs every item that was waiting before any fused hop.** Its waiting items run in the order ADR-0011 gives, then its fused hops in the order they were made. Mail an actor sent in an earlier step comes before anything it fuses in this one, so mail from one source is still handled in the order that source sent it (ADR-0009).

**The hop budget is a setting of the engine**, given to `Engine::new` beside its read limit and the bytes of mail it holds. It bounds how many fused hops one part runs in one step, so a step still ends after bounded work, and one chain cannot hold back every other actor's next step.

**A run sees the table changes it made.** A fused turn reads the table as it stood when the step began, as every turn does (ADR-0011), and over it the spawns and endings that the turns of its own run have made. Its own run starts at the turn that made its first fused send. A run is named by the first send it fused, so two runs never share what they see, even when they share a part or a chain's root. A turn of another run, or of no run, does not see them until the step is folded. Two consequences follow:

- **A child a run stood up is live to that run.** It is found by a resolve, and a reference to it is proven, in every later turn of the run, once the turn that stood it up has finished without failing. ADR-0009's promise holds: a resolve never finds an actor whose `begin` failed. To everyone else the child is live from the next step, as today.
- **An actor that ends in a run is ended to that run.** A later turn of the run does not resolve it. An actor that has ended handles nothing more, so a fused hop to it is dropped, as mail to it would be. Its closing turn runs in the next step (ADR-0011).

The other views are not read by a turn, so their records wait for the step's fold, as now.

**A child seated in the part of the turn that stood it up is seated at once**, so a run can mail it fused. A child seated anywhere else is seated when the step is applied, as today, and mail to it goes out as mail.

**A child is seated in its parent's part while that part has room.** Room means fewer actors than twice what an even spread would give each part, counting at least one. A child spawned anywhere else is spread across the parts, as today. A parent and the child it spawned are the likeliest partners in a chain, and this keeps them in one part without piling a large family into one. Seat counts are the same however many workers run, so placement is too.

**An actor type declares its handlers, as data the engine reads with the type.** For each kind of mail it handles, the declaration lists what that handler may send: a kind, and where it goes. Where it goes is one of three things:

- **a type**: an actor of that type;
- **the sender**: whoever sent the item being handled;
- **whoever handles some kinds**: any actor whose type handles all of them. This is what a reference that names a protocol, and not a type, resolves to.

An actor type that declares nothing is undeclared. Its sends are never fused and never checked, and it behaves as today. How authors write a declaration is the concern of the layer above the engine. That layer fills in this data and does not change what the engine does with it.

**The engine checks each send of a declared type against its declaration**, at the send. A send its handler does not declare is a false declaration, and the engine panics, naming the actor's type, the handler, and the send. A declaration is a promise the engine relies on, as an id is (ADR-0008), and a run built on a false one would not leave the views the program's turns imply.

**What the declarations make possible: domains, worked out each step.** When a type is first seated, the engine numbers each of its handlers and works out once which handlers each one can reach by following declared sends. When a step is planned, each item waiting is a starting point, the handler of its receiver's type for its kind. Starting points whose reach shares an actor type belong to one domain, since a type's handlers share its actors' state. A domain is the set of actors whose turns in that step can affect one another. Domains are worked out again every step from what is waiting, so two domains that a burst of traffic joined split again once it stops. Fusing does not need them: the rules above are enough for it. They are what planning by domain needs, and what domains with steps of their own would stand on. The engine works them out once something in it uses them.

## Consequences

- A chain of mail between actors in one part costs about one turn for each hop, up to the budget, and about one step for each budget's worth of hops.
- A run is invisible to the rest of the engine until the step is folded. A turn outside the run sees nothing of it during the step, as it saw nothing of other turns in the step before (ADR-0011).
- What a fused turn sees differs from what it would have seen in a step of its own in one way: it does not see what unrelated actors did earlier in the step. ADR-0009 already says a resolve made while handling unrelated mail can miss an actor about to be live, and an actor that must know waits to be told.
- Whether a chain fuses depends on placement. A chain between actors in different parts runs one step for each hop, on any number of workers. The 712 to 125 ns measured above was across parts on one worker, so it is what this gives for a chain whose actors share a part.
- Seating a child in the middle of a step is new. The worker that owns the part seats it, as it already owns the part's actors for the step. A worker has to be able to give out a seat in its own parts.
- An actor type that wants its chains fused has to declare its handlers, and a false declaration stops the engine. An undeclared type loses nothing it has today.
- The reach of each handler is worked out again whenever a new type is first seated, which happens between steps.
- A reference that names a protocol reaches every type that handles that protocol's kinds, so domains are as precise as the declarations are, and a reference whose target type is unknown makes them coarse.
- **Not decided here:** what an undeclared type does to domains; domains with steps of their own; using the time a step would spend waiting at its end, and not a fixed count, as the budget; following where references go kind by kind, from what each send declares it carries; and references that can be used only by the actor they are sent to and passed no further, which would make that following unnecessary for them.

## Alternatives considered

- **Fusing across parts when a step runs on one worker.** What the spike did. The views at a given step then depend on how many workers ran it.
- **A declared group of actor types that send only to one another, checked actor by actor.** Two instances of the group's types mailing one actor is ordinary, and a check that one actor was mailed only by its own run stopped correct programs. Order between two sources is promised nothing (ADR-0009), so mail from outside a run breaks nothing.
- **One set of table changes for each part.** Whether a turn found an actor would then depend on which runs shared its part, and placement would change what a program does.
- **Stopping a run at the first spawn or ending.** Simple, and correct, but the chains of real work spawn and close, and would stop fusing where they matter.
- **The host handing the engine its groups between steps.** The engine has no step where it learns its types, so one would have to be added, and what a host hands over can disagree with the code it describes.
- **Running ahead and undoing what proves wrong.** It needs every view to be undone, for a gain the rules above already take.
