# ADR-0031: Draw lists

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

Within one pass, placements are drawn in no order an actor may rely on (ADR-0023), and order is declared where drawing is described: the order of a frame's passes, which their author writes, and the order of one placement's records, which one actor writes (ADR-0025). Across actors this holds: order between what different actors draw is the order of passes, and no actor agrees on numbers with another.

Some drawing needs an order within one pass that changes every frame, among many placements of one actor:

- **Terrain streamed in chunks, held in slots that are used again.** A chunk that comes into range takes the slot of one that left, and each slot has placements that stand for as long as the slot does.
- **Each slot's placements blend by alpha, testing depth and writing none**, and their edges overlap their neighbours', painted soft. Where two overlap, the nearer chunk must be drawn over the farther, and which is nearer changes as the camera moves.

None of the ways a program already has to declare order fits it:

- **One placement's records**, drawn in order as they are one actor's (ADR-0023), would have every slot's data in one placement: one storage binding for every slot, at most 128 MiB on a device of default limits (`max_storage_buffer_binding_size`), a table of slots beside it, and a search in the vertex stage for which slot a vertex belongs to.
- **Passes** order what they draw, but a frame's passes are fixed when it is made (ADR-0025: `CreateFrame`), each costs about 35 µs, and the slots are many and change their order every frame.
- **Depth, or a blend that needs no order.** A program's alpha turned into coverage (ADR-0021: `Coverage::Alpha`) gives an edge of four samples five levels, where a blend gives it every level its alpha has, so the soft painted edges change. A depth for each chunk's rank in the order needs a precision of depth the program does not have: the chunks lie on one surface.

The fault seen was of the same kind. Placements of one program in one pass were drawn in the order of a hash map of their ids, and every id the render root makes is taken from one count, so making one more object of any sort at startup, unrelated to the terrain, moved a seam between two chunks (#294). Each build drew the same picture every run, and the order was still nobody's.

## Decision

**An actor may declare the order of its own placements within a pass, by sending each frame the list of them to draw, in order.**

```rust
Place { mesh: MeshId, instances: Drawn, joins: Vec<Join> }                    -> Made<PlacementId> | Refused
struct Join { frame: Frame, pass: PassName, program: ProgramId, bindings: Vec<Bind>, order: Order }
enum Order { Loose, Listed }     // Join::to::<P>(..) is Loose, as every join has been; Join::listed::<P>(..) is Listed

DrawList { frame: Frame, pass: PassName, placements: Vec<PlacementId> }      // no answer; DrawList::of::<P>(frame, placements)
```

- **A join is loose or listed.** A loose join is drawn in every frame, as ADR-0025 draws a join. A listed join is drawn only in the frames its placement is listed in, for that frame and pass. A placement may join one pass loose and another listed: a depth pass that needs no order, and the pass that blends, which does.
- **A listed placement draws only in the frames it is listed in, in list order.** Left off a frame's list, it is not drawn that frame. So an actor culls by leaving a placement out, and switches a level of detail by listing another.
- `DrawList::of::<P>` names the pass by its type, as `Join::to::<P>` does, and `frame` is a window's main frame by the window or a texture frame by its id, as a join names it (ADR-0025: `Frame::Main`). A list for a window's main frame follows the main frame when it is replaced, as the placements that joined it do.

**The list is sent every frame, and the render root keeps none of it past the frame.**

- Every turn `Tick` set off has run before `Render` is sent (ADR-0020), so a list sent in a `Tick` is in hand at `Render`. A list is for the next `Render` after it is handled, and is dropped once that `Render` has drawn, whether or not its frame was drawn.
- **A frame whose actor sent no list draws none of that actor's listed placements**, as an empty list draws none.
- **Lists are not kept until changed.** A list kept would be state to keep in step with the placements: one released while it is still listed, and what an absent list means, whether the last one or none. A list sent each frame means one thing, what this frame draws.
- **A list is not answered**, as a write is not (ADR-0023): it is sent every frame, and an answer each frame would be mail nobody reads.

**Each list is checked whole when it is handled.** Each id names a placement that the list's sender made, that is kept, and that joins that frame's pass `Listed`; and no id is named twice.

- **One that fails refuses the whole list**, which is logged, and that actor's listed placements in that pass draw nothing that frame. A list half drawn would be an order nobody declared.
- **The check rides on the lookup the draw needs anyway.** Each id is looked up as the list is handled, and the list keeps what it found, so nothing is looked up or refused at `Render` (ADR-0025). A duplicate is found by stamping each listed join with the frame it was listed in: a join already stamped with this frame is named twice.
- **A placement released after it is listed is drawn in that frame**, as its list said, and goes once the frame is drawn, as a placement keeps what it draws (ADR-0023).

**One list per actor per pass per frame. A second in the same frame replaces the first.** A list is what the frame draws, so the last one sent before `Render` is the frame's: an actor whose camera's view comes in a later turn of the `Tick` than its first list sends a newer list, and does not have to keep track of whether it sent one already. Refusing the second would give the actor that state to keep, and replacing it leaves no rule that one actor's two turns could break. Only the same actor's list is replaced: each actor has its own list in each pass.

**Order between actors is still not promised.** The lists of different actors, and the loose placements of a pass, are drawn in no order an actor may rely on; across actors, order is the order of passes (ADR-0025). One actor's list is drawn back to back, with nothing of another between its entries. The render root still sets each program's pipeline as seldom as it can: within a list, it switches only where neighbouring entries use different programs, and it still draws a pass's loose placements grouped by program.

**This reconciles with ADR-0023's refusal of an order key, and ADR-0025's rule that order is declared where drawing is described, and nowhere else.** The order is declared by the one actor that made the placements, in the mail that describes the frame's drawing. No actor agrees on numbers with another, and no number outlives the frame it orders. Nothing is taken from when placements were made, where actors stand, or when mail came.

**A list is ordinary mail.** A placement's id is eight bytes, so a list of about 1,800 placements is about 14 KB, encoded into its sender's arena and read in the next step (ADR-0011: `Arena`). That is more than a piece of mail is read within by default: `read_limit`, 1,024 bytes (ADR-0028: `EngineSettings`), about 120 ids. A program whose actors list more raises the limit, as its settings let it. The ids could instead travel in a blob (ADR-0014), as a mesh's bytes do, which no limit on mail bounds; that is a second shape of the list, left until a program needs it. Nothing new carries a list.

## Consequences

- An actor that streams chunks into slots makes each slot's placements once, joined `Listed`, and sends one list a frame, nearest last. A chunk out of view is left off; a slot whose chunk changed lists its new placements in the frame they are ready.
- Culling and level of detail are an actor's own, by what it lists, with nothing kept on the render root for either.
- ADR-0023's case for a list kept on the render root still holds for what is drawn: a list carries ids, eight bytes each, where carrying the draws themselves was 960 KB a frame for 30,000. The meshes, programs, bindings, and bind groups stay made and kept, and only the order travels. An actor that needs no order sends nothing each frame, as before.
- The render root handles one more kind, `DrawList`, within an actor's 64 (ADR-0013: `HandledList`), and holds each actor's list for a frame, with a stamp on each listed join.
- A frame whose actor's `Tick` sent no list draws nothing of its listed placements, so an actor that misses a frame shows a gap there, where a kept list would show the last frame's order. That gap is what an absent list means, and it is the same every time.
- ADR-0023 and ADR-0025 are edited to match: a join is `Loose` or `Listed`, and order within a pass may be declared by the actor that made the placements. ADR-0026's dispatches keep their rule, and are run grouped by program as a pass's loose placements are drawn.
- What is decided separately, each when it is needed:
  - **Dispatches in lists.** Dispatches of one pass still run in no order an actor may rely on, and one that needs another's output is in a later pass (ADR-0026).
  - **Refusing loose placements whose order shows**, once lists give a way to declare it: a placement whose program blends by `Alpha`, or is `Opaque` with no depth test, or one that blends `Additive` beside one that blends `Multiply`, sharing a pass with another. Then an order nobody declared could not be written. Lists are the step that makes this possible.
  - **A volatile array one actor owns and others read at the boundary of a step**, for data many actors read each frame.

## Alternatives considered

- **An order key on each placement.** A key set once and not changed keeps an old order with nothing to say so, as the camera moves on; and across actors it is the key ADR-0023 refused, numbers unrelated actors must agree on.
- **A set of placements registered once, then a permutation of its indices each frame.** Less mail for each change, about 512 B where a list is 14 KB, but a protocol of its own: indices that mean a place in the set as it was registered, a change of order and nothing else, so culling and membership need a second mechanism, and rules for releasing a placement still registered.
- **Only what moved, sent as changes to the last order.** The render root would hold an order it could not check whole, built from changes it could not see together.
- **Groups of placements kept on the render root, with a `Reorder` to change their order.** Kept state again, and rules for who is in a group and what a released placement does to it.
- **A list kept until it is changed.** Kept state: a placement released while listed, and what an absent list means.
- **Ids encoded as differences from the one before.** It saves about 11 KB that the arena does not need saved, and is a wire format every sender must learn. If it is ever wanted it belongs to the encoding, beneath every kind, not to this one.
- **The render root sorting a pass's placements by distance itself.** It would have to read what records mean, where it never reads what a record holds (ADR-0023).
- **Placements of one program in one pass drawn in the order of their ids.** Steady from run to run, but the order of when they were made, which ADR-0025 refuses: making one more object of any sort earlier moves where everything later falls.
