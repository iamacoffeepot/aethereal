# Spike: keyed sections

**Question.** How would the engine shard one actor's state by key, so that its keys' turns run side by side on the
step's workers, and where do the engine's seams fight it?

**Branch.** `spike/keyed-sections`, cut from `main` at `1d7bd99`. Never merged. File references below are to `main` at
that commit unless they name a spike file, which are to the branch.

**Run it.**

- `cargo test -p aethereal-substrate --lib keyed`: the three tests (`engine/keyed/tests.rs`).
- `cargo run --release -p aethereal-chassis-headless --example keyed_sections [program...] [workers=N,...]`: the
  benchmark.
- `cargo run --release -p aethereal-chassis-headless --example savina big world fjthrput churn pingpong`, on this
  branch and on `main`: what the stamp costs mail that is not keyed.

## Answer, first

The model works, and it fits the engine better than expected in two places and worse in four.

- **It fits the planner unchanged.** A section's mail is kept in the streams under a *lane*, a hash of the actor and
  the key, in place of a seat. The lane's low bits pick its part (ADR-0011's parts are already "the keys of a view
  that fall together"), so a keyed actor's keys spread over every part, and the plan, the packing, the costs, and the
  workers did not change at all. A section lives in its lane's part, beside that part's streams, and moves with it,
  so a worker that owns a part owns its sections: `&mut S` with no lock, and nothing to merge at the fold.
- **It costs every piece of mail.** The stamp is eight bytes on `Mailed` and on the stream's entry. With it in every
  piece of mail, `big`, `churn`, `world`, and `fjthrput` cost 2 to 4 percent more a piece of mail on one worker.
- **A section must be a sender of its own.** Mail ids are `(sender, count)` with one count per actor (ADR-0012), and a
  part drops two entries of one id (`stream.rs:228`). Two sections sending in one step on two workers would give two
  pieces one id, and one would be dropped. The spike gives each section its own range of counts.
- **Answers and stream parts cannot be stamped by their sender.** The sender of an answer does not know the asker's
  type. The key of an answer has to be a *return stamp*: computed by the asker as it asks, from the context it
  stores, carried on the request, and echoed onto the answer by the engine.
- **The motivating actor does not fit.** The HTTP stack's per-connection state holds a `Vec<u8>` and a `Held`, so it
  is not `Copy`; it numbers connections and requests across all of them, which is mutable whole-actor state; and its
  answers' contexts are stream numbers, which map to a connection only through the actor's own table.
- **Rollback is free for small sections and memory-bound for big ones.** On one worker a turn of a 64-byte section
  pays nothing measurable for its copy, a 1 KB section about 12 ns, and a 16 KB section about 515 ns.

Measured on a dictionary of 4,096 keys written by 64 senders (262,144 puts, 64-byte entries), nanoseconds a put, the
best of three runs in each of two passes:

| way                                  | 1 worker | 4 workers | 16 workers |
|--------------------------------------|---------:|----------:|-----------:|
| keyed, uniform keys                  |      177 |        55 |      80–82 |
| one ordinary actor, uniform          |   97–103 |     88–93 |      88–94 |
| 16 ordinary children (today's way)   |  154–159 |        50 |      70–75 |
| 4,096 ordinary children, one per key |  203–207 |        71 |     99–103 |
| keyed, one hot key (half the puts)   |      158 |        72 |         75 |
| 16 children, one hot key             |  147–153 |     72–74 |      73–75 |

Keyed sections cost what hand-sharding into 16 children costs when each key's run is long, 10 to 15 percent more when
every key has one put a step, and less than an actor per key, with no fixed N to choose. They do
not beat one ordinary actor on one worker, because that actor's mail from each sender joins into one run (ADR-0011,
"Mail one turn sends back to back is one record") and keyed mail joins none.

## The model as built

- `aethereal-actor/src/keyed.rs` (spike): `KeyOf<K>`, `Sectioned` (`type Section: Copy + Default + PartialEq`, and the
  actor `Sync`), `hand_section` (what a macro would emit per handler), `SectionStore<S>` (one part's sections of one
  actor), `RunsSections` and `SectionRunner<A, KEEPS, DROPS>` (the engine's view of a keyed actor; `KEEPS` and
  `DROPS` exist only to measure the copy and the drop check).
- `aethereal-actor/src/effect.rs:149` (spike): `SectionKey`, the stamp, on `Mailed::section` (`effect.rs:139`).
- `aethereal-actor/src/turn/to.rs:73` (spike): `send_keyed`, which calls `<R as KeyedTarget<K>>::section(&mail)` at the
  send and stamps it (`turn/mod.rs:886`).
- `aethereal-substrate/src/stream.rs` (spike): the entry carries the stamp (`:169`), keyed mail is kept under its lane
  (`:774`, `lane` at `:787`), and a part's items are put in order and cut into runs by seat-or-lane, actor, and section
  (`:233`–`:239`).
- `aethereal-substrate/src/engine/keyed/mod.rs` (spike): `KeyedActors`, the registry every worker reads; `Sections`,
  each part's stores; `keyed_turns`, a section's run.
- `aethereal-substrate/src/engine/mod.rs` (spike): the registry and the parts' sections on `Engine` (`:136`, `:138`),
  moved with each `Parcel` (`:394`, `:1582`); `run_part` sends a keyed run to `keyed_turns` (`:1810`); a keyed send is
  never fused (`:2445`); `add_sectioned_root` (`:796`) and `section` (`:813`).

A put flows: sender's handler → `send_keyed` computes the key and stamps `Mailed::section` → the worker routes the
mail to the streams bucket of `lane(actor, key) & (parts − 1)` → the fold appends it to that part → the next plan
packs that part onto some worker → `Shard::hand` orders it under `(lane, actor, section, sender, count)` and cuts runs
per section → `keyed_turns` finds the actor in the registry, the store in the part, the section in the store → copy
the section, run the handler on `&A` and `&mut S`, put the copy back on a fault → route what the turn asked for.

## 1. The stamp

**Where it goes.** On `Mailed` (`effect.rs:124`), beside `sent_as` (ADR-0058, `effect.rs:128`), as `SectionKey`, eight
bytes: the top bit marks a key, so `SectionKey::WHOLE` (nought) is "not keyed" and a key is any value below `2^63`, the
same trick `Tie` plays with its top bits. It is copied into the stream's `Entry` (`stream.rs:161` on `main`), since a
part orders and cuts its runs on it, and is not in `Envelope` (`aethereal-substrate/src/mail.rs:13`).

**What it costs in memory.** `Mailed` 96 → 104 bytes, `Entry` 96 → 104, a part's `(seat, Entry)` record 104 → 112:
about 8 percent more per queued piece of mail, keyed or not.

**What it costs on the send path.** For a keyed send, one call of the receiver's `KeyOf::key` (inlined: a field read
in the benchmark) and a store of eight bytes. For every send, writing eight more bytes into `Mailed`, copying them into
the entry at routing, and comparing them in the part's sort, dedup and run-cutting (`stream.rs:227`–`:240`). Measured
on `savina`, `main` against this branch, best of three, four passes each, two before the last change to the branch
and two after (ns a piece of mail):

| program  | workers | main      | spike     | change     |
|----------|--------:|----------:|----------:|-----------:|
| pingpong |       1 | 1088–1152 | 1111–1191 | 0 to +6%   |
| big      |       1 |   165–167 |   169–170 | +2%        |
| big      |      16 |   166–168 |   170–173 | +2 to +3%  |
| churn    |       1 |   206–210 |   213–214 | +2.5%      |
| churn    |       4 |   210–212 |   216–218 | +2.5%      |
| world    |       1 |   147–148 |   151–152 | +2.5%      |
| world    |       4 |     48–49 |     49–50 | noise      |
| fjthrput |       1 |   227–239 |   236–237 | 0 to +4%   |

`fjthrput` on 4 and 16 workers and `world` on 16 move by 10 to 25 percent between passes on both builds, so they are
left out. Where the stamp should live instead is in the recommendation.

**What had to bend.** A typed send knows its receiver's type only through an `ActorRef<T>` or a typed path; through
a `ProtocolRef<P>` the receiver's type is gone, so a protocol cannot reach a keyed actor unless the protocol itself
says how each kind is keyed. And `write_after` joins back-to-back mail of one kind to one receiver into a run
(`turn/mod.rs:1010`): a run is one stream entry, so it would carry one stamp for all its pieces. The spike's keyed
send joins no run. A real one would join only mail of the same key.

## 2. Queues

**How per-(actor, key) queues fit.** They are not new queues. A part's stream already keeps every item in one list and
cuts it into one run per actor when the step begins (`Shard::hand`, `stream.rs:198`). The spike makes the run key
`(lane, actor, section)` in place of `(seat, actor)`, and keeps keyed mail under its lane, so the queue of one
`(actor, key)` is that key's run in its lane's part, in sender-then-count order, exactly as an actor's queue is today.
Mail from one sender to one key keeps its order; mail to two keys keeps none, as ADR-0011 promises for two sources
(ADR-0011 line 63).

**Do ADR-0011's parts map onto keys?** Yes, directly. A part is "every key of a view whose low bits are `p`" (`view.rs`
`part`, line 305). The streams view was keyed by seat; the spike keys keyed mail by lane, and the part falls out.
Because the plan reads only parts (`plan.rs:154` `decide`, `:230` `pack`), it needed no change: a part's expected cost
is the sum of its items' costs, keyed or not, and the most costly part bounds the step as the most costly actor did
(`plan.rs:189`, `useful`). The count of workers a keyed actor uses changes every step with which of its lanes have
mail, and no state moves: a section belongs to its lane's part, and parts move between workers as they always have,
with ADR-0011's homes keeping each where it last ran.

**What had to bend.**

- **The seat slot is overloaded.** A run's `seat` (`stream.rs:80` on `main`) is a lane for a keyed run. The spike
  reuses the `u64`; a real change names it.
- **A keyed run reads nothing of its actor's part.** `turns` takes the actor out of its seat, reads its life, its
  `Sitting`, and its `Held` (`engine/mod.rs:1827`–`1833`), all in the part of its seat, which another worker may own
  in the step. A keyed run reads the registry instead, and liveness from the table as the step began
  (`engine/keyed/mod.rs:91`, spike).
- **The cost of a part's items is found by the actor's seat.** `fold::expected` finds an item's actor kind by looking
  the actor up in its own part's seats (`fold.rs:130`), so a keyed item in another part cost nothing, and the plan
  would have seen no work. The spike falls back to the registry (`fold.rs:141`, spike).
- **The slice and fusing.** A keyed run is never cut at its slice (`engine/mod.rs:1874`, `:1996` on `main`) and a
  keyed send is never fused (`hand`, `:2377` on `main`). Fusing a send into a lane is possible in principle (the
  lane's part, not the actor's), but not tried.
- **The order key grew a field** in the sort, dedup and run-cutting, paid by all mail (section 1).

**The limit.** A keyed actor's keys run on at most as many workers as there are parts (16 in the benchmark), and a hot
key pins its whole part, which also holds about 1/parts of the other keys.

## 3. The section table and the fold

**Where sections live.** In `Engine::sections`, one `Sections` per part (`engine/mod.rs:138`, spike), each a map from a
keyed actor to its store, a `SectionStore<S>` keyed by key (`keyed.rs:83`, spike). They are taken out with the part's
`Parcel` and put back with it (`:1582`, spike; `Parcel` at `:375` on `main`), as the part's guest instances are (ADR-0015
line 25).

**How a worker gets exclusive `&mut S`.** By owning the part for the step, as it owns the part's actors
(`engine/mod.rs:1519` `parcel` on `main`). Every key of one lane is in one part, one part is run by one worker, and that
worker runs the key's items one turn at a time, so no two turns of a key run at once and nothing is locked.

**How creates and drops merge at the fold.** They do not need to. A section is created in place by the worker that
owns its part, when its key's first mail is handled (`or_insert_with`, `keyed.rs:185`), and dropped in place when a
turn leaves it at its default (`keyed.rs:210`). The fold merges what turns *asked for* of views (ADR-0011), and a
section is actor state, which the engine moves with its part and never folds, as it never folds an actor. So the
model's "merged at the fold, as `fold` merges spawned children" is not needed: a child is seated at the fold because it
lands in a part another worker may own (`fold.rs:96`), and a section never does.

**What was not obvious.** A section that has sent mail cannot be dropped at its default: its count of mail sent would
start again, and its next piece could take the id of one still queued (section 9, first surprise). The spike keeps a
section that has sent anything. And dropping at the default compares the whole section with its default after every
turn, which costs as much as the copy for big sections (section 4).

## 4. Rollback

**How.** `SectionRunner::turn` (`keyed.rs:171`, spike): `let kept = cell.state;` before the handler, `cell.state =
kept;` on `Err`. `S: Copy`, so the copy is a `memcpy` to the stack and allocates nothing. The turn's effects are
dropped but for its lines, as a failed turn's are (`keep_lines`, `engine/mod.rs:2720`), and its fault is logged
(`engine/keyed/mod.rs:146`, spike). The actor does not end. The test
`a_failed_turn_leaves_its_section_as_it_was_and_the_actor_running` shows it, and fails when the copy is turned off.

**What it costs**, on one worker, 262,144 turns over 4,096 keys, each turn stirring one word of its section (ns a
turn): `keep` copies and checks for the default, `skip` only checks, `bare` does neither.

| section | keep      | skip    | bare    | copy (keep − skip) | default check (skip − bare) |
|--------:|----------:|--------:|--------:|-------------------:|----------------------------:|
|    64 B |   176–177 | 174–175 |     173 |       1 to 3 (noise) |                1 to 2 (noise) |
|   1 KB  |   207–208 | 195–197 |     182 |              11–12 |                       13–15 |
|  16 KB  | 1002–1003 | 482–488 | 356–358 |            515–520 |                     126–130 |

At 16 KB the sections are 64 MB in all, cold when their turn comes, so the copy runs at memory bandwidth: about 16 KB
in 0.5 µs. A turn that touches one cache line of its section pays for every line of it. On 4 workers the 16 KB copy
costs about 210 ns a turn (370–373 against 155–162).

**Does `Copy` keep out resources and blob references?** It keeps out every one the engine counts:

- `Handle<R>` (`resource.rs:28`) and `Given<R>` (`resource.rs:38`) are neither `Copy` nor `Clone`;
- `Bytes`, a pin on a blob (`blob.rs:285`), has a `Drop` (`blob.rs:381`), so it cannot be `Copy`;
- `Held<A>` (`held.rs:25`), a debt, is neither.

But `Copy` lets in what an actor's state may not hold: `ActorRef<T>` is `Copy` (`reference.rs:181`) and so is
`BlobId` (`blob.rs:14`), an unpinned id that keeps nothing alive; and `usize`, raw pointers, and `&'static` references
are `Copy` and are no kinds at all. `Copy` does keep out every `Vec`, `String` and map, which is what makes the copy
flat. The bound an ADR wants is `S: Plain + Copy`: plain data (ADR-0004, `plain/mod.rs:21`) that is also fixed width.
That is ADR-0009's own "fields all of fixed width" (ADR-0009 line 245), whose bytes are the same in memory and encoded,
so the copy before a turn is the encoding, and a section crosses `Within` for saving and upgrade (ADR-0009 line 221).
The spike's sections are `Copy` only, so a keyed actor's sections are not saved or moved onto new code.

## 5. Context keys

**Can the stack's answer and stream-part path be stamped at store time?** The mechanism can; the stack cannot use it.

**The mechanism.** The key of an answer cannot be computed by its sender, the actor asked, which does not know the
asker's type. So the asker computes it: when it asks, from the context it stores, through its own `KeyOf<Answer<A,
C>>`, as `#[answer(key = number)]` would declare. The request carries that key as a return stamp; the receiver's
`Owed` (`held.rs:34`) or the request's envelope keeps it; and the engine stamps the answer with it, whether the
receiver answers, holds and answers later, or the engine answers `Unanswered` for it. Stream parts are plain mail from
the server (ADR-0017 line 125), so `Open<S>` would carry the asker's return stamp, and the stream library's `Streaming`
would stamp each `Part` with it. This turns the model's "the sender computes the key" into "the asker computes its own
key for what comes back, and the engine echoes it", and costs a second eight bytes on every request (or a field in
`Open` for streams).

**What else answers need.** The contexts a request keeps and the debts an actor holds are kept in its seat, in `Held`
(`engine/mod.rs:301`: `keeping`, `waiting`, `debts`), in the part of the actor's seat. A section's turn on another
worker can reach neither. Each section would keep its own waiting contexts and debts, in its store. The spike's
sections ask nothing, hold nothing, and are asked nothing; it does not refuse any of these (see corners cut).

**Why the stack does not fit.**

- **Its key is not in its contexts.** The stack keys its connections by number (`serving/mod.rs:147`), but the context
  the engine keeps for a connection's reads is the stream's `Number` (`serving/mod.rs:358`; ADR-0017 line 126), and the
  connection is found from it through the actor's own `Streams<Reading, u64>` (`serving/mod.rs:145`, `:327`). A return
  stamp computed from `Number` alone cannot find the connection. The `Streams` would have to belong to each
  connection's section, with stream numbers that are only unique within a section, so parts would have to carry the
  section as well as the number.
- **Its sections would not be `Copy`.** A `Connection` holds a `Vec<u8>` buffer (`connection.rs:31`) and, while it
  gathers a body, a `Vec<u8>` and a `Held<Gathered>` (`connection.rs:141`).
- **It has mutable whole-actor state.** It numbers connections and requests across all of them (`serving/mod.rs:149`
  and the `requests` count after it), and keeps one listener stream.

So the stack is a keyed actor only with sections that are not flat, a rollback that is not a copy (or none), sections
that own their streams, and a barrier or a second actor for accepting.

## 6. Guests

Not built. What it would take:

- **A guest that sends to a keyed actor** stamps its mail record. Its library computes the key at the send, from the
  receiver's type it was compiled against, and writes eight more bytes in its mail record, which its host copies into
  `Mailed` (`turn/hosted.rs:309`, which the spike stamps `WHOLE`). The stamp is a claim the host cannot check without
  decoding the mail, which the model forbids, so a guest can send mail to a section whose key it does not match. That
  breaks only the receiver's own invariant, not another actor's, but it is the first claim of a guest's that the engine
  routes on and cannot hold it to (ADR-0015 holds a guest to every claim it routes on).
- **A keyed actor that is a guest** keeps its sections in the instance of each lane's part (ADR-0015 line 25), not in
  its seat's. Its whole value, the read-only part every section reads, must then be in every instance that runs one of
  its sections: taken out at `Within` and into the instance, as a child begun in another part is (ADR-0015 line 30), and
  again whenever it moves onto new code.
- **Batches** (ADR-0015 line 37) are per run, so a guest pays one crossing per key run. With one put per key per step,
  the uniform case here, that is a crossing per item, losing the batching ADR-0015 measured (about 30 ns an item). A
  batch that holds every keyed run of one actor in one part, each item with its key in its head (`records.rs:47`),
  keeps one crossing per part.
- **Rollback** is the guest library's copy, in its own memory; the host sees no sections.
- **A trap** ends every actor in its instance (ADR-0015 line 33). With sections spread over parts, a trap in one part
  loses that part's sections of a keyed actor whose seat, and whose other sections, are elsewhere. ADR-0015 would have
  to say whether that ends the whole actor or resets the lost sections to their default.

## 7. Measurements

**Where.** A 16-core, 32-thread desktop, L3 in four groups of four cores (as ADR-0011 measured), with nothing else
heavy running. Every figure is the best of three runs on a fresh engine of 16 parts, timed from the roots' `begin` to
the last step, as `savina` times its programs.

**Where the existing scenarios live.** `crates/aethereal-chassis-headless/examples/savina.rs`: `pingpong`, `count`,
`count-begin`, `fjthrput`, `big`, `fib`, `sieve`, `churn`, `churn-wide`, `world` (`PROGRAMS`, line 1098). The spike's
scenario is its own example, `examples/keyed_sections.rs` (spike), since a keyed root is added differently and its
check reads sections.

**The workload.** 64 senders each put 4,096 values into a dictionary of 4,096 keys, 64 puts a turn, telling themselves
to go on. A put stirs one word of its key's 64-byte entry with eight rounds of a mix and counts itself. Every run
checks that every put arrived. Uniform keys are drawn by a hash; hot keys send every other put to key nought.

**Results** (ns a put; puts a second is 1,000 over it):

| program            | 1 worker | 4 workers | 16 workers |
|--------------------|---------:|----------:|-----------:|
| `keyed-uniform`    |      177 |        55 |      80–82 |
| `keyed-hot`        |      158 |        72 |         75 |
| `one-uniform`      |   97–103 |     88–93 |      88–94 |
| `one-hot`          |   95–102 |     86–92 |      87–92 |
| `children-uniform` |  154–159 |        50 |      70–75 |
| `children-hot`     |  147–153 |     72–74 |      73–75 |
| `keyed-16keys`     |      151 |        48 |      61–67 |
| `children-16keys`  |  152–157 |        49 |      63–72 |
| `children-4096`    |  203–207 |        71 |     99–103 |

Each cell is the range of two passes, each the best of three runs. An earlier pass, before the last change to the
branch, agreed within a few nanoseconds but on 16 workers, which moves by up to 15 percent between passes.

What they say:

- **Keyed scales where one actor cannot**: 3.2 times faster on 4 workers than on 1, and 1.6 to 1.7 times faster than
  the one actor. One actor stays at its one-worker cost however many workers there are.
- **Keyed costs what children cost.** With 16 keys, so each key's run is long, keyed (151, 48, 61–67) matches 16
  children (152–157, 49, 63–72). With 4,096 keys, a run of about one put per key per step, keyed pays about 26 ns a
  put more than with 16 (177 against 151): a section's run costs about what an actor's run does. Against an actor per
  key it is cheaper everywhere (177, 55, 81 against 205, 71, 101). With 4,096 keys spread over 16 parts it is 10 to 15
  percent dearer than 16 children, whose runs are long.
- **The one actor wins on one worker** (97–103 against 177), because each sender's batch of 64 puts to it is one run,
  one entry in the stream, and keyed puts to 64 keys are 64.
- **A hot key holds the step** for keyed and children alike: half the puts run on one worker (72 to 75 on 4 and 16).
- **16 workers do worse than 4** for all three: a step here is about 4,000 puts, about 200 µs of work, too little to
  split 16 ways. Not a property of keys.

**The stamp's cost to other mail** is in section 1; **the rollback's** in section 4.

## 8. What the macro would emit

Written as the author would:

```rust
#[actor(keyed(by = u64, section = Words))]
impl Dictionary {
    fn begin(config: DictionaryConfig) -> Result<Self, Fault> { .. }      // the whole value: fixed here, read after

    #[tell(key = put.letter)]
    fn put(&self, words: &mut Words, put: Put, turn: &mut Turn<'_, Self>) -> Result<(), Fault> { .. }

    #[answer(key = asked.letter)]                                          // the key, from the context stored
    fn looked(&self, words: &mut Words, (answered, asked): (Answered<Found>, Asked), turn: &mut Turn<'_, Self>)
        -> Result<(), Fault> { .. }
}
```

It would emit what the spike writes by hand (`engine/keyed/tests.rs`, spike):

- `impl Sectioned for Dictionary { type Section = Words; fn section(..) }`, whose body is one dispatch on the item's
  kind to `hand_section::<Self, K>(.., Self::put)`, as `HandledList::dispatch` does for an ordinary actor.
- `impl KeyOf<Put> for Dictionary { fn key(put: &Put) -> u64 { put.letter.into_key() } }` for each `#[tell]` and
  `#[request]`, through a small trait that turns a key type into the stamp's `u64` (an integer as it is, anything else
  hashed).
- For an `#[answer]`, a return key from the context type, `fn key(context: &Asked) -> u64`, which `ask` calls as the
  context is stored (section 5).
- Not `Handles<K>` with a `&mut self` handler. A keyed actor's handlers are a separate trait (`HandlesSection<K>`), and
  `Reaches` builds for it as for `Handles`, so a keyed actor's handler without a key, or with `&mut self`, does not
  build. The spike writes a `Handles<Put>` whose whole-actor handler fails, only so that the existing `Reaches` builds.
- The stamp at the send, without a second send verb: a method on the handler trait, `fn section(mail: &K) ->
  SectionKey`, defaulting to `WHOLE` for every ordinary `Handles<K>` and emitted for a keyed one. The ordinary `send`
  calls it, so a send to an ordinary actor stamps a constant the compiler folds, and a send to a keyed one computes its
  key. The spike's separate `send_keyed` is a corner.
- In `Declared`, that the type is keyed, so the engine registers it as it is seated, and a guest's section says it
  (ADR-0015 line 51).

Writing the actor by hand was cheaper than extending `aethereal-actor-derive` for the spike.

## 9. Surprises

1. **Concurrent sections collide in mail ids, and the engine drops one silently.** An actor's mail is numbered by one
   count, carried in the actor's life (`engine/mod.rs:1863`), and a part dedups entries of equal `(seat, actor,
   sender, count)` (`stream.rs:228`). Two sections of one actor sending on two workers in one step would number from
   the same count, and a receiver would see one of two equal ids. A section has to be a sender of its own: its mail id
   is `(actor, key, count)`, or each section numbers from a range of its own (the spike's
   `first_count = (key + 1) << 32`, `keyed.rs:138`). ADR-0009's and ADR-0011's "by its sender, then in the order that
   sender sent it" then means by section.
2. **A failed turn that does not end its actor is a reversal of ADR-0009.** ADR-0009 closes the actor on any failed
   turn (line 192), and rejected putting state back because it means a copy of every actor before every turn (line
   257). For keyed actors the copy is per section and measured above, but an ADR has to say that a keyed actor's
   failed turn ends nothing, and that `end` never runs for it. The spike logs the fault as a closing turn's
   `EndFailed`, which marks nothing (`engine/keyed/mod.rs:146`, spike); a real log entry would name the key.
3. **Answers, holds, asks, and streams live in the seat, not the section** (section 5). The model's "a key from a
   stored context is computed when the context is stored" holds, but the key must travel on the request and be echoed
   by the engine, not be computed by the answer's sender.
4. **The HTTP stack, the obvious first user, does not fit** a `Copy` section or the no-mutable-whole-state rule
   (section 5).
5. **Keyed mail loses runs.** On one worker one ordinary actor beats keyed sections by almost two to one in this
   workload, because every sender's batch joins into one run for it (ADR-0011 line 91). Runs could be kept per key.
6. **A keyed run costs about what an actor's run costs.** The engine pays per run, not per item: building the turn's
   view of the table, looking up the store, and the run's bookkeeping. With one item per key per step that is about
   26 ns an item over long runs (`keyed-uniform` 177 against `keyed-16keys` 151).
7. **The plan saw no work in a keyed part** until `fold::expected` learned to find a keyed actor's kind outside its own
   part (`fold.rs:130`). Anything else that finds an item's actor by its seat in the item's part will miss the same way:
   `Shard::holds_for` (`stream.rs:544` on `main`), which asks whether mail waits for an actor before it moves onto new
   code (ADR-0054), now misses its keyed mail.
8. **The actor is held for exclusive use.** The engine takes an actor out of its seat for each run
   (`Holding::Away`, `engine/mod.rs:1830`) and `Actor` is only `Send` (`actor.rs:22`). Sections need the whole value
   shared, `&A` on many workers, so `A: Sync` and held where every worker can read it while its own seat's worker may
   also be running it. The spike keeps a second value in the registry, made by the caller.
9. **A section can still close, spawn, watch, and ask** through its `Turn`: the spike's `Turn<'_, A>` offers them all
   and the spike does not refuse them. A `Close` from one section ends the actor at the fold while its other sections
   run on in the same step on other workers, so ADR-0011's "an actor that ends handles no more mail" (line 97) holds
   only from the next step. A section's turn needs a turn type that offers none of these.
10. **Dropping at the default reads the whole section every turn**: about 128 ns a turn at 16 KB, a quarter of the
    copy.
11. **The trap domain and the actor no longer line up** for a keyed guest (section 6).
12. **The changes to other mail were not free** (section 1): 2 to 4 percent on mail-bound programs, from eight bytes.

What suggests changing the model: 1 (sections are senders), 3 (return stamps), 4 and 10 (flat sections as a
bound, lazy dropping), 5 (keep runs per key), and 9 (a section's turn type).

## What a barrier for whole-actor state would cost

Not built; refused by construction in the spike, which has no `&mut self` handler on a keyed actor.

- **Mutable whole-actor state read by sections** fits the engine's one way of sharing state (ADR-0011): the whole
  value as a view. Sections read it as it stood when the step began; turns of the whole write the next one; the fold
  publishes it. The cost is two copies of the whole value and a step of latency for every write, and it is no barrier.
- **A true barrier** (a whole turn that sees every section as of now, and before which no later section turn runs)
  needs every one of the actor's lanes in one place at once. Either the step that runs it is not split for that actor,
  which in this engine means not split at all, since a keyed actor's lanes are in every part; or it runs between steps
  on the stepping thread, which ADR-0011 names as what limits a step on many workers. Either way one barrier costs a
  step, and the plan must learn to put a keyed actor's whole mail in a step of its own.
- **Unkeyed mail in order with keyed mail from one sender** needs the same: a piece of keyed mail sent after a piece of
  whole mail must not be handled first, and they are in different parts.

## Corners cut

- A keyed actor is a root only, added by `add_sectioned_root` (`engine/mod.rs:796`, spike), seated as an ordinary
  actor whose own handlers fail. The value its sections read is a second one the caller makes.
- `send_keyed` is a verb of its own (`to.rs:73`, spike). Its mail joins no run; an ordinary `send` right after it, to
  the same receiver and kind, would join the keyed piece and inherit its stamp.
- A section's mail counts from `(key + 1) << 32`: keys below `2^30`, fewer than `2^32` pieces of mail a section. The
  actor's life does not count a section's mail.
- Sections ask nothing, hold nothing, are asked nothing, and give and take no resources. None of it is refused: a
  request to a keyed actor goes unanswered, and an ask from a section is never filed.
- A section's `Close`, `spawn`, and `watch` are not refused.
- A section's fault is logged as `EndFailed`, without its key.
- A keyed run is never cut at its slice, never fused, and not recorded for a harness (`take_turned`).
- A keyed run reads liveness from the table as the step began.
- `S: Copy + Default + PartialEq`, not `Plain`: a keyed actor's sections are not saved and do not move onto new code.
- One part's store of one actor is a `Box<dyn Any>`, downcast and looked up by key on every turn, not once a run.
- The stamp is carried by every piece of mail, in `Mailed` and in the stream's entry.
- Guests are not built: a hosted turn stamps `WHOLE`.
- The benchmark keeps its knobs and the actors it checks in statics, as `savina` does.

## Recommendation: what an ADR should say

Adopt:

- **An actor opts in with a section type and handlers that name their key.** The handler gets `&A` and `&mut S`. The
  borrow checker is the fence between keys, and the engine needs no lock.
- **Keyed mail is kept under a lane, `hash(actor, key)`, in place of a seat**, so a key's mail lands in a part, and
  the parts, the plan, the costs, and the workers stay as ADR-0011 has them. A keyed actor runs on as many workers as
  its lanes' parts are spread over that step, up to the count of parts.
- **A section lives in its lane's part** and moves with it. It is created and dropped in place, and nothing is merged
  at the fold.
- **The sender stamps the key at the typed send**, through a method of the receiver's handler trait that is `WHOLE` for
  every ordinary actor, so no second send verb exists and a send to an ordinary actor pays a constant.
- **A failed section turn puts its section back and ends nothing**, amending ADR-0009 explicitly for keyed actors.

Change from the model:

- **A section is a sender.** Its mail is numbered by the section, and ADR-0009's and ADR-0011's order rules read
  "by its source" with a section as a source.
- **Keys for answers, holds, and stream parts are return stamps**: computed by the asker as it stores its context,
  carried on the request or the `Open`, kept with the debt, and stamped by the engine. Each section keeps its own
  waiting contexts and debts.
- **`S: Plain + Copy`**, with a size an ADR states a cost for: free to 64 bytes, about 12 ns at 1 KB, memory-bound
  beyond. Either cap it, or let an actor give up rollback (and end on a fault, as today) for big sections.
- **Do not compare with the default after every turn**: drop sections at the default lazily, when a part is idle or
  trimmed (as `Shard::rest` gives back room), and never one that has sent mail.
- **Keep runs per key**: a keyed send joins the last piece when the receiver, kind, and key match.
- **Pay for the stamp only on keyed mail**: keep keyed entries in a list of their own in each part, or carry the key
  in the payload's head, so ordinary mail stays 96 bytes. ADR-0017 left the same choice open for the tie.
- **A section's turn is a type of its own** that offers no close, spawn, or watch, and no ask until each section keeps
  its own contexts.
- **Protocols**: a keyed actor is reachable only through a typed reference or path, unless a protocol states how each
  of its kinds is keyed.
- **Whole-actor mutable state, unkeyed mail, and guests** are each a further ADR: the first as a view (no barrier), the
  second as a step of its own, and the third with a decision on what a trap in one part does to a keyed actor.

Do not use it for the HTTP stack until sections may be non-flat and own their streams.

## Reinvented or library gaps

- **`KeyBits`** (`keyed.rs`, spike): a `Hasher` for keys, a multiply. The substrate's `IdBits` (`by_actor.rs:14`) hashes
  actor ids by their own bits, which suits ids, which are already hashes, and not small keys in a row; and
  `aethereal-actor` cannot depend on the substrate. *Different meaning*; a shared one would belong in
  `aethereal-data`, beside the ids.
- **`lane`** (`stream.rs:787`, spike) and the benchmark's `mix`: the SplitMix64 finaliser, twice. *One user* each;
  `aethereal-math` or `aethereal-data` has no integer mix, and one would belong in `aethereal-data`.
- **`view_part`** (`engine/mod.rs:2835`, spike) repeats `view.rs`'s private `part` (line 305), to find the part a lane
  falls in from outside a view. It should be `pub(crate)` in `view.rs`.
- **The benchmark's harness** (`keyed_sections.rs`, `run` and `main`) copies `savina.rs`'s. *One user*; an example
  cannot share code with another without a module both include.

## 10. Asks, answers and debts in sections

**Question.** The sections above never asked, held, or were asked. What an actor waits on and what it owes are kept
beside it in its seat (`Held { keeping, waiting, debts }`, `engine/mod.rs:301` on `main`), in the part of its seat,
which another worker may own in the step. Can each section keep its own, in its lane's part, with answers finding their
section by a return stamp, and what does that cost?

**Rulings this section works under.** A failed turn closes the whole actor, as ADR-0009 says, and nothing is rolled
back; whole-actor turns as a barrier are out of scope. The first spike's rollback is switched off, not left in: a
section that holds a request holds a `Held<A>`, which is not `Copy`, so the bound had to go. `Sectioned::Section` is
now `Default + Send` (`keyed.rs:48`), and the default check is the type's own `Sectioned::is_unused` (`keyed.rs:58`),
since a `Held<A>` is not `PartialEq` either.

**Run it.** `cargo test -p aethereal-substrate --lib keyed` (eight tests, `engine/keyed/tests.rs`), and
`cargo run --release -p aethereal-chassis-headless --example keyed_sections ask-keyed ask-one ask-children hold-keyed
hold-one hold-children`. Commit `f68cc69` is the benchmark's ordinary programs on the engine before return stamps, the
baseline for what they cost.

### Answer, first

It works, all five points, and the shape is simple: **a section keeps a `SectionHeld { waiting, debts }` beside its
state in its lane's part, built from the same `Waiting` and `Debts` a seat keeps, and every answer carries a return
stamp that routes it to its section's lane as keyed mail.** Nothing new is locked or merged: the worker that owns the
part owns the section's requests, as it owns its state.

Three things were not obvious, and two of them change the model:

- **The return stamp is the asking section, always.** The context is filed in the section whose turn asked, in that
  section's part. An answer stamped with any other key goes to another section, which waits on nothing of that count,
  and is dropped as a stray. So the key the asker computes from its context (`KeyOf<Answer<A, C>>`, as
  `#[answer(key = ..)]` would declare) can only ever be right when it equals the key of the turn asking, which the
  engine already knows. The declaration is redundant, and a mistake in it loses the answer silently.
- **The part, not the section, numbers a section's mail.** With one count per `(actor, part)` (`first_count`,
  `engine/keyed/mod.rs:112`), no two sections that run at once share a count, and a section dropped and made again
  goes on from its part's count. So "keep a section that has sent mail" goes away: the drop rule is only "unused, and
  waiting on nothing, and owing nothing". A section's count was what pinned every section that ever sent.
- **Ending is a sweep between steps.** The worker that runs the closing turn owns one part. The debts of the actor's
  other sections are in every other part, so the engine sweeps them once the actor is gone, between steps, on the
  stepping thread (`sweep_sections`, `engine/keyed/mod.rs:278`), at a cost of a look at every part for each keyed actor
  that ends.

### What was built, and where

File references are to the branch at the commit that adds this section.

- **The stamps.** `Mailed::returns` (`aethereal-actor/src/effect.rs:144`), the return stamp a request carries, copied
  into the stream's `Envelope` (`aethereal-substrate/src/mail.rs:31`) and read by the receiver's turn as
  `Handed::returns` (`stream.rs:697`). `Stamps { section, returns }` (`effect.rs:198`) is what a turn carries: the
  section it runs for and the return stamp of the request it handles, set on `Turning` by `stamped`
  (`turn/mod.rs:1696`) and kept on the turn's `Seat`.
- **Asking.** `Toward::ask_keyed` (`turn/to.rs:128`) asks a section of a keyed actor from a whole actor, stamping the
  receiver's key as `send_keyed` does. `Toward::ask_returning` (`turn/to.rs:144`) asks from a section: it calls the
  asker's `KeyOf<Answer<A, C>>` (`keyed.rs:22`, which now names what the key is read from, `type From`) on the context
  and stamps the result as the request's `returns`, through `Seat::request_stamped` (`turn/mod.rs:963`).
- **Echoing.** Every answer, whoever gives it, goes to the section the request's return stamp names, as its
  `section`: an answer returned now (`turn/mod.rs:941`); a held one, whose `Owed` now carries the holder's section and
  the request's return stamp (`held.rs:38`, set at `turn/mod.rs:1033`, read at `:1057`); and every `Unanswered` the
  engine says, through `Asked::unanswered`'s new `returns` (`turn/mod.rs:305`): `Failed`, `Unreplied`, and
  `Unreadable` from the turn (`ask::owed`, `engine/ask/mod.rs:254`), `Ended` for a request to a gone actor
  (`ended_before`, `say_unanswered`, `engine/mod.rs:2647`), `Ended` for a debt at the closing turn, and `Unreplied` for
  a let-go debt (`Debt::returns`, `engine/debts/mod.rs`). An answer stamped keyed is keyed mail, so it is never fused
  (the first spike's rule in `hand`).
- **A section's requests.** `SectionHeld { waiting, debts }` (`engine/keyed/mod.rs:98`), one per section that waits or
  owes, in `ActorSections::held` (`:83`) beside the store of states and the part's count. `keyed_turns` (`:174`) takes
  the section's `SectionHeld` out for its run, finds each answer's context in it (`:218`), and files what each turn
  asked, held and answered with the same `tied` the seat's turns use (`:247`); a failed turn goes through `Ran::of`,
  which ends the actor (`:249`). The rest of the run is never handed once the actor ends.
- **Letting go.** A `Held<A>` a section drops is counted as a seat's is (ADR-0017); `let_go` finds the debt in the
  section the `Owed` names (`Sections::change`, `engine/keyed/mod.rs:141`, from `engine/mod.rs:2667`).
- **The drop rule.** After a run, a section whose `SectionHeld` is empty is handed to `RunsSections::release`
  (`keyed.rs:168`), which drops its state if `is_unused`; one that waits or owes keeps both (`engine/keyed/mod.rs:262`).
- **Ending.** `sweep_sections` (`engine/keyed/mod.rs:278`), called as each step is applied (`engine/mod.rs:1219`,
  `:1473`): for each keyed actor the table no longer seats, every part's sections of it are taken out; each debt is
  answered `Ended` to its asker's section, and each waiting context is dropped with its pins.
- **Handlers.** `hand_section` (`keyed.rs:87`) takes any `Handled` (a kind, or an `Answer<A, C>`) and a handler that
  returns what an ordinary handler returns (`()`, `A`, `Later<A>`, `Reply<A>`), and sends an answer given now, as
  `Reached::hand` does. `is_answer_of` (`keyed.rs:72`) is the dispatch's test for an answer.
- **For tests.** `Engine::section_holds` and `Engine::keeps_sections` (`engine/keyed/mod.rs:328`, `:335`).

### The five points

1. **A section asks, and its answer comes to that section, with sections on two workers asking in one step: works.**
   `sections_on_two_workers_ask_in_one_step_and_each_answer_comes_back_to_its_own_section`
   (`engine/keyed/tests.rs:640`) runs two keys' parts on two workers in one step, each section asking the oracle, and
   each section hears its own answer with its own context. Broken on purpose, it fails when replies are not stamped
   with the return stamp, and when every part numbers its sections' mail from one count (the oracle's part drops one
   of two requests with one id).
2. **A section holds a request and answers it in a later turn: works.**
   `a_section_holds_a_request_and_answers_it_in_a_later_turn` (`:662`), and
   `a_held_request_a_section_lets_go_of_is_answered_unreplied` (`:680`) for a held request dropped. The benchmark's
   `hold-keyed` holds a request in a section, asks the stamper from the section with a `Held<Got>` inside its context,
   and answers from the stamp's answer, 262,144 times a run.
3. **Ending answers every debt and drops every wait: works.**
   `a_fault_in_any_section_ends_the_actor_and_every_section_s_debts_are_answered_ended` (`:709`) and
   `a_section_that_closes_its_actor_ends_it_and_every_section_s_debts_are_answered_ended` (`:726`): two sections owe a
   request, a third waits on an answer that never comes, and a fourth fails (or closes). The asker hears `Ended` for
   both, the engine goes idle, and no part keeps anything of the actor. Broken on purpose (no sweep), both fail.
4. **A section is dropped at its default only when it waits on and owes nothing: works.**
   `a_section_left_at_its_default_while_it_waits_is_kept_and_its_answer_finds_its_context` (`:740`): the section asks
   and changes nothing, so it is at its default with a context filed; it is kept, the answer finds the context, and
   once it is told to forget, it is dropped although it has sent mail. Broken on purpose (the section's requests
   dropped with it), six of the eight tests fail.
5. **Costs: measured,** below.

### What it costs

Best of three runs, nanoseconds a request (a put for `savina`'s programs, a piece of mail), each cell the range of two
passes. 64 senders each ask 4,096 questions, 64 at a time; the dictionary has 4,096 keys. `ask-*` answers at once;
`hold-*` holds each question, asks a stamper, and answers when the stamp comes back.

**(a) What the return stamp costs mail that is not keyed.** The engine before return stamps (`f68cc69`) against after,
on the same programs. The machine drifted between passes, so the change is read against the baseline pass run between
the two passes after.

| program         | workers | before      | after       | change              |
|-----------------|--------:|------------:|------------:|---------------------|
| `ask-one`       |       1 |     332–346 |     343–355 | 0 to +3%            |
| `ask-one`       |       4 |     193–204 |     198–210 | noise               |
| `ask-children`  |       1 |     355–373 |     360–383 | noise               |
| `ask-children`  |       4 |     113–118 |     118–122 | 0 to +3%            |
| `hold-one`      |       1 | 9,709–10,572 | 11,051–11,740 | +5 to +11%      |
| `hold-one`      |      16 |  8,124–8,478 |  9,171–9,730 | +13 to +20%     |
| `hold-children` |       1 |   1,682–1,797 |   1,933–1,937 | +8%           |
| `hold-children` |       4 |     851–856 |   1,156–1,165 | +35%            |
| `hold-children` |      16 |   1,178–1,278 |   1,902–1,958 | +50%          |
| `pingpong`      |       1 |   1,096–1,146 |   1,165–1,169 | +2%           |
| `big`           |       1 |     168–178 |         186 | +4.5%               |
| `churn`         |       1 |     209–221 |         226 | +2%                 |
| `world`         |       1 |     150–159 |         163 | +2.5%               |
| `fjthrput`      |       1 |     241–253 |         284 | +12%                |

`ask-*` on 16 workers moves by 10 percent between passes on both builds and is left out.

What grew: `Mailed` 104 → 112 bytes, the stream's `Envelope` 72 → 80 and its `Entry` 104 → 112 (the first spike's
section stamp and this return stamp together: 16 bytes on every piece of mail), `Owed` 40 → 56 and the `Debt` the
engine keeps by 8, a turn's `Turning` and `Seat` by 16, and the reply path by one store of eight bytes. Plain mail pays
2 to 5 percent, as the first stamp did. Held requests pay much more, worst on many workers, and this spike did not
find why: the debts list a turn that holds copies is larger, and `holders` changes compare a larger `Owed`, but neither
explains 50 percent on 16 workers. It needs a profile before an ADR prices the stamp.

**(b) A round trip through sections, an ordinary actor, and children.** After return stamps:

| program         | 1 worker      | 4 workers   | 16 workers  |
|-----------------|--------------:|------------:|------------:|
| `ask-keyed`     |       422–423 |     131–132 |     139–141 |
| `ask-one`       |       343–355 |     198–210 |     276–301 |
| `ask-children`  |       360–383 |     118–122 |     111–127 |
| `hold-keyed`    |   1,332–1,336 |     573–576 |     586–602 |
| `hold-one`      | 11,051–11,740 | 9,159–9,205 | 9,171–9,730 |
| `hold-children` |   1,933–1,937 | 1,156–1,165 | 1,902–1,958 |

- **Answered at once, sections cost what children cost on many workers, and 20 percent more than either on one.** A
  question to a section is one run, as a put was (section 9, surprise 6): the per-run cost is paid per request.
- **Held, sections win outright**, 1.3 microseconds a request on one worker against 1.9 for 16 children and 11 for one
  actor, and two to three times faster than children on 4 and 16 workers. Not because sections are cheap, but because
  what an actor holds is costly in the seat: a turn that holds or answers copies the whole list of the actor's debts
  (`Debts::filed`, `engine/debts/mod.rs:110`), and an answer finds its context by a walk over every request waiting
  (`Filed::at`, `engine/ask/mod.rs:44`). The one actor holds about 4,000 at once, each child about 250, each section
  about one. Splitting the `Held` is what wins, and an ordinary actor that holds many would want those two lists
  keyed, sections or not.

### What was not obvious

1. **The return stamp could be the engine's, not the asker's** (above). If it is the asker's, a key that disagrees
   with the asking section loses the answer as a stray, and the spike does not catch it. A key from the context is only
   needed for an answer that should reach a section other than the one asking, which is an ask from the whole actor
   (a `begin`, or the barrier turns this section leaves out) on behalf of a section: and then the context must be filed
   in that section's part, which the asking worker does not own, so it would be filed at the fold.
2. **A section's `Held` beside its state, not inside it, makes the drop rule cheap.** The engine keeps an entry only
   for a section that waits or owes, so "is its `Held` empty" is "has it an entry"; and the state is checked with
   `is_unused` only for a section with none. A section's state can still be dropped while it waits without losing
   anything (a default state is made again), so the rule that matters is that the requests are never dropped with it.
3. **`Owed` needs both sections.** The return stamp, for the answer; and the holder's section, so that the count of
   values holding a debt, taken after each part (ADR-0017's `holders`), finds the debt in the section that holds it.
   A debt's holder section has to travel inside the `Held<A>`, through any bytes it is written into.
4. **Releasing at the end of a run, not after each turn, removes the count problem the first spike met.** A section
   dropped after a turn and made again by the next item of the same run would number its mail from the start again in
   the same step. Dropping only after the run means no section is made twice in one part in one step, and the part's
   count does the rest.
5. **The actor is gone only the step after its closing turn.** A failed section turn ends the actor at the fold; the
   closing turn runs the next step in the seat's part; the table drops the actor at that fold; only then does the
   sweep run. Sections in other parts keep running until the table drops it, so they may still ask and hold during
   that step: the sweep, which runs after, catches what they filed. ADR-0011's "an actor that ends handles no more
   mail" holds from the step after the closing turn.
6. **The ending order is not the asking order.** The askers of a gone actor's debts are told in the order of parts
   and of a hash, not the order they asked. ADR-0017 promises only that each is told once.
7. **An answer to a section is keyed mail, so it never joins a run and is never fused.** A round trip from a section
   costs at least a step each way, as any mail between parts does.

### Corners cut

- The return stamp from the context is not checked against the asking section.
- Deadlines on what a section asks are never kept: the section's asks file no `Expiring`, so they are never answered
  `Expired` (`engine/keyed/mod.rs`, the scratch `expiring`).
- A section cannot run a job: a job's answer is sent to the whole actor.
- The outside seat (ADR-0036) and a guest's records answer to the whole actor: their `Owed` carries no sections.
- A keyed actor asking a keyed actor needs both stamps at once; the two verbs, `ask_keyed` and `ask_returning`, each
  set one. `Seat::request_stamped` takes both, so a third verb is a line.
- A whole-actor turn of a keyed actor still cannot ask for a section (point 1 above).
- The sweep looks at every keyed actor every step to find those gone, and at every part for each; a real one would be
  handed the endings at the fold.
- A section left neither waiting nor owing by a debt let go at the end of a part keeps its state until its next run.
- A part's count starts at `(part + 1) << 40`: a part's sections send fewer than `2^40` pieces of mail.
- The two stamps are carried by every piece of mail, in `Mailed` and the `Envelope`.
- The first spike's rollback measurement (`rb*-keep`) is gone with the rollback.

### What an ADR would need to say

- **A section keeps its own requests**: the contexts of what it asks, and what it holds, beside its state in its
  lane's part, in the same structures a seat keeps. A section that waits on an answer or owes one is never dropped;
  any other is dropped once it is unused.
- **An answer goes back to the section that asked**, stamped by the engine from the turn that asked, carried on the
  request, and put on whatever answers it: the receiver now, the receiver later through a held request, or the engine.
  Whether the author also declares it (`#[answer(key = ..)]`) is the question below.
- **A held request names its holder's section** as well as its holder, wherever it is written.
- **A keyed actor's mail is numbered by the part**, one count for each actor and part, and ADR-0012's mail id stays
  `(actor, count)`.
- **A keyed actor that ends** has every section's debts answered `Ended` and every section's waits dropped, between
  steps, once its closing turn has run; until then its sections run.
- **What the stamps cost**, priced against a profile of held requests on many workers, and whether they ride on every
  piece of mail or only on keyed mail and requests.

### Reinvented or library gaps

- **`SectionHeld`** (`engine/keyed/mod.rs:98`) is the seat's `Held` without its actor or `Keeping`: it reuses
  `Waiting` and `Debts` as they are, and adds only `is_empty` to each. Nothing duplicated.
- **`Owed::names`** (`held.rs:58`) compares a debt by holder, asker and request, ignoring the sections, where
  `has_answered` compared whole `Owed`s. *One user.*
- **`found`**, a root's reference from its id, is written in both the tests and the benchmark. *One user* each; the
  engine has no public way to hand back a root's typed reference from `add_root` but `add_sectioned_root`.
- **`sweep_sections`** writes the engine's `Unanswered` mail by hand, as `expire_due` does; `fold_brought` would have
  routed it but finds each piece's part by its receiver's seat, which is wrong for keyed mail. *Different meaning*:
  `fold_brought` should find the part from the stream entry's key, which would serve both.
