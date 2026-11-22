# ADR-0054: A running guest moves onto its kind's new code

- **Status:** Proposed
- **Date:** 2026-11-10

## Context

A program cannot change a guest's code while it runs. A screen, a tool, or a game rule written as a guest is changed by building the program again and starting it over, which loses everything its actors held. The owner wants a guest's behaviour to be changed live, by publishing new code for it, with no scripting language beside Rust: the module is the script.

What is already decided:

- **A library says which blob a kind is** (ADR-0009). A library is a root; it registers a namespace once, and the kinds named under it are its own. It moves a kind's **head** with the effect `What::Publish { kind, blob }`, refused for a blob that does not make the kind. Everyone else reads the heads as a view folded from `Publish` (ADR-0011), and an actor spawns with the head the view shows it.
- **Nothing moves a running actor** (ADR-0009). An actor stays on the blob it was made from, sees that a head moved in a turn of its own, and chooses whether and when to move. How it moves was left to a later ADR. ADR-0009 also rejected moving every running actor when its kind's blob changes, because an actor's code would then change under it at a moment it did not choose.
- **An actor is a kind, and its state crosses `Within`** (ADR-0009, ADR-0004). The engine can take any actor's state out by encoding it and put it back by decoding it, and does so for a child seated in another part (ADR-0015: a state taken out is a `Written`, its bytes and a pin on each blob it refers to, ADR-0014). Handles cross `Within` too, so a state taken out keeps its resources (ADR-0010).
- **Two kinds are the same kind when they have the same name and the same layout** (ADR-0006: `KindId`, `same_layout`). A layout ignores every name inside the schema.
- **A guest's module declares what it makes** in its section `aethereal.declared`: for each actor type, its name, its kind's id, its key, its config's kind, what it handles, and the root types it depends on (ADR-0015, ADR-0013: `Declared`; ADR-0059). The `guest!` macro writes the section. A guest runs a whole batch of turns in one call, and the call hands back where each turn ended and how the batch ended (ADR-0015).
- **ADR-0015 left open how the engine takes a guest actor's state out to replace its code.**
- **A program's roots are its list, and a running engine gains no root** (ADR-0008, ADR-0029). A guest loaded later is mail to a root the list stood up.
- **`Files` reads named namespaces by checked path** (ADR-0032: `Files`, `Namespace`, `FilePath`, `Load`). `Namespace` is an enum the engine fixes; each host's storage roots it: a directory natively, a URL relative to the page in a browser.
- **A tool is written in the crate it belongs to, and a program lists it** (ADR-0037: `Tool`, `Toolbox`). An actor's log is read with `actor_logs` (ADR-0044: `Log`).
- **Two actors of one kind can run different blobs at once**, and mail of a kind whose layout changed has another id, so it goes unhandled and is never read as something else (ADR-0009, ADR-0006).
- **A reference proves what it reaches once, when it is made, and nothing checks it again** (ADR-0016: `ProtocolRef`, `turn.cast`; ADR-0058: `Sender::speaking`). A guest moved onto code that no longer handles a kind a reference to it was proven for leaves that reference proving what is not true: mail sent through it goes unhandled, and a request through it is answered `Unreadable` (ADR-0017).
- **A native actor's kinds and protocols are the program's own.** They change only when the program is built again and started over, so what a native actor was proven to take holds for the life of the process.
- **A kind one guest module defines can be used by another module,** whose actors hold references proven for it and send it. A module published alone that changes such a kind leaves the other module's references proving what its new code does not take, and its senders writing a layout the new code does not read.
- **A guest is built where its agent works,** in a session's container (ADR-0057), and reaches the hub's store with `check_in` (ADR-0039: `check_in`). An engine reads `Code` on its own host, and nothing carried a stored module there.

## Decision

**A program's code is held by one stock root, `Library`. It reads guest modules through `Files` from a new namespace, `Code`, and publishes them in groups: the modules a program starts with, the modules a system ships, or the modules a `publish` names while the program runs. A publish moves the head of every kind its group makes, and is refused while a module that is not in the group, built with a kind the group stops handling, has guests running. Each running guest keeps what it is relied on for: every kind a reference to it was proven for, with the answers that proof allows. Between steps, the guests behind the heads a publish moved move together or not at all. The group stays on its old code while a guest's new code does not take what the guest is relied on for, while one of them answers that it moves later, and while mail or an answer of a kind their new code does not read is still on its way to one of them. When it moves, each guest's old state is read by field name into a portable form of its new state, its new code builds itself from that, and every reference the new state holds is checked against its target's new code. A move that fails leaves every guest of the group on its old code, untouched.**

```
publish { paths } ──> Library ── Files: Code/<path> for each ──> blobs ── load_code
  ── turn.publish(&publisher, &blobs) ── a module outside the group built with a kind it drops? ──> Refused::Dependents
                                      └─> Publish { kind, blob } for each kind, under one number ──> heads view
between steps, the guests behind the heads that publish moved, as one group:
  relied on for a kind the new code does not take? ──> parked
  upgrade(&self), each: any Later ──> look again after the next step
  mail or an answer of a kind the new code does not read, on its way to one? ──> a step on the old code, look again
                                                                              (past most_flush_steps: parked)
  each state taken out (Written, old schema) ──> new code: same layout? taken as is
                                               : else Portable<New> by field name ──> take_over
    then every reference in each new state, checked as a cast is, against the code once the group has moved
    ├─> every one Ok, every reference passes: the group moves, and what each is relied on for is written again
    └─> any Err: every state put back, the group parked
```

### Libraries

- **`Library` is a singleton root in a crate of its own, `aethereal-library`, and a program has one.** It reads through `Files` and serves tools, so it stands beside the capabilities, not in `aethereal-substrate`. It holds every kind's head and has no children. A program lists it after `Files`, with nothing to set:

  ```rust
  .root::<Files>()
  .root::<Library>()
  ```

- **The right to publish is a handle, `Publisher`, and only the chassis makes one.** It makes it once as the program starts and hands it to `Library` as it bootstraps, as it hands other roots their handles (ADR-0029, ADR-0010). `Publish` is asked with it for a group of blobs, `turn.publish(&self.publisher, &blobs)`, which answers the number the engine gives the publish or why it refused it, and the heads view is folded only from what it publishes. The engine keeps no record of which actor is the library: the handle is the right, so no other actor changes which code a kind runs, and one program has one library because there is one handle.
- **As it begins, `Library` lists `Code` through `Files`, reads every module there, takes each in as code, and publishes them as one group.** A turn takes a blob in as code with `turn.load_code(&blob)`, which answers the kinds its types make. Natively `Files` lists the directory; in a browser it reads a manifest of the modules, written when the page is built (ADR-0032). The reads are mail, answered in later steps, so what keeps `Code` from being published stops the program as it starts, as a refused tool offer does (ADR-0037), logged as the library's naming the modules: two modules that make one kind, since neither can be chosen over the other, a module the guest host refuses (ADR-0015), and a listing or a read that fails. A program with no `Code` publishes nothing from it.
- **A system that owns guests ships their modules and registers them with `Library` by message,** as a tool is offered to its holder (ADR-0037). It builds them into its own code with `include_bytes!`, puts each as a blob, and asks `Defaults { modules }`, answered `Defaulted`. A default fills only a kind that has no head, and never replaces code that has one, whether `Code` made it or a publish since; `Defaulted::Taken` names the kinds it published and the kinds it kept. A library still publishing `Code` holds the ask until it has, so `Code` always comes first. The modules are taken all or none, as one group: one the guest host refuses, or two that make one kind, takes none. The program's own modules are then the code a program runs with nothing in `Code`, and a module in `Code` replaces one of them for a build.
- **While it runs, `Library` publishes the modules of `Code` a caller names, as one group, `PublishModules { paths }`,** answered `ModulesPublished`: each kind with the number of the publish, or why it was refused. What can be published is what `Code` holds, which no actor writes; the right to publish any other blob stays with the handle. The number is the one `turn.publish` answered: the engine's, under which the heads view folds the publish. The library keeps no count of its own.
- **`Namespace` gains `Code`:** read-only, the guest modules a program runs. Natively its root is a setting of `Files`, `code`, beside `assets`; in a browser it is fetched relative to the page, as `Assets` is. A module is built where `Code` points, or, for an engine the hub spawns, checked into the hub's store and placed in that engine's own `Code` by the hub (ADR-0040: `place_modules`), so a rebuild is a new file to publish.
- **A spawn names a library's kind and takes its head.** `turn.spawn_from` takes an `Origin`: `Origin::Head`, the head the view shows for the child's kind, refused with `Refused::NoHead` when the kind has none; or `Origin::Blob(&code)`, a blob the parent holds, for code no library publishes (tests, the chassis). It is not called `Source`, which is the resource a host brings mail in through (ADR-0010). A parent of a library's kind holds none of its code, so updating that kind is one `Publish` for every parent that makes it. A parent that names no type spawns by the kind's name under a protocol (ADR-0059: `turn.spawn_named`).

### What a module was built with

- **`aethereal.declared` gains the id of every kind compiled into the module.** Each `#[kind]` built for a guest writes its id into the section, beside the types the `guest!` macro writes, so the list holds the kinds of every crate the module links, whether its code uses them or not. It is everything the module could send, read, or hold a reference proven for, and the engine reads it once, when the module is put.
- **A group drops a kind when a type it replaces handled the kind and the type's new code does not,** or answers it with a kind the old code did not answer it with.
- **A publish is refused while a module outside the group, built with a kind the group drops, has guests running,** with `Refused::Dependents`, naming each such module and the kinds. Its guests could hold a reference proven for that kind, in their state, in an answer's stored context, or in mail on its way, and no move of the group alone could prove those again. The remedy is a publish that names those modules too, built against the new kinds. The check reads the table and the modules as the step began. A module's own guests are always in its group, so a module that keeps its kinds to itself publishes alone.

### What a running guest is relied on for

- **Each running guest's seat keeps its reliances:** each kind a reference to it was proven for, the answers that proof allows for it (ADR-0016: `Carries`), and who proved it, a module or the program. A kind proven again by the same prover keeps the answers both proofs allow.
- **Every proof made at run time writes one, for a guest:** a cast (ADR-0016: `turn.cast`), a guard proving a sender (ADR-0058: `Sender::speaking`, and its guest host's `aethereal_sent_lists`), and a spawn by name (ADR-0059: `turn.spawn_named`). A native actor never moves, so a proof against one writes nothing. A turn writes nothing to the table, so a proof asks for the reliance as an effect, `What::Relied`, folded once the step's turns are done, and asks only when the seat's reliances as the step began do not already hold it: a guest proven again for what it is already relied on for costs no effect.
- **A proof made at build time writes none.** `reference.protocol()` needs the actor's type, which only the module that defines it names, and that module's guests move with it. `narrow` proves less than the reference it narrows, already relied on. A reference decoded from mail carries its sender's proof, since its layout names its protocol (ADR-0016).
- **Reliances only grow while a guest runs.** Knowing that the last reference proven for a kind is gone would mean counting the copies of a `Copy` value across mail, states, and stored contexts. A guest relied on for a kind its new code drops does not move. A guest's reliances end with it, and are written again as its group moves, from the references its group's guests are checked for.

### Moving a group

- **Every type a guest's module makes implements `Upgrades`, and `guest!` requires it,** so a guest that does not say how it moves does not build. A native actor does not implement it: a native kind's code is the program's own (ADR-0009: `Code::Linked`) and moves only with a rebuild.

  ```rust
  pub trait Upgrades: Actor + Portability {
      /// Asked between steps while this actor is behind its kind's head, with the rest of its group.
      fn upgrade(&self) -> Upgrade;

      /// Builds this version's state from the state the actor had. Called only when the layouts differ.
      fn take_over(old: Portable<Self>) -> Result<Self, TakeOverError> {
          Err(TakeOverError::Unwritten)
      }
  }

  pub enum Upgrade { Now, Later }
  ```

- **An actor is behind when the blob it runs is not its kind's head.** The engine knows both, so nothing is kept in the actor, and nothing is sent to it.
- **A publish's group is the running guests behind the heads it moved.** They move together, between steps, where the engine holds every part: a group's guests stand in many parts, and moving one before another would leave the moved one sending what the others' code does not read. A guest behind a head that a later publish moves again joins that publish's group, and moves to the head as it then stands, never to a version in between.
- **Between steps, the engine looks at each group that is behind, in order, and each check is a reason to stay:**
  1. **What each guest is relied on for.** Each guest's new code must handle each kind it is relied on for, and answer it only with kinds the reliance allows. A reliance proven by a module of the group on a kind the group drops is left out: that module's guests move too, and the references they hold are checked as they move. Any other reliance a guest's new code does not take parks the group, naming the guest, the kind, and who relies on it.
  2. **Asking.** Each guest is asked in a call of its own into its instance, which calls `upgrade`. It takes `&self`, so deciding changes nothing. One answer of `Later` holds the group, which is asked again after the next step.
  3. **Flushing.** Mail queued for a guest of the group of a kind its new code does not handle, or an answer a guest awaits of a kind its new code does not handle, would be dropped by the move, though the reference it came through proved it would be read. The group waits, its old code takes them in the next step, and the engine looks again. Once every guest answers `Now`, those kinds are sent only by the group's own old code, so the wait ends unless the guests keep sending them to one another: a group that has waited `most_flush_steps` steps (a setting of the engine, 64 by default) is parked.
  4. **Moving.**
- **The engine takes each guest's state out at `Within`, as for a child seated in another part, with the schema of its kind as the old module declares it.** `aethereal.declared` gains each actor type's state schema, its names included (ADR-0004: `SchemaNodes`), which `Declared` reads.
- **Each state goes into an instance of the new blob in its part,** made there if the part has none (ADR-0015). Its children stay where they stand, each on its own blob and its own kind's head.
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
- **A field whose layout differs only in the protocols of the references it holds is read, not refused as a `Mismatch`.** One declaration of a kind changes as the code changes, and one publish takes every module that uses it. A guest `InventoryScreen` that holds `model: ProtocolRef<Inventory>` keeps the same field in its code while `Inventory`'s kinds change, and the field's layout, which names them (ADR-0016), changes with them. Its portable form has the field's own type.
- **Every reference each guest of the group will hold is checked before the group moves.** Once a guest's new state is built, by `take_over` or taken in as it is, the engine finds each reference in it by its schema, which names the reference's protocol (ADR-0016), and checks it as a cast is checked, against its target's code once the group has moved. The author writes nothing for it. A reference that fails the check fails the move, `TakeOverError::Unproven { field }`: the modules were built against kinds that disagree. Each reference checked writes the reliance it proves as the group moves, in place of those the first check left out. A native actor never moves, so only guests' states are checked.
- **`take_over` runs in a call of its own into the new code's instance, as a `begin` does.** It takes no turn, so it logs nothing itself; the move's lines are logged as the actor's. **Each move logs the fields that were absent,** at `Info`: `moved to publish 12; absent: show_cooldowns`. A field that should never be absent showing there says something went wrong even where a default covered it.
- **The group moves only when every guest's `take_over` and every check of its references succeed.** `take_over`'s `Err`, a trap in it, an unwritten `take_over` for a changed layout, or a reference that fails its check puts every state taken out back into its old instance, and every guest of the group runs on its old code. The error is logged at `Error` as the guest's, with its reason. The group is **parked**: none of its guests is asked again until one of its heads moves again, since the same code would fail again against nearly the same states. The remedy is another publish.
- **Moving is not ending.** `end` is not run, and each guest's path, its watchers, and its references are unchanged. Every reference proven for a kind is held to by its reliance, so a guest that moved still takes everything a reference to it was proven for.
- **A trap in `take_over` ends the new code's instance as a trap in a turn does** (ADR-0015): each actor already living in it ends, and the instance is dropped. The group stays on its old code, parked. **An actor of a dropped instance runs nothing more.** Its host numbers each instance it makes, and an actor runs only in the instance it lives in: when its part's instance of its code is gone, or is another made since, as a later move or spawn made it again, its later turns fail, and it runs no closing turn, as ADR-0015 holds of an actor whose instance a trap dropped.

### Moving between steps

A step's turns read what the engine holds as the step began, and its changes are folded once its turns are done (ADR-0011). A move is such a change, made where the engine already folds and seats.

- **The table learns each guest's new type as its group moves,** so every turn of a step reads each guest on one version of its code, and each batch runs wholly on one version. Nothing moves an actor during a turn or a batch, and nothing an actor does in a turn arranges a move.
- **Each guest's seat, which its part keeps and the plan reads between steps, takes its new type as it moves,** so the plan weighs its next turns by the code it now runs.
- **Fusing is unchanged.** Whether a send fuses depends on parts, seats, and lives, not on types (ADR-0013), so a moved guest fuses into and out of its part as any other.

### Seeing a publish

- **Each publish is numbered by the engine, once, as it is asked,** and the number is the heads' it moved: `turn.publish` answers with it, and `Publish` carries it into the heads view, which takes it as it folds and counts nothing itself. Every publish of a step is asked by the one actor that holds the publisher, so the number is the view's last as the step began, on by each publish asked before it in the step.
- **The heads view records what became of each group.** Each look between steps is folded into it: a group that moved counts on its publish, with the fields its guests' old states lacked; a group held by `Later` or by mail still on its way counts one more step, with the guest it waits for; a parked group records why, with the guest, the kind, and who relies on it, or the guest whose `take_over` failed. A group is kept until it moves, or a later publish takes its guests. A turn reads it as the step began with `turn.moves()`, as it reads the log (ADR-0044).
- **`aethereal-library` serves two tools, and no other,** behind its `tools` feature. It serves no spawn tool: a guest is spawned by the actor that owns it, from its kind's head.
  - **`publish { paths }`** asks `Library` to publish the modules at `Code/<path>`, as one group, moving the heads of kinds other modules made before, and answers as soon as the library has: each module's hash, and each kind it makes with the number of the publish, or the modules and kinds that refused it. It has no wait: what became of the guests is read when it is wanted, with `upgrades`.
  - **`upgrades {}`** reads the heads view as the step began: each kind's head, with the number of the publish that made it, and each group that has not moved, waiting with the guest it waits for and how many steps, or parked with why. Read under the number a publish answered, it is that publish's record:

    ```
    publish 12 · ichor.ui.hud → 3f9a…, ichor.game.inventory → 81c2…
      waiting   39 guests   ichor.ui.hud:inventory answered Later, 3 steps
    publish 12 · moved 39 guests   absent: show_cooldowns (38)
    publish 13 · parked   ichor.ui.hud:party is relied on for ichor.ui.set_items by the program; 5d07… does not handle it
    ```

- **Each move and each failure is in each guest's log,** tagged with the publish number, so `actor_logs` and the dev overlay's console show them as they happen (ADR-0044, ADR-0053).

## Consequences

- **A guest's behaviour can be changed while the program runs,** by building its modules and calling `publish`. Its guests keep their state and move together once each is ready, and `upgrades` says who moved, who the group waits for, and why a group parked.
- **A reference keeps proving what it was proven for.** A guest moves only onto code that takes every kind a reference to it was proven for, so a native actor's references to guests hold for the life of the process, and so do a guest's.
- **A change to a kind is published with every module built with it.** A module that shares its kinds with other modules is published with them; one that keeps its kinds to itself publishes alone.
- **A group moves at the pace of its slowest guest.** One guest that answers `Later` holds the whole group, and a group whose guests keep mailing one another a kind they drop parks after `most_flush_steps`.
- **A move waits for the end of a step,** and moves wait for the thread between steps, where parts are not run side by side.
- **Each guest's seat holds its reliances:** a few entries for each protocol something proved a reference to it for, each written once.
- **A program has one library** (ADR-0009). One library holds every kind's head, rather than a library for each namespace, each the only one that could publish its kinds, so no namespace is registered, and a program lists no module: what is in `Code` runs, and its systems' own modules run for the kinds `Code` does not make.
- **Nothing moves a running actor without asking it** (ADR-0009). Each guest of a group chooses whether it moves now, and none is moved during a turn or a batch. The engine asks between steps, so no actor watches the heads, and none can forget to.
- **This decides what ADR-0015 left open:** a guest's state is taken out to replace its code as it is taken out for another part, and put back when the move fails.
- **A native actor never moves.** A screen meant to change live is written as a guest. ichor's actors are native today; one of its screens changes live once it is a guest.
- **Every guest type writes `upgrade`,** a few lines when it has nothing to wait for. `take_over` is written only when a version changes the state's layout.
- **The declarations of replaced code are kept for the life of the process.** ADR-0015 names guest types by pointers never freed, and left a registry that frees them "until code is put again and again as a program runs". A publish now does that. A declaration is small beside the compiled code, which goes with the last blob of its bytes; whether the declarations a long session of publishing leaves behind need a registry is decided once they are measured.
- **`aethereal.declared` grows by each type's state schema and by the module's kinds,** read once when a module is put.
- **`turn.spawn_from` takes an `Origin`,** and every caller names `Origin::Blob` or `Origin::Head`.
- **`Files` gains `Code`, and each host's settings gain its root.**
- **Two versions of one kind can run at once** while a group waits or is parked. Mail between them is safe, as ADR-0009 already holds: a changed kind is another id.

Follow-on work, in order: `Publisher` and `What::Publish` with the heads view; `Origin` on `spawn_from`; `Namespace::Code`; the state schema in `aethereal.declared`; `Upgrades`, `Upgrade`, and the call that asks in the guest's library and the native host (the browser chassis runs no guests, so it has none to ask); taking a state into a new blob's instance and back; `Portability`, its derive, and `Portable`; `aethereal-library` with `Library`, `publish`, and `upgrades`. Then, for groups: each module's kinds in `aethereal.declared`; the protocol in `ProtocolRef`'s layout (ADR-0016); reliances, `What::Relied`, and their writes at a cast, a sender's guard, and a spawn by name; `turn.publish` over a group with `Refused::Dependents`; moving groups between steps, with the reliance check, the ask, and the flush; the check of every reference in a moving guest's new state, with `TakeOverError::Unproven`; `PublishModules` and `publish { paths }`; and `upgrades` by group.

## Alternatives considered

- **The engine moves every running actor of a kind when its head moves.** A publish reaches every actor at once. An actor's code would change under it at a moment it did not choose, which ADR-0009 rejected, and an actor in the middle of a save would be moved mid-save.
- **A message that the code changed, and `turn.upgrade()` when the actor is ready.** The actor then has to remember it owes a move, a second place to keep the same fact. Asking each guest of a group decides in one place, before anything runs.
- **Each guest moved alone, before a batch of its own,** on the worker that runs its part. Its mail would be handled in the step it was sent for. A group's guests stand in many parts: a guest moved before the others would send what their old code does not read, and the references it holds would be checked against code that had not moved.
- **A token handed with the message and spent to move.** A type checks that only an actor owed a move can move. The token must be kept in the actor's state, and Rust cannot forbid dropping it, so the engine would have to catch a dropped one; the engine already knows whether an actor is behind.
- **Moving an actor that does not answer.** A default would hide how each actor moves. `guest!` requires the answer, so every guest's behaviour is written down.
- **A typed copy of the previous state, `take_over(old: HudV1)`.** Checked fields, but every version carries a copy of the last one's type, a conversion is written for each pair of versions, and an actor that skipped a version cannot be read.
- **Carrying fields by name with no code.** Nothing to write. A renamed field would silently become its default, which papers over exactly the change that needs a decision.
- **Fields marked `#[new]`, the rest required.** A missing old field would then be an error, not a default. A mark could never come off while any older actor may run, the way `required` could never leave a protobuf message.
- **A field of another layout read as absent.** The move succeeds, and the field's real value is replaced by a default no one chose.
- **A failed `take_over` retried every step.** The same code fails again against nearly the same state, and the log fills with it.
- **A failed move ends the actor, or begins it again.** A bug in the new version would end actors that work on the old one, or lose their state with no one deciding to lose it; the engine does not keep a config to begin one again.
- **Moving the guests of a group that can move, and parking the rest.** Each guest moves as soon as it can. A guest that moved would send what one left behind does not read, and the references it holds would be checked against code that never moved.
- **Promises declared on the type, `#[actor(implements(P))]`,** checked where the module is built and kept in its declaration, with moves held to them. Nothing is written at run time. A guest would keep every protocol it ever declared, whether or not anything proved a reference for it, and a cast would need the declaration, which no native type writes.
- **Refusing a publish while guests of its kinds run on code it would break.** A kind that running guests use could then not change until they closed, and the code a fix needs would wait on the guests it is meant to move. The head moves; its guests stay behind until their new code takes what they are relied on for.
- **Kinds a guest defines kept to its module,** with modules talking only in kinds the program defines. No module would depend on another, and no publish would name more than one. Two modules could share a vocabulary only through a native one.
- **The parent spawns the new version in the old one's place, with the old one's state.** The new guest has a new identity, so every reference to the old one ends with it, and each holder must learn of the new one and prove it again. A group that moves keeps every guest's identity, so every reference to it stays.
- **The engine reading old mail as the new kind, by field name.** A proof would then mean that a guest takes something convertible to what it was proven for, and a field the new layout adds would hold a default no one chose.
- **An adapter actor that takes the old kinds and sends the new.** The references proven for the old kinds point at the guest, not at the adapter.
- **Reliances that shrink as references go.** A reference is `Copy`, and its copies travel in mail, states, and stored contexts, so knowing the last is gone means counting through all of them. Dropping a holder's reliances when it ends misses the copies it mailed first.
- **Dependents found from reliances alone,** each naming the module that proved it. A module that received a reference in mail proved nothing, and still holds it.
- **Moving a group while mail of a kind it drops is on its way to it.** The new code could not read it, though the reference it was sent through proved it would be.
- **An effect that registers the library, and `Publish` taken from the registered root alone.** The engine would remember one actor as special. A handle carries the right the way every other right is carried, and a program cannot make a second.
- **The engine reads `Code` and serves the tools itself.** No actor holds the right. The core would then do file I/O and name the tool machinery, and the tools must be actors anyway, so one would need the right regardless.
- **A library for each namespace, listing its modules,** as ADR-0009 had it. Ownership by namespace guards libraries in one program from one another, which one library cannot need, and every program would list its namespaces and modules, changing its settings for each new module.
- **A default that replaces what `Code` holds, or the first publish of a kind wins.** A system's module would then take a kind from the module the program was built with, or whichever was asked first would, and which code runs would depend on the order roots begin. A default fills only a kind with no head, and waits for `Code`.
- **Defaults listed in the library's settings, or handed it by the chassis.** Every program would list each system's modules where it lists the library. A system that owns guests knows its modules, so it asks for them as a toolbox offers its tools, and a program that lists the system has them.
- **A publish that waits for its guests to settle,** answering once its group has moved or parked, or when a wait ends. A call that waits either polls the heads view, looking again every few milliseconds, or needs the engine to raise an event as a group settles, which nothing else reads. What it would answer is already in the heads view, kept as each group is looked at, so it is there to query when it is wanted: `publish` answers at once, and `upgrades` reads the moves.
- **A `spawn` tool beside `publish`.** An agent could stand up a guest no actor owns, which nothing would end or reach by a typed path. A guest is spawned by the actor that owns it, from its kind's head.
- **A parent ships its children's code.** Updating a child means rebuilding or re-handing every parent that makes it. A library's head lets a parent name only the kind.
- **An engine that reads modules from the hub's store over its link.** How a blob crosses to another engine is not decided (ADR-0014), and an engine reads its own `Code` through `Files`, as it reads `Assets`, natively and in a browser. The hub places a stored module where that engine's `Files` reads it (ADR-0040).
- **A scripting language beside Rust.** A second language, runtime, and set of bindings, for what a module already does once it can be replaced live.
