# ADR-0031: Draw lists

- **Status:** Proposed
- **Date:** 2026-10-22

## Context

Within one pass, placements are drawn in no order an actor may rely on (ADR-0023), and order is declared where drawing is described: the order of a frame's passes, which their author writes, and the order of one placement's records, which one actor writes (ADR-0025). Across actors this holds: order between what different actors draw is the order of passes, and no actor agrees on numbers with another.

Some drawing needs an order within one pass that changes as the camera moves, among many placements of one actor:

- **Terrain streamed in chunks, held in slots that are used again.** A chunk that comes into range takes the slot of one that left, and each slot has placements that stand for as long as the slot does.
- **Each slot's placements blend by alpha, testing depth and writing none**, and their edges overlap their neighbours', painted soft. Where two overlap, the nearer chunk must be drawn over the farther, and which is nearer changes as the camera moves.

None of the ways a program already has to declare order fits it:

- **One placement's records**, drawn in order as they are one actor's (ADR-0023), would have every slot's data in one placement: one storage binding for every slot, at most 128 MiB on a device of default limits (`max_storage_buffer_binding_size`), a table of slots beside it, and a search in the vertex stage for which slot a vertex belongs to.
- **Passes** order what they draw, but a frame's passes are fixed when it is made (ADR-0025: `CreateFrame`), each costs about 35 µs, and the slots are many and change their order as the camera moves.
- **Depth, or a blend that needs no order.** A program's alpha turned into coverage (ADR-0021: `Coverage::Alpha`) gives an edge of four samples five levels, where a blend gives it every level its alpha has, so the soft painted edges change. A depth for each chunk's rank in the order needs a precision of depth the program does not have: the chunks lie on one surface.

The fault seen was of the same kind. Placements of one program in one pass were drawn in the order of a hash map of their ids, and every id the render root makes is taken from one count, so making one more object of any sort at startup, unrelated to the terrain, moved a seam between two chunks (#294). Each build drew the same picture every run, and the order was still nobody's.

## Decision

**An actor may declare the order of its own placements within a pass, by sending the list of them to draw, in order. The list is kept until the actor sends another.**

```rust
Place { mesh: MeshId, instances: Drawn, joins: Vec<Join> }                    -> Made<PlacementId> | Refused
struct Join { frame: Frame, pass: PassName, program: ProgramId, bindings: Vec<Bind>, order: Order }
enum Order { Loose, Listed }     // Join::to::<P>(..) is Loose, as every join has been; Join::listed::<P>(..) is Listed

DrawList { frame: Frame, pass: PassName, placements: Vec<PlacementId> }      // no answer; DrawList::of::<P>(frame, placements)
```

- **A join is loose or listed.** A loose join is drawn in every frame, as ADR-0025 draws a join. A listed join is drawn while its placement is in its maker's list for that frame and pass. A placement may join one pass loose and another listed: a depth pass that needs no order, and the pass that blends, which does.
- **A listed placement draws while it is listed, in list order.** A new list that leaves it out stops it from the next `Render`. So an actor culls by leaving a placement out, and switches a level of detail by listing another.
- `DrawList::of::<P>` names the pass by its type, as `Join::to::<P>` does, and `frame` is a window's main frame by the window or a texture frame by its id, as a join names it (ADR-0025: `Frame::Main`). A list for a window's main frame follows the main frame when it is replaced, as the placements that joined it do.

**A list is kept until it is replaced.** Every `Render` draws each actor's last list for each pass, and a new list takes the place of its sender's last for that frame and pass at the next `Render` after it is handled. There is no other mail to draw.

```
Tick:   actor → DrawList [a, b, c]
Render: draws a, b, c
Tick:   (nothing sent)
Render: draws a, b, c       — the last list stands
Tick:   actor → DrawList [c, a]
Render: draws c, a          — reordered, b culled
```

- Every turn `Tick` set off has run before `Render` is sent (ADR-0020), so a list sent in a `Tick` is drawn from that frame's `Render`.
- **A fixed order is sent once, and a changing order is sent when it changes.** An actor that sorts its placements every frame sends a list only when the sort comes out different. An actor whose order is fixed needs no `Tick` to keep drawing it.
- **A placement joined `Listed` that is in no list is not drawn**: nothing has ordered it. One made and joined `Listed` draws from the first `Render` after a list that names it.
- **An empty list is a list.** It draws none of its sender's listed placements in that pass, and is how an actor stops drawing them all.
- **The render root keeps one list for each actor in each pass of each frame.** A list for a frame that ends on a texture goes when that frame does.
- **A list is not answered**, as a write is not (ADR-0023): an order that changes as the camera moves is sent as often as it changes, and an answer each time would be mail nobody reads.

**A listed placement released drops out of its maker's list.** The list and the release are both the maker's, as only a placement's maker releases it (ADR-0023: who may change what), so the two never go out of step. The release takes the placement's entry out of the list in the release's own turn, keeping the rest in order, and the next `Render` draws the rest. A placement keeps what it draws until it is released, and a placement released is drawn no more, listed or loose (ADR-0023). **An actor that ends takes its lists with it**, as its placements go when it ends (ADR-0021).

**Each list is checked whole when it is handled.** Each id names a placement that the list's sender made, that is kept, and that joins that frame's pass `Listed`; and no id is named twice.

- **One that fails is refused whole**, which is logged, and **that actor's last list for that pass stands.** A list half drawn would be an order nobody declared, and a bad list never blanks what its sender drew there.
- **The check rides on the lookup the draw needs anyway.** Each id is looked up as the list is handled, and the list keeps what it found, so nothing is looked up or refused at `Render` (ADR-0025). A release changes a list between `Render`s, in the release's turn, never at one. A duplicate is found by stamping each listed join with a number the render root counts up for each list it checks: a join already stamped with this list's number is named twice.

**A later list replaces an earlier one, whenever it comes.** Two lists in the same step or the same frame are two lists: the last handled before a `Render` is what that `Render` draws. An actor whose camera's view comes in a later turn of the `Tick` than its first list sends a newer one, and keeps no track of whether it sent one already. Only the same actor's list is replaced: each actor has its own list in each pass.

**Order between actors is still not promised.** The lists of different actors, and the loose placements of a pass, are drawn in no order an actor may rely on; across actors, order is the order of passes (ADR-0025). One actor's list is drawn back to back, with nothing of another between its entries. The render root still sets each program's pipeline as seldom as it can: within a list, it switches only where neighbouring entries use different programs, and it still draws a pass's loose placements grouped by program.

**This reconciles with ADR-0023's refusal of an order key, and ADR-0025's rule that order is declared where drawing is described, and nowhere else.** The order is declared by the one actor that made the placements, in the mail that describes its drawing, and kept as what is drawn is kept, until that actor changes it. No actor agrees on numbers with another, and nothing another actor does changes it. Nothing is taken from when placements were made, where actors stand, or when mail came.

**A list is ordinary mail.** A placement's id is eight bytes, so a list of about 1,800 placements is about 14 KB, encoded into its sender's arena and read in the next step (ADR-0011: `Arena`). That is more than a piece of mail is read within by default: `read_limit`, 1,024 bytes (ADR-0028: `EngineSettings`), about 120 ids. A program whose actors list more raises the limit, as its settings let it. The ids could instead travel in a blob (ADR-0014), as a mesh's bytes do, which no limit on mail bounds; that is a second shape of the list, left until a program needs it. Nothing new carries a list.

## Consequences

- An actor that streams chunks into slots makes each slot's placements once, joined `Listed`, and sends a list, nearest last, when the order changes or a chunk comes or goes. A chunk out of view is left off the next list; a slot whose chunk changed lists its new placements once they are ready, and releasing the old ones takes them out of the list meanwhile.
- Culling and level of detail are an actor's own, by what it lists, with nothing kept on the render root for either but the list.
- ADR-0023's case for a list kept on the render root holds for the order too: a list carries ids, eight bytes each, where carrying the draws themselves was 960 KB a frame for 30,000, and it is carried only when it changes. The meshes, programs, bindings, and bind groups stay made and kept, and only the order travels. An actor that needs no order sends nothing, as before; one whose order is fixed sends one list, ever.
- **The render root keeps state for lists: one `Vec` of placements for each actor in each pass of each frame**, with a stamp on each listed join. It cannot go out of step with the placements, as only its owner changes it: by a new list, checked whole, or by releasing a listed placement, which takes it out; and the owner ending takes it. No other actor's mail, release, or ending touches it.
- The render root handles one more kind, `DrawList`, within an actor's 64 (ADR-0013: `HandledList`).
- An actor that sends no list in a frame draws its last list's order there: a list it was late with shows the order before it for that frame, never a gap.
- ADR-0023 and ADR-0025 are edited to match: a join is `Loose` or `Listed`, and order within a pass may be declared by the actor that made the placements. ADR-0026's dispatches keep their rule, and are run grouped by program as a pass's loose placements are drawn.
- What is decided separately, each when it is needed:
  - **Dispatches in lists.** Dispatches of one pass still run in no order an actor may rely on, and one that needs another's output is in a later pass (ADR-0026).
  - **Refusing loose placements whose order shows**, once lists give a way to declare it: a placement whose program blends by `Alpha`, or is `Opaque` with no depth test, or one that blends `Additive` beside one that blends `Multiply`, sharing a pass with another. Then an order nobody declared could not be written. Lists are the step that makes this possible.
  - **A volatile array one actor owns and others read at the boundary of a step**, for data many actors read each frame.

## Alternatives considered

- **A list sent every frame, kept no longer than the frame.** Nothing would be kept, and a list would mean one thing, what this frame draws. But an actor whose order is fixed would send it again every frame, and handle `Tick` for nothing else, and a frame it missed would draw none of its listed placements.
- **An order key on each placement.** A change of order is a write to every placement whose key moves, each checked alone, where a list is one piece of mail checked whole; and across actors it is the key ADR-0023 refused, numbers unrelated actors must agree on.
- **A set of placements registered once, then a permutation of its indices for each change.** Less mail for each change, about 512 B a change where a list is 14 KB a change, but a protocol of its own: indices that mean a place in the set as it was registered, a change of order and nothing else, so culling and membership need a second mechanism, and rules for releasing a placement still registered.
- **Only what moved, sent as changes to the last order.** The render root would hold an order it could not check whole, built from changes it could not see together.
- **Groups of placements made on the render root, with a `Reorder` to change their order.** A group is a resource of its own, made, released, and joined, with rules for who is in it, and a `Reorder` of a whole group is a list under another name.
- **Ids encoded as differences from the one before.** It saves about 11 KB that the arena does not need saved, and is a wire format every sender must learn. If it is ever wanted it belongs to the encoding, beneath every kind, not to this one.
- **The render root sorting a pass's placements by distance itself.** It would have to read what records mean, where it never reads what a record holds (ADR-0023).
- **Placements of one program in one pass drawn in the order of their ids.** Steady from run to run, but the order of when they were made, which ADR-0025 refuses: making one more object of any sort earlier moves where everything later falls.
