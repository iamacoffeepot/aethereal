//! The native guest host: guests' code run with wasmtime (ADR-0015).
//!
//! A blob's code is compiled once. Each part runs it in an instance of its
//! own, which the first actor of the blob to run in the part loads into the
//! part's [`Instances`] and every other actor of it there reuses, so nothing is
//! shared between workers and nothing is locked. A batch's items are written
//! straight into the guest's inbox, and what its turns asked for is read where
//! the guest wrote it. What a turn asks of the engine to spawn a child, and the
//! child's seat, the guest asks in a call out, answered from what the batch was
//! lent. A child seated in another part than its parent's is taken out of the
//! parent's instance, and into its own part's on its first batch (ADR-0015).
//! A child of other code, a native type or another blob's, the host begins in a
//! call out, apart from the turn that spawns it, and the turn takes in what its
//! `begin` asked for when it is handed back. An instance is lent out of its
//! part's instances for each call into it, so a chain of `begin`s that comes
//! back to it is refused.

use aethereal_actor::records::{
    self, Born, CancelRecord, ClaimRecord, Gone, Handed, HoldRecord, HolderRecord, ItemHead, JobRecord, Kept, Keying,
    LineRecord, Naming, Other, OutHead, Parent, PinRecord, RefRecord, Requesting, SendRecord, Span, SpawnRecord,
    Spawning, StageRecord, StoodRecord, Stopped, TakenOver, TurnEnds, UnreadRecord, WatchRecord, blob_at, refusal,
};
use aethereal_actor::section::{self, Made};
use aethereal_actor::{
    Above, Arena, Asked, Batch, Birth, BlobHash, BlobId, BlobType, Broke, Bytes, Children, Claimed, Declared, Due,
    FalseSend, FalseStage, FalseWatch, Fault, Gift, Guests, Holds, Hosted, How, Instances, Item, Keeping, Loadable,
    Maker, Memory as InstanceMemory, Moving, NotHeld, NotOwed, Peeked, Pinning, Preparing, Puts, Reads, Refused,
    Request, Role, Runs, SeatNumber, Seated, Seats, Table, Tie, Tied, Turning, Unfit, Unspawned, Upgrade, Upgraded,
    Was, Why, Written, holders, take_loose_ticket,
};
use aethereal_data::{
    ActorId, ActorPath, Budget, KeyName, KindId, KindName, SchemaNode, SchemaNodes, from_bytes, to_bytes,
};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::iter;
use std::mem;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};
use wasmtime::{Caller, Extern, InstancePre, Linker, Memory, Module, Store, TypedFunc};

/// What runs guests' code with wasmtime: one compiler, for every blob, and the blobs it has compiled, by their ids,
/// as the engine's [`Guests`] (ADR-0015).
#[derive(Default)]
pub struct Runtime {
    engine: wasmtime::Engine,
    codes: Arc<Codes>,
    /// Each code compiled, by its bytes' hash, so the same code put again, into this engine or another, is compiled
    /// once. Locked, as one runtime may serve several engines, each of which puts code between its own steps.
    compiled: RwLock<HashMap<BlobHash, Compiled>>,
}

/// The blobs a runtime has taken in as code, by their ids: each code reaches the rest, as a child of another blob's
/// its actor spawns is begun from them in a call out. Locked, as every worker reads it while it runs its parts, apart
/// from the mail each handles, and the engine adds and lets go of code between steps.
type Codes = RwLock<HashMap<BlobId, Code>>;

/// Code as it is compiled, shared by every blob of the same bytes.
#[derive(Clone)]
struct Compiled {
    key: u64,
    module: InstancePre<Lending>,
    made: Arc<[&'static Declared]>,
}

/// A blob of guest code, compiled, and what each actor type it makes declares: what every part makes its instance
/// of the blob from. Cloning it shares the compiled code.
#[derive(Clone)]
struct Code {
    /// Each type it makes, of its blob, as the engine names it in a spawn, in the order the section lists them: kept
    /// for as long as the process runs, as what they declare is.
    types: Arc<[&'static BlobType]>,
    /// Every code the runtime that took it in holds, without keeping them.
    codes: Weak<Codes>,
    /// What its instances are kept under in a part's [`Instances`]: a number no other code compiled in the process
    /// has.
    key: u64,
    /// The module, with the calls out it imports linked in.
    module: InstancePre<Lending>,
    made: Arc<[&'static Declared]>,
}

/// Code an actor runs on, and the pin on its blob that keeps the blob while the actor runs (ADR-0009): a clone of
/// the one the engine handed as the actor began, or of its spawner's, as its blob is the same.
#[derive(Clone)]
struct Running {
    code: Code,
    /// Held for as long as the actor runs on the code, and read for which blob it is, to tell whether the actor is
    /// behind its kind's head (ADR-0054).
    pin: Bytes,
}

/// The key the next code compiled takes. Atomic, as runtimes serving any engine compile at once, and each code's key
/// must differ from every other's in the instances of a part.
static CODES: AtomicU64 = AtomicU64::new(0);

/// The number the next instance made takes. Atomic, as instances are made on whichever worker runs their part, and each
/// instance's number must differ from every other's, so an actor of an instance a trap dropped is never run in one
/// made after it (ADR-0015).
static INSTANCES: AtomicU64 = AtomicU64::new(0);

/// One instance of a blob, a part's, and the calls its host makes of it.
struct Instance {
    /// Its number among the instances the process has made, which no other takes.
    number: u64,
    store: Store<Lending>,
    memory: Memory,
    inbox: TypedFunc<u32, u32>,
    out: TypedFunc<(), u64>,
    begin: TypedFunc<(u32, u64, u64, u32), u32>,
    take_in: TypedFunc<(u32, u64, u64, u32), u32>,
    take_out: TypedFunc<u32, u64>,
    turns: TypedFunc<(u32, u32, u32, u64), u64>,
    upgrade: TypedFunc<u32, u32>,
    take_over: TypedFunc<(u32, u64, u64, u32), u64>,
    pins: TypedFunc<(), u64>,
    holders: TypedFunc<(), u64>,
    end: TypedFunc<(u32, u32), u64>,
    drop: TypedFunc<u32, ()>,
    /// Where the guest's inbox is, and how many bytes it has room for: asked for again only when it must grow.
    inbox_at: usize,
    inbox_room: usize,
    /// The actors that live in it: the part drops it when the last ends, and a trap in it ends them all.
    actors: Vec<Seated>,
    /// The pins its actors hold on blobs: dropped with it, as a trap drops it (ADR-0014).
    holdings: Holdings,
}

/// The pins an instance's actors hold on blobs, by blob: how many references to each the instance holds, and one pin
/// of the engine's that keeps the blob while it holds any (ADR-0014). An instance's actors share one memory and trust
/// one another, so its references are counted together.
#[derive(Default)]
struct Holdings {
    blobs: BTreeMap<BlobId, Holding>,
}

/// How many references to one blob an instance holds, and the pin that keeps the blob for them.
struct Holding {
    pin: Bytes,
    count: u64,
}

/// What an instance's calls out are answered from: lent for the length of one call into it.
enum Lending {
    /// No call is under way, and nothing is lent.
    Between,
    /// A call is under way, lent what answers its calls out, which lives on the caller's stack for the call.
    During(NonNull<Answering<'static>>),
}

// SAFETY: a `Lending` crosses threads only as `Between`, when the instance moves with its part: it is `During` only
// for the length of one call into the instance, on the thread that makes it, which sets it back before it returns.
unsafe impl Send for Lending {}

/// What a call's calls out are answered from: what its batch, or its root's `begin`, was lent, the instances of its
/// part but its own, which is lent out while the call is under way, and what it gave.
struct Answering<'a> {
    table: &'a dyn Table,
    made: &'a dyn Children,
    seats: &'a mut Seats,
    puts: &'a mut Puts,
    instances: &'a mut Instances,
    codes: &'a Weak<Codes>,
    given: &'a mut Given,
    /// The pins the call's instance holds, as the call began.
    holdings: &'a Holdings,
    /// What the actor whose turns or `begin` the call runs holds of resources, which a child of other code it spawns
    /// may be handed in its config (ADR-0010), and the buffers it keeps, which what it puts is copied into (ADR-0042).
    holds: Holds<'a>,
    /// The actor whose turns or `begin` the call runs, which makes the blobs the call puts (ADR-0042).
    me: Seated,
}

/// What the host gave a call into a guest that is still to be handed to the engine: the seats it gave the guest's
/// children, in the order given, and the children of other code it began for the guest, in the order the guest
/// asked for them, each with the turn of the call that asked.
#[derive(Default)]
struct Given {
    seats: Vec<SeatNumber>,
    births: VecDeque<(u32, Foreign)>,
    /// The pins on the blobs offered to the call's instance, which it may take references to though it holds none:
    /// what the items handed in, a config, a state taken in, or a put refer to (ADR-0014).
    offered: Vec<Bytes>,
    /// The tickets of the resources the host handed to children of other code in their configs as it began them,
    /// before the call's claims of them are held to: what the guest claims of them is already done (ADR-0010).
    handed: Vec<u64>,
}

/// A child of other code its host was asked to begin for a guest, apart from the guest's turn, which takes in what
/// its `begin` asked for when it is handed back (ADR-0015).
enum Foreign {
    /// It began, from `from`, run by `runs`, keeping `keeping`, and its `begin` asked for what is in `asked`.
    Began { me: Seated, from: aethereal_actor::Code, runs: Box<dyn Runs>, asked: Asked, keeping: Keeping },
    /// It was refused so: of what its `begin` asked for, only the lines in `asked` are kept.
    Refused { asked: Asked, refused: Refused },
    /// It began, and the guest took it back, as the `begin` that spawned it was refused: only the lines in `asked`
    /// are kept.
    TakenBack { asked: Asked },
}

/// An actor whose code runs in a guest's instance, as the engine holds it: the instance is its part's, which it
/// takes from what its part lends each batch and turn.
struct Guest {
    running: Running,
    me: Seated,
    lives: Lives,
    declared: &'static Declared,
    /// What the guest's turns of the batch prepared last asked for, and how far its host has handed them back.
    ran: Ran,
    /// What the host gave the batch prepared last, not yet handed to the engine.
    given: Given,
    /// Whether a move onto its kind's head failed, so it is not asked again until the head moves on (ADR-0054).
    parked: Parked,
}

/// Whether a guest actor is parked: its move onto its kind's head failed, and the same code would fail again against
/// nearly the same state, so it is not asked again until the head moves on (ADR-0054).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Parked {
    No,
    /// On the head of this blob, which its move onto failed.
    On(BlobId),
}

/// Where a guest actor's state is.
enum Lives {
    /// In its part's instance, the one numbered `instance`, under the handle the guest knows it by.
    In { handle: u32, instance: u64 },
    /// Taken out of the instance it began in, which is another part's, as its path and state written at `Within`,
    /// with the pins on the blobs they refer to: taken into its own part's instance on its first batch.
    Out { state: Vec<u8>, carried: Vec<Bytes> },
}

/// What a guest's batch left to hand back.
#[derive(Clone, Copy)]
enum Ran {
    /// No batch yet.
    Nothing,
    /// Its turns' records lie where `head` says in the guest's memory: the `next` is to be handed back, and the turns
    /// before it end where `before` says.
    At { head: OutHead, next: u32, before: TurnEnds },
    /// The call that ran them broke, for why it says: every turn of the batch fails.
    Broke(Broke),
    /// The instance the actor lived in is gone, as a trap in a call of another actor dropped it: every turn of the
    /// batch fails, and nothing of the part's instance of its code, made since, is touched (ADR-0015).
    Lost,
}

/// The top bit of a seat given to a guest's child, set when the seat is in another part than its parent's.
const ELSEWHERE_BIT: u64 = 1 << 63;

/// The handle a guest answers for an actor it did not take in, and writes for a child of a turn that failed, which it
/// dropped as the engine drops it.
const NO_HANDLE: u32 = u32::MAX;

impl Runtime {
    /// A host whose guests' compiled code is named to a profiler through a perf map: a spike's measure.
    ///
    /// # Panics
    ///
    /// Panics when the runtime cannot be made so.
    #[must_use]
    pub fn profiled() -> Self {
        let mut config = wasmtime::Config::new();
        config.profiler(wasmtime::ProfilingStrategy::PerfMap);
        let Ok(engine) = wasmtime::Engine::new(&config) else {
            panic!("a runtime that names its code to a profiler is made");
        };

        Self { engine, codes: Arc::default(), compiled: RwLock::default() }
    }

    /// The code of the blob `blob`, of `bytes`: compiled, and what it makes read, unless the same bytes were before.
    /// Refused when it is not fit to run.
    fn code(&self, blob: BlobId, bytes: &[u8]) -> Result<Code, Unfit> {
        let hash = BlobHash::of(bytes);
        let known = self.compiled.read().ok().and_then(|compiled| compiled.get(&hash).cloned());
        let Compiled { key, module, made } = if let Some(compiled) = known {
            compiled
        } else {
            let compiled = self.compile(bytes)?;
            if let Ok(mut known) = self.compiled.write() {
                known.insert(hash, compiled.clone());
            }
            compiled
        };

        let types = made.iter().map(|&declared| &*Box::leak(Box::new(BlobType { blob, declared }))).collect();

        Ok(Code { types, codes: Arc::downgrade(&self.codes), key, module, made })
    }

    /// Compiles `bytes` as guest code, and reads what it makes: refused when it is not fit to run.
    fn compile(&self, bytes: &[u8]) -> Result<Compiled, Unfit> {
        let module = Module::new(&self.engine, bytes).map_err(|error| Unfit::Uncompiled(error.to_string()))?;
        let section = declared_section(bytes).ok_or(Unfit::Undeclared)?;
        let mut budget = Budget::new(1 << 20);
        let made = section::read(section, &mut budget)?;
        let exports = ["memory", "aethereal_inbox", "aethereal_out", "aethereal_begin", "aethereal_take_in"];
        let more = ["aethereal_take_out", "aethereal_turns", "aethereal_pins", "aethereal_end", "aethereal_drop"];
        let more = more.into_iter().chain(["aethereal_read_limit", "aethereal_holders", "aethereal_upgrade"]);
        let more = more.chain(["aethereal_take_over"]);
        for export in exports.into_iter().chain(more) {
            let has = module.get_export(export).is_some();
            if !has {
                return Err(Unfit::Unexported(export));
            }
        }
        let module = calls_out(&self.engine)
            .and_then(|linker| linker.instantiate_pre(&module))
            .map_err(|error| Unfit::Unlinked(error.to_string()))?;

        let key = CODES.fetch_add(1, Ordering::Relaxed);

        Ok(Compiled { key, module, made: made.iter().map(Made::leak).collect() })
    }
}

/// The calls out a guest makes, each answered from what the call under way was lent (ADR-0015).
fn calls_out(engine: &wasmtime::Engine) -> wasmtime::Result<Linker<Lending>> {
    let mut linker = Linker::new(engine);
    reads_out(&mut linker)?;
    spawns_out(&mut linker)?;
    blobs_out(&mut linker)?;

    Ok(linker)
}

/// Links the calls out that read the table into `linker`.
fn reads_out(linker: &mut Linker<Lending>) -> wasmtime::Result<()> {
    linker.func_wrap(
        "aethereal",
        "aethereal_child",
        |mut caller: Caller<'_, Lending>, parent: u64, at: u32, len| {
            let naming = guest_bytes(&mut caller, Span { at, len });
            let naming = naming.and_then(|naming| from_bytes::<Naming>(&naming, &mut Budget::new(1 << 10)).ok());
            answer(&mut caller, |answering| {
                let Some(naming) = naming else {
                    return u64::MAX;
                };
                answering.table.child(ActorId::new(parent), &naming.segment()).bits()
            })
        },
    )?;
    linker.func_wrap("aethereal", "aethereal_child_seat", |mut caller: Caller<'_, Lending>, id: u64| {
        answer(&mut caller, |answering| {
            answering.table.child_seat(ActorId::new(id)).map_or(u64::MAX, SeatNumber::number)
        })
    })?;
    linker.func_wrap("aethereal", "aethereal_has_made", |mut caller: Caller<'_, Lending>, id: u64| {
        answer(&mut caller, |answering| u32::from(answering.made.has_made(ActorId::new(id))))
    })?;
    linker.func_wrap(
        "aethereal",
        "aethereal_resolve",
        |mut caller: Caller<'_, Lending>, at: u32, len: u32, seated: u32| {
            let path = guest_bytes(&mut caller, Span { at, len });
            let path = path.and_then(|path| from_bytes::<ActorPath>(&path, &mut Budget::new(1 << 16)).ok());
            let resolved = answer(&mut caller, |answering| path.and_then(|path| answering.table.resolve(&path)))?;
            let Some(resolved) = resolved else {
                return Ok(0);
            };
            write_seated(&mut caller, resolved, seated)?;

            Ok(1)
        },
    )?;
    linker.func_wrap(
        "aethereal",
        "aethereal_reach",
        |mut caller: Caller<'_, Lending>, at: u32, len: u32, kind: u64, seated: u32| {
            let path = guest_bytes(&mut caller, Span { at, len });
            let path = path.and_then(|path| from_bytes::<ActorPath>(&path, &mut Budget::new(1 << 16)).ok());
            let kind = from_bytes::<KindId>(&kind.to_le_bytes(), &mut Budget::new(8)).ok();
            // A path or kind the guest wrote that does not read back reaches no actor.
            let reached = answer(&mut caller, |answering| {
                let nowhere = Seated { id: ActorId::new(0), seat: SeatNumber::NOWHERE };
                let (Some(path), Some(kind)) = (path, kind) else {
                    return nowhere;
                };
                answering.table.reach(&path, kind)
            })?;

            write_seated(&mut caller, reached, seated)
        },
    )?;
    linker.func_wrap(
        "aethereal",
        "aethereal_reach_child",
        |mut caller: Caller<'_, Lending>, parent: u64, at: u32, len: u32, kind: u64, seated: u32| {
            let naming = guest_bytes(&mut caller, Span { at, len });
            let naming = naming.and_then(|naming| from_bytes::<Naming>(&naming, &mut Budget::new(1 << 10)).ok());
            let kind = from_bytes::<KindId>(&kind.to_le_bytes(), &mut Budget::new(8)).ok();
            // A segment or kind the guest wrote that does not read back finds no child.
            let found = answer(&mut caller, |answering| {
                let (Some(naming), Some(kind)) = (naming, kind) else {
                    return None;
                };
                answering.table.reach_child(ActorId::new(parent), &naming.segment(), kind)
            })?;
            let Some(found) = found else {
                return Ok(0);
            };
            write_seated(&mut caller, found, seated)?;

            Ok(1)
        },
    )?;
    linker.func_wrap("aethereal", "aethereal_is_live", |mut caller: Caller<'_, Lending>, id: u64| {
        answer(&mut caller, |answering| u32::from(answering.table.is_live(ActorId::new(id))))
    })?;
    linker.func_wrap("aethereal", "aethereal_held", |mut caller: Caller<'_, Lending>, id: u64| {
        answer(&mut caller, |answering| u32::from(answering.table.held(ActorId::new(id))))
    })?;
    linker.func_wrap(
        "aethereal",
        "aethereal_lists",
        |mut caller: Caller<'_, Lending>, id: u64, at: u32, len: u32| {
            let kinds = guest_bytes(&mut caller, Span { at, len });
            let kinds = kinds.and_then(|kinds| from_bytes::<Vec<KindId>>(&kinds, &mut Budget::new(1 << 10)).ok());
            answer(&mut caller, |answering| {
                // Ids the guest wrote that do not read back are listed by no type.
                let Some(kinds) = kinds else {
                    return 0;
                };
                u32::from(answering.table.lists(ActorId::new(id), &kinds))
            })
        },
    )?;

    Ok(())
}

/// Writes `seated`, its id and seat, as 16 bytes into the guest's memory at `at`.
fn write_seated(caller: &mut Caller<'_, Lending>, seated: Seated, at: u32) -> wasmtime::Result<()> {
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&seated.id.bits().to_le_bytes());
    bytes[8..].copy_from_slice(&seated.seat.number().to_le_bytes());

    write_guest(caller, at, &bytes)
}

/// Links the calls out that seat and spawn children, and that read the head a child is spawned from, into `linker`.
fn spawns_out(linker: &mut Linker<Lending>) -> wasmtime::Result<()> {
    linker.func_wrap("aethereal", "aethereal_take_seat", |mut caller: Caller<'_, Lending>| {
        answer(&mut caller, |answering| {
            let seat = answering.seats.take();
            answering.given.seats.push(seat);
            let is_elsewhere = !answering.seats.is_own(seat);
            let elsewhere = if is_elsewhere {
                ELSEWHERE_BIT
            } else {
                0
            };

            seat.number() | elsewhere
        })
    })?;
    linker.func_wrap("aethereal", "aethereal_give_back_seat", |mut caller: Caller<'_, Lending>, seat: u64| {
        answer(&mut caller, |answering| {
            let seat = SeatNumber::new(seat);
            let given = answering.given.seats.iter().position(|handed| *handed == seat);
            if let Some(given) = given {
                answering.given.seats.remove(given);
                answering.seats.give_back(seat);
                answering.given.take_back(seat, answering.seats);
            }
        })
    })?;
    linker.func_wrap("aethereal", "aethereal_now", |mut caller: Caller<'_, Lending>| {
        answer(&mut caller, |answering| answering.table.now().nanos())
    })?;
    linker.func_wrap("aethereal", "aethereal_runs_stage", |mut caller: Caller<'_, Lending>, stage: u64| {
        let stage = from_bytes::<KindId>(&stage.to_le_bytes(), &mut Budget::new(8));
        answer(&mut caller, |answering| u32::from(stage.is_ok_and(|stage| answering.table.runs_stage(stage))))
    })?;
    linker.func_wrap("aethereal", "aethereal_spawn", |mut caller: Caller<'_, Lending>, at: u32, len: u32| {
        let spawning = guest_bytes(&mut caller, Span { at, len });
        answer(&mut caller, |answering| {
            // Read within the engine's limit, as a config the engine reads is.
            let mut budget = Budget::new(answering.instances.read_limit());
            let spawning = spawning.and_then(|bytes| from_bytes::<Spawning>(&bytes, &mut budget).ok());
            spawning.map_or_else(|| refusal(&Refused::NotMade), |spawning| answering.spawn(&spawning))
        })
    })?;
    linker.func_wrap(
        "aethereal",
        "aethereal_head",
        |mut caller: Caller<'_, Lending>, at: u32, len: u32, blob: u32| {
            let kind = guest_bytes(&mut caller, Span { at, len });
            let kind = kind.and_then(|kind| from_bytes::<KindName>(&kind, &mut Budget::new(1 << 10)).ok());
            // A name the guest wrote that does not read back has no head.
            let head = answer(&mut caller, |answering| kind.and_then(|kind| answering.head(&kind)))?;
            let Some(head) = head else {
                return Ok(0);
            };
            let mut written = [0u8; 12];
            written[..4].copy_from_slice(&head.engine().to_le_bytes());
            written[4..].copy_from_slice(&head.bits().to_le_bytes());
            write_guest(&mut caller, blob, &written)?;

            Ok(1)
        },
    )?;

    Ok(())
}

/// Links the calls out that read and put blobs, and that read a refusal's fault, into `linker`.
fn blobs_out(linker: &mut Linker<Lending>) -> wasmtime::Result<()> {
    linker.func_wrap(
        "aethereal",
        "aethereal_blob_len",
        |mut caller: Caller<'_, Lending>, engine: u32, bits: u64| {
            answer(&mut caller, |answering| {
                let blob = answering.blob(BlobId::new(engine, bits));
                blob.map_or(u64::MAX, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX))
            })
        },
    )?;
    linker.func_wrap(
        "aethereal",
        "aethereal_blob_read",
        |mut caller: Caller<'_, Lending>, engine: u32, bits: u64, at: u32| {
            let bytes = answer(&mut caller, |answering| answering.blob(BlobId::new(engine, bits)).map(<[u8]>::to_vec))?;
            write_guest(&mut caller, at, &bytes.unwrap_or_default())
        },
    )?;
    linker.func_wrap(
        "aethereal",
        "aethereal_blob_hash",
        |mut caller: Caller<'_, Lending>, engine: u32, bits: u64, at: u32| {
            let hash = answer(&mut caller, |answering| answering.hash(BlobId::new(engine, bits)))?;
            let Some(hash) = hash else {
                return Ok(0);
            };
            write_guest(&mut caller, at, &hash.bytes())?;

            Ok(1)
        },
    )?;
    linker.func_wrap("aethereal", "aethereal_put", |mut caller: Caller<'_, Lending>, at: u32, len: u32, id: u32| {
        let bytes = guest_bytes_kept(&mut caller, Span { at, len })?;
        let put =
            answer(&mut caller, |answering| bytes.ok_or(Refused::NotMade).and_then(|bytes| answering.put(bytes)))?;
        let put = match put {
            Ok(blob) => blob,
            Err(refused) => return Ok(refusal(&refused)),
        };
        let mut written = [0u8; 12];
        written[..4].copy_from_slice(&put.engine().to_le_bytes());
        written[4..].copy_from_slice(&put.bits().to_le_bytes());
        write_guest(&mut caller, id, &written)?;

        Ok(0)
    })?;
    linker.func_wrap("aethereal", "aethereal_refusal", |mut caller: Caller<'_, Lending>, at: u32| {
        let fault =
            answer(&mut caller, |answering| answering.given.births.back().and_then(|(_, birth)| birth.fault()))?;
        let Some(fault) = fault else {
            return Ok(());
        };
        let mut bytes = fault.kind().plain_bytes().to_vec();
        bytes.extend_from_slice(fault.bytes());
        let memory = caller.get_export("memory").and_then(Extern::into_memory);
        let Some(memory) = memory else {
            return Err(wasmtime::Error::msg("a guest that reads a refusal has no memory"));
        };

        memory.write(&mut caller, usize::try_from(at).unwrap_or(usize::MAX), &bytes).map_err(wasmtime::Error::from)
    })?;

    Ok(())
}

/// Answers a call out with `answer`, from what the call under way was lent. A call out made with nothing lent, which
/// only a call the host does not lend to makes, traps the guest.
fn answer<R>(caller: &mut Caller<'_, Lending>, answer: impl FnOnce(&mut Answering<'_>) -> R) -> wasmtime::Result<R> {
    let Lending::During(mut answering) = *caller.data() else {
        return Err(wasmtime::Error::msg("a call out made outside a call that answers one"));
    };
    // SAFETY: `During` holds a pointer to the `Answering` its call lent, which lives on the stack of
    // `Instance::lending`, and is set back to `Between` before that returns. The guest's call out runs within that
    // call, on its thread, and nothing else reaches the `Answering` while it runs.
    let answering = unsafe { answering.as_mut() };

    Ok(answer(answering))
}

/// Writes `bytes` at `at` in the memory of the guest that makes a call out.
fn write_guest(caller: &mut Caller<'_, Lending>, at: u32, bytes: &[u8]) -> wasmtime::Result<()> {
    let memory = caller.get_export("memory").and_then(Extern::into_memory);
    let Some(memory) = memory else {
        return Err(wasmtime::Error::msg("a guest answered a call out has no memory"));
    };

    memory.write(caller, usize::try_from(at).unwrap_or(usize::MAX), bytes).map_err(wasmtime::Error::from)
}

/// The bytes `span` says lie in the memory of the guest that makes a call out: nothing when they lie outside it.
fn guest_bytes(caller: &mut Caller<'_, Lending>, span: Span) -> Option<Vec<u8>> {
    let memory = caller.get_export("memory")?.into_memory()?;

    lying(memory.data(&*caller), span).map(<[u8]>::to_vec)
}

/// The bytes `span` says lie in the memory of the guest that makes a call out, copied into a buffer taken from those
/// the actor whose call it is keeps, to put: so a guest's buffers are reused as a native actor's are (ADR-0042).
/// Nothing when they lie outside its memory, and then no buffer is taken.
fn guest_bytes_kept(caller: &mut Caller<'_, Lending>, span: Span) -> wasmtime::Result<Option<Vec<u8>>> {
    let Some(memory) = caller.get_export("memory").and_then(Extern::into_memory) else {
        return Ok(None);
    };
    let Some(room) = lying(memory.data(&*caller), span).map(<[u8]>::len) else {
        return Ok(None);
    };
    let mut buffer = answer(caller, |answering| answering.buffer(room))?;
    if let Some(bytes) = lying(memory.data(&*caller), span) {
        buffer.extend_from_slice(bytes);
    }

    Ok(Some(buffer))
}

impl<'a> Answering<'a> {
    /// What answers the calls out of a call that runs the turns or `begin` of the actor `me`, from what its batch was
    /// lent, the instances of its part, and the code its runtime holds.
    fn of(
        Reads { table, made, seats, puts, holds }: Reads<'a>,
        instances: &'a mut Instances,
        codes: &'a Weak<Codes>,
        given: &'a mut Given,
        holdings: &'a Holdings,
        me: Seated,
    ) -> Self {
        Self { table, made, seats, puts, instances, codes, given, holdings, holds, me }
    }

    /// A pin on the blob `blob`, when the call's instance holds one or one was offered to the call: what lets its guest
    /// name the blob (ADR-0014).
    fn knows(&self, blob: BlobId) -> Option<Bytes> {
        self.holdings.pin(blob).or_else(|| self.given.offer(blob))
    }

    /// The bytes of the blob `blob`, when the guest may read it.
    fn blob(&self, blob: BlobId) -> Option<&[u8]> {
        self.knows(blob)?;

        self.puts.read(blob).or_else(|| self.table.blob(blob))
    }

    /// The content hash of the blob `blob`, when the guest may read it: the one the store kept, or one computed now and
    /// noted for the store to keep, as a native turn's is.
    fn hash(&mut self, blob: BlobId) -> Option<BlobHash> {
        if let Some(kept) = self.table.blob_hash(blob) {
            self.knows(blob)?;
            return Some(kept);
        }
        let hash = BlobHash::of(self.blob(blob)?);
        self.puts.hashed(blob, hash);

        Some(hash)
    }

    /// A buffer with room for at least `room` bytes, from those the actor whose call it is keeps, to copy what its
    /// guest puts into (ADR-0042).
    fn buffer(&mut self, room: usize) -> Vec<u8> {
        self.holds.keeping.buffer(room)
    }

    /// The head of the kind `kind`, as the heads view stood when the step began, offered to the call so its guest may
    /// spawn from it: answers with its blob's id, and nothing when the kind has no head (ADR-0054).
    fn head(&mut self, kind: &KindName) -> Option<BlobId> {
        let pin = self.table.head(kind)?;
        let id = pin.id();
        self.given.offered.push(pin);

        Some(id)
    }

    /// Puts `bytes` as a blob made by the actor whose call it is, into the worker's puts, and offers it to the call:
    /// answers with its id.
    fn put(&mut self, bytes: Vec<u8>) -> Result<BlobId, Refused> {
        let pin = self.puts.pinned(bytes, Maker::Actor(self.me))?;
        let id = pin.id();
        self.given.offered.push(pin);

        Ok(id)
    }

    /// Begins the child of other code `spawning` names, apart from the call under way, into what it asks for of its
    /// own, lent the call's seats and puts: kept for the turn that spawned it to take in. Answers as [`refusal`]
    /// writes it. A child the host gave no seat, or one it began already, is refused.
    fn spawn(&mut self, spawning: &Spawning) -> u64 {
        let me = spawning.me();
        let is_given = self.given.seats.contains(&me.seat);
        let is_begun = self.given.births.iter().any(|(_, birth)| birth.is(me));
        if !is_given || is_begun {
            return refusal(&Refused::NotMade);
        }
        // Each reference in the config names a blob the guest may name, and its pin goes with the config.
        let mut refers = Vec::with_capacity(spawning.refs.len());
        for &at in &spawning.refs {
            let pin = spawning.config.get(at as usize..).and_then(blob_at).and_then(|blob| self.knows(blob));
            let Some(pin) = pin else {
                return refusal(&Refused::NotMade);
            };
            refers.push((at, pin));
        }
        let mut asked = Asked::writing(Arena::default(), Seats::default(), Puts::new(0, 0, 0));
        // Each resource the config holds is handed to the child as it begins, from what its spawner holds: given by
        // it, as natively.
        let mut handed = Vec::with_capacity(spawning.carried.len());
        for &ticket in &spawning.carried {
            let Some((kept, was)) = self.holds.hand(ticket) else {
                self.put_back(&mut asked, &mut Keeping::default(), handed);
                return refusal(&Refused::NotMade);
            };
            asked.buffers_mut().gifts.push(Gift::loose(spawning.by(), kept));
            handed.push((ticket, was));
        }
        self.lend(&mut asked);
        let mut keeping = Keeping::default();
        let began = self.begin(spawning, &refers, &mut asked, &mut keeping);
        self.lend(&mut asked);
        let (birth, answer) = match began {
            Ok((from, runs)) => {
                self.given.handed.extend(handed.iter().map(|(ticket, _)| *ticket));
                (Foreign::Began { me, from, runs, asked, keeping }, 0)
            }
            Err(refused) => {
                // A child that never began took nothing, so what it was handed goes back where it was, as natively
                // it never left its spawner's gifts.
                self.put_back(&mut asked, &mut keeping, handed);
                // The child never began, so neither did any it stood up: their seats go back, and the guest gives
                // back the child's own as it takes the spawn back.
                for below in asked.take_stood() {
                    self.seats.give_back(below.seated().seat);
                }
                let answer = refusal(&refused);
                (Foreign::Refused { asked, refused }, answer)
            }
        };
        self.given.births.push_back((spawning.turn, birth));

        answer
    }

    /// Puts back each resource in `handed` where it was, with the ticket it was handed under, out of what a refused
    /// child begun apart was handed, `asked`, and kept as it began, `keeping`.
    fn put_back(&mut self, asked: &mut Asked, keeping: &mut Keeping, handed: Vec<(u64, Was)>) {
        for (ticket, was) in handed {
            let left = take_loose_ticket(&mut asked.buffers_mut().gifts, ticket);
            let kept = left.or_else(|| keeping.give_up_any(ticket).map(|(kept, _)| kept));
            if let Some(kept) = kept {
                self.holds.put_back(kept, was);
            }
        }
    }

    /// Swaps the call's seats and puts with `asked`'s: lends them to a child begun apart, and takes them back.
    fn lend(&mut self, asked: &mut Asked) {
        let (seats, puts, _) = asked.lent();
        mem::swap(self.seats, seats);
        mem::swap(self.puts, puts);
    }

    /// Begins the child `spawning` names into `asked`, from the native type the program keeps of its kind, or the
    /// type of its blob, when that declares itself as the guest holds it: answers with its code, and the child.
    fn begin(
        &mut self,
        spawning: &Spawning,
        refers: &[Pinning],
        asked: &mut Asked,
        keeping: &mut Keeping,
    ) -> Result<(aethereal_actor::Code, Box<dyn Runs>), Refused> {
        let carried = &spawning.carried;
        let me = spawning.me();
        let path = &spawning.path;
        let above = Above::Elsewhere(spawning.by());
        let table = self.table;
        match spawning.from {
            Other::Linked => {
                let link = self.instances.links().get(&spawning.kind).ok_or(Refused::NotMade)?;
                let is_same = section::of(link.declared) == spawning.declared;
                if !is_same {
                    return Err(Refused::NotMade);
                }
                let instances = &mut *self.instances;
                let birth = Birth { me, path, above, table, asked, instances, refers, keeping, carried };

                Ok((aethereal_actor::Code::Linked(link.declared), (link.begin)(&spawning.config, birth)?))
            }
            Other::Blob { engine, bits } => {
                // The guest spawns only from code it holds a reference to, as a native turn does.
                let pin = self.knows(BlobId::new(engine, bits)).ok_or(Refused::NotMade)?;
                let blob = pin.id();
                let codes = self.codes.upgrade().ok_or(Refused::NotMade)?;
                let made = codes.read().ok().and_then(|codes| codes.get(&blob)?.of_kind(&spawning.kind));
                let made = made.ok_or(Refused::NotMade)?;
                let is_same = section::of(made.declared) == spawning.declared;
                if !is_same {
                    return Err(Refused::NotMade);
                }
                let guests = self.instances.guests();
                let instances = &mut *self.instances;
                let birth = Birth { me, path, above, table, asked, instances, refers, keeping, carried };

                Ok((aethereal_actor::Code::Blob(made), guests.begin(&pin, made, &spawning.config, birth)?))
            }
        }
    }
}

impl Given {
    /// Whether it holds nothing.
    fn is_empty(&self) -> bool {
        let no_seats = self.seats.is_empty();
        let no_births = self.births.is_empty();
        let no_offers = self.offered.is_empty();
        let none_handed = self.handed.is_empty();

        no_seats && no_births && no_offers && none_handed
    }

    /// A pin on the blob `blob`, when one was offered to the call.
    fn offer(&self, blob: BlobId) -> Option<Bytes> {
        self.offered.iter().find(|offered| offered.id() == blob).cloned()
    }

    /// Notes that the guest took back the child in `seat`, whose seat went back to `seats`: when it is a child of
    /// other code that began, the seats of the children it stood up go back too, and only its lines are kept.
    fn take_back(&mut self, seat: SeatNumber, seats: &mut Seats) {
        let at = self.births.iter().position(|(_, birth)| birth.is_in(seat));
        let taken = at.and_then(|at| Some((at, self.births.remove(at)?)));
        if let Some((at, (turn, Foreign::Began { mut asked, .. }))) = taken {
            for below in asked.take_stood() {
                seats.give_back(below.seated().seat);
            }
            self.births.insert(at, (turn, Foreign::TakenBack { asked }));
        }
    }

    /// Gives back with `give_back` every seat it holds, and those of the children each child of other code that
    /// began stood up, and lets go of every such child: what the call asked for is not handed to the engine.
    fn give_back(&mut self, mut give_back: impl FnMut(SeatNumber)) {
        for seat in self.seats.drain(..) {
            give_back(seat);
        }
        for (_, birth) in self.births.drain(..) {
            if let Foreign::Began { mut asked, .. } = birth {
                for below in asked.take_stood() {
                    give_back(below.seated().seat);
                }
            }
        }
        self.offered.clear();
        self.handed.clear();
    }
}

impl Holdings {
    /// A pin on the blob `blob`, when the instance holds a reference to it.
    fn pin(&self, blob: BlobId) -> Option<Bytes> {
        self.blobs.get(&blob).map(|holding| holding.pin.clone())
    }

    /// Takes in `changes`, what a call into the instance changed of its references to blobs, a pin on a blob it held
    /// none of taken from `offered`: false when one names a blob the instance held no reference to and none was
    /// offered, or drops more than it holds, as its code then came by an id it was not given (ADR-0014).
    fn change(&mut self, changes: impl IntoIterator<Item = PinRecord>, offered: &[Bytes]) -> bool {
        for PinRecord { blob, by } in changes {
            let had = self.blobs.get(&blob).map_or(0, |holding| holding.count);
            let Some(count) = had.checked_add_signed(by) else {
                return false;
            };
            match (had, count) {
                (_, 0) => {
                    self.blobs.remove(&blob);
                }
                (0, _) => {
                    let Some(pin) = offered.iter().find(|offered| offered.id() == blob) else {
                        return false;
                    };
                    self.blobs.insert(blob, Holding { pin: pin.clone(), count });
                }
                _ => {
                    if let Some(holding) = self.blobs.get_mut(&blob) {
                        holding.count = count;
                    }
                }
            }
        }

        true
    }
}

impl Foreign {
    /// Whether it is the child `me` that began.
    fn is(&self, me: Seated) -> bool {
        matches!(self, Self::Began { me: began, .. } if *began == me)
    }

    /// Whether it is the child in `seat` that began.
    fn is_in(&self, seat: SeatNumber) -> bool {
        matches!(self, Self::Began { me, .. } if me.seat == seat)
    }

    /// The code of the child in `seat`, when it is that child and it began.
    fn code_in(&self, seat: SeatNumber) -> Option<aethereal_actor::Code> {
        match self {
            Self::Began { me, from, .. } if me.seat == seat => Some(*from),
            _ => None,
        }
    }

    /// The fault its `begin` returned, when it was refused for one.
    fn fault(&self) -> Option<Fault> {
        match self {
            Self::Refused { refused: Refused::Began(fault), .. } => Some(fault.clone()),
            _ => None,
        }
    }

    /// Hands it to `hosted`, the turn that spawned it: what its `begin` asked for and the child when it began, and only
    /// the lines it logged otherwise. Nothing when the turn spawned no such child.
    fn hand(self, hosted: &mut Hosted<'_>) -> Option<()> {
        match self {
            Self::Began { me, runs, asked, keeping, .. } => hosted.take_in(me, asked, runs, keeping),
            Self::Refused { asked, .. } | Self::TakenBack { asked } => {
                hosted.take_lines(&asked);
                Some(())
            }
        }
    }
}

impl Code {
    /// The type it makes of the kind `kind`.
    fn of_kind(&self, kind: &KindName) -> Option<&'static BlobType> {
        self.types.iter().copied().find(|made| made.declared.name() == kind)
    }

    /// Which of the types it makes is the one of id `kind`, and what it declares.
    fn made_of(&self, kind: KindId) -> Option<(u32, &'static Declared)> {
        let of = self.made.iter().position(|declared| declared.id() == kind)?;

        Some((u32::try_from(of).ok()?, self.made[of]))
    }

    /// Its instance in the part whose `instances` are lent: loaded there by the first of its actors to want it.
    fn instance<'i>(&self, instances: &'i mut Instances) -> &'i mut Instance {
        let read_limit = instances.read_limit();

        instances.get_or_load(self.key, || Instance::load(self, read_limit))
    }
}

/// A guest's memory is the instance's linear memory, which its actors share, named by the first of them (ADR-0053).
impl Loadable for Instance {
    fn memory(&self) -> InstanceMemory {
        let Some(first) = self.actors.first() else {
            return InstanceMemory::Uncounted;
        };
        let bytes = u64::try_from(self.memory.data_size(&self.store)).unwrap_or(u64::MAX);
        let actors = u32::try_from(self.actors.len()).unwrap_or(u32::MAX);

        InstanceMemory::Guest { bytes, actors, first: *first }
    }
}

impl Instance {
    /// A new instance of `code`, whose reads of mail, config, and state allocate at most `read_limit` bytes, as the
    /// engine's do.
    ///
    /// # Panics
    ///
    /// Panics when the code cannot be instantiated, which its checks as it entered rule out.
    fn load(code: &Code, read_limit: usize) -> Self {
        let mut store = Store::new(code.module.module().engine(), Lending::Between);
        let Ok(instance) = code.module.instantiate(&mut store) else {
            panic!("guest code that entered is instantiated");
        };
        let memory = instance.get_memory(&mut store, "memory");
        let inbox = instance.get_typed_func(&mut store, "aethereal_inbox");
        let out = instance.get_typed_func(&mut store, "aethereal_out");
        let begin = instance.get_typed_func(&mut store, "aethereal_begin");
        let take_in = instance.get_typed_func(&mut store, "aethereal_take_in");
        let take_out = instance.get_typed_func(&mut store, "aethereal_take_out");
        let turns = instance.get_typed_func(&mut store, "aethereal_turns");
        let pins = instance.get_typed_func(&mut store, "aethereal_pins");
        let end = instance.get_typed_func(&mut store, "aethereal_end");
        let drop = instance.get_typed_func(&mut store, "aethereal_drop");
        let limit = instance.get_typed_func::<u32, ()>(&mut store, "aethereal_read_limit");
        let upgrade = instance.get_typed_func(&mut store, "aethereal_upgrade");
        let take_over = instance.get_typed_func(&mut store, "aethereal_take_over");
        let (Some(memory), Ok(inbox), Ok(out), Ok(begin), Ok(take_in), Ok(take_out), Ok(turns), Ok(pins), Ok(end)) =
            (memory, inbox, out, begin, take_in, take_out, turns, pins, end)
        else {
            panic!("guest code that entered has its exports");
        };
        let (Ok(drop), Ok(holders)) = (drop, instance.get_typed_func(&mut store, "aethereal_holders")) else {
            panic!("guest code that entered has its exports");
        };
        let (Ok(upgrade), Ok(take_over)) = (upgrade, take_over) else {
            panic!("guest code that entered has its exports");
        };
        // A limit past what a guest's memory can address holds it to that memory.
        let read_limit = u32::try_from(read_limit).unwrap_or(u32::MAX);
        let Ok(()) = limit.and_then(|limit| limit.call(&mut store, read_limit)) else {
            panic!("guest code that entered takes its read limit");
        };

        Self {
            number: INSTANCES.fetch_add(1, Ordering::Relaxed),
            store,
            memory,
            inbox,
            out,
            begin,
            take_in,
            take_out,
            turns,
            upgrade,
            take_over,
            pins,
            holders,
            end,
            drop,
            inbox_at: 0,
            inbox_room: 0,
            actors: Vec::new(),
            holdings: Holdings::default(),
        }
    }
}

/// Makes `call` into the instance whose store is `store`, its calls out answered from `answering` for its length.
fn lending<R>(
    store: &mut Store<Lending>,
    answering: &mut Answering<'_>,
    call: impl FnOnce(&mut Store<Lending>) -> R,
) -> R {
    *store.data_mut() = Lending::During(NonNull::from(answering).cast());
    let called = call(store);
    *store.data_mut() = Lending::Between;

    called
}

impl Guests for Runtime {
    fn load(&self, code: BlobId, bytes: &[u8]) -> Result<(), Unfit> {
        let compiled = self.code(code, bytes)?;
        if let Ok(mut codes) = self.codes.write() {
            codes.insert(code, compiled);
        }

        Ok(())
    }

    /// Lets go of the blob's code, and of its bytes' compiled code once no blob of those bytes is left: code put again
    /// and again as it is changed would otherwise keep every compiled version for as long as the runtime runs.
    fn unload(&self, code: BlobId) {
        let Ok(mut codes) = self.codes.write() else {
            return;
        };
        let Some(gone) = codes.remove(&code) else {
            return;
        };
        let is_held = codes.values().any(|held| held.key == gone.key);
        drop(codes);
        // The same bytes put while this lets go of them may be compiled again, under a key of their own: their actors
        // then run in instances of their own, which nothing shares.
        if !is_held && let Ok(mut compiled) = self.compiled.write() {
            compiled.retain(|_, compiled| compiled.key != gone.key);
        }
    }

    fn made(&self, code: BlobId, declared: &'static Declared) -> Option<&'static BlobType> {
        let codes = self.codes.read().ok()?;

        codes.get(&code)?.types.iter().copied().find(|made| *made.declared == *declared)
    }

    fn makes(&self, code: BlobId, kind: &KindName) -> bool {
        let Ok(codes) = self.codes.read() else {
            return false;
        };

        codes.get(&code).is_some_and(|code| code.of_kind(kind).is_some())
    }

    fn kinds(&self, code: BlobId) -> Vec<KindName> {
        let Ok(codes) = self.codes.read() else {
            return Vec::new();
        };
        let Some(code) = codes.get(&code) else {
            return Vec::new();
        };
        let mut kinds: Vec<KindName> = code.types.iter().map(|made| made.declared.name().clone()).collect();
        kinds.sort();
        kinds.dedup();

        kinds
    }

    /// Begins the child in its part's instance of the blob: its `begin` runs there, handed where it stands and its
    /// config, and what it asked for is read back as a root's or a child's `begin` is (ADR-0015). Refused while that
    /// instance is lent to a call into it that is under way.
    fn begin(
        &self,
        pin: &Bytes,
        made: &'static BlobType,
        config: &[u8],
        birth: Birth<'_>,
    ) -> Result<Box<dyn Runs>, Refused> {
        let (blob, declared) = (pin.id(), made.declared);
        let code = self.codes.read().ok().and_then(|codes| codes.get(&blob).cloned()).ok_or(Refused::NotMade)?;
        let running = Running { code, pin: pin.clone() };
        let code = &running.code;
        let (of, _) = code.made_of(declared.id()).ok_or(Refused::NotMade)?;
        let Birth { me, path, above, table, asked, instances, refers, keeping, .. } = birth;
        let parent = match above {
            Above::Chassis => Parent::Chassis,
            Above::Actor(above, _) | Above::Elsewhere(above) => {
                Parent::Beneath { id: above.id.bits(), seat: above.seat.number() }
            }
        };
        let born = to_bytes(&Born { path: path.clone(), parent }).map_err(|_| Refused::NotMade)?;
        let handed: Vec<u8> = count(born.len()).to_le_bytes().into_iter().chain(born).chain(config.to_vec()).collect();

        let read_limit = instances.read_limit();
        let mut held = instances.lend(code.key, || Instance::load(code, read_limit))?;
        let (seats, puts, _, gifts) = asked.lent_gifts();
        let holds = Holds { keeping: &mut *keeping, gifts };
        let reads = Reads { table, made: &(), seats, puts, holds };
        // What the config refers to is offered to the call, as the child takes references of its own to it.
        let offered = refers.iter().map(|(_, pin)| pin.clone()).collect();
        let mut given = Given { offered, ..Given::default() };
        let mut holdings = mem::take(&mut held.holdings);
        let mut answering = Answering::of(reads, instances, &code.codes, &mut given, &holdings, me);
        let began = held.begin(of, me, &handed, &mut answering);
        let began = began.and_then(|began| held.take_pins(&began.1, &mut holdings, &given).map(|()| began));
        let began = began.and_then(|began| held.take_holders(&began.1, &[me]).map(|()| began));
        held.holdings = holdings;
        // A child seated in another part than the one it began in is taken out, to be taken into its own part's
        // instance on its first batch, as a child its guest spawned is.
        let is_here = asked.seats_mut().is_own(me.seat);
        let mut hosted = Hosted::beginning(me, path, above, table, asked, declared, keeping);
        let replayed = began.and_then(|(handle, wrote)| {
            let Instance { number, store, memory, actors, holdings, .. } = &mut *held;
            let data = memory.data(&*store);
            let ends = TurnEnds::default();
            let living = (*number, actors);
            let replayed = replay(data, &running, &wrote, 0, ends, &mut given, holdings, living, &mut hosted)?;
            Ok((handle, replayed))
        });
        given.give_back(|seat| hosted.give_back(seat));
        let (handle, (began, _)) = match replayed {
            Ok(replayed) => replayed,
            Err(broke) => {
                // A `begin` that broke ends every actor in the instance, as a turn that broke does (ADR-0015).
                hosted.broken(broke, held.actors.iter().copied());
                instances.drop_instance::<Instance>(code.key);
                return Err(Refused::Began(broke.fault()));
            }
        };
        if let Err(fault) = began {
            instances.put_back(code.key, held);
            return Err(Refused::Began(fault));
        }
        let lives = if is_here {
            held.actors.push(me);
            Lives::In { handle, instance: held.number }
        } else {
            match held.take_out(handle) {
                Ok(lives) => lives,
                Err(broke) => {
                    // A call that broke taking the actor out ends every other actor in the instance, as a `begin`
                    // that broke does.
                    hosted.broken(broke, held.actors.iter().copied());
                    instances.drop_instance::<Instance>(code.key);
                    return Err(Refused::Began(broke.fault()));
                }
            }
        };
        instances.put_back(code.key, held);

        let given = Given::default();

        Ok(Box::new(Guest { running, me, lives, declared, ran: Ran::Nothing, given, parked: Parked::No }))
    }
}

impl Guest {
    /// Whether the instance the actor lived in is gone, as a trap dropped it: its part has no instance of its code, or
    /// one made since, which does not hold it. It runs nothing there (ADR-0015).
    fn is_lost(&self, instances: &mut Instances) -> bool {
        let Lives::In { instance, .. } = self.lives else {
            return false;
        };

        instances.get::<Instance>(self.running.code.key).is_none_or(|held| held.number != instance)
    }

    /// What the actor answers, asked in a call of its own whether to move onto its kind's head, in its part whose
    /// `instances` are lent (ADR-0054).
    ///
    /// # Errors
    ///
    /// What became of it when the call broke its instance: it ended, with every other actor of the instance.
    fn ask(&mut self, instances: &mut Instances) -> Result<Upgrade, Upgraded> {
        let key = self.running.code.key;
        let read_limit = instances.read_limit();
        let instance = instances.get_or_load(key, || Instance::load(&self.running.code, read_limit));
        let answer = self.handle_in(instance).and_then(|handle| instance.upgrade(handle));

        answer.map_err(|broke| Upgraded::BrokeAsking { ended: self.ended(broke, instances) })
    }

    /// Moves the actor onto `head`, its kind's head, in its part whose `instances` are lent: its state taken out of
    /// the code it runs, at `Within`, and into the head's instance in the part, made there when the part has none: as
    /// it is, when the kinds are the same, or through its new type's `take_over` when their layouts differ. When the
    /// state is not taken, it is put back, the actor runs on its old code, and it is parked (ADR-0054).
    fn move_onto(&mut self, head: &Bytes, instances: &mut Instances) -> Upgraded {
        let codes = self.running.code.codes.upgrade();
        let code = codes.as_deref().and_then(|codes| codes.read().ok()?.get(&head.id()).cloned());
        let onto = code.and_then(|code| {
            let made = code.of_kind(self.declared.name())?;
            let (of, declared) = code.made_of(made.declared.id())?;
            Some((code, of, declared))
        });
        let from = self.running.code.made_of(self.declared.id());
        let (Some((code, of, declared)), Some((was, _))) = (onto, from) else {
            // A head is published only for a blob that makes its kind, so this is code its host lost track of.
            self.parked = Parked::On(head.id());
            let reason = String::from("the head is not code that makes the kind");
            return Upgraded::Parked { reason, ended: Vec::new() };
        };
        let (state, carried) = match self.take_out_state(instances) {
            Ok(taken) => taken,
            Err(broke) => return Upgraded::BrokeMoving { ended: self.ended(broke, instances) },
        };

        let old = self.declared.state().nodes();
        let new = declared.state().nodes();
        let read_limit = instances.read_limit();
        let fresh = instances.get_or_load(code.key, || Instance::load(&code, read_limit));
        let is_same = declared.id() == self.declared.id();
        let taken = if is_same {
            fresh.take_in(of, self.me, &state, &carried).map(Ok)
        } else {
            to_bytes(&*old)
                .map_err(|_| Broke::Garbled)
                .and_then(|schema| fresh.take_over(of, self.me, (&schema, &state), &carried, read_limit))
        };
        let (reason, ended) = match taken {
            Ok(Ok(handle)) => {
                fresh.actors.push(self.me);
                self.lives = Lives::In { handle, instance: fresh.number };
                self.leave(instances);
                self.running = Running { code, pin: head.clone() };
                self.declared = declared;
                self.parked = Parked::No;
                return Upgraded::Moved { declared, absent: absent(&old, &new) };
            }
            Ok(Err(reason)) => {
                let is_empty = fresh.actors.is_empty();
                if is_empty {
                    instances.drop_instance::<Instance>(code.key);
                }
                (reason, Vec::new())
            }
            Err(broke) => {
                // What the call left in the new code's instance cannot be trusted, so nothing runs in it again
                // (ADR-0015).
                let ended = fresh.actors.iter().map(|actor| (*actor, broke.fault())).collect();
                instances.drop_instance::<Instance>(code.key);
                (format!("its new code broke taking it over: {broke:?}"), ended)
            }
        };
        if let Err(broke) = self.put_back(was, &state, &carried, instances) {
            let mut own = self.ended(broke, instances);
            own.extend(ended);
            return Upgraded::BrokeMoving { ended: own };
        }
        self.parked = Parked::On(head.id());

        Upgraded::Parked { reason, ended }
    }

    /// The actor's state, taken out of the code it runs, at `Within`, with a pin on each blob it refers to: out of its
    /// part's instance, which `instances` holds, or as it was taken out of another part's instance it began in.
    ///
    /// # Errors
    ///
    /// Why the call that took it out broke.
    fn take_out_state(&mut self, instances: &mut Instances) -> Result<(Vec<u8>, Vec<Bytes>), Broke> {
        let handle = match &self.lives {
            Lives::In { handle, .. } => *handle,
            Lives::Out { state, carried } => return Ok((state.clone(), carried.clone())),
        };
        // An actor in its part's instance is kept there, which only a trap drops, and that ends it.
        let held = instances.get::<Instance>(self.running.code.key).ok_or(Broke::Trapped)?;
        let Lives::Out { state, carried } = held.take_out(handle)? else {
            return Err(Broke::Garbled);
        };
        held.actors.retain(|actor| *actor != self.me);

        Ok((state, carried))
    }

    /// Puts the state taken out of the actor, `state` and the pins it carries, `carried`, back where it was: into its
    /// part's instance as the `was`th type of its code, or kept out as it was (ADR-0054).
    ///
    /// # Errors
    ///
    /// Why the call that took it back in broke.
    fn put_back(&mut self, was: u32, state: &[u8], carried: &[Bytes], instances: &mut Instances) -> Result<(), Broke> {
        let is_out = matches!(self.lives, Lives::Out { .. });
        if is_out {
            return Ok(());
        }
        let held = instances.get::<Instance>(self.running.code.key).ok_or(Broke::Trapped)?;
        let handle = held.take_in(was, self.me, state, carried)?;
        held.actors.push(self.me);
        self.lives = Lives::In { handle, instance: held.number };

        Ok(())
    }

    /// Leaves the instance of the code the actor ran, which drops it when no other actor runs in it (ADR-0015).
    fn leave(&self, instances: &mut Instances) {
        let key = self.running.code.key;
        let is_last = instances.get::<Instance>(key).is_some_and(|held| held.actors.is_empty());
        if is_last {
            instances.drop_instance::<Instance>(key);
        }
    }

    /// The actors that end as a call into the instance of the code the actor runs broke, for why `broke` says: the
    /// actor, and every other actor of the instance, which the part drops (ADR-0015).
    fn ended(&self, broke: Broke, instances: &mut Instances) -> Vec<(Seated, Fault)> {
        let key = self.running.code.key;
        let others = instances.get::<Instance>(key).map(|held| held.actors.clone()).unwrap_or_default();
        let others = others.into_iter().filter(|actor| *actor != self.me);
        let ended = iter::once(self.me).chain(others).map(|actor| (actor, broke.fault())).collect();
        instances.drop_instance::<Instance>(key);

        ended
    }

    /// The handle the guest keeps the actor under in `held`, its part's instance: taken in first when it was taken out
    /// of another part's.
    ///
    /// # Errors
    ///
    /// Why the call that took it in broke.
    fn handle_in(&mut self, held: &mut Instance) -> Result<u32, Broke> {
        let handle = match &self.lives {
            Lives::In { handle, .. } => *handle,
            Lives::Out { state, carried } => {
                // The actor's type is one its code makes, as it began from that code.
                let (of, _) = self.running.code.made_of(self.declared.id()).ok_or(Broke::Garbled)?;
                let handle = held.take_in(of, self.me, state, carried)?;
                held.actors.push(self.me);
                self.lives = Lives::In { handle, instance: held.number };
                handle
            }
        };

        Ok(handle)
    }

    /// Ends in the guest each child the turn between `was` and `ran` stood up and the guest kept in the instance, as the
    /// turn failed: the engine drops it with the rest of what the turn asked for and never ends it, though the guest
    /// still holds its state. Kept out of line, as turns seldom fail.
    #[cold]
    #[inline(never)]
    fn end_unkept(&self, was: Ran, ran: Ran, instances: &mut Instances) {
        let (Ran::At { head, before, .. }, Ran::At { before: ends, .. }) = (was, ran) else {
            return;
        };
        let Some(held) = instances.get::<Instance>(self.running.code.key) else {
            return;
        };
        let data = held.memory.data(&held.store);
        let kept: Vec<u32> = (before.stood..ends.stood)
            .filter_map(|index| StoodRecord::read(record(data, head.stood_at, index, StoodRecord::BYTES)?))
            .filter_map(|stood| match stood.held {
                Kept::Here { handle } if handle != NO_HANDLE => Some(handle),
                _ => None,
            })
            .collect();
        for handle in kept {
            held.drop_actor(handle);
        }
    }
}

impl Runs for Guest {
    fn declared(&self) -> &'static Declared {
        self.declared
    }

    /// Always: a guest's code is costly to enter, so it runs each batch's turns in one call.
    fn prepares(&self) -> bool {
        true
    }

    /// Runs every turn of the batch in the guest, in one call into its instance, its calls out answered from what the
    /// batch was lent, and keeps what they asked for for [`Self::turn`] to hand back. The guest stops after a turn that
    /// fails or closes it, as the engine does. An actor taken out of the instance it began in, another part's, is
    /// first taken into this part's.
    fn prepare(&mut self, preparing: Preparing<'_>) {
        let (batch, instances, reads, sent) = preparing.hosted();
        // An actor a trap ended runs nothing more, though its ending is folded only once the step is.
        let is_lost = self.is_lost(instances);
        if is_lost {
            self.ran = Ran::Lost;
            return;
        }
        // The engine prepares no batch while a call into a guest is under way, so the instance is never lent out here.
        let read_limit = instances.read_limit();
        let Ok(mut held) = instances.lend(self.running.code.key, || Instance::load(&self.running.code, read_limit))
        else {
            self.ran = Ran::Broke(Broke::Trapped);
            return;
        };
        let handle = match self.handle_in(&mut held) {
            Ok(handle) => handle,
            Err(broke) => {
                instances.put_back(self.running.code.key, held);
                self.ran = Ran::Broke(broke);
                return;
            }
        };
        let (items, len) = held.hand_in(batch, &mut self.given.offered);
        let mut holdings = mem::take(&mut held.holdings);
        let Reads { table, made, seats, puts, holds } = reads;
        let lent = Reads { table, made, seats: &mut *seats, puts, holds };
        let codes = &self.running.code.codes;
        let mut answering = Answering::of(lent, &mut *instances, codes, &mut self.given, &holdings, self.me);
        let Instance { store, turns, .. } = &mut *held;
        let called = lending(store, &mut answering, |store| turns.call(store, (handle, items, len, sent)));
        let wrote = called.map_err(|_| Broke::Trapped).and_then(|out| held.out_head(out));
        // The references a child taken out of the instance in the call holds go with it, so their pins are offered
        // to its turn before the call's own changes, which drop the instance's references to them, are taken in.
        let given = &mut self.given;
        let wrote = wrote.and_then(|wrote| held.offer_taken_out(&wrote, &holdings, given).map(|()| wrote));
        let wrote = wrote.and_then(|wrote| held.take_pins(&wrote, &mut holdings, given).map(|()| wrote));
        let wrote = wrote.and_then(|wrote| held.take_holders(&wrote, &[]).map(|()| wrote));
        held.holdings = holdings;
        instances.put_back(self.running.code.key, held);
        self.ran = match wrote {
            Ok(wrote) => Ran::At { head: wrote, next: 0, before: TurnEnds::default() },
            Err(broke) => {
                // Nothing the broken call stood up will be handed to the engine, so every seat it took goes back now.
                given.give_back(|seat| seats.give_back(seat));
                Ran::Broke(broke)
            }
        };
    }

    /// Hands back the next turn [`Self::prepare`] ran: what it asked for, each send checked against what the guest
    /// declares, and each child it stood up. When the call that ran it broke, the turn fails for why it broke, every
    /// other actor in the instance ends with it, and the part drops the instance (ADR-0015). A turn
    /// that fails gives back the seats the batch took for children it never handed to the engine.
    fn turn(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        let role = match &item {
            Item::Mail(mail) if self.declared.lists(mail.kind()) => Role::Kind(mail.kind()),
            Item::Mail(mail) if self.declared.stages(mail.kind()) => Role::Stage(mail.kind()),
            Item::Mail(_) => Role::Others,
            Item::Ended(_) => Role::Ended,
            Item::Answer { mail, .. } => Role::Answer(mail.kind()),
            Item::Unanswered { asking, .. } => Role::Answer(asking.answer),
        };
        let (mut hosted, instances) = turning.hosted(self.declared, role);
        let replayed = match self.ran {
            Ran::At { head, next, before } => {
                let Instance { number, store, memory, actors, holdings, .. } = self.running.code.instance(instances);
                let data = memory.data(&*store);
                let living = (*number, actors);
                replay(data, &self.running, &head, next, before, &mut self.given, holdings, living, &mut hosted)
                    .map(|(replayed, ends)| (replayed, Ran::At { head, next: next + 1, before: ends }))
            }
            // The engine hands back no turn of a batch it did not prepare: were it to, no record of the turn would be
            // there to read.
            Ran::Nothing => Err(Broke::Garbled),
            Ran::Broke(broke) => Err(broke),
            // The trap that dropped its instance ended every actor of it.
            Ran::Lost => return Err(Broke::Trapped.fault()),
        };
        let (replayed, ran) = match replayed {
            Ok(replayed) => replayed,
            Err(broke) => {
                self.ran = Ran::Broke(broke);
                self.given.give_back(|seat| hosted.give_back(seat));
                // What the broken call left in the instance's memory cannot be trusted, so nothing runs in it again.
                if let Some(held) = instances.get::<Instance>(self.running.code.key) {
                    hosted.broken(broke, held.actors.iter().copied());
                }
                instances.drop_instance::<Instance>(self.running.code.key);
                return Err(broke.fault());
            }
        };
        let was = mem::replace(&mut self.ran, ran);
        let is_last = matches!(ran, Ran::At { head, next, .. } if next == head.ran);
        let is_failed = replayed.is_err();
        if is_failed {
            self.end_unkept(was, ran, instances);
        }
        let gives_back = !self.given.is_empty() && (is_failed || is_last);
        if gives_back {
            self.given.give_back(|seat| hosted.give_back(seat));
        }

        replayed
    }

    /// Runs the actor's closing turn in its part's instance, each send checked against what its type's `end` declares,
    /// and drops the instance when it was the last actor in it. An actor taken out of another part's instance is
    /// taken into this part's first. An instance a trap dropped is not loaded again, so an actor of one runs no
    /// closing turn: the trap left its state half changed (ADR-0015).
    fn end(mut self: Box<Self>, closing: aethereal_actor::Closing<'_>) -> Result<(), Fault> {
        let (mut hosted, why, instances) = closing.hosted(self.declared);
        let key = self.running.code.key;
        let was_trapped = self.is_lost(instances);
        if was_trapped {
            return Ok(());
        }
        // The engine runs no closing turn while a call into a guest is under way, so the instance is never lent out
        // here.
        let read_limit = instances.read_limit();
        let Ok(mut held) = instances.lend(key, || Instance::load(&self.running.code, read_limit)) else {
            return Ok(());
        };
        let handle = match self.handle_in(&mut held) {
            Ok(handle) => handle,
            Err(broke) => {
                // A call that broke taking the actor in ends every actor in the instance, as a closing turn that
                // broke does.
                hosted.broken(broke, held.actors.iter().copied());
                instances.drop_instance::<Instance>(key);
                return Err(broke.fault());
            }
        };
        let mut given = Given::default();
        let mut holdings = mem::take(&mut held.holdings);
        let (table, seats, puts) = hosted.lent();
        // A closing turn spawns nothing, so no resource is handed on in a call out of it.
        let (mut keeping, mut gifts) = (Keeping::default(), Vec::new());
        let holds = Holds { keeping: &mut keeping, gifts: &mut gifts };
        let reads = Reads { table, made: &(), seats, puts, holds };
        let mut answering = Answering::of(reads, instances, &self.running.code.codes, &mut given, &holdings, self.me);
        let wrote = held.end(handle, why, &mut answering);
        let wrote = wrote.and_then(|wrote| held.take_pins(&wrote, &mut holdings, &given).map(|()| wrote));
        let wrote = wrote.and_then(|wrote| held.take_holders(&wrote, &[]).map(|()| wrote));
        held.holdings = holdings;
        held.actors.retain(|actor| *actor != self.me);
        let replayed = wrote.and_then(|wrote| {
            let Instance { number, store, memory, actors, holdings, .. } = &mut *held;
            let data = memory.data(&*store);
            let living = (*number, actors);
            replay(data, &self.running, &wrote, 0, TurnEnds::default(), &mut given, holdings, living, &mut hosted)
        });
        let (ended, _) = match replayed {
            Ok(replayed) => replayed,
            Err(broke) => {
                // A closing turn that broke ends every other actor in the instance, as a turn that broke does
                // (ADR-0015).
                hosted.broken(broke, held.actors.iter().copied());
                instances.drop_instance::<Instance>(key);
                return Err(broke.fault());
            }
        };
        let is_last = held.actors.is_empty();
        if is_last {
            instances.drop_instance::<Instance>(key);
        } else {
            instances.put_back(key, held);
        }

        ended
    }

    /// Asked before each batch, and once between steps when the actor has no mail as its kind's head moves: when it
    /// runs other code than `moving`'s head and is not parked there, asks it whether to move, and moves it as
    /// [`Self::move_onto`] says when it answers `Now` (ADR-0054). An actor a trap ended is not asked.
    fn upgrade(&mut self, moving: Moving<'_>) -> Upgraded {
        let Moving { head, instances } = moving;
        let is_behind = self.is_behind(head.id());
        let asks = is_behind && !self.is_lost(instances);
        if !asks {
            return Upgraded::Unasked;
        }

        match self.ask(instances) {
            Ok(Upgrade::Later) => Upgraded::Later,
            Ok(Upgrade::Now) => self.move_onto(head, instances),
            Err(broke) => broke,
        }
    }

    /// Whether the actor runs other code than `head`, the blob its kind's head names, and is not parked on it.
    fn is_behind(&self, head: BlobId) -> bool {
        let runs_other = head != self.running.pin.id();
        let is_parked = self.parked == Parked::On(head);

        runs_other && !is_parked
    }
}

impl Instance {
    /// Runs the `begin` of the `of`th type the code makes, for `me`, from `handed`, its segment's naming and config,
    /// its calls out answered from `answering`: the handle the guest keeps the actor under, and the head of what it
    /// asked for.
    ///
    /// # Errors
    ///
    /// Why the call broke.
    fn begin(
        &mut self,
        of: u32,
        me: Seated,
        handed: &[u8],
        answering: &mut Answering<'_>,
    ) -> Result<(u32, OutHead), Broke> {
        let len = self.write_inbox(handed);
        let Self { store, begin, out, .. } = self;
        let handle = lending(store, answering, |store| begin.call(store, (of, me.id.bits(), me.seat.number(), len)));
        let handle = handle.map_err(|_| Broke::Trapped)?;
        let out = out.call(&mut *store, ()).map_err(|_| Broke::Trapped)?;
        let wrote = self.out_head(out).and_then(one_turn)?;

        Ok((handle, wrote))
    }

    /// Takes the actor the guest keeps under `handle` out of the instance: where it stands and its state, as the guest
    /// wrote them, and a pin on each blob they refer to, which the instance no longer counts.
    ///
    /// # Errors
    ///
    /// Why the call broke: [`Broke::Garbled`] too when it wrote nothing, as it held no such actor or its state did
    /// not encode.
    fn take_out(&mut self, handle: u32) -> Result<Lives, Broke> {
        let out = self.take_out.call(&mut self.store, handle).map_err(|_| Broke::Trapped)?;
        let state = self.written(out)?;
        let is_written = !state.is_empty();
        if !is_written {
            return Err(Broke::Garbled);
        }
        let state = state.to_vec();
        let carried = carried_pins(&state, &self.holdings, &[])?;
        self.holders(&[])?;
        let pins = self.pins()?;
        let is_honest = self.holdings.change(pins, &[]);
        if !is_honest {
            return Err(Broke::Forged);
        }

        Ok(Lives::Out { state, carried })
    }

    /// Takes `me`, an actor of the `of`th type the code makes, into the instance from `state`, its path and state as
    /// the guest that took it out wrote them, the references in them to the blobs `carried` pins: the handle the guest
    /// keeps it under.
    ///
    /// # Errors
    ///
    /// Why the call broke: [`Broke::Garbled`] too when the guest did not take the actor in, as its state did not read
    /// back.
    fn take_in(&mut self, of: u32, me: Seated, state: &[u8], carried: &[Bytes]) -> Result<u32, Broke> {
        let len = self.write_inbox(state);
        let handle = self.take_in.call(&mut self.store, (of, me.id.bits(), me.seat.number(), len));
        let handle = handle.map_err(|_| Broke::Trapped)?;
        self.holders(&[me])?;
        let pins = self.pins()?;
        let is_honest = self.holdings.change(pins, carried);
        if !is_honest {
            return Err(Broke::Forged);
        }
        let is_taken = handle != NO_HANDLE;
        if !is_taken {
            return Err(Broke::Garbled);
        }

        Ok(handle)
    }

    /// What the actor the guest keeps under `handle` answers, asked whether to move onto its kind's head in a call with
    /// no turn (ADR-0054).
    ///
    /// # Errors
    ///
    /// Why the call broke: [`Broke::Garbled`] too when it answered neither, as it holds no such actor.
    fn upgrade(&mut self, handle: u32) -> Result<Upgrade, Broke> {
        let answer = self.upgrade.call(&mut self.store, handle).map_err(|_| Broke::Trapped)?;

        match answer {
            0 => Ok(Upgrade::Later),
            1 => Ok(Upgrade::Now),
            _ => Err(Broke::Garbled),
        }
    }

    /// Builds `me`, an actor of the `of`th type the code makes, in the instance from the state another version of its
    /// code left: that version's state schema, as it is written, and the state as that version took it out, the
    /// references in it to the blobs `carried` pins, its new type's `take_over` handed its portable form. The handle
    /// the guest keeps it under, or the reason it was refused, read within `read_limit` bytes (ADR-0054).
    ///
    /// # Errors
    ///
    /// Why the call broke: a trap in `take_over` among them.
    fn take_over(
        &mut self,
        of: u32,
        me: Seated,
        (schema, state): (&[u8], &[u8]),
        carried: &[Bytes],
        read_limit: usize,
    ) -> Result<Result<u32, String>, Broke> {
        let handed: Vec<u8> = count(schema.len()).to_le_bytes().iter().chain(schema).chain(state).copied().collect();
        let len = self.write_inbox(&handed);
        let out = self.take_over.call(&mut self.store, (of, me.id.bits(), me.seat.number(), len));
        let out = out.map_err(|_| Broke::Trapped)?;
        let taken = from_bytes::<TakenOver>(self.written(out)?, &mut Budget::new(read_limit));
        let taken = taken.map_err(|_| Broke::Garbled)?;
        self.holders(&[me])?;
        let pins = self.pins()?;
        let is_honest = self.holdings.change(pins, carried);
        if !is_honest {
            return Err(Broke::Forged);
        }
        let built = match taken {
            TakenOver::Taken { handle } => Ok(handle),
            TakenOver::Refused { reason } => Err(reason),
        };

        Ok(built)
    }

    /// Runs the closing turn of the actor the guest keeps under `handle`, which runs as `why` says, its calls out
    /// answered from `answering`: the head of what it asked for.
    ///
    /// # Errors
    ///
    /// Why the call broke.
    fn end(&mut self, handle: u32, why: Why, answering: &mut Answering<'_>) -> Result<OutHead, Broke> {
        let Self { store, end, .. } = self;
        let out = lending(store, answering, |store| end.call(store, (handle, records::why_code(why))));

        self.out_head(out.map_err(|_| Broke::Trapped)?).and_then(one_turn)
    }

    /// Drops the actor the guest keeps under `handle`, with no closing turn, which drops its state and its references,
    /// and takes in what that changed of the instance's pins.
    fn drop_actor(&mut self, handle: u32) {
        let dropped = self.drop.call(&mut self.store, handle);
        let pins = dropped.map_err(|_| Broke::Trapped).and_then(|()| self.holders(&[])).and_then(|()| self.pins());
        if let Ok(pins) = pins {
            self.holdings.change(pins, &[]);
        }
    }

    /// What the guest's last call changed of its references to blobs, asked of it after a call that writes no other
    /// records.
    ///
    /// # Errors
    ///
    /// Why the call broke.
    fn pins(&mut self) -> Result<Vec<PinRecord>, Broke> {
        let out = self.pins.call(&mut self.store, ()).map_err(|_| Broke::Trapped)?;
        let pins = self.written(out)?.chunks(PinRecord::BYTES).map(PinRecord::read);

        pins.collect::<Option<_>>().ok_or(Broke::Garbled)
    }

    /// The bytes a call wrote in the guest's memory, where `out` says they lie: its address in the high 32 bits, and
    /// its length in the low.
    ///
    /// # Errors
    ///
    /// [`Broke::Garbled`] when they do not lie in its memory.
    fn written(&self, out: u64) -> Result<&[u8], Broke> {
        let at = u32::try_from(out >> 32).unwrap_or(u32::MAX);
        let len = u32::try_from(out & u64::from(u32::MAX)).unwrap_or(u32::MAX);
        let span = Span { at, len };

        lying(self.memory.data(&self.store), span).ok_or(Broke::Garbled)
    }

    /// Takes into `holdings` what the call that wrote `wrote` changed of the instance's references to blobs, a
    /// reference to a blob it held none of allowed when `given` offered it.
    ///
    /// # Errors
    ///
    /// [`Broke::Forged`] when the guest named a blob it was not given, and [`Broke::Garbled`] when the records do not
    /// read back.
    fn take_pins(&self, wrote: &OutHead, holdings: &mut Holdings, given: &Given) -> Result<(), Broke> {
        // Most calls change no reference to a blob.
        if wrote.pins == 0 {
            return Ok(());
        }
        let data = self.memory.data(&self.store);
        let pins = (0..wrote.pins).map(|index| PinRecord::read(record(data, wrote.pins_at, index, PinRecord::BYTES)?));
        let pins: Vec<PinRecord> = pins.collect::<Option<_>>().ok_or(Broke::Garbled)?;
        let is_honest = holdings.change(pins, &given.offered);
        if !is_honest {
            return Err(Broke::Forged);
        }

        Ok(())
    }

    /// Takes in what the call that wrote `wrote` changed of how many values hold its actors' debts (ADR-0017).
    ///
    /// # Errors
    ///
    /// [`Broke::Garbled`] when the records do not read back.
    #[inline]
    fn take_holders(&self, wrote: &OutHead, also: &[Seated]) -> Result<(), Broke> {
        // Most calls make, write and drop no held request.
        if wrote.holders == 0 {
            return Ok(());
        }
        self.take_written_holders(wrote, also)
    }

    /// As [`Self::take_holders`], for a call that wrote some. Kept out of line, as most write none.
    #[cold]
    #[inline(never)]
    fn take_written_holders(&self, wrote: &OutHead, also: &[Seated]) -> Result<(), Broke> {
        let data = self.memory.data(&self.store);
        let records = (0..wrote.holders)
            .map(|index| HolderRecord::read(record(data, wrote.holders_at, index, HolderRecord::BYTES)?));
        let records: Vec<HolderRecord> = records.collect::<Option<_>>().ok_or(Broke::Garbled)?;
        self.let_go(records, also);

        Ok(())
    }

    /// Takes in what the guest's last call changed of how many values hold its actors' debts, asked of it after a call
    /// that writes no other records (ADR-0017).
    ///
    /// # Errors
    ///
    /// Why the call broke.
    fn holders(&mut self, also: &[Seated]) -> Result<(), Broke> {
        let out = self.holders.call(&mut self.store, ()).map_err(|_| Broke::Trapped)?;
        let records = self.written(out)?.chunks(HolderRecord::BYTES).map(HolderRecord::read);
        let records: Vec<HolderRecord> = records.collect::<Option<_>>().ok_or(Broke::Garbled)?;
        self.let_go(records, also);

        Ok(())
    }

    /// Notes each of `records` whose debt is one of the instance's actors', or of `also`, as a native held request's
    /// changes are noted. A guest's code says nothing of another code's debts, so the rest are passed over, as natively
    /// a change to a debt of an actor in another part is (ADR-0017).
    fn let_go(&self, records: Vec<HolderRecord>, also: &[Seated]) {
        for HolderRecord { owed, by } in records {
            let is_ours = self.actors.contains(&owed.holder) || also.contains(&owed.holder);
            if is_ours {
                holders::change(owed, by);
            }
        }
    }

    /// Offers to the batch's turns a pin on each blob each child the call that wrote `wrote` took out of the instance
    /// refers to, from `holdings`, the instance's as the call began, or `given`'s offers.
    ///
    /// # Errors
    ///
    /// [`Broke::Forged`] when the instance held no reference to one, and [`Broke::Garbled`] when the records do not
    /// read back.
    fn offer_taken_out(&self, wrote: &OutHead, holdings: &Holdings, given: &mut Given) -> Result<(), Broke> {
        // Most calls stand up no child.
        let stood = (wrote.watches_at.saturating_sub(wrote.stood_at)) as usize / StoodRecord::BYTES;
        if stood == 0 {
            return Ok(());
        }
        let data = self.memory.data(&self.store);
        for index in 0..count(stood) {
            let stood = record(data, wrote.stood_at, index, StoodRecord::BYTES).and_then(StoodRecord::read);
            let Kept::Out { state } = stood.ok_or(Broke::Garbled)?.held else {
                continue;
            };
            let carried = carried_pins(lying(data, state).ok_or(Broke::Garbled)?, holdings, &given.offered)?;
            given.offered.extend(carried);
        }

        Ok(())
    }

    /// Writes the items of `batch` straight into the guest's inbox, each an [`ItemHead`] and then its bytes, adds to
    /// `offered` a pin on each blob an item refers to, and answers with how many items and how many bytes.
    fn hand_in(&mut self, batch: &dyn Batch, offered: &mut Vec<Bytes>) -> (u32, u32) {
        let mut items = 0;
        let mut len = 0;
        batch.each(&mut |peeked| {
            peeked.item.pinned().each(|_, pin| offered.push(pin.clone()));
            items += 1;
            let (handed, bytes) = match &peeked.item {
                Item::Mail(mail) => (handed_mail(mail.kind(), peeked.tie, peeked.due), mail.bytes()),
                Item::Ended(ended) => match ended.how() {
                    How::Closed => (Handed::Ended(Gone::Closed), &[][..]),
                    How::Failed(fault) => (Handed::Ended(Gone::Failed { kind: fault.kind() }), fault.bytes()),
                    How::Earlier => (Handed::Ended(Gone::Earlier), &[][..]),
                },
                Item::Answer { .. } | Item::Unanswered { .. } => {
                    len = self.hand_in_filed(&peeked, len, offered);
                    return;
                }
            };
            let head = ItemHead { handed, source: peeked.source, chain: peeked.chain, len: count(bytes.len()) };
            let at = len;
            len += ItemHead::BYTES + bytes.len();
            self.make_room(len);
            let inbox = &mut self.memory.data_mut(&mut self.store)[self.inbox_at..];
            inbox[at..at + ItemHead::BYTES].copy_from_slice(&head.to_bytes());
            inbox[at + ItemHead::BYTES..len].copy_from_slice(bytes);
        });

        (items, count(len))
    }

    /// Writes `peeked`, what became of a request, into the guest's inbox at `at`, its [`ItemHead`], its mail's bytes
    /// and then the context its asker kept for it, adds to `offered` a pin on each blob the context refers to, and
    /// answers with where it ends (ADR-0014, ADR-0017). Kept out of line, as most items are mail.
    #[cold]
    #[inline(never)]
    fn hand_in_filed(&mut self, peeked: &Peeked<'_>, at: usize, offered: &mut Vec<Bytes>) -> usize {
        let (mail, asking) = match &peeked.item {
            Item::Answer { mail, asking } | Item::Unanswered { mail, asking } => (mail, asking),
            Item::Mail(_) | Item::Ended(_) => return at,
        };
        // The context goes in with the blob of each reference it holds, as a state taken in does, and the call is
        // offered their pins, as the guest takes references of its own to them.
        let mut context = Vec::new();
        let pins = asking.context.pins();
        records::write_carried(&mut context, asking.context.bytes(), pins.iter().map(Bytes::id));
        offered.extend(pins.iter().cloned());
        let (request, len, due) = (asking.count, count(context.len()), asking.due);
        let handed = match &peeked.item {
            Item::Answer { .. } => Handed::Answer { kind: mail.kind(), request, context: len, due },
            _ => Handed::Unanswered { answer: asking.answer, request, context: len, due },
        };
        let bytes = mail.bytes();
        let item_len = bytes.len() + context.len();
        let head = ItemHead { handed, source: peeked.source, chain: peeked.chain, len: count(item_len) };
        let (bytes_at, context_at, len) =
            (at + ItemHead::BYTES, at + ItemHead::BYTES + bytes.len(), at + ItemHead::BYTES + item_len);
        self.make_room(len);
        let inbox = &mut self.memory.data_mut(&mut self.store)[self.inbox_at..];
        inbox[at..bytes_at].copy_from_slice(&head.to_bytes());
        inbox[bytes_at..context_at].copy_from_slice(bytes);
        inbox[context_at..len].copy_from_slice(&context);

        len
    }

    /// Copies `bytes` into the guest's inbox, and answers with how many there are.
    fn write_inbox(&mut self, bytes: &[u8]) -> u32 {
        self.make_room(bytes.len());
        self.memory.data_mut(&mut self.store)[self.inbox_at..][..bytes.len()].copy_from_slice(bytes);

        count(bytes.len())
    }

    /// Makes the guest's inbox hold at least `len` bytes, keeping what it holds: at least twice its room when it grows,
    /// so a batch that writes item after item asks for room only now and then.
    fn make_room(&mut self, len: usize) {
        let short = self.inbox_room < len;
        if !short {
            return;
        }
        let room = len.max(2 * self.inbox_room).max(256);
        let Ok(at) = self.inbox.call(&mut self.store, count(room)) else {
            panic!("a guest makes room for what it is handed");
        };
        self.inbox_at = at as usize;
        self.inbox_room = room;
    }

    /// The head of what the guest's last call asked for, which `out` says where it lies.
    ///
    /// # Errors
    ///
    /// [`Broke::Garbled`] when it does not read back.
    fn out_head(&self, out: u64) -> Result<OutHead, Broke> {
        OutHead::read(self.written(out)?).ok_or(Broke::Garbled)
    }
}

/// Hands the `next` turn of those `head` says the guest ran into `hosted`, read from the guest's memory `data` where
/// it wrote them, the turns before it ending where `before` says: the children it spawned, their seats taken from
/// those `given` holds, which the host gave the call; the children of other code the host began for it, from
/// `given`; its sends, each checked against what its sender declares; its lines; the children that began, each kept
/// in the instance, the one `living` numbers, added to the actors `living` holds unless the turn failed; and how it
/// ended. Answers with that, and where it ends.
///
/// # Errors
///
/// Why the call broke, as the guest ran no such turn, or its records do not read back or name what it was not
/// given.
#[allow(clippy::too_many_arguments, reason = "what a replay reads and what it hands back to, each its own")]
fn replay(
    data: &[u8],
    running: &Running,
    head: &OutHead,
    next: u32,
    before: TurnEnds,
    given: &mut Given,
    holdings: &Holdings,
    (instance, actors): (u64, &mut Vec<Seated>),
    hosted: &mut Hosted<'_>,
) -> Result<(Result<(), Fault>, TurnEnds), Broke> {
    let actors_before = actors.len();
    let replayed = replay_records(data, running, head, next, before, given, holdings, (instance, actors), hosted);
    // A failed turn's children are dropped by the engine, and never live in the instance; nor do those of a turn whose
    // records do not read back.
    let has_more = actors.len() > actors_before;
    let is_kept = matches!(replayed, Ok((Ok(()), _)));
    if has_more && !is_kept {
        actors.truncate(actors_before);
    }

    replayed
}

/// What [`replay`] hands back, the children kept in the instance added to the actors `living` holds whatever the turn's
/// end.
#[allow(clippy::too_many_arguments, reason = "what a replay reads and what it hands back to, each its own")]
fn replay_records(
    data: &[u8],
    running: &Running,
    head: &OutHead,
    next: u32,
    before: TurnEnds,
    given: &mut Given,
    holdings: &Holdings,
    living: (u64, &mut Vec<Seated>),
    hosted: &mut Hosted<'_>,
) -> Result<(Result<(), Fault>, TurnEnds), Broke> {
    let was_run = next < head.ran;
    if !was_run {
        return Err(Broke::Garbled);
    }
    let ends = record(data, head.turns_at, next, TurnEnds::BYTES).and_then(TurnEnds::read).ok_or(Broke::Garbled)?;
    // Most calls read all the mail they are handed.
    let unread = head.unread > 0;
    if unread {
        unread_in(data, head, next, hosted)?;
    }
    let spawns = ends.spawns > before.spawns;
    if spawns && !spawn(data, &running.code, head, before, ends, given, hosted)? {
        return Ok((Err(Fault::new(&FalseSend).unwrap_or_else(|_| Broke::Garbled.fault())), ends));
    }
    // Each child of other code is handed over before the turn's sends are checked, which may be to it.
    let has_births = !given.births.is_empty();
    if has_births {
        hand_births(given, next, hosted)?;
    }
    // A request held is held before the turn's sends are checked, so a reply to it among them is refused (ADR-0017).
    let holds = ends.holds > before.holds;
    if holds && !held(data, head, before, hosted)? {
        return Ok((Err(NotOwed.into()), ends));
    }
    // A job takes a count among what its runner sends, so each is filed between the sends it ran between (ADR-0018).
    let mut jobs = Unfiled::of(data, head, before, ends)?;
    for index in before.sends..ends.sends {
        let honest = jobs.file_due(data, holdings, given, hosted)?;
        if let Err(fault) = honest {
            return Ok((Err(fault), ends));
        }
        let send =
            record(data, head.sends_at, index, SendRecord::BYTES).and_then(SendRecord::read).ok_or(Broke::Garbled)?;
        let bytes = lying(data, send.bytes).ok_or(Broke::Garbled)?;
        let request = match send.request {
            Requesting::Not => Request::Not,
            Requesting::Asks { answer, context, due } => asks(data, (answer, due), context, holdings, given)?,
            Requesting::Answers { request } => Request::Answers { request },
        }; // Most mail refers to no blob, and a call whose mail refers to none wrote no reference.
        let pins = if head.refs == 0 {
            Some(Vec::new())
        } else {
            send_pins(data, head, index, bytes, holdings, given)
        };
        let is_refused =
            pins.is_none_or(|pins| hosted.send(send.by, send.to, send.kind, bytes, request, &pins).is_err());
        if is_refused {
            return Ok((Err(Fault::new(&FalseSend).unwrap_or_else(|_| Broke::Garbled.fault())), ends));
        }
    }
    let honest = jobs.file_rest(data, holdings, given, hosted)?;
    if let Err(fault) = honest {
        return Ok((Err(fault), ends));
    }
    cancels(data, head, before, ends, hosted)?;
    for index in before.lines..ends.lines {
        let line =
            record(data, head.lines_at, index, LineRecord::BYTES).and_then(LineRecord::read).ok_or(Broke::Garbled)?;
        let text = lying(data, line.text).and_then(|text| str::from_utf8(text).ok()).ok_or(Broke::Garbled)?;
        hosted.log(line.by, line.level, text).ok_or(Broke::Garbled)?;
    }
    let stands = ends.stood > before.stood;
    if stands {
        stand(data, running, head, before, ends, given, holdings, living, hosted)?;
    }
    for index in before.watches..ends.watches {
        let watch = record(data, head.watches_at, index, WatchRecord::BYTES)
            .and_then(WatchRecord::read)
            .ok_or(Broke::Garbled)?;
        let is_false = hosted.watch(watch.by, watch.of).is_err();
        if is_false {
            return Ok((Err(Fault::new(&FalseWatch).unwrap_or_else(|_| Broke::Garbled.fault())), ends));
        }
    }
    // Most turns claim nothing of a resource.
    let claims = ends.claims > before.claims;
    if claims {
        let honest = hold_to_claims(data, head, before, ends, given, hosted)?;
        if let Err(fault) = honest {
            return Ok((Err(fault), ends));
        }
    }
    // Most turns ask nothing of the lifecycle.
    let stages = ends.stages > before.stages;
    if stages {
        let honest = hold_to_stages(data, head, before, ends, hosted)?;
        if let Err(fault) = honest {
            return Ok((Err(fault), ends));
        }
    }
    let is_last = next + 1 == head.ran;
    let ended = match head.stopped {
        _ if !is_last => Ok(()),
        Stopped::No => Ok(()),
        // A `begin` cannot close its actor, so a record that says one did does not read back.
        Stopped::Closed if !hosted.may_close() => return Err(Broke::Garbled),
        Stopped::Closed => {
            hosted.close();
            Ok(())
        }
        Stopped::Failed { kind, bytes } => {
            Err(Fault::of_parts(kind, lying(data, bytes).ok_or(Broke::Garbled)?.to_vec()))
        }
    };
    Ok((ended, ends))
}

/// Asks `hosted`'s engine to answer or drop the mail of the call's `turn`th turn, when the guest's library found it did
/// not read and ran no handler for it: the engine answers a request `Unreadable`, and drops other mail, as the actor's
/// log records (ADR-0017). Kept out of line, as most mail reads.
///
/// # Errors
///
/// [`Broke::Garbled`] when a record does not read back, or names a turn that handled no item.
#[cold]
#[inline(never)]
fn unread_in(data: &[u8], head: &OutHead, turn: u32, hosted: &mut Hosted<'_>) -> Result<(), Broke> {
    for index in 0..head.unread {
        let record = record(data, head.unread_at, index, UnreadRecord::BYTES)
            .and_then(UnreadRecord::read)
            .ok_or(Broke::Garbled)?;
        let is_turn = record.turn == turn;
        if is_turn {
            hosted.unread(record.reason).ok_or(Broke::Garbled)?;
        }
    }

    Ok(())
}

/// Holds the guest to what a turn claimed of its resources, from the first claim after those `before` ends at to the
/// last `ends` does, in the order claimed, each sent piece of mail it names counted from the turn's first (ADR-0010).
/// What the host handed a child of other code as it began it, which `given` notes, was done already: the claims of it
/// are passed over, and the claim that it went in the config must be of one handed so. Answers with the fault the turn
/// fails with when a claim is false.
///
/// # Errors
///
/// [`Broke::Garbled`] when a record does not read back.
#[cold]
#[inline(never)]
fn hold_to_claims(
    data: &[u8],
    head: &OutHead,
    before: TurnEnds,
    ends: TurnEnds,
    given: &Given,
    hosted: &mut Hosted<'_>,
) -> Result<Result<(), Fault>, Broke> {
    let false_claim = || Err(Fault::new(&NotHeld).unwrap_or_else(|_| Broke::Garbled.fault()));
    let last = head.ran.checked_sub(1).ok_or(Broke::Garbled)?;
    let all = record(data, head.turns_at, last, TurnEnds::BYTES).and_then(TurnEnds::read).ok_or(Broke::Garbled)?;
    let claims_at = head.claims_at(all.watches).ok_or(Broke::Garbled)?;
    for index in before.claims..ends.claims {
        let ClaimRecord(mut claim) =
            record(data, claims_at, index, ClaimRecord::BYTES).and_then(ClaimRecord::read).ok_or(Broke::Garbled)?;
        let was_handed = given.handed.contains(&claim.ticket);
        if was_handed {
            continue;
        }
        if let Claimed::Carried { send } = claim.what {
            let Some(send) = send.checked_sub(before.sends) else {
                return Ok(false_claim());
            };
            claim.what = Claimed::Carried { send };
        }
        let is_false = hosted.claim(claim).is_err();
        if is_false {
            return Ok(false_claim());
        }
    }

    Ok(Ok(()))
}

/// Hands `hosted` the children of other code `given` holds that the call's `turn`th turn spawned, the first it holds:
/// kept out of line, as most turns spawn none.
///
/// # Errors
///
/// [`Broke::Garbled`] when the turn spawned no such child.
#[cold]
#[inline(never)]
fn hand_births(given: &mut Given, turn: u32, hosted: &mut Hosted<'_>) -> Result<(), Broke> {
    while let Some((spawned_in, _)) = given.births.front()
        && *spawned_in == turn
    {
        let (_, birth) = given.births.pop_front().ok_or(Broke::Garbled)?;
        birth.hand(hosted).ok_or(Broke::Garbled)?;
    }

    Ok(())
}

/// Hands `hosted` the spawns a turn recorded, from the first after those `before` ends at to the last `ends` does, each
/// seat taken from those `given` holds: of the guest's own code, or of other code the host began for it. Answers
/// whether each was declared: a spawn the spawner's declaration does not list stops the rest, and fails the turn
/// (ADR-0022).
///
/// # Errors
///
/// [`Broke::Garbled`] when a record does not read back, or names what the call did not make.
fn spawn(
    data: &[u8],
    code: &Code,
    head: &OutHead,
    before: TurnEnds,
    ends: TurnEnds,
    given: &mut Given,
    hosted: &mut Hosted<'_>,
) -> Result<bool, Broke> {
    for index in before.spawns..ends.spawns {
        let spawn = record(data, head.spawns_at, index, SpawnRecord::BYTES)
            .and_then(SpawnRecord::read)
            .ok_or(Broke::Garbled)?;
        let foreign = given.births.iter().find_map(|(_, birth)| birth.code_in(spawn.seat));
        let from = match foreign {
            Some(from) => from,
            None => aethereal_actor::Code::Blob(code.types[code.made_of(spawn.kind).ok_or(Broke::Garbled)?.0 as usize]),
        };
        let is_kind = from.declared().id() == spawn.kind;
        if !is_kind {
            return Err(Broke::Garbled);
        }
        let key = match spawn.key {
            Keying::Singleton => None,
            Keying::Keyed(key) => {
                let key = lying(data, key).and_then(|key| from_bytes::<KeyName>(key, &mut Budget::new(1 << 10)).ok());
                Some(key.ok_or(Broke::Garbled)?)
            }
        };
        let seat = given.seats.iter().position(|seat| *seat == spawn.seat).ok_or(Broke::Garbled)?;
        given.seats.remove(seat);
        match hosted.spawn(spawn.by, from, key, spawn.seat) {
            Ok(_) => {}
            Err(Unspawned::Unadmitted) => return Ok(false),
            Err(Unspawned::Garbled) => return Err(Broke::Garbled),
        }
    }

    Ok(true)
}

/// Stands up, in `hosted`, the children that began in a turn, from the first after those `before` ends at to the
/// last `ends` does, each kept in the instance, the one `living` numbers, added to the actors `living` holds, and the
/// seat of each the guest dropped put back in `handed`.
///
/// # Errors
///
/// Why the call broke, as a record does not read back or names what the call did not make, or a child taken out
/// refers to a blob the call was not given.
#[allow(clippy::too_many_arguments, reason = "what a replay reads and what it hands back to, each its own")]
fn stand(
    data: &[u8],
    running: &Running,
    head: &OutHead,
    before: TurnEnds,
    ends: TurnEnds,
    given: &mut Given,
    holdings: &Holdings,
    (instance, actors): (u64, &mut Vec<Seated>),
    hosted: &mut Hosted<'_>,
) -> Result<(), Broke> {
    let handed = &mut given.seats;
    for index in before.stood..ends.stood {
        let stood =
            record(data, head.stood_at, index, StoodRecord::BYTES).and_then(StoodRecord::read).ok_or(Broke::Garbled)?;
        let me = stood.me;
        let (by, declared) = hosted.spawned(me).ok_or(Broke::Garbled)?;
        let is_above = by == stood.above;
        if !is_above {
            return Err(Broke::Garbled);
        }
        let lives = match stood.held {
            // A child of other code was handed to the engine with what its `begin` asked for.
            Kept::Elsewhere => continue,
            // A child the guest dropped, as the turn that stood it up failed, is not handed to the engine.
            Kept::Here { handle: NO_HANDLE } => {
                handed.push(me.seat);
                continue;
            }
            Kept::Here { handle } => {
                actors.push(me);
                Lives::In { handle, instance }
            }
            Kept::Out { state } => {
                let state = lying(data, state).ok_or(Broke::Garbled)?;
                let carried = carried_pins(state, holdings, &given.offered)?;
                Lives::Out { state: state.to_vec(), carried }
            }
        };
        let (running, ran, given, parked) = (running.clone(), Ran::Nothing, Given::default(), Parked::No);
        let runs = Guest { running, me, lives, declared, ran, given, parked };
        hosted.stand(me, Box::new(runs)).ok_or(Broke::Garbled)?;
    }

    Ok(())
}

/// A pin on each blob the references in the piece of mail `bytes` name, each at where its reference begins in them,
/// which the call's `index`th send, of those `head` says it made, sends: each a blob the instance holds a reference to
/// in `holdings`, or `given` offered. Nothing when one is not, as the guest sent a reference it was not given, or the
/// records do not read back.
fn send_pins(
    data: &[u8],
    head: &OutHead,
    index: u32,
    bytes: &[u8],
    holdings: &Holdings,
    given: &Given,
) -> Option<Vec<Pinning>> {
    let mut pins = Vec::new();
    for at in 0..head.refs {
        let reference = RefRecord::read(record(data, head.refs_at, at, RefRecord::BYTES)?)?;
        let is_this = reference.send == index;
        if is_this {
            let blob = blob_at(bytes.get(reference.at as usize..)?)?;
            let pin = holdings.pin(blob).or_else(|| given.offer(blob))?;
            pins.push((reference.at, pin));
        }
    }

    Some(pins)
}

/// A pin on each blob an actor's state taken out of a guest refers to, `bytes` as [`records::carried`] reads them:
/// each a blob `holdings` hold a reference to, or `offered` offers.
///
/// # Errors
///
/// [`Broke::Forged`] when one is not, and [`Broke::Garbled`] when they do not read back.
fn carried_pins(bytes: &[u8], holdings: &Holdings, offered: &[Bytes]) -> Result<Vec<Bytes>, Broke> {
    let (_, blobs) = records::carried(bytes).ok_or(Broke::Garbled)?;
    let offer = |blob: BlobId| offered.iter().find(|offered| offered.id() == blob).cloned();
    let pins = blobs.into_iter().map(|blob| holdings.pin(blob).or_else(|| offer(blob)));

    pins.collect::<Option<_>>().ok_or(Broke::Forged)
}

/// `wrote`, the head of what a call that runs one turn asked for.
///
/// # Errors
///
/// [`Broke::Garbled`] when it says the call ran another number of turns.
fn one_turn(wrote: OutHead) -> Result<OutHead, Broke> {
    let is_one = wrote.ran == 1;
    if !is_one {
        return Err(Broke::Garbled);
    }

    Ok(wrote)
}

/// The `index`th record of `len` bytes of those that begin at `at` in the guest's memory `data`.
fn record(data: &[u8], at: u32, index: u32, len: usize) -> Option<&[u8]> {
    let start = usize::try_from(at).ok()? + usize::try_from(index).ok()? * len;

    data.get(start..start + len)
}

/// The bytes `span` says lie in the guest's memory `data`.
fn lying(data: &[u8], span: Span) -> Option<&[u8]> {
    let start = usize::try_from(span.at).ok()?;

    data.get(start..start + usize::try_from(span.len).ok()?)
}

/// Whether `hosted` holds the request its turn handles, as its guest's code held it, the turn's after `before` among
/// the call's that `head` heads in `data` (ADR-0017). Kept out of line, as most turns hold nothing.
///
/// # Errors
///
/// [`Broke::Garbled`] when the record does not read back.
#[cold]
#[inline(never)]
fn held(data: &[u8], head: &OutHead, before: TurnEnds, hosted: &mut Hosted<'_>) -> Result<bool, Broke> {
    let last = head.ran.checked_sub(1).ok_or(Broke::Garbled)?;
    let all = record(data, head.turns_at, last, TurnEnds::BYTES).and_then(TurnEnds::read).ok_or(Broke::Garbled)?;
    let holds_at = head.holds_at(all.watches, all.claims).ok_or(Broke::Garbled)?;
    let hold = record(data, holds_at, before.holds, HoldRecord::BYTES).and_then(HoldRecord::read);
    let HoldRecord { answer } = hold.ok_or(Broke::Garbled)?;

    Ok(hosted.hold(answer).is_ok())
}

/// Holds the guest to what its turn, the one after `before` among the call's that `head` heads in `data`, asked of the
/// program's lifecycle, up to `ends`, adding each to `hosted` (ADR-0020). Kept out of line, as most turns ask nothing
/// of it.
///
/// # Errors
///
/// [`Broke::Garbled`] when a record does not read back. A false ask is the turn's fault, [`FalseStage`].
#[cold]
#[inline(never)]
fn hold_to_stages(
    data: &[u8],
    head: &OutHead,
    before: TurnEnds,
    ends: TurnEnds,
    hosted: &mut Hosted<'_>,
) -> Result<Result<(), Fault>, Broke> {
    let last = head.ran.checked_sub(1).ok_or(Broke::Garbled)?;
    let all = record(data, head.turns_at, last, TurnEnds::BYTES).and_then(TurnEnds::read).ok_or(Broke::Garbled)?;
    let stages_at = head.stages_at(all.watches, all.claims, all.holds).ok_or(Broke::Garbled)?;
    for index in before.stages..ends.stages {
        let stage = record(data, stages_at, index, StageRecord::BYTES).and_then(StageRecord::read);
        let StageRecord { by, asked } = stage.ok_or(Broke::Garbled)?;
        let is_false = hosted.stage(by, asked).is_err();
        if is_false {
            return Ok(Err(Fault::new(&FalseStage).unwrap_or_else(|_| Broke::Garbled.fault())));
        }
    }

    Ok(Ok(()))
}

/// The jobs one of a guest's turns ran, as its host files them among the turn's sends: each at the count it took among
/// what its runner sends, in the order they ran (ADR-0018).
enum Unfiled {
    /// The turn ran none.
    None,
    /// Its job records lie from `at`, and the `next`th to the `end`th are still to be filed.
    Left { at: u32, next: u32, end: u32 },
}

impl Unfiled {
    /// The jobs of the turn after `before` among the call's that `head` heads in `data`, up to `ends`.
    ///
    /// # Errors
    ///
    /// [`Broke::Garbled`] when where they lie does not read back.
    fn of(data: &[u8], head: &OutHead, before: TurnEnds, ends: TurnEnds) -> Result<Self, Broke> {
        // Most turns run no job.
        let runs = ends.jobs > before.jobs;
        if !runs {
            return Ok(Self::None);
        }
        let last = head.ran.checked_sub(1).ok_or(Broke::Garbled)?;
        let all = record(data, head.turns_at, last, TurnEnds::BYTES).and_then(TurnEnds::read).ok_or(Broke::Garbled)?;
        let at = head.jobs_at(all).ok_or(Broke::Garbled)?;

        Ok(Self::Left { at, next: before.jobs, end: ends.jobs })
    }

    /// Whether any is still to be filed.
    const fn is_left(&self) -> bool {
        matches!(self, Self::Left { .. })
    }

    /// As [`Self::run_due`], for a turn that may have no job left to file.
    ///
    /// # Errors
    ///
    /// As [`Self::run_due`].
    #[inline]
    fn file_due(
        &mut self,
        data: &[u8],
        holdings: &Holdings,
        given: &Given,
        hosted: &mut Hosted<'_>,
    ) -> Result<Result<(), Fault>, Broke> {
        // Most turns run no job.
        if !self.is_left() {
            return Ok(Ok(()));
        }
        self.run_due(data, holdings, given, hosted)
    }

    /// Files every job left once the turn's sends are handed in, as [`Self::run_due`] does: each must be due by then.
    ///
    /// # Errors
    ///
    /// As [`Self::run_due`], and [`Broke::Garbled`] when a job is left that is not due, as it takes a count no send of
    /// its runner leaves it.
    #[inline]
    fn file_rest(
        &mut self,
        data: &[u8],
        holdings: &Holdings,
        given: &Given,
        hosted: &mut Hosted<'_>,
    ) -> Result<Result<(), Fault>, Broke> {
        let honest = self.file_due(data, holdings, given, hosted)?;
        if self.is_left() {
            return Err(Broke::Garbled);
        }

        Ok(honest)
    }

    /// Files in `hosted`, in the order they ran, each job whose count is the next its runner takes: the jobs it ran
    /// before the next piece of mail it sends. Each refers to blobs `holdings` hold a reference to or `given` offered,
    /// and its context too. Answers with the fault the turn fails with when one is false. Kept out of line, as most
    /// turns run no job.
    ///
    /// # Errors
    ///
    /// [`Broke::Forged`] when a job or its context refers to a blob that is neither, and [`Broke::Garbled`] when a
    /// record does not read back.
    #[cold]
    #[inline(never)]
    fn run_due(
        &mut self,
        data: &[u8],
        holdings: &Holdings,
        given: &Given,
        hosted: &mut Hosted<'_>,
    ) -> Result<Result<(), Fault>, Broke> {
        while let Self::Left { at, next, end } = *self {
            let job = record(data, at, next, JobRecord::BYTES).and_then(JobRecord::read).ok_or(Broke::Garbled)?;
            let is_due = hosted.next_count(job.by) == Some(job.count);
            if !is_due {
                break;
            }
            let carried = lying(data, job.job).ok_or(Broke::Garbled)?;
            let lent = carried_pins(carried, holdings, &given.offered)?;
            let (bytes, _) = records::carried(carried).ok_or(Broke::Garbled)?;
            // A job is cancelled by its asker, and carries no deadline (ADR-0041).
            let answering = (job.answer, Due::Never);
            let Request::Asks { context, .. } = asks(data, answering, job.context, holdings, given)? else {
                return Err(Broke::Garbled);
            };
            let is_false = hosted.run(job.by, job.kind, job.answer, bytes.to_vec(), lent, context).is_err();
            if is_false {
                return Ok(Err(Fault::new(&FalseSend).unwrap_or_else(|_| Broke::Garbled.fault())));
            }
            let is_last = next + 1 == end;
            *self = if is_last {
                Self::None
            } else {
                Self::Left { at, next: next + 1, end }
            };
        }

        Ok(Ok(()))
    }
}

/// As [`cancel`], for a turn that may cancel no job.
///
/// # Errors
///
/// As [`cancel`].
#[inline]
fn cancels(
    data: &[u8],
    head: &OutHead,
    before: TurnEnds,
    ends: TurnEnds,
    hosted: &mut Hosted<'_>,
) -> Result<(), Broke> {
    // Most turns cancel no job.
    let cancels = ends.cancels > before.cancels;
    if !cancels {
        return Ok(());
    }
    cancel(data, head, before, ends, hosted)
}

/// Cancels in `hosted` each job the turn after `before` among the call's that `head` heads in `data` cancelled, up to
/// `ends`, as its guest's code cancelled it (ADR-0018). Kept out of line, as most turns cancel none.
///
/// # Errors
///
/// [`Broke::Garbled`] when a record does not read back.
#[cold]
#[inline(never)]
fn cancel(data: &[u8], head: &OutHead, before: TurnEnds, ends: TurnEnds, hosted: &mut Hosted<'_>) -> Result<(), Broke> {
    let last = head.ran.checked_sub(1).ok_or(Broke::Garbled)?;
    let all = record(data, head.turns_at, last, TurnEnds::BYTES).and_then(TurnEnds::read).ok_or(Broke::Garbled)?;
    let cancels_at = head.cancels_at(all).ok_or(Broke::Garbled)?;
    for index in before.cancels..ends.cancels {
        let cancelled = record(data, cancels_at, index, CancelRecord::BYTES).and_then(CancelRecord::read);
        let CancelRecord { job } = cancelled.ok_or(Broke::Garbled)?;
        hosted.cancel(job);
    }

    Ok(())
}

/// What mail of `kind` tied as `tie` is handed into a guest as: mail that asks a request, due as `due` says, which its
/// turn may reply to or hold, and whose deadline what it asks inherits; or mail as it came (ADR-0017, ADR-0041).
fn handed_mail(kind: KindId, tie: Tie, due: Due) -> Handed {
    match tie.read() {
        Tied::Asks(request) => Handed::Asks { kind, request, due },
        Tied::Tell | Tied::Answers(_) | Tied::Unanswered(_) => Handed::Mail { kind },
    }
}

/// What a guest's send that asks a request, answered with a kind `answer` names by `due`, is to it, the context it
/// keeps for the request where `context` says in `data`, as [`records::carried`] reads it, with a pin on each blob it
/// refers to: each a blob `holdings` hold a reference to, or `given` offered. Kept out of line, as most mail asks
/// nothing.
///
/// # Errors
///
/// [`Broke::Forged`] when it refers to a blob that is neither, and [`Broke::Garbled`] when it does not read back.
#[cold]
#[inline(never)]
fn asks(
    data: &[u8],
    (answer, due): (KindId, Due),
    context: Span,
    holdings: &Holdings,
    given: &Given,
) -> Result<Request, Broke> {
    let carried = lying(data, context).ok_or(Broke::Garbled)?;
    let pins = carried_pins(carried, holdings, &given.offered)?;
    let (context, _) = records::carried(carried).ok_or(Broke::Garbled)?;

    Ok(Request::Asks { answer, context: Written::new(context.to_vec(), pins), due })
}

/// A count the guest's 32-bit memory holds.
fn count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// The names of the top-level fields of `new`, an actor's state schema in the code it moves onto, that `old`, its
/// schema in the code it ran, lacks: the fields the new code was handed absent and worked out, as the move logs them
/// (ADR-0054). None when either is not a struct's, whose fields have no names.
fn absent(old: &SchemaNodes, new: &SchemaNodes) -> Vec<String> {
    let (SchemaNode::Struct { fields: was, .. }, SchemaNode::Struct { fields: now, .. }) = (old.root(), new.root())
    else {
        return Vec::new();
    };

    now.iter().filter(|field| was.iter().all(|had| had.name != field.name)).map(|field| field.name.clone()).collect()
}

/// The bytes of the `aethereal.declared` section of the module `bytes`, when it has one.
fn declared_section(bytes: &[u8]) -> Option<&[u8]> {
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let Ok(wasmparser::Payload::CustomSection(custom)) = payload
            && custom.name() == section::NAME
        {
            return Some(custom.data());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Monotonic;
    use aethereal_actor::{
        Actor, ActorRef, Answer, Answered, Beginning, Bytes, Carries, Ending, Forged, Given, Handle, Handles, Links,
        Origin, Protocol, Publisher, Seated, Services, ToAny, ToSender, ToType, Trapped, Turn, Unanswered,
    };
    use aethereal_actor::{Anywhere, ChildOf, Level, Tick, Time, Top};
    use aethereal_actor::{JobId, JobType};
    use aethereal_data::{ActorPath, Kind, KindName, PathSegment, Plain, Schema, SchemaNodes};
    use aethereal_guest_echo as echo;
    use aethereal_substrate::harness::{Harness, Jobbed, Unheard};
    use aethereal_substrate::{CodeRefused, Engine, Inline, Jobs, Setup, Taken};
    use std::num::NonZeroUsize;
    use std::sync::Mutex;

    /// The echo guest the build script built.
    const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

    /// `engine`, running guests with wasmtime, and the echo guest put into it as code.
    fn with_echo(setup: Setup<()>) -> (Engine, Bytes) {
        let mut engine = setup.with_guests(Arc::new(Runtime::default())).ready();
        let Ok(code) = engine.put_code(ECHO.to_vec()) else {
            panic!("the echo guest is fit to run");
        };

        (engine, code)
    }

    /// The state of the live native actor `id` stands for, which is a `T`, as of the last step: what it kept of what
    /// it was told and answered, which is how a test reads what it saw.
    fn state<T: 'static>(engine: &Engine, id: ActorId) -> &T {
        let Some(seat) = engine.table().seat(id) else {
            panic!("the actor is live");
        };
        let Some(state) = engine.state(Seated { id, seat }) else {
            panic!("the actor's state is here, and of the type asked for");
        };

        state
    }

    /// A ping, as the echo guest declares it: the same name and layout, so the same kind.
    #[derive(Plain)]
    #[kind(name = "aether.guest.ping")]
    struct Ping {
        n: u64,
    }

    #[derive(Plain)]
    #[kind(name = "aether.guest.pong")]
    struct Pong {
        n: u64,
    }

    /// How many pings a pinger sends.
    #[derive(Plain)]
    #[kind(name = "aether.test.volley")]
    struct Volley {
        pings: u64,
    }

    /// A native actor that pings the echo guest, and pings it again on each pong while it has pings left, counting the
    /// pongs it has been answered with.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.pinger")]
    struct Pinger {
        pongs: u64,
    }

    impl ChildOf for Pinger {
        type Parent = Top;
    }

    impl Actor for Pinger {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(volley: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(echo) = turn.resolve(&echo).and_then(|found| turn.cast(found)) {
                turn.to(echo).send(&Ping { n: volley.pings })?;
            }

            Ok(Self { pongs: 0 })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Pinger {
        type Sends = (ToSender<Ping>,);

        fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.pongs += 1;
            if pong.n > 0 {
                turn.reply(&Ping { n: pong.n - 1 })?;
            }

            Ok(())
        }
    }

    #[test]
    fn a_native_actor_and_a_guest_answer_each_other() {
        // Catches mail lost on its way into a guest or out of it, a guest's reply that does not reach its sender, and
        // a section that does not declare the guest's handler, which its host would refuse the mail for.
        let Some(parts) = NonZeroUsize::new(4) else {
            panic!("four is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 64));
        let Ok(_) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("the echo begins");
        };
        let Ok(pinger) = engine.add_root::<Pinger>(&(), Volley { pings: 9 }) else {
            panic!("the pinger begins");
        };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }

        assert_eq!(state::<Pinger>(&engine, pinger).pongs, 10);
    }

    /// The protocol the echo guest is asked through: a ping, answered with a pong.
    struct Pings;

    impl Protocol for Pings {
        type Kinds = (echo::Ping,);
    }

    impl Carries<echo::Ping> for Pings {
        type Answers = (echo::Pong,);
    }

    /// What a questioner keeps until its ping is answered: which ping it was.
    #[derive(Plain)]
    #[kind(name = "aether.test.which")]
    struct Which {
        at: u64,
    }

    /// A native actor that spawns the echo guest from its code and asks it two pings, keeping which each was, and keeps
    /// what it was answered: each pong with what it kept for its ping.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.questioner")]
    struct Questioner {
        code: Bytes,
        questioned: Vec<(u64, u64)>,
    }

    impl ChildOf for Questioner {
        type Parent = Top;
    }

    impl Actor for Questioner {
        type Config = Self;
        type Key = ();
        type Handles = (Volley, Answer<echo::Pong, Which>);
        type BeginSends = (ToType<Self, Volley>,);
        type EndSends = ();

        fn begin(questioner: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            turn.to(turn.me()).send(&Volley { pings: 2 })?;

            Ok(questioner)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Volley> for Questioner {
        type Sends = (aethereal_actor::Asks<Pings, echo::Ping, Which>,);

        fn handle(&mut self, volley: Volley, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            let Ok(echo) = turn.spawn_from::<echo::Echo>(Origin::Blob(&self.code), &(), &echo::EchoConfig) else {
                return Ok(());
            };
            for at in 0..volley.pings {
                turn.to(echo.protocol()).ask(&echo::Ping { n: at * 10 }, &Which { at })?;
            }

            Ok(())
        }
    }

    impl Handles<Answer<echo::Pong, Which>> for Questioner {
        type Sends = ();

        fn handle(
            &mut self,
            (pong, which): (Answered<echo::Pong>, Which),
            _: &mut Turn<'_, Self, ()>,
        ) -> Result<(), Fault> {
            let Answered::Yes(pong) = pong else {
                return Ok(());
            };
            self.questioned.push((pong.n, which.at));

            Ok(())
        }
    }

    #[test]
    fn a_guest_answers_a_native_actors_request() {
        // Catches a guest's reply to a request that its host sends on as plain mail, which the asker cannot place,
        // and an answer tied to the wrong one of two requests.
        let Some(parts) = NonZeroUsize::new(4) else {
            panic!("four is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 64));
        let Ok(questioner) = engine.add_root::<Questioner>(&(), Questioner { code, questioned: Vec::new() }) else {
            panic!("the questioner begins");
        };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }

        assert_eq!(state::<Questioner>(&engine, questioner).questioned, [(0, 0), (10, 1)]);
    }

    /// What a petitioner was answered: which request it was, and its answer's number or why it had none.
    type Petitioned = (u64, Result<u64, Unanswered>);

    /// What a petitioner is made from: the steward guest's code, and how the steward is to release what it holds.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.petitioning")]
    struct Petitioning {
        code: Bytes,
        how: u64,
    }

    /// A native actor that spawns the steward guest from its code, asks it a ping, a weighing and a ping, one after
    /// another, and tells it how to release them, keeping what it was answered.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.petitioner")]
    struct Petitioner {
        petitioning: Petitioning,
        petitioned: Vec<Petitioned>,
    }

    impl ChildOf for Petitioner {
        type Parent = Top;
    }

    impl Actor for Petitioner {
        type Config = Petitioning;
        type Key = u64;
        type Handles = (Volley, Answer<echo::Pong, Which>, Answer<echo::Weight, Which>);
        type BeginSends = (ToType<Self, Volley>,);
        type EndSends = ();

        fn begin(petitioning: Petitioning, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            turn.to(turn.me()).send(&Volley { pings: 2 })?;

            Ok(Self { petitioning, petitioned: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Volley> for Petitioner {
        type Sends = (
            aethereal_actor::Asks<echo::Pings, echo::Ping, Which>,
            aethereal_actor::Asks<echo::Weighs, echo::Weigh, Which>,
            ToType<echo::Steward, echo::Release>,
        );

        fn handle(&mut self, _: Volley, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            let Petitioning { code, how } = &self.petitioning;
            let Ok(steward) = turn.spawn_from::<echo::Steward>(Origin::Blob(code), &(), &echo::EchoConfig) else {
                return Err(Fault::new(&Doom)?);
            };
            // Held in one call into the guest, each answered with its own kind.
            turn.to(steward.protocol()).ask(&echo::Ping { n: 0 }, &Which { at: 0 })?;
            turn.to(steward.protocol()).ask(&echo::Weigh { n: 5 }, &Which { at: 1 })?;
            turn.to(steward.protocol()).ask(&echo::Ping { n: 20 }, &Which { at: 2 })?;
            turn.to(steward).send(&echo::Release { how: *how })?;

            Ok(())
        }
    }

    impl Handles<Answer<echo::Pong, Which>> for Petitioner {
        type Sends = ();

        fn handle(
            &mut self,
            (pong, which): (Answered<echo::Pong>, Which),
            _: &mut Turn<'_, Self, ()>,
        ) -> Result<(), Fault> {
            let answered = match pong {
                Answered::Yes(pong) => Ok(pong.n),
                Answered::No(why) => Err(why),
            };
            self.petitioned(which.at, answered);

            Ok(())
        }
    }

    impl Handles<Answer<echo::Weight, Which>> for Petitioner {
        type Sends = ();

        fn handle(
            &mut self,
            (weight, which): (Answered<echo::Weight>, Which),
            _: &mut Turn<'_, Self, ()>,
        ) -> Result<(), Fault> {
            let answered = match weight {
                Answered::Yes(weight) => Ok(weight.n),
                Answered::No(why) => Err(why),
            };
            self.petitioned(which.at, answered);

            Ok(())
        }
    }

    impl Petitioner {
        /// Keeps what became of the request it asked `at`th.
        fn petitioned(&mut self, at: u64, answered: Result<u64, Unanswered>) {
            self.petitioned.push((at, answered));
        }
    }

    #[test]
    fn a_guest_holds_a_request_and_its_asker_hears_what_became_of_it() {
        // Catches a guest's hold its host does not file, or files as the hold of another turn of the call, an answer to a held request its host sends as plain mail or
        // as a reply to the item, a held request dropped in the guest that its asker never hears of, one the guest
        // closed on that is never said to have ended, one written into a request's context and read back that is
        // let go of as its first value is dropped, and an answer from the guest's closing turn its host refuses or
        // the engine sends as well as saying the guest ended.
        for (parts, hops) in [(1, 0), (1, 64), (4, 0), (4, 64)] {
            let Some(parts) = NonZeroUsize::new(parts) else {
                panic!("no engine has no parts");
            };
            let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, hops));
            let mut petitioners = Vec::new();
            for how in 0..5 {
                let petitioning = Petitioning { code: code.clone(), how };
                let Ok(petitioner) = engine.add_root::<Petitioner>(&how, petitioning) else {
                    panic!("the petitioner begins");
                };
                petitioners.push((how, petitioner));
            }
            let clock = Monotonic::new();
            while !engine.is_idle() {
                engine.step(Time::ZERO, &clock);
            }

            let mut petitioned = Vec::new();
            for (how, petitioner) in petitioners {
                let answered = &state::<Petitioner>(&engine, petitioner).petitioned;
                petitioned.extend(answered.iter().map(|(at, answer)| (how, *at, *answer)));
            }
            petitioned.sort_by_key(|(how, at, _)| (*how, *at));
            let (unreplied, ended) = (Err(Unanswered::Unreplied), Err(Unanswered::Ended));
            let expected = [
                (0, 0, Ok(100)),
                (0, 1, Ok(105)),
                (0, 2, Ok(120)),
                (1, 0, unreplied),
                (1, 1, unreplied),
                (1, 2, unreplied),
                (2, 0, ended),
                (2, 1, ended),
                (2, 2, ended),
                (3, 0, Ok(200)),
                (3, 1, Ok(305)),
                (3, 2, Ok(220)),
                (4, 0, Ok(400)),
                (4, 1, Ok(405)),
                (4, 2, Ok(420)),
            ];
            assert_eq!(petitioned, expected, "{parts} parts, {hops} hops");
        }
    }

    /// A native actor that spawns the shelver guest from its code and pings it, keeping what it is told: each pong's
    /// number, and a hundred and each book's length.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.shelving")]
    struct Shelving {
        shelved: Vec<u64>,
    }

    impl ChildOf for Shelving {
        type Parent = Top;
    }

    impl Actor for Shelving {
        type Config = Inquiring;
        type Key = ();
        type Handles = (echo::Pong, echo::Book);
        type BeginSends = (ToType<echo::Shelver, echo::Ping>,);
        type EndSends = ();

        fn begin(inquiring: Inquiring, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let Ok(shelver) = turn.spawn_from::<echo::Shelver>(Origin::Blob(&inquiring.code), &(), &echo::EchoConfig)
            else {
                return Err(Fault::new(&Doom)?);
            };
            turn.to(shelver).send(&echo::Ping { n: 3 })?;

            Ok(Self { shelved: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Pong> for Shelving {
        type Sends = ();

        fn handle(&mut self, pong: echo::Pong, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
            self.shelved.push(pong.n);

            Ok(())
        }
    }

    impl Handles<echo::Book> for Shelving {
        type Sends = ();

        fn handle(&mut self, book: echo::Book, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
            let len = 100 + turn.read(&book.blob).len() as u64;
            self.shelved.push(len);

            Ok(())
        }
    }

    #[test]
    fn a_guest_s_run_of_mail_after_other_mail_holds_its_own_references() {
        // Catches the references in a guest's run of mail placed among the call's sends from the run's own first,
        // which pins the blobs of a run that does not come first on other mail.
        let (mut engine, code) = with_echo(Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0));
        let Ok(shelving) = engine.add_root::<Shelving>(&(), Inquiring { code }) else {
            panic!("the shelving begins");
        };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }

        assert_eq!(state::<Shelving>(&engine, shelving).shelved, [3, 103, 104]);
    }

    /// What a guest heard of each request it asked, by which it was: its pong's number, or why it had none.
    type Heard = (u64, Result<u64, Unanswered>);

    /// A native actor that spawns the querent guest from its code and has it ask, keeping what it hears: once more
    /// when it hears its first query's last request went unanswered.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.inquirer")]
    struct Inquirer {
        querent: ActorRef<echo::Querent>,
        heard: Vec<Heard>,
    }

    /// What an inquirer is made from: the querent guest's code.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.inquiring")]
    struct Inquiring {
        code: Bytes,
    }

    impl ChildOf for Inquirer {
        type Parent = Top;
    }

    impl Actor for Inquirer {
        type Config = Inquiring;
        type Key = ();
        type Handles = (echo::Heard,);
        type BeginSends = (ToType<echo::Querent, echo::Query>,);
        type EndSends = ();

        fn begin(inquiring: Inquiring, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let Ok(querent) = turn.spawn_from(Origin::Blob(&inquiring.code), &(), &echo::EchoConfig) else {
                return Err(Fault::new(&Doom)?);
            };
            turn.to(querent).send(&echo::Query { first: 0, pings: 2 })?;

            Ok(Self { querent, heard: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Heard> for Inquirer {
        type Sends = (ToType<echo::Querent, echo::Query>,);

        fn handle(&mut self, heard: echo::Heard, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            // A second query, asked in a later call into the guest than the first.
            let is_first_done = matches!(heard, echo::Heard::No { at: 2, .. });
            if is_first_done {
                turn.to(self.querent).send(&echo::Query { first: 3, pings: 1 })?;
            }
            let heard = match heard {
                echo::Heard::Pong { at, n } => (at, Ok(n)),
                echo::Heard::No { at, why } => (at, Err(why)),
            };
            self.heard.push(heard);

            Ok(())
        }
    }

    #[test]
    fn a_guest_asks_and_is_handed_what_became_of_each_request_with_what_it_kept() {
        // Catches a guest's request its host sends on as a tell, an answer handed into a guest as plain mail, which
        // its answer handler never sees, a context lost or tied to the wrong request on the way back in, a call's
        // requests filed with the contexts of an earlier call's, and an unanswered request whose word never reaches
        // the guest's handler.
        // In one part and in several, each piece of mail waiting its turn and handed on as it is sent.
        for (parts, hops) in [(1, 0), (1, 64), (4, 0), (4, 64)] {
            let Some(parts) = NonZeroUsize::new(parts) else {
                panic!("no engine has no parts");
            };
            let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, hops));
            let Ok(inquirer) = engine.add_root::<Inquirer>(&(), Inquiring { code }) else {
                panic!("the inquirer begins");
            };
            let clock = Monotonic::new();
            while !engine.is_idle() {
                engine.step(Time::ZERO, &clock);
            }

            let mut heard = state::<Inquirer>(&engine, inquirer).heard.clone();
            heard.sort_by_key(|(at, _)| *at);
            let unreplied = Err(Unanswered::Unreplied);
            let expected = [(0, Ok(0)), (1, Ok(10)), (2, unreplied), (3, Ok(30)), (4, unreplied)];
            assert_eq!(heard, expected, "{parts} parts, {hops} hops");
        }
    }

    /// A native actor that spawns the opener guest from its code, which stands up one more beneath it, and that one a
    /// native inlet, keeping what they hear of the request each asked as it began, by which it was.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.opening")]
    struct Opening {
        opened: Vec<Heard>,
    }

    impl ChildOf for Opening {
        type Parent = Top;
    }

    impl Actor for Opening {
        type Config = Inquiring;
        type Key = ();
        type Handles = (echo::Heard,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(inquiring: Inquiring, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let opening = echo::Opening { n: 3, below: 1 };
            let Ok(_) = turn.spawn_from::<echo::Opener>(Origin::Blob(&inquiring.code), &3, &opening) else {
                return Err(Fault::new(&Doom)?);
            };

            Ok(Self { opened: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Heard> for Opening {
        type Sends = ();

        fn handle(&mut self, heard: echo::Heard, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
            let heard = match heard {
                echo::Heard::Pong { at, n } => (at, Ok(n)),
                echo::Heard::No { at, why } => (at, Err(why)),
            };
            self.opened.push(heard);

            Ok(())
        }
    }

    #[test]
    fn a_guest_asks_in_its_begin_and_so_does_a_child_its_begin_stands_up() {
        // Catches a request a guest's `begin` asked that its host refuses, as only a turn that handles an item asked
        // before, one asked by a child the guest stood up in its own code taken for its parent's, one asked by a native
        // child its host began apart lost as that `begin` is taken in, and any filed beside an actor that never asked
        // it.
        for (parts, hops) in [(1, 0), (1, 64), (4, 0), (4, 64)] {
            let Some(parts) = NonZeroUsize::new(parts) else {
                panic!("no engine has no parts");
            };
            let engine = Engine::new(parts, 1024, 0, 1 << 20, hops).with_links(Links::default().with::<echo::Inlet>());
            let (mut engine, code) = with_echo(engine);
            let Ok(opening) = engine.add_root::<Opening>(&(), Inquiring { code }) else {
                panic!("the opening begins");
            };
            let clock = Monotonic::new();
            while !engine.is_idle() {
                engine.step(Time::ZERO, &clock);
            }

            let mut opened = state::<Opening>(&engine, opening).opened.clone();
            opened.sort_by_key(|(at, _)| *at);
            assert_eq!(opened, [(3, Ok(3)), (4, Ok(4)), (5, Ok(5))], "{parts} parts, {hops} hops");
        }
    }

    /// A native actor that spawns the stasher guest from its code and has it stash a blob of three bytes, keeping what
    /// it hears: how many bytes the blob it kept held, and its pong's number.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.stashing")]
    struct Stashing {
        stashed: Vec<Heard>,
    }

    impl ChildOf for Stashing {
        type Parent = Top;
    }

    impl Actor for Stashing {
        type Config = Inquiring;
        type Key = ();
        type Handles = (echo::Heard,);
        type BeginSends = (ToType<echo::Stasher, echo::Stash>,);
        type EndSends = ();

        fn begin(inquiring: Inquiring, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let Ok(stasher) = turn.spawn_from::<echo::Stasher>(Origin::Blob(&inquiring.code), &(), &echo::EchoConfig)
            else {
                return Err(Fault::new(&Doom)?);
            };
            turn.to(stasher).send(&echo::Stash { len: 3 })?;

            Ok(Self { stashed: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Heard> for Stashing {
        type Sends = ();

        fn handle(&mut self, heard: echo::Heard, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            let heard = match heard {
                echo::Heard::Pong { at, n } => (at, Ok(n)),
                echo::Heard::No { at, why } => (at, Err(why)),
            };
            self.stashed.push(heard);

            Ok(())
        }
    }

    #[test]
    fn a_blob_a_guest_s_context_refers_to_is_kept_and_offered_back_with_its_answer() {
        // Catches a guest's context whose blobs its host pins nothing for, freed once the guest lets go of its own
        // reference, and a context handed back without offering them, which takes its reference read back for one
        // the guest's code forged.
        for (parts, hops) in [(1, 0), (1, 64), (4, 0), (4, 64)] {
            let Some(parts) = NonZeroUsize::new(parts) else {
                panic!("no engine has no parts");
            };
            let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, hops));
            let Ok(stashing) = engine.add_root::<Stashing>(&(), Inquiring { code }) else {
                panic!("the stashing begins");
            };
            let clock = Monotonic::new();
            while !engine.is_idle() {
                engine.step(Time::ZERO, &clock);
            }

            assert_eq!(state::<Stashing>(&engine, stashing).stashed, [(3, Ok(0))], "{parts} parts, {hops} hops");
        }
    }

    /// The word to a lender to lend its nester a book.
    #[derive(Plain)]
    #[kind(name = "aether.test.lend")]
    struct Lend;

    /// A native actor that spawns the nester guest from its code, then puts a blob of three bytes and mails it to the
    /// nester in a turn of its own, keeping none of it, and keeps each pong it is told.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.lender")]
    struct Lender {
        nester: ActorRef<echo::Nester>,
        lent: Vec<u64>,
    }

    impl ChildOf for Lender {
        type Parent = Top;
    }

    impl Actor for Lender {
        type Config = Inquiring;
        type Key = ();
        type Handles = (Lend, Pong);
        type BeginSends = (ToType<Self, Lend>,);
        type EndSends = ();

        fn begin(inquiring: Inquiring, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let Ok(nester) = turn.spawn_from(Origin::Blob(&inquiring.code), &(), &echo::EchoConfig) else {
                return Err(Fault::new(&Doom)?);
            };
            turn.to(turn.me()).send(&Lend)?;

            Ok(Self { nester, lent: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Lend> for Lender {
        type Sends = (ToType<echo::Nester, echo::Book>,);

        fn handle(&mut self, _: Lend, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            let Ok(blob) = turn.put(vec![7; 3]) else {
                return Err(Fault::new(&Doom)?);
            };
            turn.to(self.nester).send(&echo::Book { blob })?;

            Ok(())
        }
    }

    impl Handles<Pong> for Lender {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.lent.push(pong.n);

            Ok(())
        }
    }

    #[test]
    fn a_blob_mailed_to_a_guest_and_handed_on_in_a_native_child_s_config_is_kept_and_offered() {
        // Catches mail a guest is handed, as a hop or as it waited, whose blobs its host does not offer the call, which
        // takes the reference the guest reads for one it forged; and a config a guest hands a native child read
        // without the pins its birth carries, so the child takes no reference to the blob it was made from.
        for (parts, hops) in [(1, 0), (1, 64), (4, 0), (4, 64)] {
            let Some(parts) = NonZeroUsize::new(parts) else {
                panic!("no engine has no parts");
            };
            let engine =
                Engine::new(parts, 1024, 0, 1 << 20, hops).with_links(Links::default().with::<echo::Nestling>());
            let (mut engine, code) = with_echo(engine);
            let Ok(lender) = engine.add_root::<Lender>(&(), Inquiring { code }) else {
                panic!("the lender begins");
            };
            let clock = Monotonic::new();
            while !engine.is_idle() {
                engine.step(Time::ZERO, &clock);
            }

            assert_eq!(state::<Lender>(&engine, lender).lent, [203], "{parts} parts, {hops} hops");
        }
    }

    /// The word to close, as the echo guest declares it.
    #[derive(Plain)]
    #[kind(name = "aether.guest.shut")]
    struct Shut;

    /// A native actor that sends the echo guest a ping, the word to close, and another ping, all in its `begin`, and
    /// counts the pongs it is answered with.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.closer")]
    struct Closer {
        pongs: u64,
    }

    impl ChildOf for Closer {
        type Parent = Top;
    }

    impl Actor for Closer {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping, Shut), Ping>, ToAny<(Ping, Shut), Shut>);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(echo) = turn.resolve(&echo).and_then(|found| turn.cast(found)) {
                turn.to(echo).send(&Ping { n: 0 })?.send(&Shut)?.send(&Ping { n: 0 })?;
            }

            Ok(Self { pongs: 0 })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Closer {
        type Sends = ();

        fn handle(&mut self, _: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.pongs += 1;

            Ok(())
        }
    }

    #[test]
    fn a_guest_that_closes_answers_nothing_after() {
        // Catches a guest's close lost on its way out of the guest: the echo would then be asked for the turn of the
        // ping after it, which the guest did not run, and fail.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(echo) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("the echo begins");
        };
        let Ok(closer) = engine.add_root::<Closer>(&(), Volley { pings: 0 }) else {
            panic!("the closer begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }

        let failed = engine.log(echo).any(|entry| matches!(entry, aethereal_actor::Entry::Failed(_)));
        assert!(!failed, "the echo closed, and did not fail");
        assert_eq!(state::<Closer>(&engine, closer).pongs, 1);
    }

    #[test]
    fn a_guest_s_memory_is_counted_and_named_by_its_first_actor() {
        // Catches an instance whose memory the engine never measures, or names by no actor, so the band's guest meter
        // reads none while a guest runs (ADR-0053).
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(echo) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("the echo begins");
        };
        let Some(path) = engine.table().path(echo) else {
            panic!("the echo stands at a path");
        };

        let figures = engine.engine_stats();

        let named = figures.largest_guests.first().map(|guest| (guest.actor.clone(), guest.actors));
        assert!(figures.guest_bytes > 0, "{figures:?}");
        assert_eq!(named, Some((aethereal_actor::ActorName::Path(path), 1)));
    }

    /// A native actor that pings the echo guest once as it begins, and keeps the number of each pong it is answered
    /// with, its own or another's.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.caller")]
    struct Caller {
        key: u64,
        answered: Vec<u64>,
    }

    impl ChildOf for Caller {
        type Parent = Top;
    }

    impl Actor for Caller {
        type Config = Volley;
        type Key = u64;
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(volley: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(echo) = turn.resolve(&echo).and_then(|found| turn.cast(found)) {
                turn.to(echo).send(&Ping { n: volley.pings })?;
            }

            Ok(Self { key: volley.pings, answered: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Caller {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.answered.push(pong.n);

            Ok(())
        }
    }

    #[test]
    fn each_of_a_batch_of_callers_is_answered_once() {
        // Catches a turn handed back what another turn of its batch sent, a send handed back twice or not at all, and
        // an item written over another as the inbox grows while a batch is written into it.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(_) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("the echo begins");
        };
        let mut callers = Vec::new();
        for key in 0..50 {
            let Ok(caller) = engine.add_root::<Caller>(&key, Volley { pings: key }) else {
                panic!("a caller begins");
            };
            callers.push(caller);
        }
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }

        let answered: Vec<(u64, Vec<u64>)> = callers
            .into_iter()
            .map(|caller| state::<Caller>(&engine, caller))
            .map(|caller| (caller.key, caller.answered.clone()))
            .collect();
        let each_its_own: Vec<(u64, Vec<u64>)> = (0..50).map(|key| (key, vec![key])).collect();
        assert_eq!(answered, each_its_own);
    }

    /// A native actor that tells the mirror guest to close, and volleys with the echo guest, both of one blob, counting
    /// the pongs it is answered with.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.volleyer")]
    struct Volleyer {
        pongs: u64,
    }

    impl ChildOf for Volleyer {
        type Parent = Top;
    }

    impl Actor for Volleyer {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping, Shut), Ping>, ToAny<(Ping, Shut), Shut>);
        type EndSends = ();

        fn begin(volley: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let mirror = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.mirror"), &()));
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(mirror) = turn.resolve(&mirror).and_then(|found| turn.cast(found)) {
                turn.to(mirror).send(&Shut)?;
            }
            if let Some(echo) = turn.resolve(&echo).and_then(|found| turn.cast(found)) {
                turn.to(echo).send(&Ping { n: volley.pings })?;
            }

            Ok(Self { pongs: 0 })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Volleyer {
        type Sends = (ToSender<Ping>,);

        fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.pongs += 1;
            if pong.n > 0 {
                turn.reply(&Ping { n: pong.n - 1 })?;
            }

            Ok(())
        }
    }

    /// A native actor that asks the timekeeper guest the time as it begins, and keeps the time it was last told.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.time_asker")]
    struct TimeAsker {
        told: u64,
    }

    impl ChildOf for TimeAsker {
        type Parent = Top;
    }

    impl Actor for TimeAsker {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let keeper = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.timekeeper"), &()));
            if let Some(keeper) = turn.resolve(&keeper).and_then(|found| turn.cast(found)) {
                turn.to(keeper).send(&Ping { n: 0 })?;
            }

            Ok(Self { told: 0 })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for TimeAsker {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.told = pong.n;

            Ok(())
        }
    }

    #[test]
    fn a_guest_s_turn_reads_the_time_its_step_was_given() {
        // Catches a guest's host answering its call out for the time with anything but the step's.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(_) = engine.add_root_from::<echo::Timekeeper>(&code, &(), &echo::EchoConfig) else {
            panic!("the timekeeper begins");
        };
        let Ok(asker) = engine.add_root::<TimeAsker>(&(), Volley { pings: 0 }) else {
            panic!("the asker begins");
        };
        let clock = Monotonic::new();
        engine.step(Time::from_nanos(5), &clock);
        engine.step(Time::from_nanos(9), &clock);

        assert_eq!(state::<TimeAsker>(&engine, asker).told, 5);
    }

    #[test]
    fn a_guest_subscribes_to_the_lifecycle_and_asks_the_program_to_stop() {
        // Catches a guest's subscriptions and its request to stop going unread by its host: the guest would be sent no
        // stage, and the program would not stop.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(starter) = engine.add_root_from::<echo::Starter>(&code, &(), &echo::EchoConfig) else {
            panic!("the starter begins");
        };
        let Some(seat) = engine.table().seat(starter) else {
            panic!("the starter is seated");
        };
        let starter = Seated { id: starter, seat };
        engine.start();
        let clock = Monotonic::new();
        for _ in 0..8 {
            engine.step(Time::ZERO, &clock);
        }

        assert_eq!(engine.lifecycle(), aethereal_substrate::Lifecycle::Stopped);
        let said: Vec<&str> = engine
            .log(starter)
            .filter_map(|entry| match entry {
                aethereal_actor::Entry::Line { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(said, ["started", "stopping"]);
    }

    /// A native actor that spawns a ticker from the echo's blob as it begins, and logs whether its engine made it.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.breeder")]
    struct Breeder {
        code: Bytes,
    }

    impl ChildOf for Breeder {
        type Parent = Top;
    }

    impl Actor for Breeder {
        type Config = Self;
        type Key = ();
        type Handles = ();
        type BeginSends = ();
        type EndSends = ();

        fn begin(breeder: Self, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let spawned = turn.spawn_from::<echo::Ticker>(Origin::Blob(&breeder.code), &(), &echo::EchoConfig);
            let said = match spawned {
                Ok(_) => "ticker made",
                Err(Refused::Unstaged(stage)) if stage.same(KindId::of::<Tick>()) => "ticker refused for tick",
                Err(_) => "ticker refused",
            };
            turn.log(Level::Info, said);

            Ok(breeder)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    /// The lines `id` has logged on `engine`.
    fn lines(engine: &Engine, id: ActorId) -> Vec<String> {
        let Some(seat) = engine.table().seat(id) else {
            panic!("the actor is seated");
        };
        let lines = engine.log(Seated { id, seat }).filter_map(|entry| match entry {
            aethereal_actor::Entry::Line { text, .. } => Some(text.clone()),
            _ => None,
        });

        lines.collect()
    }

    #[test]
    fn a_child_of_a_guest_s_blob_that_subscribes_to_a_stage_its_engine_does_not_run_is_refused() {
        // Catches a guest's host answering its call out for whether a stage is run with anything but its engine's
        // cycle, and a native turn's spawn from a blob that checks no stage: a child would be made on an engine that
        // never sends it its stage, or refused on one that does.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let said = |setup: Setup<()>| {
            let (mut engine, code) = with_echo(setup);
            let Ok(metronome) = engine.add_root_from::<echo::Metronome>(&code, &(), &echo::EchoConfig) else {
                panic!("the metronome begins");
            };
            let Ok(breeder) = engine.add_root::<Breeder>(&(), Breeder { code }) else {
                panic!("the breeder begins");
            };

            [lines(&engine, metronome), lines(&engine, breeder)]
        };
        let ticking = Engine::new(parts, 1024, 0, 1 << 20, 0).with_cycle::<(Tick,)>();

        assert_eq!(said(ticking), [["ticker made"], ["ticker made"]]);
        let refused = [["ticker refused for tick"], ["ticker refused for tick"]];
        assert_eq!(said(Engine::new(parts, 1024, 0, 1 << 20, 0)), refused);
    }

    /// What a boss tells the foreman guest, as it begins.
    #[derive(Plain)]
    #[kind(name = "aether.test.order")]
    struct Order {
        what: u64,
    }

    /// A native actor that tells the foreman guest, as it begins, to run a chore of five, to call off its jobs, or to
    /// close: as its order is nought, one, or anything else.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.boss")]
    struct Boss;

    impl ChildOf for Boss {
        type Parent = Top;
    }

    impl Actor for Boss {
        type Config = Order;
        type Key = u64;
        type Handles = (Pong,);
        type BeginSends =
            (ToAny<(echo::Go,), echo::Go>, ToAny<(echo::CallOff,), echo::CallOff>, ToAny<(echo::Shut,), echo::Shut>);
        type EndSends = ();

        fn begin(order: Order, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let foreman = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.foreman"), &()));
            let Some(found) = turn.resolve(&foreman) else {
                return Err(Fault::new(&Doom)?);
            };
            match order.what {
                0 => {
                    if let Some(foreman) = turn.cast::<(echo::Go,)>(found) {
                        turn.to(foreman).send(&echo::Go { n: 5 })?;
                    }
                }
                1 => {
                    if let Some(foreman) = turn.cast::<(echo::CallOff,)>(found) {
                        turn.to(foreman).send(&echo::CallOff)?;
                    }
                }
                _ => {
                    if let Some(foreman) = turn.cast::<(echo::Shut,)>(found) {
                        turn.to(foreman).send(&echo::Shut)?;
                    }
                }
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Boss {
        type Sends = ();

        fn handle(&mut self, _: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    /// A host's runner of the foreman's jobs, which runs them as [`Inline`] does, or holds each until it is let go of.
    /// A test reads what the engine asked of it from the engine's own record, [`stopped`].
    struct Holding {
        inline: Inline,
        holds: bool,
        /// The jobs it holds. A runner is handed jobs through `&self` from wherever the engine runs, as [`Jobs`] is
        /// `Send` and `Sync`, so what it keeps between being handed a job and letting it go is behind a lock: no mail
        /// orders it.
        held: Mutex<Vec<(JobId, JobType, Taken)>>,
    }

    impl Holding {
        /// Runs the foreman's jobs, holding each until it is let go of when `holds`.
        fn new(holds: bool) -> Arc<Self> {
            let inline = Inline::new(Arc::new(Services::none()));

            Arc::new(Self { inline, holds, held: Mutex::default() })
        }

        /// Runs each job it holds.
        fn let_go(&self) {
            let held = self.held.lock().map(|mut held| mem::take(&mut *held)).unwrap_or_default();
            for (id, of, job) in held {
                self.inline.start(id, of, job);
            }
        }
    }

    impl Jobs for Holding {
        fn start(&self, id: JobId, of: JobType, job: Taken) {
            if !self.holds {
                self.inline.start(id, of, job);
                return;
            }
            if let Ok(mut held) = self.held.lock() {
                held.push((id, of, job));
            }
        }

        fn stop(&self, _: JobId) {}

        fn most(&self) -> usize {
            usize::MAX
        }
    }

    /// The jobs `engine` has asked its host to stop since this was last asked, by their counts, as it records them.
    fn stopped(engine: &mut Engine) -> Vec<u64> {
        let stops = engine.take_jobbed().into_iter().filter_map(|jobbed| match jobbed {
            Jobbed::Stopped(id) => Some(id.mail().count),
            Jobbed::Started { .. } => None,
        });

        stops.collect()
    }

    /// An engine of `parts` parts and `hops` hops whose jobs `jobs` runs, letting its guests run the foreman's jobs, the
    /// foreman guest begun in it, and the bosses of each of `orders`, by their keys and orders, begun after it
    /// together, each settled before the next. It records its jobs from the start, so [`stopped`] reads every stop.
    fn foreman(parts: usize, hops: usize, jobs: Arc<Holding>, orders: &[&[(u64, u64)]]) -> (Engine, ActorId) {
        let foremans = vec![JobType::of::<echo::Chore>(), JobType::of::<echo::Measure>()];

        foreman_letting(foremans, parts, hops, jobs, orders)
    }

    /// As [`foreman`], letting its guests run the jobs of `types`.
    fn foreman_letting(
        types: Vec<JobType>,
        parts: usize,
        hops: usize,
        jobs: Arc<Holding>,
        orders: &[&[(u64, u64)]],
    ) -> (Engine, ActorId) {
        let Some(parts) = NonZeroUsize::new(parts) else {
            panic!("no engine has no parts");
        };
        let setup = Engine::new(parts, 1024, 0, 1 << 20, hops).with_jobs(jobs).with_guest_jobs(types);
        let (mut engine, code) = with_echo(setup);
        engine.record_jobs();
        let Ok(foreman) = engine.add_root_from::<echo::Foreman>(&code, &(), &echo::EchoConfig) else {
            panic!("the foreman begins");
        };
        for together in orders {
            for &(key, what) in *together {
                let Ok(_) = engine.add_root::<Boss>(&key, Order { what }) else {
                    panic!("the boss begins");
                };
            }
            settle(&mut engine);
        }

        (engine, foreman)
    }

    #[test]
    fn a_guest_runs_jobs_and_is_answered_with_what_it_kept_for_each() {
        // Catches a guest's job its host never reads, or reads and files with no context, so no answer runs a turn;
        // one filed at another count than its guest's id names, among the replies its turn sends either side of it, or
        // filed without taking a count, so the job after it is never due; a context handed back with another job's
        // answer; and a blob the job refers to not handed out with it, so it measures nothing. The foreman's `begin`
        // measures a blob of three bytes, and a turn of a later step runs chores of five and six between two replies.
        for (parts, hops) in [(1, 0), (1, 64), (4, 0), (4, 64)] {
            let (mut engine, foreman) = foreman(parts, hops, Holding::new(false), &[&[(1, 0)]]);
            let ran = lines(&engine, foreman);
            engine.bring();
            settle(&mut engine);

            let answered = ["ran 1", "ran 3 and 4", "0 answered 3", "5 answered 6", "6 answered 7"];
            assert_eq!(ran, answered[..2], "nothing is answered before a bring: {parts} parts, {hops} hops");
            assert_eq!(lines(&engine, foreman), answered, "{parts} parts, {hops} hops");
        }
    }

    #[test]
    fn a_guest_s_job_of_a_type_its_engine_does_not_let_guests_run_is_answered_that_no_runner_runs_it() {
        // Catches a guest's job run with no code to run it, or handed its runner to answer, and one of a type the
        // engine lets guests run answered so: a guest cannot hand its host code, so its host runs only the types the
        // program lists. The foreman's engine lets its guests run measures and no chores.
        let measures = vec![JobType::of::<echo::Measure>()];
        let (mut engine, foreman) = foreman_letting(measures, 1, 0, Holding::new(false), &[&[(1, 0)]]);
        engine.bring();
        settle(&mut engine);

        let answered = ["ran 1", "ran 3 and 4", "0 answered 3", "5 unanswered: Faulted", "6 unanswered: Faulted"];
        assert_eq!(lines(&engine, foreman), answered);
        let Some(seat) = engine.table().seat(foreman) else {
            panic!("the foreman is seated");
        };
        let faulted: Vec<KindId> = engine
            .log(Seated { id: foreman, seat })
            .filter_map(|entry| match entry {
                aethereal_actor::Entry::JobFaulted(fault) => Some(fault.kind()),
                _ => None,
            })
            .collect();
        let no_runner = KindId::of::<aethereal_substrate::NoRunner>();
        assert_eq!(faulted, [no_runner, no_runner]);
    }

    #[test]
    fn a_guest_s_cancelled_jobs_are_never_answered_and_its_host_is_asked_to_stop_them() {
        // Catches a guest's cancel its host never reads, so a job it called off is answered; and a job id a guest's
        // turn reckons from the count its own turn began at rather than its actor's, or a job its host files without
        // counting it, so its cancel names a job it did not run, or a later turn of the batch counts from a count its
        // host does not. The foreman calls off its `begin`'s job and those of two turns of one batch, held by their
        // host, in a turn after them.
        let holding = Holding::new(true);
        let (mut engine, foreman) = foreman(1, 0, Arc::clone(&holding), &[&[(1, 0), (2, 0)], &[(3, 1)]]);
        holding.let_go();
        engine.bring();
        settle(&mut engine);

        assert_eq!(lines(&engine, foreman), ["ran 1", "ran 3 and 4", "ran 7 and 8", "called off"]);
        assert_eq!(stopped(&mut engine), [1, 3, 4, 7, 8]);
    }

    #[test]
    fn a_guest_s_job_keeps_its_blob_until_its_actor_ends_and_is_answered_to_no_one() {
        // Catches a blob a guest's job refers to that its host pins nothing for, freed while the job is in flight once
        // the guest lets go of its own reference, and one its host keeps once the actor that ran the job has ended. The
        // foreman's `begin` measures a blob it keeps nowhere else, held by its host, and it closes before the job is let
        // go of.
        let holding = Holding::new(true);
        let (mut engine, _) = foreman(1, 0, Arc::clone(&holding), &[&[(1, 0)]]);
        let in_flight = engine.blobs_held().0;
        let Ok(_) = engine.add_root::<Boss>(&2, Order { what: 2 }) else {
            panic!("the boss begins");
        };
        settle(&mut engine);
        holding.let_go();
        engine.bring();
        settle(&mut engine);

        assert_eq!((in_flight, engine.blobs_held().0), (2, 0), "the code's blob and the job's, then neither");
    }

    #[test]
    fn a_job_a_native_child_of_a_guest_runs_as_its_host_begins_it_is_answered() {
        // Catches the jobs of a child of other code that its host begins apart from the guest's call dropped as what
        // its `begin` asked for is taken into the turn that spawned it, while the context it kept is filed: the child
        // would wait for good on an answer that never comes.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let setup = Engine::new(parts, 1024, 0, 1 << 20, 0).with_links(Links::default().with::<echo::Apprentice>());
        let (mut engine, code) = with_echo(setup.with_jobs(Holding::new(false)));
        let Ok(_) = engine.add_root_from::<echo::Overseer>(&code, &(), &echo::EchoConfig) else {
            panic!("the overseer begins");
        };
        engine.bring();
        settle(&mut engine);

        let overseer = PathSegment::new(KindName::new("aether.guest.overseer"), &());
        let apprentice = PathSegment::new(KindName::new("aether.guest.apprentice"), &());
        let Ok(path) = ActorPath::from_root(&overseer).child(&apprentice) else {
            panic!("the path is not too deep");
        };
        let Some(apprentice) = engine.table().resolve(&path) else {
            panic!("the apprentice began");
        };
        assert_eq!(lines(&engine, apprentice), ["7 answered 8"]);
    }

    #[test]
    fn a_guest_answers_from_its_instance_after_another_in_it_has_ended() {
        // Catches two actors of one blob in one part each loading an instance of their own, and an instance dropped
        // when an actor in it ends while another still runs in it: the echo, asked for its turns from an instance that
        // does not hold it, would fail.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(echo) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("the echo begins");
        };
        let Ok(_) = engine.add_root_from::<echo::Mirror>(&code, &(), &echo::EchoConfig) else {
            panic!("the mirror begins");
        };
        let Ok(volleyer) = engine.add_root::<Volleyer>(&(), Volley { pings: 3 }) else {
            panic!("the volleyer begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }

        let failed = engine.log(echo).any(|entry| matches!(entry, aethereal_actor::Entry::Failed(_)));
        assert!(!failed, "the echo ran on in its instance");
        assert_eq!(state::<Volleyer>(&engine, volleyer).pongs, 4);
    }

    /// The word to crash, as the echo guest declares it.
    #[derive(Plain)]
    #[kind(name = "aether.guest.crash")]
    struct Crash;

    /// A native actor that tells the mirror guest to crash as it begins.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.crasher")]
    struct Crasher;

    impl ChildOf for Crasher {
        type Parent = Top;
    }

    impl Actor for Crasher {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Crash,), Crash>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let mirror = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.mirror"), &()));
            if let Some(mirror) = turn.resolve(&mirror).and_then(|found| turn.cast(found)) {
                turn.to(mirror).send(&Crash)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Crasher {
        type Sends = ();

        fn handle(&mut self, _: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    #[test]
    fn a_trap_ends_every_actor_in_its_instance() {
        // Catches a trap that ends only the actor whose turn trapped, leaving an actor that lives in the same memory
        // looking live, and the ending a host reports for it dropped with the rest of the failed turn.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(echo) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("the echo begins");
        };
        let Ok(mirror) = engine.add_root_from::<echo::Mirror>(&code, &(), &echo::EchoConfig) else {
            panic!("the mirror begins");
        };
        let Ok(_) = engine.add_root::<Crasher>(&(), Volley { pings: 0 }) else {
            panic!("the crasher begins");
        };
        let (Some(echo_seat), Some(mirror_seat)) = (engine.table().seat(echo), engine.table().seat(mirror)) else {
            panic!("the echo and the mirror are seated");
        };
        let echo = Seated { id: echo, seat: echo_seat };
        let mirror = Seated { id: mirror, seat: mirror_seat };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }

        let trapped = |actor| {
            let is_trap = |entry: &aethereal_actor::Entry| match entry {
                aethereal_actor::Entry::Failed(fault) => fault.kind() == KindId::of::<Trapped>(),
                _ => false,
            };
            engine.log(actor).any(is_trap)
        };
        assert!(trapped(mirror), "the mirror's turn trapped");
        assert!(trapped(echo), "the echo, in the same instance, ended with it");
        assert!(!engine.table().is_live(echo.id));
    }

    #[test]
    fn a_trap_in_a_begin_ends_every_actor_in_its_instance() {
        // Catches a `begin` that traps leaving the actors already in its instance looking live, whether its host
        // reports no ending for them or the engine drops what it reported with the rest of the failed `begin`.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(echo) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("the echo begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };

        let doomed = engine.add_root_from::<echo::Doomed>(&code, &(), &echo::EchoConfig);
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }

        let is_trap = |entry: &aethereal_actor::Entry| match entry {
            aethereal_actor::Entry::Failed(fault) => fault.kind() == KindId::of::<Trapped>(),
            _ => false,
        };
        let refused_trapped = matches!(&doomed, Err(Refused::Began(fault)) if fault.kind() == KindId::of::<Trapped>());
        assert!(refused_trapped, "the doomed root is refused with its trap");
        assert!(engine.log(echo).any(is_trap), "the echo, in the same instance, ended with it");
        assert!(!engine.table().is_live(echo.id));
    }

    #[test]
    fn a_child_its_guest_cannot_take_out_to_its_part_traps_its_instance() {
        // Catches a child whose state does not encode to be taken out to another part dropped in silence while its
        // spawn stays, so the table holds it live with no actor; and the trap that bug in the guest's code makes
        // ending the actors of another instance than the one it trapped, or none.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        // A root sits in the part with fewest actors, so the echo and the mirror are each in an instance of their own,
        // and the litter in one of theirs.
        let Ok(echo) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("the echo begins");
        };
        let Ok(mirror) = engine.add_root_from::<echo::Mirror>(&code, &(), &echo::EchoConfig) else {
            panic!("the mirror begins");
        };
        let (Some(echo_seat), Some(mirror_seat)) = (engine.table().seat(echo), engine.table().seat(mirror)) else {
            panic!("the echo and the mirror are seated");
        };
        let siblings = [Seated { id: echo, seat: echo_seat }, Seated { id: mirror, seat: mirror_seat }];

        // Past the one child its part has room for, the litter's homebodies are seated in the other part.
        let children = 4;
        let began = engine.add_root_from::<echo::Litter>(&code, &(), &echo::LitterConfig { children });
        settle(&mut engine);

        let refused_trapped = matches!(&began, Err(Refused::Began(fault)) if fault.kind() == KindId::of::<Trapped>());
        assert!(refused_trapped, "the litter is refused with its trap");

        let is_trap = |entry: &aethereal_actor::Entry| match entry {
            aethereal_actor::Entry::Failed(fault) => fault.kind() == KindId::of::<Trapped>(),
            _ => false,
        };
        // Whether each sibling ended with the trap in its log, and whether it is live.
        let mut fates: Vec<(bool, bool)> = siblings
            .iter()
            .map(|sibling| (engine.log(*sibling).any(is_trap), engine.table().is_live(sibling.id)))
            .collect();
        fates.sort_unstable();
        assert_eq!(
            fates,
            [(false, true), (true, false)],
            "the actor in the litter's instance ended with the trap in its log, and the other lives on"
        );

        let litter = ActorPath::from_root(&PathSegment::new(echo::Litter::NAME, &()));
        for key in 0..children {
            let Ok(path) = litter.child(&PathSegment::new(echo::Homebody::NAME, &key)) else {
                panic!("a homebody's path is not too deep");
            };
            let seated = engine.table().resolve(&path).and_then(|id| engine.table().seat(id));
            assert_eq!(seated, None, "{path} is not left in the table");
        }
    }

    /// The word to a brood to spawn one more child, as the echo guest declares it.
    #[derive(Plain)]
    #[kind(name = "aether.guest.grow")]
    struct Grow;

    /// What an asker pings.
    #[derive(Plain)]
    #[kind(name = "aether.test.asks")]
    struct Asks {
        paths: Vec<ActorPath>,
    }

    /// A native actor of these tests that keeps a number for each answer, ping, or light it is handed, in its own
    /// state: what a test reads it saw through.
    trait Tallies: 'static {
        /// What it has kept, in the order it kept it.
        fn tally(&self) -> &[u64];
    }

    /// What the live native actor `id` stands for, a `T`, has kept.
    fn kept<T: Tallies>(engine: &Engine, id: ActorId) -> &[u64] {
        state::<T>(engine, id).tally()
    }

    /// `kept`, in order of size: for what several actors kept, or one kept in no order a test reasons about.
    fn sorted<'kept>(kept: impl IntoIterator<Item = &'kept [u64]>) -> Vec<u64> {
        let mut sorted: Vec<u64> = kept.into_iter().flatten().copied().collect();
        sorted.sort_unstable();

        sorted
    }

    /// A native actor that pings each actor at the paths it is given with a ping of zero, and keeps what each answers.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.asker")]
    struct Asker {
        answers: Vec<u64>,
    }

    impl Tallies for Asker {
        fn tally(&self) -> &[u64] {
            &self.answers
        }
    }

    impl ChildOf for Asker {
        type Parent = Top;
    }

    impl Actor for Asker {
        type Config = Asks;
        type Key = u64;
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(asks: Asks, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            for path in &asks.paths {
                if let Some(asked) = turn.resolve(path).and_then(|found| turn.cast(found)) {
                    turn.to(asked).send(&Ping { n: 0 })?;
                }
            }

            Ok(Self { answers: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Asker {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.answers.push(pong.n);

            Ok(())
        }
    }

    /// A native actor that tells the brood to grow, as it begins.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.grower")]
    struct Grower;

    impl ChildOf for Grower {
        type Parent = Top;
    }

    impl Actor for Grower {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(Grow,), Grow>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            if let Some(brood) = turn.resolve(&brood_path()).and_then(|found| turn.cast(found)) {
                turn.to(brood).send(&Grow)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    /// The path of the brood root.
    fn brood_path() -> ActorPath {
        ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.brood"), &()))
    }

    /// The path of the adder keyed `key` beneath `above`.
    fn adder(above: &ActorPath, key: u64) -> ActorPath {
        let Ok(path) = above.child(&PathSegment::new(KindName::new("aether.guest.adder"), &key)) else {
            panic!("an adder's path is not too deep");
        };

        path
    }

    /// An engine of `parts` parts with a brood of `children` adders begun in it.
    fn brood(parts: usize, children: u64) -> Engine {
        let Some(parts) = NonZeroUsize::new(parts) else {
            panic!("a brood's engine has parts");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(_) = engine.add_root_from::<echo::Brood>(&code, &(), &echo::BroodConfig { children }) else {
            panic!("the brood begins");
        };

        engine
    }

    /// Steps `engine` until it is idle.
    fn settle(engine: &mut Engine) {
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }
    }

    #[test]
    fn a_guest_spawns_children_that_answer_from_their_own_state() {
        // Catches a spawn in a guest's `begin`, one beneath a child in the child's `begin`, or one in a turn, that
        // the engine never stands up or stands at the wrong path; and a child whose state is not its own.
        let mut engine = brood(1, 3);
        let Ok(_) = engine.add_root::<Grower>(&(), Volley { pings: 0 }) else {
            panic!("the grower begins");
        };
        settle(&mut engine);

        let first = adder(&brood_path(), 0);
        let paths =
            vec![adder(&first, 0), first, adder(&brood_path(), 1), adder(&brood_path(), 2), adder(&brood_path(), 3)];
        let Ok(asker) = engine.add_root::<Asker>(&0, Asks { paths }) else {
            panic!("the asker begins");
        };
        settle(&mut engine);

        assert_eq!(sorted([kept::<Asker>(&engine, asker)]), [1, 11, 21, 31, 1001]);
    }

    #[test]
    fn a_child_seated_in_another_part_is_taken_into_that_parts_instance() {
        // Catches a child seated in another part than its parent's that runs in no instance, in its parent's, or with
        // state that did not survive being taken out and in.
        let parts = 2;
        let mut engine = brood(parts, 8);
        settle(&mut engine);

        let part_of = |path: &ActorPath| {
            let seat = engine.table().resolve(path).and_then(|id| engine.table().seat(id));
            let Some(seat) = seat else {
                panic!("{path} is seated");
            };
            seat.index() % parts
        };
        let brood_part = part_of(&brood_path());
        let elsewhere = (0..8).filter(|key| part_of(&adder(&brood_path(), *key)) != brood_part).count();
        assert!(elsewhere > 0, "some adder is seated in another part than the brood");

        let first = adder(&brood_path(), 0);
        let mut paths: Vec<ActorPath> = (0..8).map(|key| adder(&brood_path(), key)).collect();
        paths.push(adder(&first, 0));
        let Ok(asker) = engine.add_root::<Asker>(&1, Asks { paths }) else {
            panic!("the asker begins");
        };
        settle(&mut engine);

        assert_eq!(sorted([kept::<Asker>(&engine, asker)]), [1, 11, 21, 31, 41, 51, 61, 71, 1001]);
    }

    /// What a keeper is made from: its key.
    #[derive(Plain)]
    #[kind(name = "aether.test.keeps")]
    struct Keeps {
        key: u64,
    }

    /// A native actor that keeps each ping it is sent, with a hundred for each of its key.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.keeper")]
    struct Keeper {
        key: u64,
        pings: Vec<u64>,
    }

    impl Tallies for Keeper {
        fn tally(&self) -> &[u64] {
            &self.pings
        }
    }

    impl ChildOf for Keeper {
        type Parent = Anywhere;
    }

    impl Actor for Keeper {
        type Config = Keeps;
        type Key = u64;
        type Handles = (Ping,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(keeps: Keeps, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self { key: keeps.key, pings: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Ping> for Keeper {
        type Sends = ();

        fn handle(&mut self, ping: Ping, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.pings.push(100 * self.key + ping.n);

            Ok(())
        }
    }

    #[test]
    fn a_guest_resolves_a_path_to_the_actor_it_names() {
        // Catches a guest's resolve that finds nothing, finds an actor that is not live, or hands back an id or seat
        // its mail does not reach the actor by.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let mut keepers = Vec::new();
        for key in [0, 1] {
            let Ok(keeper) = engine.add_root::<Keeper>(&key, Keeps { key }) else {
                panic!("a keeper begins");
            };
            keepers.push(keeper);
        }
        let keeper = |key: u64| ActorPath::from_root(&PathSegment::new(Keeper::NAME, &key));
        let seek = echo::Seek { paths: vec![keeper(0), keeper(9), keeper(1)] };
        let Ok(_) = engine.add_root_from::<echo::Seeker>(&code, &(), &seek) else {
            panic!("the seeker begins");
        };
        settle(&mut engine);

        assert_eq!(sorted(keepers.iter().map(|keeper| kept::<Keeper>(&engine, *keeper))), [0, 102]);
    }

    #[test]
    fn a_guest_asks_the_actor_at_a_typed_path_through_its_host() {
        // Catches a guest's ask at a path that its own table, which holds no other actor's type, never finds, and one
        // at a place that holds no actor that its host never answers, rather than answering it `Ended` (ADR-0022).
        let mut rung = Vec::new();
        for has_bell in [true, false] {
            let (mut engine, code) = with_echo(Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0));
            let Ok(keeper) = engine.add_root::<Keeper>(&0, Keeps { key: 0 }) else {
                panic!("the keeper begins");
            };
            if has_bell {
                let Ok(_) = engine.add_root_from::<echo::Bell>(&code, &0, &echo::EchoConfig) else {
                    panic!("the bell begins");
                };
            }
            let path = ActorPath::from_root(&PathSegment::new(Keeper::NAME, &0u64));
            let Ok(_) = engine.add_root_from::<echo::Ringer>(&code, &(), &echo::Ring { keeper: path }) else {
                panic!("the ringer begins");
            };
            settle(&mut engine);
            rung.extend_from_slice(kept::<Keeper>(&engine, keeper));
        }

        let rung = sorted([rung.as_slice()]);
        assert_eq!(rung, [5, 90], "the bell's pong of one more than four, and the ask at no bell unanswered");
    }

    #[test]
    fn a_guest_root_that_reaches_a_typed_path_of_singletons_is_refused_while_no_actor_of_its_type_is_live_there() {
        // Catches a guest's section that loses the kind of a path it reaches, so the place reads as empty though its
        // actor is live, and a guest root made though the place it reaches is empty (ADR-0022).
        let (mut engine, code) = with_echo(Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0));
        let refused = engine.add_root_from::<echo::Sexton>(&code, &(), &echo::EchoConfig).map(|_| ());
        let Ok(_) = engine.add_root_from::<echo::Steeple>(&code, &(), &echo::EchoConfig) else {
            panic!("the steeple begins");
        };
        let made = engine.add_root_from::<echo::Sexton>(&code, &(), &echo::EchoConfig).map(|_| ());

        assert_eq!(refused, Err(Refused::Missing(echo::Steeple::NAME)));
        assert_eq!(made, Ok(()));
    }

    /// What a vestry is made from: the blob it spawns its sexton from.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.vestment")]
    struct Vestment {
        code: Bytes,
    }

    /// A native actor that spawns a sexton of the echo guest's blob as it begins, and keeps one when it was refused for
    /// the steeple it reaches, and zero when it was made.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.vestry")]
    struct Vestry {
        spawned: Vec<u64>,
    }

    impl Tallies for Vestry {
        fn tally(&self) -> &[u64] {
            &self.spawned
        }
    }

    impl ChildOf for Vestry {
        type Parent = Top;
    }

    impl Actor for Vestry {
        type Config = Vestment;
        type Key = u64;
        type Handles = ();
        type BeginSends = ();
        type EndSends = ();

        fn begin(vestment: Vestment, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let sexton: Result<ActorRef<echo::Sexton>, _> =
                turn.spawn_from(Origin::Blob(&vestment.code), &(), &echo::EchoConfig);
            let answer = match sexton {
                Ok(_) => 0,
                Err(Refused::Missing(name)) if name == echo::Steeple::NAME => 1,
                Err(_) => 2,
            };

            Ok(Self { spawned: vec![answer] })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    #[test]
    fn a_native_turn_that_spawns_from_a_guest_s_blob_a_child_that_reaches_an_empty_typed_path_is_refused() {
        // Catches a spawn from a blob that does not check the paths its child's section says it reaches (ADR-0022).
        let (mut engine, code) = with_echo(Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0));
        let Ok(first) = engine.add_root::<Vestry>(&0, Vestment { code: code.clone() }) else {
            panic!("the first vestry begins");
        };
        let Ok(_) = engine.add_root_from::<echo::Steeple>(&code, &(), &echo::EchoConfig) else {
            panic!("the steeple begins");
        };
        let Ok(second) = engine.add_root::<Vestry>(&1, Vestment { code }) else {
            panic!("the second vestry begins");
        };

        let spawned = (kept::<Vestry>(&engine, first), kept::<Vestry>(&engine, second));
        assert_eq!(spawned, (&[1][..], &[0][..]), "refused while the steeple is empty, and made once it is not");
    }

    #[test]
    fn a_guest_s_handler_of_a_stage_sends_what_it_declares() {
        // Catches a guest's stage, sent as mail of its kind, taken by its host for mail the guest does not list, so
        // what its handler of the stage declares is not found and its send is refused (ADR-0020).
        let (mut engine, code) = with_echo(Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0));
        let Ok(keeper_id) = engine.add_root::<Keeper>(&0, Keeps { key: 0 }) else {
            panic!("the keeper begins");
        };
        let keeper = ActorPath::from_root(&PathSegment::new(Keeper::NAME, &0u64));
        let Ok(_) = engine.add_root_from::<echo::Chimer>(&code, &(), &echo::Ring { keeper }) else {
            panic!("the chimer begins");
        };
        engine.start();
        settle(&mut engine);

        assert_eq!(kept::<Keeper>(&engine, keeper_id), [7]);
    }

    #[test]
    fn a_guest_finds_a_child_beneath_a_reference_it_holds_by_its_type_and_key() {
        // Catches a guest's `turn.child` that misses a child its turn stood up, which its host's table does not hold
        // yet, one its host's table holds, or finds one beneath an actor that has none, so what its host answers does
        // not cross intact (ADR-0022).
        let (mut engine, code) = with_echo(Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0));
        let Ok(keeper_id) = engine.add_root::<Keeper>(&0, Keeps { key: 0 }) else {
            panic!("the keeper begins");
        };
        let keeper = ActorPath::from_root(&PathSegment::new(Keeper::NAME, &0u64));
        let Ok(_) = engine.add_root_from::<echo::Usher>(&code, &(), &echo::Ring { keeper }) else {
            panic!("the usher begins");
        };
        engine.start();
        settle(&mut engine);

        assert_eq!(kept::<Keeper>(&engine, keeper_id), [110]);
    }

    #[test]
    fn a_guest_casts_a_reference_to_a_protocol_by_what_its_host_reads_of_the_type() {
        // Catches a guest's cast that answers by its own table, which holds no other code's types, one its host
        // answers for a type that lists only some of the protocol's kinds, and ids that do not cross intact.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let mut keepers = Vec::new();
        for key in [0, 1] {
            let Ok(keeper) = engine.add_root::<Keeper>(&key, Keeps { key }) else {
                panic!("a keeper begins");
            };
            keepers.push(keeper);
        }
        let keeper = |key: u64| ActorPath::from_root(&PathSegment::new(Keeper::NAME, &key));
        let seek = echo::Seek { paths: vec![keeper(0), keeper(1)] };
        let Ok(_) = engine.add_root_from::<echo::Caster>(&code, &(), &seek) else {
            panic!("the caster begins");
        };
        settle(&mut engine);

        assert_eq!(sorted(keepers.iter().map(|keeper| kept::<Keeper>(&engine, *keeper))), [0, 101]);
    }

    /// What a shiner is made from: nothing.
    #[derive(Plain)]
    #[kind(name = "aether.test.shining")]
    struct Shining;

    /// A native actor that takes each lamp it is handed and keeps its light: what reads that a lamp came through a
    /// guest whole, as only native code opens one.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.shiner")]
    struct Shiner {
        lights: Vec<u64>,
    }

    impl Tallies for Shiner {
        fn tally(&self) -> &[u64] {
            &self.lights
        }
    }

    impl ChildOf for Shiner {
        type Parent = Top;
    }

    impl Actor for Shiner {
        type Config = Shining;
        type Key = ();
        type Handles = (echo::Hand,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(_: Shining, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self { lights: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Hand> for Shiner {
        type Sends = ();

        fn handle(&mut self, hand: echo::Hand, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            for lamp in hand.lamps {
                let mut lamp = turn.take(lamp)?;
                let light = turn.open(&mut lamp)?.light;
                self.lights.push(light);
            }

            Ok(())
        }
    }

    /// What a lighter is made from: the actor it hands its lamps to, and the light of each.
    #[derive(Plain)]
    #[kind(name = "aether.test.lighting")]
    struct Lighting {
        to: ActorPath,
        lights: Vec<u64>,
    }

    /// A native actor that makes a lamp of each light it is made with as it begins, keeps it, and hands each in a hand
    /// of its own to the actor `to` names, so they arrive as turns of one batch.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.lighter")]
    struct Lighter;

    impl ChildOf for Lighter {
        type Parent = Top;
    }

    impl Actor for Lighter {
        type Config = Lighting;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(echo::Hand,), echo::Hand>,);
        type EndSends = ();

        fn begin(lighting: Lighting, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let Lighting { to, lights } = lighting;
            let to = turn.resolve(&to).and_then(|found| turn.cast::<(echo::Hand,)>(found));
            let Some(to) = to else {
                return Err(Fault::new(&Doom)?);
            };
            for light in lights {
                let lamp = turn.keep(echo::Lamp { light })?;
                let lamps = vec![turn.give(lamp)?];
                turn.to(to).send(&echo::Hand { lamps })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    /// An engine running the echo blob, keeping the bearer for it, with a shiner keeping its lights, and the shiner
    /// by its path and its id.
    fn lit() -> (Engine, Bytes, ActorPath, ActorId) {
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let engine = Engine::new(parts, 1024, 0, 1 << 20, 0).with_links(Links::default().with::<echo::Bearer>());
        let (mut engine, code) = with_echo(engine);
        let Ok(shiner) = engine.add_root::<Shiner>(&(), Shining) else {
            panic!("the shiner begins");
        };

        (engine, code, ActorPath::from_root(&PathSegment::new(Shiner::NAME, &())), shiner)
    }

    #[test]
    fn a_guest_takes_resources_in_mail_and_passes_them_on_to_native_code() {
        // Catches a guest's take, give, or send of a resource its host does not carry out, one carried with the wrong
        // piece of mail of a batch of several, and a resource a guest's mail does not carry to its receiver.
        let (mut engine, code, shiner, shiner_id) = lit();
        let porting = echo::Porting { to: shiner, lamps: Vec::new(), how: 0 };
        let Ok(_) = engine.add_root_from::<echo::Porter>(&code, &0, &porting) else {
            panic!("the porter begins");
        };
        let porter = ActorPath::from_root(&PathSegment::new(echo::Porter::NAME, &0));
        let Ok(_) = engine.add_root::<Lighter>(&(), Lighting { to: porter, lights: vec![3, 4] }) else {
            panic!("the lighter begins");
        };
        settle(&mut engine);

        assert_eq!(sorted([kept::<Shiner>(&engine, shiner_id)]), [3, 4]);
    }

    #[test]
    fn a_guest_takes_resources_from_its_config_and_hands_them_to_children_of_its_own_and_other_code() {
        // Catches a guest root that cannot take what the chassis handed in for it, a guest's child of its own code
        // that cannot take what its parent gave it, and a native child its host begins for a guest that is not handed
        // what its config holds, or does not keep what it took once it is seated.
        let (mut engine, code, shiner, shiner_id) = lit();
        for how in [0, 1, 2] {
            let lamps = vec![engine.hand(echo::Lamp { light: 10 + how })];
            let porting = echo::Porting { to: shiner.clone(), lamps, how };
            let began = engine.add_root_from::<echo::Porter>(&code, &(10 + how), &porting);
            let Ok(_) = began else {
                panic!("the porter begins: {began:?}");
            };
        }
        settle(&mut engine);

        assert_eq!(sorted([kept::<Shiner>(&engine, shiner_id)]), [10, 11, 12]);
    }

    /// The reason a mortal fails.
    #[derive(Plain)]
    #[kind(name = "aether.test.doom")]
    struct Doom;

    /// A native actor that ends on its first ping: the one keyed zero closes, and any other fails.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.mortal")]
    struct Mortal {
        key: u64,
    }

    impl ChildOf for Mortal {
        type Parent = Top;
    }

    impl Actor for Mortal {
        type Config = Volley;
        type Key = u64;
        type Handles = (Ping,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(volley: Volley, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self { key: volley.pings })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Ping> for Mortal {
        type Sends = ();

        fn handle(&mut self, _: Ping, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            if self.key > 0 {
                return Err(Fault::new(&Doom)?);
            }
            turn.close();

            Ok(())
        }
    }

    #[test]
    fn a_guest_is_handed_the_endings_of_the_actors_it_watches() {
        // Catches a guest's watch its host never hands the engine, an ending never handed into the guest or handed in
        // as mail, and one whose source or how it ended is lost on the way in.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(keeper) = engine.add_root::<Keeper>(&0, Keeps { key: 0 }) else {
            panic!("the keeper begins");
        };
        for key in [0, 1] {
            let Ok(_) = engine.add_root::<Mortal>(&key, Volley { pings: key }) else {
                panic!("a mortal begins");
            };
        }
        let mortal = |key: u64| ActorPath::from_root(&PathSegment::new(Mortal::NAME, &key));
        let tell = ActorPath::from_root(&PathSegment::new(Keeper::NAME, &0u64));
        let watching = echo::Watching { paths: vec![mortal(0), mortal(1)], tell };
        let Ok(_) = engine.add_root_from::<echo::Watcher>(&code, &(), &watching) else {
            panic!("the watcher begins");
        };
        settle(&mut engine);

        let Ok(_) = engine.add_root::<Asker>(&2, Asks { paths: vec![mortal(0), mortal(1)] }) else {
            panic!("the asker begins");
        };
        settle(&mut engine);

        assert_eq!(sorted([kept::<Keeper>(&engine, keeper)]), [1, 12]);
    }

    /// What a nest is made from: the blob its reporters are spawned from, and how many.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.nesting")]
    struct Nesting {
        code: Bytes,
        reporters: u64,
    }

    /// A word to a nest to ping each of its reporters.
    #[derive(Plain)]
    #[kind(name = "aether.test.poke")]
    struct Poke;

    /// A native actor that spawns reporters of the echo guest's blob as it begins, pings each when poked, and keeps
    /// what each tells it.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.nest")]
    struct Nest {
        reporters: Vec<ActorRef<echo::Reporter>>,
        told: Vec<u64>,
    }

    impl Tallies for Nest {
        fn tally(&self) -> &[u64] {
            &self.told
        }
    }

    impl ChildOf for Nest {
        type Parent = Top;
    }

    impl Actor for Nest {
        type Config = Nesting;
        type Key = ();
        type Handles = (Poke, Pong);
        type BeginSends = ();
        type EndSends = ();

        fn begin(nesting: Nesting, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let mut reporters = Vec::new();
            for key in 0..nesting.reporters {
                let config = echo::AddBy { by: 10 * key, deeper: 0 };
                let Ok(reporter) = turn.spawn_from(Origin::Blob(&nesting.code), &key, &config) else {
                    return Err(Fault::new(&Doom)?);
                };
                reporters.push(reporter);
            }
            let unmade: Result<ActorRef<Keeper>, _> =
                turn.spawn_from(Origin::Blob(&nesting.code), &0, &Keeps { key: 0 });
            let impostor: Result<ActorRef<Impostor>, _> =
                turn.spawn_from(Origin::Blob(&nesting.code), &9, &echo::AddBy { by: 0, deeper: 0 });
            let (unmade, impostor) = (unmade.err(), impostor.err());
            let both_refused = unmade == Some(Refused::NotMade) && impostor == Some(Refused::NotMade);
            if !both_refused {
                return Err(Fault::new(&Doom)?);
            }

            Ok(Self { reporters, told: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Poke> for Nest {
        type Sends = (ToType<echo::Reporter, echo::Ping>,);

        fn handle(&mut self, _: Poke, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            turn.to(&self.reporters).send(&echo::Ping { n: 1 })?;

            Ok(())
        }
    }

    impl Handles<Pong> for Nest {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.told.push(pong.n);

            Ok(())
        }
    }

    /// A native type named as the echo guest's reporter is, which declares other handlers: a blob that makes a reporter
    /// does not make this.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.guest.reporter")]
    struct Impostor;

    impl ChildOf for Impostor {
        type Parent = Nest;
    }

    impl Actor for Impostor {
        type Config = echo::AddBy;
        type Key = u64;
        type Handles = (Poke,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(_: echo::AddBy, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Poke> for Impostor {
        type Sends = ();

        fn handle(&mut self, _: Poke, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    /// A native actor that pokes the nest as it begins.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.poker")]
    struct Poker;

    impl ChildOf for Poker {
        type Parent = Top;
    }

    impl Actor for Poker {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(Poke,), Poke>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let nest = ActorPath::from_root(&PathSegment::new(Nest::NAME, &()));
            if let Some(nest) = turn.resolve(&nest).and_then(|found| turn.cast(found)) {
                turn.to(nest).send(&Poke)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    #[test]
    fn a_native_actor_spawns_guests_from_a_blob_that_tell_it_in_later_turns() {
        // Catches a spawn from a blob that never stands its child up, stands it where its parent cannot reach, or
        // begins it of the wrong type; a guest child that loses its parent once its `begin` is done; and a spawn of
        // a type the blob does not make that is not refused.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(nest) = engine.add_root::<Nest>(&(), Nesting { code, reporters: 4 }) else {
            panic!("the nest begins, with its reporters, and is refused the keeper");
        };
        settle(&mut engine);
        let Ok(_) = engine.add_root::<Poker>(&(), Volley { pings: 0 }) else {
            panic!("the poker begins");
        };
        settle(&mut engine);

        assert_eq!(sorted([kept::<Nest>(&engine, nest)]), [1, 11, 21, 31]);
    }

    /// `code`, as other bytes that compile to the same code: with a custom section of its own after it, so it is a blob
    /// of its own, compiled apart, whose actors run in instances of their own.
    fn salted(code: &[u8]) -> Vec<u8> {
        let name = b"aethereal.salt";
        let payload_len = u8::try_from(1 + name.len()).unwrap_or(u8::MAX);

        code.iter().copied().chain([0, payload_len, u8::try_from(name.len()).unwrap_or(u8::MAX)]).chain(*name).collect()
    }

    /// What a hen is made from: the blob its hatcher is spawned from, and the other blob the hatcher spawns from.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.henning")]
    struct Henning {
        home: Bytes,
        other: Bytes,
    }

    /// A native actor that spawns a hatcher of the echo guest's blob as it begins, and keeps what it tells it.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.hen")]
    struct Hen {
        told: Vec<u64>,
    }

    impl Tallies for Hen {
        fn tally(&self) -> &[u64] {
            &self.told
        }
    }

    impl ChildOf for Hen {
        type Parent = Top;
    }

    impl Actor for Hen {
        type Config = Henning;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(henning: Henning, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let hatch = echo::Hatch { other: henning.other, home: henning.home.clone() };
            let Ok(_) = turn.spawn_from::<echo::Hatcher>(Origin::Blob(&henning.home), &(), &hatch) else {
                return Err(Fault::new(&Doom)?);
            };

            Ok(Self { told: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Hen {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.told.push(pong.n);

            Ok(())
        }
    }

    /// A native actor that tells the hen's hatcher to grow twice, as it begins, so both words are one batch's.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.rouser")]
    struct Rouser;

    impl ChildOf for Rouser {
        type Parent = Top;
    }

    impl Actor for Rouser {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(Grow,), Grow>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let hen = ActorPath::from_root(&PathSegment::new(Hen::NAME, &()));
            let Ok(hatcher) = hen.child(&PathSegment::new(echo::Hatcher::NAME, &())) else {
                return Err(Fault::new(&Doom)?);
            };
            if let Some(hatcher) = turn.resolve(&hatcher).and_then(|found| turn.cast(found)) {
                turn.to(hatcher).send(&Grow)?.send(&Grow)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    #[test]
    fn a_guest_spawns_children_of_other_code_that_its_host_begins() {
        // Catches a guest's spawn of a native type or of another blob's that never stands its child up, loses what
        // the child's `begin` sent, or stands it where its parent cannot reach; a native type the program does not
        // keep, or keeps as other than the guest holds it, that is not refused; children of other code handed to the
        // wrong turn of a batch; and a chain of `begin`s that comes back to an instance waiting on a call out that is
        // not refused, or is refused once that call has returned.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let engine = Engine::new(parts, 1024, 0, 1 << 21, 0).with_links(Links::default().with::<echo::Chick>());
        let (mut engine, home) = with_echo(engine);
        let Ok(other) = engine.put_code(salted(ECHO)) else {
            panic!("the echo guest, salted, is fit to run");
        };
        let Ok(hen) = engine.add_root::<Hen>(&(), Henning { home, other }) else {
            panic!("the hen begins, with its hatcher");
        };
        settle(&mut engine);
        let Ok(_) = engine.add_root::<Rouser>(&(), Volley { pings: 0 }) else {
            panic!("the rouser begins");
        };
        settle(&mut engine);

        assert_eq!(sorted([kept::<Hen>(&engine, hen)]), [20, 102, 112, 306, 400, 505, 505, 605, 605]);
    }

    /// What a publishing root is made from: the engine's one publisher, and the kinds it publishes, each the head of
    /// the blob at the same place, in order.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.publishes")]
    struct Publishes {
        publisher: Given<Publisher>,
        kinds: Vec<KindName>,
        blobs: Vec<Bytes>,
    }

    /// What a publish or a spawn was answered, as an actor's state can hold it: [`Refused`] does not encode.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
    enum Granted {
        /// It was done.
        Done,
        /// The blob does not make the kind.
        NotMade,
        /// The kind has no head.
        NoHead,
        /// Anything else, which these tests never expect.
        Other,
    }

    impl<T> From<Result<T, Refused>> for Granted {
        fn from(answer: Result<T, Refused>) -> Self {
            match answer {
                Ok(_) => Self::Done,
                Err(Refused::NotMade) => Self::NotMade,
                Err(Refused::NoHead) => Self::NoHead,
                Err(_) => Self::Other,
            }
        }
    }

    /// A native root that takes the engine's one publisher from its config and publishes as it begins, as the root
    /// that holds a program's code does, keeping each publish it asked by its kind, with what it was answered
    /// (ADR-0054).
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.publishing")]
    struct Publishing {
        publisher: Handle<Publisher>,
        published: Vec<(KindName, Granted)>,
    }

    impl ChildOf for Publishing {
        type Parent = Top;
    }

    impl Actor for Publishing {
        type Config = Publishes;
        type Key = ();
        type Handles = ();
        type BeginSends = ();
        type EndSends = ();

        fn begin(config: Publishes, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let Publishes { publisher, kinds, blobs } = config;
            let held = turn.take(publisher)?;
            let mut published = Vec::new();
            for (kind, blob) in kinds.iter().zip(&blobs) {
                published.push((kind.clone(), Granted::from(turn.publish(&held, kind, blob))));
            }

            Ok(Self { publisher: held, published })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    /// What an inheritor is made from: nothing.
    #[derive(Plain)]
    #[kind(name = "aether.test.inherit")]
    struct Inherit;

    /// A native root that spawns an echo from the head of the echo's kind as it begins, keeping what it was answered.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.inheritor")]
    struct Inheritor {
        inherited: Granted,
    }

    impl ChildOf for Inheritor {
        type Parent = Top;
    }

    impl Actor for Inheritor {
        type Config = Inherit;
        type Key = u64;
        type Handles = ();
        type BeginSends = ();
        type EndSends = ();

        fn begin(_: Inherit, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let spawned = turn.spawn_from::<echo::Echo>(Origin::Head, &(), &echo::EchoConfig);

            Ok(Self { inherited: Granted::from(spawned) })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    #[test]
    fn a_spawn_from_a_kind_s_head_takes_the_blob_published_last_for_it() {
        // Catches a spawn from the head that does not read the heads view, or is not refused while the kind has none;
        // a publish of a blob that does not make its kind that is not refused, or that moves the head all the same; and
        // the publishes of one step folded out of the order asked, or numbered otherwise than one after another.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let (mut engine, first) = with_echo(Engine::new(parts, 1024, 0, 1 << 21, 0));
        let Ok(second) = engine.put_code(ECHO.to_vec()) else {
            panic!("the echo guest is fit to run again, as a blob of its own");
        };
        let Ok(first_heir) = engine.add_root::<Inheritor>(&1, Inherit) else {
            panic!("the first inheritor begins");
        };
        let Ok(publisher) = engine.publisher() else {
            panic!("the engine hands out its publisher once");
        };
        let unmade = KindName::new("aether.test.unmade");
        let kinds = vec![echo::Echo::NAME, unmade.clone(), echo::Echo::NAME];
        let blobs = vec![first.clone(), first, second.clone()];
        let Ok(publishing) = engine.add_root::<Publishing>(&(), Publishes { publisher, kinds, blobs }) else {
            panic!("the publishing root begins");
        };
        let Ok(second_heir) = engine.add_root::<Inheritor>(&2, Inherit) else {
            panic!("the second inheritor begins");
        };
        settle(&mut engine);

        let expected =
            [(echo::Echo::NAME, Granted::Done), (unmade, Granted::NotMade), (echo::Echo::NAME, Granted::Done)];
        assert_eq!(state::<Publishing>(&engine, publishing).published, expected);
        let head = engine.head(&echo::Echo::NAME).map(|head| (head.blob().id(), head.number()));
        assert_eq!(head, Some((second.id(), 2)));
        let inherited = [first_heir, second_heir].map(|heir| state::<Inheritor>(&engine, heir).inherited);
        assert_eq!(inherited, [Granted::NoHead, Granted::Done]);
        let inheritor = ActorPath::from_root(&PathSegment::new(Inheritor::NAME, &2_u64));
        let Ok(echo) = inheritor.child(&PathSegment::new(echo::Echo::NAME, &())) else {
            panic!("an echo stands beneath an inheritor");
        };
        assert!(engine.table().resolve(&echo).is_some());
    }

    #[test]
    fn a_guest_spawns_a_child_from_its_kind_s_head_through_its_host() {
        // Catches a guest's read of a head its host does not answer, answers without offering the blob's pin, so the
        // spawn from it is refused, or answers while the kind has none.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let (mut engine, home) = with_echo(Engine::new(parts, 1024, 0, 1 << 21, 0));
        let Ok(reporters) = engine.put_code(salted(ECHO)) else {
            panic!("the echo guest, salted, is fit to run");
        };
        let unpublished = engine.add_root_from::<echo::Heir>(&home, &(), &echo::EchoConfig);
        let Err(Refused::Began(fault)) = unpublished else {
            panic!("the heir is refused as it begins, with no head to spawn from");
        };
        let why = fault.read::<echo::Unhatched>(&mut Budget::new(64));
        assert_eq!(why.ok().flatten().map(|unhatched| unhatched.why), Some(9), "refused for having no head");
        let Ok(publisher) = engine.publisher() else {
            panic!("the engine hands out its publisher once");
        };
        let config = Publishes { publisher, kinds: vec![echo::Reporter::NAME], blobs: vec![reporters] };
        let Ok(_) = engine.add_root::<Publishing>(&(), config) else {
            panic!("the publishing root begins");
        };

        let inherited = engine.add_root_from::<echo::Heir>(&home, &(), &echo::EchoConfig);

        assert!(inherited.is_ok(), "the heir spawns its reporter from the head");
        let heir = ActorPath::from_root(&PathSegment::new(echo::Heir::NAME, &()));
        let Ok(reporter) = heir.child(&PathSegment::new(echo::Reporter::NAME, &5_u64)) else {
            panic!("a reporter stands beneath the heir");
        };
        assert!(engine.table().resolve(&reporter).is_some());
    }

    /// A native actor that keeps each pong it is sent.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.ledger")]
    struct Ledger {
        pongs: Vec<u64>,
    }

    impl Tallies for Ledger {
        fn tally(&self) -> &[u64] {
            &self.pongs
        }
    }

    impl ChildOf for Ledger {
        type Parent = Top;
    }

    impl Actor for Ledger {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(_: Volley, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self { pongs: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Ledger {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.pongs.push(pong.n);

            Ok(())
        }
    }

    /// The path of the librarian root.
    fn librarian() -> ActorPath {
        ActorPath::from_root(&PathSegment::new(echo::Librarian::NAME, &()))
    }

    /// A native actor that pings the librarian for a book of five as it begins, keeps how long the book it is sent is
    /// and what its first byte is, read from the store, and sends the book back; and keeps the pong it is answered
    /// with too.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.borrower")]
    struct Borrower {
        read: Vec<u64>,
    }

    impl Tallies for Borrower {
        fn tally(&self) -> &[u64] {
            &self.read
        }
    }

    impl ChildOf for Borrower {
        type Parent = Top;
    }

    impl Actor for Borrower {
        type Config = Volley;
        type Key = ();
        type Handles = (echo::Book, Pong);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            if let Some(librarian) = turn.resolve(&librarian()).and_then(|found| turn.cast(found)) {
                turn.to(librarian).send(&Ping { n: 5 })?;
            }

            Ok(Self { read: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Book> for Borrower {
        type Sends = (ToSender<echo::Book>,);

        fn handle(&mut self, book: echo::Book, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            let bytes = turn.read(&book.blob);
            let read = [bytes.len() as u64, bytes.first().copied().map_or(0, u64::from)];
            self.read.extend(read);
            turn.reply(&book)?;

            Ok(())
        }
    }

    impl Handles<Pong> for Borrower {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.read.push(pong.n);

            Ok(())
        }
    }

    /// A native actor that tells the librarian to shut, as it begins.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.shutter")]
    struct Shutter;

    impl ChildOf for Shutter {
        type Parent = Top;
    }

    impl Actor for Shutter {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(echo::Shut,), echo::Shut>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            if let Some(librarian) = turn.resolve(&librarian()).and_then(|found| turn.cast(found)) {
                turn.to(librarian).send(&echo::Shut)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    #[test]
    fn a_guest_holds_the_blobs_it_refers_to_and_no_other() {
        // Catches a guest's reference that does not keep its blob, one read out of its config or mail that its host
        // does not take, a blob it read or put that its host does not serve it, mail it sends whose reference does not
        // keep the blob until it is read, a reference kept past its actor's end, and a reference made from an id the
        // guest was never given that its host honours.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(ledger) = engine.add_root::<Ledger>(&(), Volley { pings: 0 }) else {
            panic!("the ledger begins");
        };
        let Ok(book) = engine.put_blob(b"hello world".to_vec()) else {
            panic!("the book is put");
        };
        let tell = ActorPath::from_root(&PathSegment::new(Ledger::NAME, &()));
        let Ok(_) = engine.add_root_from::<echo::Librarian>(&code, &(), &echo::Shelf { book, tell }) else {
            panic!("the librarian begins, and reads its book");
        };
        settle(&mut engine);
        let kept_by_librarian = engine.blobs_held().0;

        let Ok(borrower) = engine.add_root::<Borrower>(&(), Volley { pings: 0 }) else {
            panic!("the borrower begins");
        };
        settle(&mut engine);

        // An echo keeps the blob's instance after the librarian ends, so what the librarian held goes as it ends, and
        // not with the instance.
        let Ok(_) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("an echo begins");
        };
        let Ok(_) = engine.add_root::<Shutter>(&(), Volley { pings: 0 }) else {
            panic!("the shutter begins");
        };
        settle(&mut engine);
        let after_closing = engine.blobs_held().0;

        let Ok(secret) = engine.put_blob(b"secret".to_vec()) else {
            panic!("the secret is put");
        };
        let forge = echo::Forge { engine: secret.id().engine(), bits: secret.id().bits() };
        let forged = engine.add_root_from::<echo::Forger>(&code, &(), &forge);

        let told = [kept::<Ledger>(&engine, ledger), kept::<Borrower>(&engine, borrower)];
        assert_eq!(sorted(told), [5, 7, 11, 105]);
        // The blob lent out goes once the mail that carried it is gone, by the time the librarian has closed.
        assert_eq!((kept_by_librarian, after_closing), (2, 1));
        assert!(matches!(forged, Err(Refused::Began(_))));
    }

    /// What a patron asks the librarian for: a book of `len` bytes, and whether to keep it.
    #[derive(Plain)]
    #[kind(name = "aether.test.patron_config")]
    struct Borrowing {
        len: u64,
        keeps: bool,
    }

    /// A native actor that pings the librarian for a book as it begins, and keeps the book it is sent or drops it, as
    /// its config says.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.patron")]
    struct Patron {
        keeps: bool,
        books: Vec<Bytes>,
    }

    impl ChildOf for Patron {
        type Parent = Top;
    }

    impl Actor for Patron {
        type Config = Borrowing;
        type Key = u64;
        type Handles = (echo::Book,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(borrowing: Borrowing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            if let Some(librarian) = turn.resolve(&librarian()).and_then(|found| turn.cast(found)) {
                turn.to(librarian).send(&Ping { n: borrowing.len })?;
            }

            Ok(Self { keeps: borrowing.keeps, books: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Book> for Patron {
        type Sends = ();

        fn handle(&mut self, book: echo::Book, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            if self.keeps {
                self.books.push(book.blob);
            }

            Ok(())
        }
    }

    #[test]
    fn a_guests_put_is_copied_into_a_buffer_its_actor_kept() {
        // Catches a host that copies what its guest puts into a new buffer each time, so a guest's buffers are never
        // reused, and one that hands the buffer of a guest's freed blob back to no actor (ADR-0042): the librarian's
        // book of six hundred is put in the buffer its book of a thousand was, which the store counts by its capacity.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(book) = engine.put_blob(b"hello world".to_vec()) else {
            panic!("the book is put");
        };
        let tell = ActorPath::from_root(&PathSegment::new(Patron::NAME, &0_u64));
        let Ok(_) = engine.add_root_from::<echo::Librarian>(&code, &(), &echo::Shelf { book, tell }) else {
            panic!("the librarian begins");
        };
        let Ok(_) = engine.add_root::<Patron>(&1, Borrowing { len: 1000, keeps: false }) else {
            panic!("the first patron begins");
        };
        settle(&mut engine);
        // The mail that carried the first book keeps it until that mail's arena is written again.
        let clock = Monotonic::new();
        for _ in 0..3 {
            engine.step(Time::ZERO, &clock);
        }
        let (blobs, bytes) = engine.blobs_held();

        let Ok(_) = engine.add_root::<Patron>(&2, Borrowing { len: 600, keeps: true }) else {
            panic!("the second patron begins");
        };
        settle(&mut engine);

        assert_eq!(engine.blobs_held(), (blobs + 1, bytes + 1000));
    }

    #[test]
    fn a_guest_that_forges_a_reference_ends_its_instance_for_forging_it() {
        // Catches a reference made from an id the guest was never given that its host honours, one its host reports as
        // a trap or another break than the one it is, and an actor in the same instance that ends for another reason
        // than the one its instance was dropped for.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(echo) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("an echo begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };
        let Ok(secret) = engine.put_blob(b"secret".to_vec()) else {
            panic!("the secret is put");
        };

        let forge = echo::Forge { engine: secret.id().engine(), bits: secret.id().bits() };
        let forged = engine.add_root_from::<echo::Hoarder>(&code, &(), &forge);
        settle(&mut engine);

        let is_forged = |fault: &Fault| fault.kind() == KindId::of::<Forged>();
        let refused_forged = matches!(&forged, Err(Refused::Began(fault)) if is_forged(fault));
        let ended_forged = |entry: &aethereal_actor::Entry| match entry {
            aethereal_actor::Entry::Failed(fault) => is_forged(fault),
            _ => false,
        };
        assert!(refused_forged, "the hoarder is refused for forging");
        assert!(engine.log(echo).any(ended_forged), "the echo, in the same instance, ended for the forging");
        assert!(!engine.table().is_live(echo.id));
    }

    /// A native actor that pings the spendthrift for a book of three, as it begins.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.dun")]
    struct Dun;

    impl ChildOf for Dun {
        type Parent = Top;
    }

    impl Actor for Dun {
        type Config = Volley;
        type Key = ();
        type Handles = (echo::Book,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let spendthrift = ActorPath::from_root(&PathSegment::new(echo::Spendthrift::NAME, &()));
            if let Some(spendthrift) = turn.resolve(&spendthrift).and_then(|found| turn.cast(found)) {
                turn.to(spendthrift).send(&Ping { n: 3 })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Book> for Dun {
        type Sends = ();

        fn handle(&mut self, _: echo::Book, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    #[test]
    fn a_child_of_a_guests_turn_its_host_fails_is_ended_in_the_guest() {
        // Catches a child the guest kept in its instance in a turn whose send its host refused, which the engine drops
        // and never ends: its state, and the references in it, would stay in the instance for as long as it lives.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        // An echo keeps the blob's instance after the spendthrift fails.
        let Ok(_) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("an echo begins");
        };
        let Ok(spendthrift) = engine.add_root_from::<echo::Spendthrift>(&code, &(), &echo::EchoConfig) else {
            panic!("the spendthrift begins");
        };
        let Some(seat) = engine.table().seat(spendthrift) else {
            panic!("the spendthrift is seated");
        };
        let spendthrift = Seated { id: spendthrift, seat };
        settle(&mut engine);
        let before = engine.blobs_held().0;

        let Ok(_) = engine.add_root::<Dun>(&(), Volley { pings: 0 }) else {
            panic!("the dun begins");
        };
        settle(&mut engine);
        let after = engine.blobs_held().0;

        let failed = engine.log(spendthrift).any(|entry| matches!(entry, aethereal_actor::Entry::Failed(_)));
        assert!(failed, "the spendthrift's false send failed it");
        assert_eq!(after, before, "the book its librarian kept is let go");
    }

    #[test]
    fn a_guest_reads_within_the_engines_limit() {
        // Catches a guest that reads its config, or its host what it hands in, within a limit of its own and not the
        // engine's.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut tell = ActorPath::from_root(&PathSegment::new(echo::Librarian::NAME, &()));
        while let Ok(deeper) = tell.child(&PathSegment::new(echo::Librarian::NAME, &())) {
            tell = deeper;
        }
        let mut began = Vec::new();
        for read_limit in [256, 1 << 20] {
            let (mut engine, code) = with_echo(Engine::new(parts, read_limit, 0, 1 << 20, 0));
            let Ok(book) = engine.put_blob(b"hello world".to_vec()) else {
                panic!("the book is put");
            };
            let shelf = echo::Shelf { book, tell: tell.clone() };
            began.push(engine.add_root_from::<echo::Librarian>(&code, &(), &shelf).is_ok());
        }

        assert_eq!(began, [false, true], "a shelf past the engine's limit is not read");
    }

    /// A native actor that keeps each pong it is told.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.mourner")]
    struct Mourner {
        pongs: Vec<u64>,
    }

    impl Tallies for Mourner {
        fn tally(&self) -> &[u64] {
            &self.pongs
        }
    }

    impl ChildOf for Mourner {
        type Parent = Top;
    }

    impl Actor for Mourner {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(_: Volley, _: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self { pongs: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Mourner {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
            self.pongs.push(pong.n);

            Ok(())
        }
    }

    /// A native actor that, as it begins, pings the first parting twice and tells it to shut, and pings the second
    /// once and then with a zero, which fails it.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.dismisser")]
    struct Dismisser;

    impl ChildOf for Dismisser {
        type Parent = Top;
    }

    impl Actor for Dismisser {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(Ping, echo::Shut), Ping>, ToAny<(Ping, echo::Shut), echo::Shut>);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let parting = |key: u64| ActorPath::from_root(&PathSegment::new(echo::Parting::NAME, &key));
            if let Some(first) = turn.resolve(&parting(1)).and_then(|found| turn.cast(found)) {
                turn.to(first).send(&Ping { n: 1 })?.send(&Ping { n: 1 })?.send(&echo::Shut)?;
            }
            if let Some(second) = turn.resolve(&parting(2)).and_then(|found| turn.cast(found)) {
                turn.to(second).send(&Ping { n: 1 })?.send(&Ping { n: 0 })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    #[test]
    fn a_guests_closing_turn_runs_on_its_state_and_sends_what_its_end_declares() {
        // Catches a guest actor that ends with no closing turn, one whose closing turn runs on another's state or is
        // not told why it runs, and a closing turn's send checked against what another of its handlers declares.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let Ok(mourner) = engine.add_root::<Mourner>(&(), Volley { pings: 0 }) else {
            panic!("the mourner begins");
        };
        let tell = ActorPath::from_root(&PathSegment::new(Mourner::NAME, &()));
        for key in [1, 2] {
            let farewell = echo::Farewell { tell: tell.clone() };
            let Ok(_) = engine.add_root_from::<echo::Parting>(&code, &key, &farewell) else {
                panic!("a parting begins");
            };
        }
        let Ok(_) = engine.add_root::<Dismisser>(&(), Volley { pings: 0 }) else {
            panic!("the dismisser begins");
        };
        settle(&mut engine);

        assert_eq!(sorted([kept::<Mourner>(&engine, mourner)]), [611, 620]);
    }

    #[test]
    fn a_guest_state_taken_into_another_parts_instance_keeps_the_blobs_it_refers_to() {
        // Catches a child taken out of one part's instance whose blob is let go while its state is out, one taken into
        // its own part's instance that is not served the blob its state refers to, and a reference still counted in
        // the instance it left, or counted twice in the one it joined, that keeps the blob once the child has ended.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        // An echo and the archive are seated in the two parts, each keeping its part's instance for as long as it
        // lives, so a reference left counted in either keeps the book.
        let Ok(_) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
            panic!("an echo begins");
        };
        let Ok(book) = engine.put_blob(b"hello world".to_vec()) else {
            panic!("the book is put");
        };
        let Ok(_) = engine.add_root_from::<echo::Archive>(&code, &(), &echo::Stacks { book, readers: 8 }) else {
            panic!("the archive begins, with its readers");
        };
        settle(&mut engine);

        let part_of = |path: &ActorPath| {
            let seat = engine.table().resolve(path).and_then(|id| engine.table().seat(id));
            let Some(seat) = seat else {
                panic!("{path} is seated");
            };
            seat.index() % parts.get()
        };
        let archive = ActorPath::from_root(&PathSegment::new(echo::Archive::NAME, &()));
        let echo = ActorPath::from_root(&PathSegment::new(echo::Echo::NAME, &()));
        let reader = |key: u64| {
            let Ok(path) = archive.child(&PathSegment::new(echo::Reader::NAME, &key)) else {
                panic!("a reader's path is not too deep");
            };
            path
        };
        let readers: Vec<ActorPath> = (0..8).map(reader).collect();
        let elsewhere = readers.iter().filter(|path| part_of(path) != part_of(&archive)).count();
        assert_ne!(part_of(&echo), part_of(&archive), "the echo and the archive keep both parts' instances");
        assert!(elsewhere > 0, "some reader is seated in another part than the archive");
        let held_while_reading = engine.blobs_held().0;

        let Ok(asker) = engine.add_root::<Asker>(&8, Asks { paths: readers }) else {
            panic!("the asker begins");
        };
        settle(&mut engine);

        assert_eq!(kept::<Asker>(&engine, asker), [11; 8]);
        // The code and the book while the readers live, and the code alone once they have closed.
        assert_eq!((held_while_reading, engine.blobs_held().0), (2, 1));
    }

    #[test]
    fn compiled_code_is_let_go_of_with_the_last_blob_of_its_bytes() {
        // Catches compiled code kept once the engine has let go of every blob of its bytes, which a runtime whose code
        // is put again each time it is changed would keep forever, and compiled code let go of while another blob of
        // the same bytes is held.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let runtime = Arc::new(Runtime::default());
        let mut engine =
            Engine::new(parts, 1024, 0, 1 << 21, 0).with_guests(Arc::clone(&runtime) as Arc<dyn Guests>).ready();
        let (Ok(first), Ok(second)) = (engine.put_code(ECHO.to_vec()), engine.put_code(ECHO.to_vec())) else {
            panic!("the echo guest is fit to run");
        };
        assert_ne!(first.id(), second.id(), "each put is a blob of its own");
        let compiled = || runtime.compiled.read().map_or(usize::MAX, |compiled| compiled.len());
        let clock = Monotonic::new();

        drop(first);
        engine.step(Time::ZERO, &clock);
        let with_one = compiled();
        drop(second);
        engine.step(Time::ZERO, &clock);

        assert_eq!((with_one, compiled()), (1, 0));
    }

    #[test]
    fn code_whose_blob_is_freed_makes_nothing() {
        // Catches code kept once the engine has let go of its blob, which would run actors from bytes no longer held.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let runtime = Arc::new(Runtime::default());
        let mut engine =
            Engine::new(parts, 1024, 0, 1 << 20, 0).with_guests(Arc::clone(&runtime) as Arc<dyn Guests>).ready();
        let Ok(code) = engine.put_code(ECHO.to_vec()) else {
            panic!("the echo guest is fit to run");
        };
        let id = code.id();
        let echo = Declared::of::<echo::Echo>();
        let was_made = runtime.made(id, echo).is_some();
        drop(code);
        engine.step(Time::ZERO, &Monotonic::new());

        assert_eq!((was_made, runtime.made(id, echo).is_some()), (true, false));
    }

    #[test]
    fn code_whose_section_misstates_a_state_is_refused_where_it_is_put() {
        // Catches a host that takes a section in without checking each type's state against its id, so a moving
        // actor's state would be read by a schema of another layout than its kind's (ADR-0006, ADR-0054).
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).with_guests(Arc::new(Runtime::default())).ready();
        let Some(declared) = declared_section(ECHO) else {
            panic!("the echo guest declares what it makes");
        };
        let Ok(mut made) = section::read(declared, &mut Budget::new(1 << 20)) else {
            panic!("the echo guest's section reads back");
        };
        let Some(first) = made.first_mut() else {
            panic!("the echo guest makes a type");
        };
        first.state = SchemaNodes::of(&u8::SHAPE);
        let misstated = first.name.clone();
        let Ok(section) = to_bytes(&made) else {
            panic!("a section encodes");
        };

        let refused = engine.put_code(redeclared(ECHO, &section)).err();

        assert_eq!(refused, Some(CodeRefused::Unfit(Unfit::Misstated(misstated))));
    }

    /// `code`, with a section `aethereal.declared` of `section` before its own, which its host reads in its place.
    fn redeclared(code: &[u8], section: &[u8]) -> Vec<u8> {
        let name = section::NAME.as_bytes();
        let payload: Vec<u8> =
            leb(name.len()).into_iter().chain(name.iter().copied()).chain(section.iter().copied()).collect();
        let (header, sections) = code.split_at(8);

        header
            .iter()
            .copied()
            .chain([0])
            .chain(leb(payload.len()))
            .chain(payload)
            .chain(sections.iter().copied())
            .collect()
    }

    /// `value` as a module writes a size: seven bits a byte, low first, the high bit set on every byte but the last.
    fn leb(value: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut left = value;
        loop {
            let low = u8::try_from(left & 0x7f).unwrap_or(0);
            left >>= 7;
            if left == 0 {
                bytes.push(low);

                return bytes;
            }
            bytes.push(low | 0x80);
        }
    }

    /// When a prober's ping is due: a millisecond after the start of the test's clock.
    const PROBE_DUE: Time = Time::from_nanos(1_000_000);

    /// What a prober is made from: the envoy guest's code.
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.prober_config")]
    struct ProberConfig {
        code: Bytes,
    }

    /// What became of a request, as a prober keeps it: the pong it was answered with, or why it was not, with which
    /// request it was.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
    enum Probed {
        Pong { at: u64, n: u64 },
        No { at: u64, why: Unanswered },
    }

    impl Probed {
        /// What the envoy told, kept.
        const fn of(heard: &echo::Heard) -> Self {
            match *heard {
                echo::Heard::Pong { at, n } => Self::Pong { at, n },
                echo::Heard::No { at, why } => Self::No { at, why },
            }
        }
    }

    /// A native actor that spawns the envoy guest from its code as it begins and asks it a ping of `7` within
    /// [`PROBE_DUE`], keeping what became of its ping and what the envoy told it of its own ask (ADR-0041).
    #[derive(aethereal_data::Bound)]
    #[kind(name = "aether.test.prober")]
    struct Prober {
        /// What became of its ping.
        answered: Vec<Probed>,
        /// What the envoy told it became of its own ask.
        told: Vec<Probed>,
    }

    impl ChildOf for Prober {
        type Parent = Top;
    }

    impl Actor for Prober {
        type Config = ProberConfig;
        type Key = ();
        type Handles = (Answer<echo::Pong, Which>, echo::Heard);
        type BeginSends = (aethereal_actor::Asks<Pings, echo::Ping, Which>,);
        type EndSends = ();

        fn begin(config: ProberConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
            let Ok(envoy) = turn.spawn_from::<echo::Envoy>(Origin::Blob(&config.code), &(), &echo::EchoConfig) else {
                return Err(Fault::new(&echo::Shut)?);
            };
            turn.to(envoy.protocol()).within(PROBE_DUE).ask(&echo::Ping { n: 7 }, &Which { at: 7 })?;

            Ok(Self { answered: Vec::new(), told: Vec::new() })
        }

        fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Answer<echo::Pong, Which>> for Prober {
        type Sends = ();

        fn handle(
            &mut self,
            (pong, which): (Answered<echo::Pong>, Which),
            _: &mut Turn<'_, Self, ()>,
        ) -> Result<(), Fault> {
            let probed = match pong {
                Answered::Yes(pong) => Probed::Pong { at: which.at, n: pong.n },
                Answered::No(why) => Probed::No { at: which.at, why },
            };
            self.answered.push(probed);

            Ok(())
        }
    }

    impl Handles<echo::Heard> for Prober {
        type Sends = ();

        fn handle(&mut self, heard: echo::Heard, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
            self.told.push(Probed::of(&heard));

            Ok(())
        }
    }

    /// What the prober `id` stands for in `engine` was answered and told.
    fn probed(engine: &Engine, id: ActorId) -> (Vec<Probed>, Vec<Probed>) {
        let Some(seat) = engine.table().seat(id) else {
            panic!("the prober is seated");
        };
        let Some(prober) = engine.state::<Prober>(Seated { id, seat }) else {
            panic!("the prober is live");
        };

        (prober.answered.clone(), prober.told.clone())
    }

    #[test]
    fn a_guests_ask_for_a_request_it_holds_inherits_the_requests_deadline() {
        // Catches a guest's ask that drops the deadline of the request its turn handles, its library or its host
        // filing it due never: the steward it asks would hold the envoy's request for good once the prober gave up.
        let Some(parts) = NonZeroUsize::new(4) else {
            panic!("four is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 64));
        let Ok(prober) = engine.add_root::<Prober>(&(), ProberConfig { code }) else {
            panic!("the prober begins");
        };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(Time::ZERO, &clock);
        }
        let waiting = probed(&engine, prober);

        engine.step(PROBE_DUE, &clock);
        while !engine.is_idle() {
            engine.step(PROBE_DUE, &clock);
        }

        assert_eq!(waiting, (Vec::new(), Vec::new()), "nothing comes before the deadline");
        let expired = Probed::No { at: 7, why: Unanswered::Expired };
        assert_eq!(probed(&engine, prober), (vec![expired], vec![expired]), "the envoy's ask expired with the ping");
    }

    /// What became of a ping or a parcel asked of the echo: the number its pong held, or why none came.
    fn pong_of(answered: Answered<echo::Pong>) -> Result<u64, Unanswered> {
        match answered {
            Answered::Yes(pong) => Ok(pong.n),
            Answered::No(why) => Err(why),
        }
    }

    #[test]
    fn a_guest_asked_what_it_cannot_read_lives_on_and_its_asker_hears_unreadable() -> Result<(), Unheard> {
        // Catches a guest's library that fails the turn of mail it cannot read, which ends the guest, and a host that
        // does not tell its engine, so the asker hears `Unreplied`, or nothing.
        for (parts, hops) in [(1, 0), (4, 64)] {
            let Some(parts) = NonZeroUsize::new(parts) else {
                panic!("an engine has parts");
            };
            let (mut engine, code) = with_echo(Engine::new(parts, 256, 1 << 20, 1 << 20, hops));
            let Ok(_) = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig) else {
                panic!("the echo begins");
            };
            let mut harness = Harness::bare(engine)?;
            let echo = harness.root::<echo::Echo>();

            let past = harness.ask(echo, &echo::Parcel { text: "7".repeat(1024) })?;
            let unread = harness.answer(past).map(pong_of)?;
            let within = harness.ask(echo, &echo::Parcel { text: "7".repeat(16) })?;
            let read = harness.answer(within).map(pong_of)?;

            assert_eq!(unread, Err(Unanswered::Unreadable), "{parts} parts, {hops} hops");
            assert_eq!(read, Ok(16), "{parts} parts, {hops} hops: the echo lived to answer the next");
        }

        Ok(())
    }
}
