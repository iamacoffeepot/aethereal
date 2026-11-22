# Spike: a cycle of kinds hashed as one unit when the program is built

Spike code, never merged. Branch `spike/kind-cycles`, cut from the parked commit `2d17695` (a protocol reference's
layout names its protocol, ADR-0016), which stopped because `aether.window.pointed` names its own id.

Toolchain: `rust-toolchain.toml` pins `1.97.1` (`rustc 1.97.1 (8bab26f4f 2026-07-14)`), with clippy, rustfmt,
rust-src, and the wasm32 target. Nothing was installed.

## Verdict

**Works, with limits.** Every non-generic derived kind gets a static node; `KindId::of::<K>()` stays a constant that
walks the graph of nodes, finds `K`'s strongly connected component, and hashes a cycle once. `Pointed` builds and has a
constant id, as do a two-kind cycle, a cycle through a named protocol and its answer, and kinds outside each cycle
that refer in. A flat description of a cycle makes the same ids at run time, in whatever order it lists the members.
Kinds with no references keep their ids (every existing `// Tripwire:` passes). The whole workspace builds and its
1,785 tests and 167 doctests pass.

The limits:

- The pointer from the trait to a node must be a **raw pointer**. A constant holding a `&'static` to a static makes
  the compiler evaluate the static first, and the cycle comes back as E0391.
- A constant cannot compare pointers, so each node carries its **site** (`module_path::Type@file:line:column`), and
  the walk tells nodes apart by it.
- **No generic kind in a cycle.** This is enforced when the program is built, in two ways (answer 5).
- **Two members of one name and one blanked layout** stop the build.
- **Cost grows with the size of a cycle.** About 4 to 6 ms of const-eval per id for realistic shapes, but about 200 ms
  per id for a 64-member ring. The whole-workspace build is unchanged within noise.

## What was built

| Where | What |
|---|---|
| `aethereal-data/src/kind/shape.rs` | `KindShape { name, site, shape: &'static Shape }`, `Site::{Defined { at }, Unsited}`, and `KindPtr`, a `*const KindShape` that is `Sync`. |
| `aethereal-data/src/kind/graph.rs` | The const walk: breadth-first discovery into a fixed table, iterative Tarjan, then cycles resolved in completion order. `kind_id(KindPtr)`. |
| `aethereal-data/src/kind/layout.rs` | The const layout walk takes a `Walk`: edges consumed in shape order, with three modes, `Acyclic`, `Blank`, and `Positioned`. The flat layout takes the same three modes through `Members`. |
| `aethereal-data/src/kind/cycle.rs` | `KindDescription::{Alone { name, schema }, Cycle { members, this }}`, `CycleMember`, `CycleFault`; `KindDescription::of(KindPtr)` and `.id()`. |
| `aethereal-data/src/schema/protocol.rs` | `ProtocolShape::{Told(&[KindPtr]), Named(&[Answered { kind: KindPtr, answers }])}`. Flat: `Referent::{Kind(KindId), Member(u32)}` in `ProtocolNode`. `KindIds` / `AnsweredIds`: const sorted id sets, which replace `Ascending`. |
| `aethereal-data/src/kind/mod.rs` | `Kind::KIND: KindPtr`. Its default points at a node made in a constant (`Site::Unsited`). |
| `aethereal-data-derive/src/plain/{mod,kind}.rs` | A non-generic type gets `Schema` and `Kind` without field bounds, and a `static NODE` (below). A field's `SHAPE` is spanned at its type. |
| `aethereal-actor/src/declared/protocols.rs` | `Kinds::NODES`. `Listed::PROTOCOL` and `CarriedBy::ANSWERED` are built from nodes, not ids. |
| `aethereal-actor/src/section/state.rs` | A state's references are written as `Referent::Kind` ids, sorted from `KindIds`. |
| tests | `kind/cycle_tests.rs` (data), `declared/protocols.rs::cycle_tests` (actor), `pointing/tests.rs::pointed_is_a_cycle_of_one_with_a_constant_id` (window). |

## Evidence

- `pointed_is_a_cycle_of_one_with_a_constant_id` (window, real `ProtocolRef<Hearer>`):
  - `const POINTED = KindId::of::<Pointed>()` evaluates.
  - `KindDescription::of(Pointed::KIND)` is a cycle of 1, and its `.id()` equals `POINTED`.
  - `Reach`, outside the cycle and referring in, is `Alone` with an equal id.
- Data tests (stand-in reference type `To<P>`):
  - `cycle_ids_are_constants` (tripwire) pins:
    - `Subscribe` `0x27d56a99a8bcf760`
    - `Update` `0x837aa9d7c71dc731`
    - `Watch` (outside, refers into both) `0xe298084c7f937fb0`
    - a Pointed-shaped cycle of one `0x45f0aefff1a463b2`
    - its `Reach` `0xb77a1c97f68143d1`
  - `descriptions_make_the_constant_ids`: the flat path equals the const path for all five.
  - `listing_order_does_not_change_an_id`: the members reversed and their `Member` indices swapped give the same id.
  - `a_changed_member_changes_every_id_that_reaches_it`: widening `Update.value` changes `Subscribe`, `Update`, and
    `Watch`.
  - `a_cycle_redefined_alike_has_the_same_ids`: the site never enters the hash.
  - `members_of_a_cycle_have_distinct_ids`.
  - `a_described_cycle_that_is_not_one_is_refused`.
- Actor test `a_cycle_through_a_named_protocol_and_its_answer_has_constant_ids`. `#[protocol(Ask(answers = Reply))]`
  with `Ask { back: ProtocolRef<Asking> }` and `Reply { to: ProtocolRef<Asking> }` is a 2-cycle that runs through an
  entry and its answer. Const and flat ids agree. `Hold`, outside, agrees too.
- `pinned_ids_match`, `pinned_layout_hashes_match`, and `deep_description_id_matches` pass unchanged: a kind with no
  reference hashes as at `2d17695`, and so as on `main`.
- Negative cases, in a scratch crate on the branch's crates:

  | Case | Result |
  |---|---|
  | Non-generic `A { to: ProtocolRef<(G<u8>,)> }`, `G<T> { back: ProtocolRef<(A,)>, value: T }` | E0080: evaluation panicked: a cycle of kinds passes through a generic kind or one whose `Kind` is written by hand … (reported at `KindId::of::<A>`) |
  | All-generic `G<T> ↔ H<T>` | E0275: overflow evaluating `(G<Leaf>,): Listed`. Generic derives keep their bounds. |
  | `A`, `B` both `neg.same`, each referring to the other | E0080: two kinds of one name and one layout are members of one cycle |
  | Generic `G<T> { to: ProtocolRef<(A,)> }` outside `A`'s cycle | builds |

## Cost

Clean `cargo build -j 8 --workspace --all-targets --all-features`, fresh target directories, same machine:

| | `a7a4db5` (main; `2d17695` does not build) | spike |
|---|---|---|
| whole workspace | 101.5 s | 103.4 s |
| `aethereal-window` lib unit | 3.04 s | 3.17 s |
| `aethereal-ui` lib unit | 10.74 s | 10.66 s |
| `aethereal-data` lib unit | 3.30 s | 4.02 s (the new code) |
| `aethereal-actor` lib unit | 10.24 s | 10.89 s |

A touch-and-rebuild of window, ui, or the headless chassis is equal to within 0.1 s (3 rounds each). Incremental
compilation reuses the evaluated ids.

Const-eval alone (`cargo check`, `CARGO_INCREMENTAL=0`, 64 ids in one constant):

| 64 kinds | seconds |
|---|---|
| no references | 0.25 |
| 32 two-kind cycles | 0.52 |
| each referring to a named protocol of 4 kinds, 2 with answers | 0.60 |
| one 64-member ring cycle | 13.0 |

No const-eval limit or `long_running_const_eval` lint was hit. The ring's cost is the walk repeated per id: each id
walks the whole cycle and writes every member's layout twice, and the members are ordered by an O(n²) insertion sort.
The `[KindSet; 64]` scratch arrays a named protocol's fold initialises are a further cost the real implementation
should shrink (answers are at most one kind).

## Answers

**1. The trait-level cycle.** It is an inductive cycle through four impls:

- `Pointed: Kind`, whose derived `impl Kind … where PointerEvent: Schema` copied the `Schema` field bounds;
- `PointerEvent: Schema` (derived, `where Over: Schema`);
- `Over: Schema` (`where ProtocolRef<Hearer>: Schema`);
- `impl<P: Listed> Schema for ProtocolRef<P>`, then `impl<A: Kind> Listed for (A,)`, which needs `Pointed: Kind`.

`Kind: Schema` takes no part: a supertrait is not proven again when an impl is selected. The removable part is the
derive's field where-clauses on a **non-generic** type. A where-clause on a concrete type guards nothing an impl needs:
a field whose type is not `Schema` still fails, at its `<T as Schema>::SHAPE`, now spanned at the field's type. The
codec bounds (`Encode` / `Decode` / `Plain`) are kept: `ProtocolRef`'s codec does not ask for `Listed`, so they are not
in the cycle. The existing `HoldsCell` `compile_fail` doctest still fails. `impl<P: Listed> Schema for ProtocolRef<P>`
stays, as it is what names the protocol. Generic derives keep their bounds.

**2. A cyclic shape graph at compile time.** Yes, through a raw pointer.

- `static NODE: KindShape = KindShape { shape: &<T as Schema>::SHAPE, … }`, with `Kind::KIND = &raw const NODE`, builds
  for `Pointed`.
- With `const KIND: &'static KindShape = &NODE` it does not. Const-evaluating `KIND` evaluates the static's
  initializer (E0391), which is what makes it a cycle. The same happens in a 4-line repro with
  `static S: Node = Node { next: C }; const C: &Node = &S;`.
- A `*const` in a constant is not followed. A `const fn` may read an immutable static through it.
- The node is reached through `Kind::KIND`, `Kinds::NODES`, `Listed::PROTOCOL`, and `Shape::Reference`.

**3. The hash in const eval.** Yes. `KindId::of::<T>() = const { kind_id(T::KIND) }`. The walk has four steps:

1. **Discover.** Breadth-first from the root into a fixed table of at most 128 kinds and 1,024 edges; past either, a
   const panic stops the build. Each kind's edges are contiguous, in the order a layout walk meets references: a
   told list in order, a named entry's kind then its answers.
2. **Components.** Tarjan's algorithm with explicit frame and node stacks; no recursion over the graph. The only
   recursion left is within one kind's shape tree, which is bounded by the type and does not follow references, as
   before.
3. **Resolve.** Components are resolved in completion order, so every component a cycle reaches is known first.
   - A lone kind with no self-edge is hashed exactly as at `2d17695`: its references are sets of ids, ascending.
   - A cycle has every member blanked, then gets each member's blank layout, then the members ordered by
     (name bytes, blank layout).
   - A tie panics.
   - Then the positions, then `c = H(CYCLE, n, for each position: name length, name, positioned layout)`.
   - Each member's id is `from_layout(name, H(CYCLE, c, position))`, so `from_layout` stays the one place an id is
     made.
4. **Encoding inside a cycle.** A set is the count of outside ids, the ids ascending, then the count of members. In
   the positioned layout the members' positions follow, ascending. A named protocol's members' entries are folded by
   position, or, while blank, in the order of their answers' hashes.

**4. The flat form.** Yes. A reference's protocol holds `Referent::{Kind(id), Member(listed)}`, ids then members,
ascending.

- `KindDescription::Cycle { members: [CycleMember { name, schema }], this }` carries the whole cycle.
- The host checks that every `Member` is inside the list and that the members are one strongly connected component (a
  lone member names itself). It then recomputes the blank layouts and the order itself, refuses a tie, maps listed
  places to positions, and hashes.
- Faults: `NoSuchMember`, `MemberPastList`, `NotOneCycle`, `Tied`.
- `KindDescription::of(KindPtr)` builds the description at run time from the same graph, using pointer identity,
  which a running program has.
- Identical ids on both sides are shown for every case above, the reversed listing included.

A **state's** flat schema needs only ids: no protocol takes an actor's state. The section writer is unchanged in
shape. A state whose own kind sat in a cycle would need the cycle form in the section; that is not built (see rules).

**5. Generic kinds.** A generic kind has no static. Its `Kind::KIND` is the trait default, a node made in a constant,
with `Site::Unsited`. The walk never merges two unsited nodes.

- **Outside a cycle** it works: `Open<S>`, `Part<S>`, `Toolbox<L, R>`, and the generic case above build.
- **In a cycle with at least one non-generic member**, the static breaks the const cycle, so the walk sees it and
  refuses it with a readable const panic, E0080.
- **In a cycle of generic kinds only**, the generic derive's bounds bring back E0275 at the trait level. Without those
  bounds it would be rustc's const cycle, E0391.

So "a cycle may not pass through a generic kind, or a kind whose `Kind` is written by hand" is enforced when the
program is built. It is acceptable: no kind today needs it, and a generic member would need an identity for each
instance, which a constant cannot get. Its weaknesses:

- The E0275 form's message is rustc's.
- Both errors appear where an id is first computed, not where the kind is defined.

**6. Cost.** See [Cost](#cost): no measurable change to window, ui, or the workspace; a few milliseconds per id
for realistic shapes; quadratic-to-cubic growth in cycle size. The ids of kinds with no references are unchanged: the
existing tripwires pass.

**7. Fallback.** Not needed. Two things were found on the way:

- The route through `&'static` fails (answer 2).
- Pointer identity is unavailable in const eval. A site string is the stable route.

A naming-by-name identity (in-cycle kinds by name) was considered and rejected: two versions of one kind with the same
name and shallow layout, reachable together, would be merged and given one id. No run-time id computation is used:
`KindDescription::of` runs at run time only to describe a kind to a host, and checks against the constant.

## Shape of a real implementation

Derive output for a non-generic `#[kind(name = …)]` type:

```rust
impl Schema for Pointed { const SHAPE: Shape = /* fields, no where-clause */; }
const _: () = {
    static NODE: KindShape = KindShape {
        name: <Pointed as Kind>::NAME,
        site: Site::Defined { at: concat!(module_path!(), "::", stringify!(Pointed), "@", file!(), ":", line!(), ":", column!()) },
        shape: &<Pointed as Schema>::SHAPE,
    };
    impl Kind for Pointed {
        const NAME: KindName = KindName::new("aether.window.pointed");
        const KIND: KindPtr = unsafe { KindPtr::defined(&raw const NODE) };
    }
};
```

Trait changes:

- `Kind` gains `const KIND: KindPtr`, with a default for generic and hand-written kinds.
- `Schema` is unchanged.
- `Kinds` gains `NODES`.
- `ProtocolShape` and `Answered` hold `KindPtr`.
- `ProtocolNode` holds `Referent`.
- `Ascending` gives way to const sorted id sets.

The hash, in outline:

```
kind_id(root):
  graph = discover(root)            // BFS, sites dedupe, unsited never merge, caps panic
  comps = tarjan(graph)             // explicit stacks, completion order
  for comp in comps:
    if lone and no self-edge: id = from_layout(name, layout(shape, Acyclic))
    else:
      require every member sited
      blank[m] = layout(m, Blank); order by (name, blank); tie -> panic
      c = H(CYCLE, n, [len(name), name, layout(m, Positioned)] in order)
      id[m] = from_layout(name, H(CYCLE, c, position(m)))
  return id[root]
```

Before merging, also:

- Emit `const _: KindId = KindId::of::<T>();` for each non-generic kind, so an error is reported at its definition.
- Make the scratch arrays smaller.
- Add a `compile_fail` doctest for each refusal.
- Clear the clippy pedantic findings: casts and visibility. The spike leaves them.

## Rules an ADR would state

1. A kind's references reach kinds through their nodes. The ids follow from the graph the nodes make. The graph's
   edges are the kinds each reference's protocol lists, a named protocol's answers included.
2. A strongly connected component of more than one kind, or of one kind with an edge to itself, is a cycle. A kind in
   no cycle is hashed as before, with each kind it names written as its id.
3. A cycle is hashed once:
   - Members are ordered by name bytes, then by their layout with members blanked.
   - Members are named by position in each other's layouts.
   - The cycle hash folds each member's name and positioned layout in order.
   - A member's id is the cycle hash with its position.
   - A change to any member changes every member's id and every id that reaches the cycle.
4. Two members of one name and one blanked layout stop the build. Bisimilar members always tie, so an accepted cycle
   holds no duplicate member. Colour refinement could accept the rest later.
5. A cycle's members are non-generic derived kinds. A generic or hand-written kind may hold references but may not be a
   member of a cycle. Both are enforced when the program is built.
6. A kind's graph reaches at most 128 kinds and 1,024 edges; beyond that the build stops.
7. A node's site tells it apart and is never hashed: one kind defined twice alike has one id.
8. A member of a cycle is described with its whole cycle. A host recomputes the order, and refuses a description that
   is not one strongly connected cycle, names a member past the list, or ties.
9. An actor's state is never a member of a cycle; the state section carries ids only. To be stated, or the section
   gains the cycle form.

## Choices made here (nothing was asked)

- **Tie:** refuse, rather than refining by colour.
- **Site:** module path, type name, file, line, column. No package version: a registry crate's `file!` already holds
  its versioned directory, and `env!` is not guaranteed outside cargo.
- **Encoding inside a cycle:** each member's positions follow the outside ids. A kind outside any cycle keeps
  `2d17695`'s bytes.
- **Member id:** `from_layout(name, H(CYCLE, c, position))`, so that `from_layout` stays the only place an id is made.
- **Caps:** 128 kinds and 1,024 edges.
- **Answers:** at most 4 per named entry (asserted).
- **`KindPtr: PartialEq`:** compares ids, at run time only.
- **Flat wire form:** `ProtocolNode` now holds `Referent`, a selector before each id. This is pre-1.0, and
  `2d17695` was never merged.
- **Baseline for cost:** `a7a4db5`, as `2d17695` does not build.
