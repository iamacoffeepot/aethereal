# ADR-0054: A running guest moves onto its kind's new code

- **Status:** Proposed
- **Date:** 2026-11-10

## Context

A program cannot change a guest's code while it runs. A screen, a tool, or a game rule written as a guest is changed by building the program again and starting it over, which loses everything its actors held. The owner wants a guest's behaviour to be changed live, by publishing new code for it, with no scripting language beside Rust: the module is the script.

What is already decided:

- **A library says which blob a kind is** (ADR-0009). A library is a root; it registers a namespace once, and the kinds named under it are its own. It moves a kind's **head** with the effect `What::Publish { kind, blob }`, refused for a blob that does not make the kind. Everyone else reads the heads as a view folded from `Publish` (ADR-0011), and an actor spawns with the head the view shows it. None of this is built: no `Publish`, no heads, no library.
- **Nothing moves a running actor** (ADR-0009). An actor stays on the blob it was made from, sees that a head moved in a turn of its own, and chooses whether and when to move. How it moves was left to a later ADR. ADR-0009 also rejected moving every running actor when its kind's blob changes, because an actor's code would then change under it at a moment it did not choose.
- **An actor is a kind, and its state crosses `Within`** (ADR-0009, ADR-0004). The engine can take any actor's state out by encoding it and put it back by decoding it, and does so for a child seated in another part (ADR-0015: a state taken out is a `Written`, its bytes and a pin on each blob it refers to, ADR-0014). Handles cross `Within` too, so a state taken out keeps its resources (ADR-0010).
- **Two kinds are the same kind when they have the same name and the same layout** (ADR-0006: `KindId`, `same_layout`). A layout ignores every name inside the schema.
- **A guest's module declares what it makes** in its section `aethereal.declared`: for each actor type, its name, its kind's id, its key, its config's kind, what it handles, and what each handler may send (ADR-0015, ADR-0013: `Declared`). The `guest!` macro writes the section. A guest runs a whole batch of turns in one call, and the call hands back where each turn ended and how the batch ended (ADR-0015).
- **ADR-0015 left open how the engine takes a guest actor's state out to replace its code.**
- **A program's roots are its list, and a running engine gains no root** (ADR-0008, ADR-0029). A guest loaded later is mail to a root the list stood up.
- **`Files` reads named namespaces by checked path** (ADR-0032: `Files`, `Namespace`, `FilePath`, `Load`). `Namespace` is an enum the engine fixes; each host's storage roots it: a directory natively, a URL relative to the page in a browser.
- **A tool is written in the crate it belongs to, and a program lists it** (ADR-0037: `Tool`, `Toolbox`). An actor's log is read with `actor_logs` (ADR-0044: `Log`).
- **Two actors of one kind can run different blobs at once**, and mail of a kind whose layout changed has another id, so it goes unhandled and is never read as something else (ADR-0009, ADR-0006).

## Decision

**A program's code is held by one stock root, `Library`. It reads every guest module through `Files` from a new namespace, `Code`, as the program starts, and publishes their kinds' heads; a system that ships its own modules has them published only for kinds with no head, and the `publish` tool adds or replaces one while it runs. A running guest that is behind its kind's head is asked, before each batch of its turns, whether to move now. When it answers that it does, it moves at once, and the batch runs on its new code; its old state is read by field name into a portable form of its new state, and its new code builds itself from that. A move that fails leaves the actor on its old code, untouched, and the batch runs there.**

```
publish tool ──> Library ── Files: Code/<path> ──> blob ── load_code ── Publish { kind, blob } per kind ──> heads view
each guest of kind K behind K's head and not parked on it, before each batch: upgrade(&self)
  ├─> Later: the batch runs on the old code
  └─> Now: moved at once, on the worker that runs its part:
        state taken out (Written, old schema) ──> new code: same layout? taken as is
                                                         : else Portable<New> by field name ──> take_over
          ├─> Ok: moved ──> the batch runs on the new code, in the same step
          └─> Err: put back, parked ──> the batch runs on the old code
each guest of kind K with no mail as K's head moves: asked once, between steps, and moved there on Now
```

### Libraries

- **`Library` is a singleton root in a crate of its own, `aethereal-library`, and a program has one.** It reads through `Files` and serves tools, so it stands beside the capabilities, not in `aethereal-substrate`. It holds every kind's head and has no children. A program lists it after `Files`, with nothing to set:

  ```rust
  .root::<Files>()
  .root::<Library>()
  ```

- **The right to publish is a handle, `Publisher`, and only the chassis makes one.** It makes it once as the program starts and hands it to `Library` as it bootstraps, as it hands other roots their handles (ADR-0029, ADR-0010). `Publish` is asked with it, `turn.publish(&self.publisher, kind, &blob)`, which answers the number the engine gives the publish, and the heads view is folded only from what it publishes. The engine keeps no record of which actor is the library: the handle is the right, so no other actor changes which code a kind runs, and one program has one library because there is one handle.
- **As it begins, `Library` lists `Code` through `Files`, reads every module there, takes each in as code, and publishes the head of every kind each makes.** A turn takes a blob in as code with `turn.load_code(&blob)`, which answers the kinds its types make. Natively `Files` lists the directory; in a browser it reads a manifest of the modules, written when the page is built (ADR-0032). The reads are mail, answered in later steps, so what keeps `Code` from being published stops the program as it starts, as a refused tool offer does (ADR-0037), logged as the library's naming the modules: two modules that make one kind, since neither can be chosen over the other, a module the guest host refuses (ADR-0015), and a listing or a read that fails. A program with no `Code` publishes nothing from it.
- **A system that owns guests ships their modules and registers them with `Library` by message,** as a tool is offered to its holder (ADR-0037). It builds them into its own code with `include_bytes!`, puts each as a blob, and asks `Defaults { modules }`, answered `Defaulted`. A default fills only a kind that has no head, and never replaces code that has one, whether `Code` made it or a publish since; `Defaulted::Taken` names the kinds it published and the kinds it kept. A library still publishing `Code` holds the ask until it has, so `Code` always comes first. The modules are taken all or none: one the guest host refuses, or two that make one kind, takes none. The program's own modules are then the code a program runs with nothing in `Code`, and a module in `Code` replaces one of them for a build.
- **While it runs, `Library` publishes a module of `Code` for whoever asks it `PublishModule { path }`,** answered `ModulePublished` with each kind and the number of its publish. What can be published is what `Code` holds, which no actor writes; the right to publish any other blob stays with the handle. Each number is the one `turn.publish` answered: the engine's, under which the heads view folds the publish. The library keeps no count of its own.
- **`Namespace` gains `Code`:** read-only, the guest modules a program runs. Natively its root is a setting of `Files`, `code`, beside `assets`; in a browser it is fetched relative to the page, as `Assets` is. A module is built where `Code` points, so a rebuild is a new file to publish.
- **A spawn names a library's kind and takes its head.** `turn.spawn_from` takes an `Origin`: `Origin::Head`, the head the view shows for the child's kind, refused with `Refused::NoHead` when the kind has none; or `Origin::Blob(&code)`, a blob the parent holds, for code no library publishes (tests, the chassis). It is not called `Source`, which is the resource a host brings mail in through (ADR-0010). A parent of a library's kind holds none of its code, so updating that kind is one `Publish` for every parent that makes it.

### Asking a guest to move

- **Every type a guest's module makes implements `Upgrades`, and `guest!` requires it,** so a guest that does not say how it moves does not build. A native actor does not implement it: a native kind's code is the program's own (ADR-0009: `Code::Linked`) and moves only with a rebuild.

  ```rust
  pub trait Upgrades: Actor + Portability {
      /// Asked before each batch of its turns while this actor is behind its kind's head.
      fn upgrade(&self) -> Upgrade;

      /// Builds this version's state from the state the actor had. Called only when the layouts differ.
      fn take_over(old: Portable<Self>) -> Result<Self, TakeOverError> {
          Err(TakeOverError::Unwritten)
      }
  }

  pub enum Upgrade { Now, Later }
  ```

- **An actor is behind when the blob it runs is not its kind's head.** The engine knows both, so nothing is kept in the actor, and nothing is sent to it.
- **Before each batch of an actor that is behind, and not parked on its kind's head, its host asks it,** in a call of its own into its instance, which calls `upgrade`. It takes `&self`, so deciding changes nothing. A batch is the turns an actor takes between two looks at the clock, and a hop is a batch of one (ADR-0015, ADR-0013), so an actor with a burst of mail is asked once for every 32 items, not before each.
  - **`Later`:** the batch runs on the old code, as if nothing were published. The actor is asked again before its next.
  - **`Now`:** the actor moves at once, on the worker that runs its part, with the part's instances it is lent, as a batch's `prepare` is (ADR-0015), and the batch is prepared and run on its new code, in the same step. When the move fails, the actor is parked and the batch runs on its old code.
  - Each batch runs wholly on one version of the code. Nothing moves an actor during a turn, or between two turns of one batch, and nothing an actor does in a turn arranges a move for later.
- **An actor that is behind and has no mail is asked once, between steps, after the head moved,** in the same call, and moved there when it answers `Now`, where the engine holds every part. An idle actor would otherwise never be asked, and never move. Asking it in a step would need the plan to hand out work no mail made, so it stays between steps, where the engine already folds and seats.
- **A head that moves again while an actor is behind changes only where it moves to.** It moves to the head as it stands when it answers `Now`, never to a version in between.

### Moving

- **The engine takes the actor's state out at `Within`, as for a child seated in another part, with the schema of its kind as the old module declares it.** `aethereal.declared` gains each actor type's state schema, its names included (ADR-0004: `SchemaNodes`), which `Declared` reads.
- **The state goes into an instance of the new blob in the same part,** made there if the part has none (ADR-0015). Its children stay where they stand, each on its own blob and its own kind's head.
- **When the old and new kinds have the same id, the state is taken in as it is.** Same name and same layout mean the same bytes (ADR-0006), so `take_over` is not called.
- **When the layouts differ, the new code is handed its portable form.** `#[derive(Portability)]` on the state type writes a struct with the same field names, each an `Option` of the field's type, and links the two through a trait. `Portable<T>` is an alias over that link, so inside an impl for `Hud`, `Portable<Self>` is the generated struct and its fields are plain field accesses. `Upgrades` requires `Portability`.

  ```rust
  pub trait Portability {
      type Form;
      fn read(old: &OldState<'_>) -> Result<Self::Form, TakeOverError>;   // by field name, with the old schema
  }

  pub type Portable<T> = <T as Portability>::Form;

  // what `#[derive(Portability)]` writes for v2's `Hud`
  pub struct PortableHud {
      pub save: Option<Save>,
      pub health: Option<f32>,
      pub show_cooldowns: Option<bool>,
  }

  impl Portability for Hud {
      type Form = PortableHud;
      fn read(old: &OldState<'_>) -> Result<PortableHud, TakeOverError> { .. }
  }

  // written by hand, in `impl Upgrades for Hud`
  fn take_over(old: Portable<Self>) -> Result<Self, TakeOverError> {
      let Some(save) = old.save else { return Err(TakeOverError::missing("save")) };
      let health = old.health.unwrap_or(100.0);

      Ok(Hud { save, health, show_cooldowns: old.show_cooldowns.unwrap_or(false) })
  }
  ```

  - **Each field is filled by name from the old state,** read with the old schema: a field the old state has, of the same layout, is `Some`. A field it lacks is `None`: it is new, and the code gives it a value or works it out from the others. A field the old state has that the new one lacks is dropped with the old state's bytes, and a reference in it with them. A handle in it is not released at the move: a handle has no `Drop`, so its resource stays with its actor until the actor closes (ADR-0010).
  - **A field the old state has with another layout refuses the move,** with `TakeOverError::Mismatch { field }`. A default would quietly throw its value away. To change a field's type, a version names it anew.
  - **Every field is an `Option`, with no required fields.** A required field could never be safely added or removed while an older version may still run; protobuf dropped `required` in proto3 for that reason. A field that must be present is checked in `take_over`, which can refuse.
  - The portable form is built by the schema walk of ADR-0007, without the old Rust type, so a version takes over from any earlier one, however many versions it skips, and an older version takes over from a newer one when a head is moved back. It is read within the engine's read limit, and the handles in it are the ones the `Written` holds.
- **`take_over` runs in a call of its own into the new code's instance, as a `begin` does.** It takes no turn, so it logs nothing itself; the move's lines are logged as the actor's. **Each move logs the fields that were absent,** at `Info`: `moved to publish 12; absent: show_cooldowns`. A field that should never be absent showing there says something went wrong even where a default covered it.
- **A move that fails leaves the actor as it was.** `take_over`'s `Err`, a trap in it, or an unwritten `take_over` for a changed layout puts the taken-out state back into the old instance, and the actor runs on its old code. The error is logged at `Error` as the actor's, with its reason. The actor is **parked**: it is not asked again until its kind's head moves again, since the same code would fail again against nearly the same state, every batch. The remedy is another publish.
- **Moving is not ending.** `end` is not run, and the actor's path, its watchers, and its references are unchanged. Mail of a kind whose layout the new code changed goes unhandled, as between two actors on different versions (ADR-0009).

### Moving within a step

A step's turns read what the engine holds as the step began, and its changes are folded once its turns are done (ADR-0011). A move made before a batch is such a change, so it is held to the same rules.

- **The table learns an actor's new type at the fold,** from an effect the engine writes for it, `What::Moved`, as it learns of every spawn and ending. Until then, the rest of the step resolves, casts, and checks sends to the actor against its type as the step began, as every turn of a step reads the table (ADR-0011, ADR-0013). Mail of a kind the new code does not handle goes unhandled, as between two actors on different versions (ADR-0009). Only the actor's seat, which its own part keeps and the plan reads between steps, takes its new type as it moves, so the plan weighs its next turns by the code it now runs.
- **The actor's own sends are checked against the type whose code ran them.** Its host holds the type it moved onto from the move on, so the batch's sends are checked against what the new code declares, as ADR-0015 checks every send against the handler that ran.
- **Fusing is unchanged.** Whether a send fuses depends on parts, seats, and lives, not on types (ADR-0013), so a moved actor fuses into and out of its part as any other. A hop to an actor that is behind is a batch of one, so it is asked before it.
- **Domains count the head.** ADR-0013 works out each step's domains from the types its waiting items reach. Within a step, a moved actor's new code may send where its old type declared no send. So once anything plans by domain, an actor behind its kind's head is counted under its head's type as well as its own. Nothing plans by domain yet (ADR-0013), so this is the rule that use must keep.
- **A trap in `take_over` ends the new code's instance as a trap in a turn does** (ADR-0015): each actor already living in it ends in the step, and the instance is dropped. The actor that moved stays on its old code, parked. **An actor of a dropped instance runs nothing more.** Its host numbers each instance it makes, and an actor runs only in the instance it lives in: when its part's instance of its code is gone, or is another made since, as a later move or spawn made it again, its later turns in the step fail, and it runs no closing turn, as ADR-0015 holds of an actor whose instance a trap dropped. A trap in a turn is held to the same rule; before moves, only a spawn or a child taken in from another part could make the instance again in the step.

### Seeing a publish

- **Each publish is numbered by the engine, once, as it is asked,** and the number is the head's: `turn.publish` answers with it, and `Publish` carries it into the heads view, which takes it as it folds and counts nothing itself. Every publish of a step is asked by the one actor that holds the publisher, so the number is the view's last as the step began, on by each publish asked before it in the step.
- **The heads view records what became of each guest found behind its kind's head.** Each ask before a batch or between steps is folded into it: a move counts on the head it moved onto, with the fields its old state lacked; an answer of `Later` counts one more batch on the head it was asked about; a failed move parks it there; a guest that has mail as the head moves is recorded as still to be asked, so who has yet to answer is known. A guest is kept until it moves or its life is over. A turn reads it as the step began with `turn.moves()`, as it reads the log (ADR-0044).
- **`aethereal-library` serves two tools, and no other,** behind its `tools` feature. It serves no spawn tool: a guest is spawned by the actor that owns it, from its kind's head.
  - **`publish { path }`** asks `Library` to publish the module at `Code/<path>`, moving the heads of kinds another module made before, and answers as soon as the library has: the module's hash, and each kind it makes with the number of its publish. It has no wait: what became of the guests of those kinds is read when it is wanted, with `upgrades`.
  - **`upgrades {}`** reads the heads view as the step began: each kind's head, with the number of the publish that made it, how many actors have moved onto it, and the fields their old states lacked; and every running guest that is not on its kind's head, still to be asked, behind with how many batches it has answered `Later` before, or parked, with the publish it failed on and why. Read for the kinds a publish made, under the numbers it answered, it is that publish's record:

    ```
    publish 12 · ichor.ui.hud → 3f9a…
      moved     38   absent: show_cooldowns
      later      1   ichor.ui.hud:inventory   answered Later, 3 batches
      parked     1   ichor.ui.hud:party       take_over: save missing
    ```

- **Each move and each failure is in the actor's log,** tagged with the publish number, so `actor_logs` and the dev overlay's console show them as they happen (ADR-0044, ADR-0053).

## Consequences

- **A guest's behaviour can be changed while the program runs,** by building its module and calling `publish`. Its actors keep their state and move before a batch of their own choosing, and `upgrades` says who moved, who answered `Later`, and who failed.
- **A program has one library** (ADR-0009). One library holds every kind's head, rather than a library for each namespace, each the only one that could publish its kinds, so no namespace is registered, and a program lists no module: what is in `Code` runs, and its systems' own modules run for the kinds `Code` does not make.
- **Nothing moves a running actor** (ADR-0009). An actor chooses whether and when it moves, and is never moved during a turn or a batch. The engine asks before each batch, so no actor watches the heads, and none can forget to.
- **This decides what ADR-0015 left open:** a guest's state is taken out to replace its code as it is taken out for another part, and put back when the move fails.
- **A native actor never moves.** A screen meant to change live is written as a guest. ichor's actors are native today; one of its screens changes live once it is a guest.
- **Every guest type writes `upgrade`,** a few lines when it has nothing to wait for. `take_over` is written only when a version changes the state's layout.
- **The declarations of replaced code are kept for the life of the process.** ADR-0015 names guest types by pointers never freed, and left a registry that frees them "until code is put again and again as a program runs". A publish now does that. A declaration is small beside the compiled code, which goes with the last blob of its bytes; whether the declarations a long session of publishing leaves behind need a registry is decided once they are measured.
- **`aethereal.declared` grows by each type's state schema,** read once when a module is put.
- **`turn.spawn_from` takes an `Origin`,** and every caller names `Origin::Blob` or `Origin::Head`.
- **`Files` gains `Code`, and each host's settings gain its root.**
- **Two versions of one kind can run at once** while some actors answer `Later` or are parked. Mail between them is safe, as ADR-0009 already holds: a changed kind is another id.

Follow-on work, in order: `Publisher` and `What::Publish` with the heads view; `Origin` on `spawn_from`; `Namespace::Code`; the state schema in `aethereal.declared`; `Upgrades`, `Upgrade`, and the call that asks in the guest's library and the native host (the browser chassis runs no guests, so it has none to ask); taking a state into a new blob's instance and back; `Portability`, its derive, and `Portable`; `aethereal-library` with `Library`, `publish`, and `upgrades`.

## Alternatives considered

- **The engine moves every running actor of a kind when its head moves.** A publish reaches every actor at once. An actor's code would change under it at a moment it did not choose, which ADR-0009 rejected, and an actor in the middle of a save would be moved mid-save.
- **A message that the code changed, and `turn.upgrade()` when the actor is ready.** The actor then has to remember it owes a move, a second place to keep the same fact. Asking before each batch decides in one place, before anything runs.
- **Asking before each turn, inside the guest's call for its batch, and moving between steps.** An actor would move at the first turn it was ready for, not the first batch. The batch stops before that turn, and its mail, and every later hop to the actor in the step, waits a step for the move, carried over as an actor cut off at its slice is: a batch would end with a new ending, the guest's record of each turn would carry its answer, and the engine would keep a list of actors to move between steps. Asking before the batch needs none of it, and the mail is handled in the step it was sent for.
- **Moving between steps, after a batch answered `Now`.** Nothing changes within a step, but the batch's mail waits a step for the move, and every move waits for the thread between steps, where the parts are not run side by side.
- **A token handed with the message and spent to move.** A type checks that only an actor owed a move can move. The token must be kept in the actor's state, and Rust cannot forbid dropping it, so the engine would have to catch a dropped one; the engine already knows whether an actor is behind.
- **Moving an actor that does not answer.** A default would hide how each actor moves. `guest!` requires the answer, so every guest's behaviour is written down.
- **A typed copy of the previous state, `take_over(old: HudV1)`.** Checked fields, but every version carries a copy of the last one's type, a conversion is written for each pair of versions, and an actor that skipped a version cannot be read.
- **Carrying fields by name with no code.** Nothing to write. A renamed field would silently become its default, which papers over exactly the change that needs a decision.
- **Fields marked `#[new]`, the rest required.** A missing old field would then be an error, not a default. A mark could never come off while any older actor may run, the way `required` could never leave a protobuf message.
- **A field of another layout read as absent.** The move succeeds, and the field's real value is replaced by a default no one chose.
- **A failed `take_over` retried every batch.** The same code fails again against nearly the same state, and the log fills with it.
- **A failed move ends the actor, or begins it again.** A bug in the new version would end actors that work on the old one, or lose their state with no one deciding to lose it; the engine does not keep a config to begin one again.
- **A scripting language beside Rust.** A second language, runtime, and set of bindings, for what a module already does once it can be replaced live.
- **An effect that registers the library, and `Publish` taken from the registered root alone.** The engine would remember one actor as special. A handle carries the right the way every other right is carried, and a program cannot make a second.
- **The engine reads `Code` and serves the tools itself.** No actor holds the right. The core would then do file I/O and name the tool machinery, and the tools must be actors anyway, so one would need the right regardless.
- **A library for each namespace, listing its modules,** as ADR-0009 had it. Ownership by namespace guards libraries in one program from one another, which one library cannot need, and every program would list its namespaces and modules, changing its settings for each new module.
- **A default that replaces what `Code` holds, or the first publish of a kind wins.** A system's module would then take a kind from the module the program was built with, or whichever was asked first would, and which code runs would depend on the order roots begin. A default fills only a kind with no head, and waits for `Code`.
- **Defaults listed in the library's settings, or handed it by the chassis.** Every program would list each system's modules where it lists the library. A system that owns guests knows its modules, so it asks for them as a toolbox offers its tools, and a program that lists the system has them.
- **A publish that waits for its guests to settle,** answering once every running actor of its kinds has moved, is parked, or has answered `Later`, or when a wait ends. A call that waits either polls the heads view, looking again every few milliseconds, or needs the engine to raise an event as a kind settles, which nothing else reads. What it would answer is already in the heads view, kept as each guest is asked, so it is there to query when it is wanted: `publish` answers at once, and `upgrades` reads the moves.
- **A `spawn` tool beside `publish`.** An agent could stand up a guest no actor owns, which nothing would end or reach by a typed path. A guest is spawned by the actor that owns it, from its kind's head.
- **A parent ships its children's code.** Updating a child means rebuilding or re-handing every parent that makes it. A library's head lets a parent name only the kind.
- **Modules checked into the hub's store and sent to the engine.** The store never gives bytes back (ADR-0039), and how a blob crosses to another engine is not decided (ADR-0014). An engine reads its own `Code` through `Files`, as it reads `Assets`, natively and in a browser.
