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
    self, Born, ClaimRecord, Gone, Handed, ItemHead, Kept, Keying, LineRecord, Naming, Other, OutHead, Parent,
    PinRecord, RefRecord, SendRecord, Span, SpawnRecord, Spawning, StoodRecord, Stopped, TurnEnds, WatchRecord,
    blob_at, refusal,
};
use aethereal_actor::section::{self, Made};
use aethereal_actor::{
    Above, Arena, Asked, Batch, Birth, BlobHash, BlobId, BlobRef, BlobType, Broke, Children, Claimed, Declared,
    FalseSend, FalseWatch, Fault, Gift, Guests, Holds, Hosted, How, Instances, Item, Keeping, NotHeld, Pinning,
    Preparing, Puts, Reads, Refused, Role, Runs, SeatNumber, Seated, Seats, Table, Turning, Unfit, Was, Why,
    take_loose_ticket,
};
use aethereal_data::{ActorId, ActorPath, Budget, KeyName, KindId, KindName, from_bytes, to_bytes};
use std::collections::{BTreeMap, HashMap, VecDeque};
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
    /// The blob it was compiled from, which each of its actors pins while it runs (ADR-0009).
    blob: BlobId,
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

/// The key the next code compiled takes. Atomic, as runtimes serving any engine compile at once, and each code's key
/// must differ from every other's in the instances of a part.
static CODES: AtomicU64 = AtomicU64::new(0);

/// One instance of a blob, a part's, and the calls its host makes of it.
struct Instance {
    store: Store<Lending>,
    memory: Memory,
    inbox: TypedFunc<u32, u32>,
    out: TypedFunc<(), u64>,
    begin: TypedFunc<(u32, u64, u64, u32), u32>,
    take_in: TypedFunc<(u32, u64, u64, u32), u32>,
    take_out: TypedFunc<u32, u64>,
    turns: TypedFunc<(u32, u32, u32), u64>,
    pins: TypedFunc<(), u64>,
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
    pin: BlobRef,
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
    /// may be handed in its config (ADR-0010).
    holds: Holds<'a>,
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
    offered: Vec<BlobRef>,
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
    code: Code,
    /// The pin on its code's blob, which the engine keeps while the actor runs on it (ADR-0009).
    #[expect(dead_code, reason = "a pin is held for as long as it lives, and never read")]
    pin: BlobRef,
    me: Seated,
    lives: Lives,
    declared: &'static Declared,
    /// What the guest's turns of the batch prepared last asked for, and how far its host has handed them back.
    ran: Ran,
    /// What the host gave the batch prepared last, not yet handed to the engine.
    given: Given,
}

/// Where a guest actor's state is.
enum Lives {
    /// In its part's instance, under the handle the guest knows it by.
    In { handle: u32 },
    /// Taken out of the instance it began in, which is another part's, as its path and state written at `Within`,
    /// with the pins on the blobs they refer to: taken into its own part's instance on its first batch.
    Out { state: Vec<u8>, carried: Vec<BlobRef> },
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
}

/// The top bit of a seat given to a guest's child, set when the seat is in another part than its parent's.
const ELSEWHERE_BIT: u64 = 1 << 63;

/// The handle a guest writes for a child it did not keep.
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

        Ok(Code { blob, types, codes: Arc::downgrade(&self.codes), key, module, made })
    }

    /// Compiles `bytes` as guest code, and reads what it makes: refused when it is not fit to run.
    fn compile(&self, bytes: &[u8]) -> Result<Compiled, Unfit> {
        let module = Module::new(&self.engine, bytes).map_err(|error| Unfit::Uncompiled(error.to_string()))?;
        let section = declared_section(bytes).ok_or(Unfit::Undeclared)?;
        let mut budget = Budget::new(1 << 20);
        let made = from_bytes::<Vec<Made>>(section, &mut budget).map_err(|_| Unfit::Undeclared)?;
        let exports = ["memory", "aethereal_inbox", "aethereal_out", "aethereal_begin", "aethereal_take_in"];
        let more = ["aethereal_take_out", "aethereal_turns", "aethereal_pins", "aethereal_end", "aethereal_drop"];
        let more = more.into_iter().chain(["aethereal_read_limit"]);
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
            let mut bytes = [0u8; 16];
            bytes[..8].copy_from_slice(&resolved.id.bits().to_le_bytes());
            bytes[8..].copy_from_slice(&resolved.seat.number().to_le_bytes());
            let memory = caller.get_export("memory").and_then(Extern::into_memory);
            let Some(memory) = memory else {
                return Err(wasmtime::Error::msg("a guest that resolves a path has no memory"));
            };
            memory.write(&mut caller, usize::try_from(seated).unwrap_or(usize::MAX), &bytes)?;

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

/// Links the calls out that seat and spawn children into `linker`.
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
    linker.func_wrap("aethereal", "aethereal_spawn", |mut caller: Caller<'_, Lending>, at: u32, len: u32| {
        let spawning = guest_bytes(&mut caller, Span { at, len });
        answer(&mut caller, |answering| {
            // Read within the engine's limit, as a config the engine reads is.
            let mut budget = Budget::new(answering.instances.read_limit());
            let spawning = spawning.and_then(|bytes| from_bytes::<Spawning>(&bytes, &mut budget).ok());
            spawning.map_or_else(|| refusal(&Refused::NotMade), |spawning| answering.spawn(&spawning))
        })
    })?;

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
        let bytes = guest_bytes(&mut caller, Span { at, len });
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

impl<'a> Answering<'a> {
    /// What answers a call's calls out, from what its batch was lent, the instances of its part, and the code its
    /// runtime holds.
    fn of(
        Reads { table, made, seats, puts, holds }: Reads<'a>,
        instances: &'a mut Instances,
        codes: &'a Weak<Codes>,
        given: &'a mut Given,
        holdings: &'a Holdings,
    ) -> Self {
        Self { table, made, seats, puts, instances, codes, given, holdings, holds }
    }

    /// A pin on the blob `blob`, when the call's instance holds one or one was offered to the call: what lets its guest
    /// name the blob (ADR-0014).
    fn knows(&self, blob: BlobId) -> Option<BlobRef> {
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

    /// Puts `bytes` as a blob, into the worker's puts, and offers it to the call: answers with its id.
    fn put(&mut self, bytes: Vec<u8>) -> Result<BlobId, Refused> {
        let pin = self.puts.pinned(bytes)?;
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
                let blob = BlobId::new(engine, bits);
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

                Ok((aethereal_actor::Code::Blob(made), guests.begin(made, &spawning.config, birth)?))
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
    fn offer(&self, blob: BlobId) -> Option<BlobRef> {
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
    fn pin(&self, blob: BlobId) -> Option<BlobRef> {
        self.blobs.get(&blob).map(|holding| holding.pin.clone())
    }

    /// Takes in `changes`, what a call into the instance changed of its references to blobs, a pin on a blob it held
    /// none of taken from `offered`: false when one names a blob the instance held no reference to and none was
    /// offered, or drops more than it holds, as its code then came by an id it was not given (ADR-0014).
    fn change(&mut self, changes: impl IntoIterator<Item = PinRecord>, offered: &[BlobRef]) -> bool {
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
        let (Some(memory), Ok(inbox), Ok(out), Ok(begin), Ok(take_in), Ok(take_out), Ok(turns), Ok(pins), Ok(end)) =
            (memory, inbox, out, begin, take_in, take_out, turns, pins, end)
        else {
            panic!("guest code that entered has its exports");
        };
        let Ok(drop) = drop else {
            panic!("guest code that entered has its exports");
        };
        // A limit past what a guest's memory can address holds it to that memory.
        let read_limit = u32::try_from(read_limit).unwrap_or(u32::MAX);
        let Ok(()) = limit.and_then(|limit| limit.call(&mut store, read_limit)) else {
            panic!("guest code that entered takes its read limit");
        };

        Self {
            store,
            memory,
            inbox,
            out,
            begin,
            take_in,
            take_out,
            turns,
            pins,
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

    /// Begins the child in its part's instance of the blob: its `begin` runs there, handed where it stands and its
    /// config, and what it asked for is read back as a root's or a child's `begin` is (ADR-0015). Refused while that
    /// instance is lent to a call into it that is under way.
    fn begin(&self, made: &'static BlobType, config: &[u8], birth: Birth<'_>) -> Result<Box<dyn Runs>, Refused> {
        let BlobType { blob, declared } = *made;
        let code = self.codes.read().ok().and_then(|codes| codes.get(&blob).cloned()).ok_or(Refused::NotMade)?;
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
        let mut held = instances.lend(code.key, || Instance::load(&code, read_limit))?;
        let (seats, puts, _, gifts) = asked.lent_gifts();
        let holds = Holds { keeping: &mut *keeping, gifts };
        let reads = Reads { table, made: &(), seats, puts, holds };
        // What the config refers to is offered to the call, as the child takes references of its own to it.
        let offered = refers.iter().map(|(_, pin)| pin.clone()).collect();
        let mut given = Given { offered, ..Given::default() };
        let mut holdings = mem::take(&mut held.holdings);
        let mut answering = Answering::of(reads, instances, &code.codes, &mut given, &holdings);
        let began = held.begin(of, me, &handed, &mut answering);
        let began = began.and_then(|began| held.take_pins(&began.1, &mut holdings, &given).map(|()| began));
        held.holdings = holdings;
        // A child seated in another part than the one it began in is taken out, to be taken into its own part's
        // instance on its first batch, as a child its guest spawned is.
        let is_here = asked.seats_mut().is_own(me.seat);
        let mut hosted = Hosted::beginning(me, path, above, table, asked, declared, keeping);
        let replayed = began.and_then(|(handle, wrote)| {
            let Instance { store, memory, actors, holdings, .. } = &mut *held;
            let data = memory.data(&*store);
            let ends = TurnEnds::default();
            let replayed = replay(data, &code, &wrote, 0, ends, &mut given, holdings, actors, &mut hosted)?;
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
            Lives::In { handle }
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

        let pin = BlobRef::pin(code.blob);
        Ok(Box::new(Guest { code, pin, me, lives, declared, ran: Ran::Nothing, given: Given::default() }))
    }
}

impl Guest {
    /// The handle the guest keeps the actor under in `held`, its part's instance: taken in first when it was taken out
    /// of another part's.
    ///
    /// # Errors
    ///
    /// Why the call that took it in broke.
    fn handle_in(&mut self, held: &mut Instance) -> Result<u32, Broke> {
        let handle = match &self.lives {
            Lives::In { handle } => *handle,
            Lives::Out { state, carried } => {
                // The actor's type is one its code makes, as it began from that code.
                let (of, _) = self.code.made_of(self.declared.id()).ok_or(Broke::Garbled)?;
                let handle = held.take_in(of, self.me, state, carried)?;
                held.actors.push(self.me);
                self.lives = Lives::In { handle };
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
        let Some(held) = instances.get::<Instance>(self.code.key) else {
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
        let (batch, instances, reads) = preparing.hosted();
        // The engine prepares no batch while a call into a guest is under way, so the instance is never lent out here.
        let read_limit = instances.read_limit();
        let Ok(mut held) = instances.lend(self.code.key, || Instance::load(&self.code, read_limit)) else {
            self.ran = Ran::Broke(Broke::Trapped);
            return;
        };
        let handle = match self.handle_in(&mut held) {
            Ok(handle) => handle,
            Err(broke) => {
                instances.put_back(self.code.key, held);
                self.ran = Ran::Broke(broke);
                return;
            }
        };
        let (items, len) = held.hand_in(batch, &mut self.given.offered);
        let mut holdings = mem::take(&mut held.holdings);
        let Reads { table, made, seats, puts, holds } = reads;
        let lent = Reads { table, made, seats: &mut *seats, puts, holds };
        let mut answering = Answering::of(lent, &mut *instances, &self.code.codes, &mut self.given, &holdings);
        let Instance { store, turns, .. } = &mut *held;
        let called = lending(store, &mut answering, |store| turns.call(store, (handle, items, len)));
        let wrote = called.map_err(|_| Broke::Trapped).and_then(|out| held.out_head(out));
        // The references a child taken out of the instance in the call holds go with it, so their pins are offered
        // to its turn before the call's own changes, which drop the instance's references to them, are taken in.
        let given = &mut self.given;
        let wrote = wrote.and_then(|wrote| held.offer_taken_out(&wrote, &holdings, given).map(|()| wrote));
        let wrote = wrote.and_then(|wrote| held.take_pins(&wrote, &mut holdings, given).map(|()| wrote));
        held.holdings = holdings;
        instances.put_back(self.code.key, held);
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
            Item::Mail(_) => Role::Others,
            Item::Ended(_) => Role::Ended,
            Item::Answer { mail, .. } => Role::Answer(mail.kind()),
            Item::Unanswered { asking, .. } => Role::Answer(asking.answer),
        };
        let (mut hosted, instances) = turning.hosted(self.declared, role);
        let replayed = match self.ran {
            Ran::At { head, next, before } => {
                let Instance { store, memory, actors, holdings, .. } = self.code.instance(instances);
                let data = memory.data(&*store);
                replay(data, &self.code, &head, next, before, &mut self.given, holdings, actors, &mut hosted)
                    .map(|(replayed, ends)| (replayed, Ran::At { head, next: next + 1, before: ends }))
            }
            // The engine hands back no turn of a batch it did not prepare: were it to, no record of the turn would be
            // there to read.
            Ran::Nothing => Err(Broke::Garbled),
            Ran::Broke(broke) => Err(broke),
        };
        let (replayed, ran) = match replayed {
            Ok(replayed) => replayed,
            Err(broke) => {
                self.ran = Ran::Broke(broke);
                self.given.give_back(|seat| hosted.give_back(seat));
                // What the broken call left in the instance's memory cannot be trusted, so nothing runs in it again.
                if let Some(held) = instances.get::<Instance>(self.code.key) {
                    hosted.broken(broke, held.actors.iter().copied());
                }
                instances.drop_instance::<Instance>(self.code.key);
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
        let key = self.code.key;
        let is_in = matches!(self.lives, Lives::In { .. });
        let has_instance = instances.get::<Instance>(key).is_some();
        let was_trapped = is_in && !has_instance;
        if was_trapped {
            return Ok(());
        }
        // The engine runs no closing turn while a call into a guest is under way, so the instance is never lent out
        // here.
        let read_limit = instances.read_limit();
        let Ok(mut held) = instances.lend(key, || Instance::load(&self.code, read_limit)) else {
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
        let mut answering = Answering::of(reads, instances, &self.code.codes, &mut given, &holdings);
        let wrote = held.end(handle, why, &mut answering);
        let wrote = wrote.and_then(|wrote| held.take_pins(&wrote, &mut holdings, &given).map(|()| wrote));
        held.holdings = holdings;
        held.actors.retain(|actor| *actor != self.me);
        let replayed = wrote.and_then(|wrote| {
            let Instance { store, memory, actors, holdings, .. } = &mut *held;
            let data = memory.data(&*store);
            replay(data, &self.code, &wrote, 0, TurnEnds::default(), &mut given, holdings, actors, &mut hosted)
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
    fn take_in(&mut self, of: u32, me: Seated, state: &[u8], carried: &[BlobRef]) -> Result<u32, Broke> {
        let len = self.write_inbox(state);
        let handle = self.take_in.call(&mut self.store, (of, me.id.bits(), me.seat.number(), len));
        let handle = handle.map_err(|_| Broke::Trapped)?;
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
        let pins = dropped.map_err(|_| Broke::Trapped).and_then(|()| self.pins());
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
    fn hand_in(&mut self, batch: &dyn Batch, offered: &mut Vec<BlobRef>) -> (u32, u32) {
        let mut items = 0;
        let mut len = 0;
        batch.each(&mut |peeked| {
            peeked.pinned.each(|_, pin| offered.push(pin.clone()));
            let (handed, bytes) = match &peeked.item {
                // A batch is read before its turns run, which is where an answer is told from its mail, so an answer is
                // handed in as the mail it is (ADR-0017).
                Item::Mail(mail) | Item::Answer { mail, .. } | Item::Unanswered { mail, .. } => {
                    (Handed::Mail { kind: mail.kind() }, mail.bytes())
                }
                Item::Ended(ended) => match ended.how() {
                    How::Closed => (Handed::Ended(Gone::Closed), &[][..]),
                    How::Failed(fault) => (Handed::Ended(Gone::Failed { kind: fault.kind() }), fault.bytes()),
                    How::Earlier => (Handed::Ended(Gone::Earlier), &[][..]),
                },
            };
            let head = ItemHead { handed, source: peeked.source, chain: peeked.chain, len: count(bytes.len()) };
            let at = len;
            len += ItemHead::BYTES + bytes.len();
            self.make_room(len);
            let inbox = &mut self.memory.data_mut(&mut self.store)[self.inbox_at..];
            inbox[at..at + ItemHead::BYTES].copy_from_slice(&head.to_bytes());
            inbox[at + ItemHead::BYTES..len].copy_from_slice(bytes);
            items += 1;
        });

        (items, count(len))
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
/// in the instance added to `actors` unless the turn failed; and how it ended. Answers with that, and where it ends.
///
/// # Errors
///
/// Why the call broke, as the guest ran no such turn, or its records do not read back or name what it was not
/// given.
#[allow(clippy::too_many_arguments, reason = "what a replay reads and what it hands back to, each its own")]
fn replay(
    data: &[u8],
    code: &Code,
    head: &OutHead,
    next: u32,
    before: TurnEnds,
    given: &mut Given,
    holdings: &Holdings,
    actors: &mut Vec<Seated>,
    hosted: &mut Hosted<'_>,
) -> Result<(Result<(), Fault>, TurnEnds), Broke> {
    let actors_before = actors.len();
    let replayed = replay_records(data, code, head, next, before, given, holdings, actors, hosted);
    // A failed turn's children are dropped by the engine, and never live in the instance; nor do those of a turn whose
    // records do not read back.
    let has_more = actors.len() > actors_before;
    let is_kept = matches!(replayed, Ok((Ok(()), _)));
    if has_more && !is_kept {
        actors.truncate(actors_before);
    }

    replayed
}

/// What [`replay`] hands back, the children kept in the instance added to `actors` whatever the turn's end.
#[allow(clippy::too_many_arguments, reason = "what a replay reads and what it hands back to, each its own")]
fn replay_records(
    data: &[u8],
    code: &Code,
    head: &OutHead,
    next: u32,
    before: TurnEnds,
    given: &mut Given,
    holdings: &Holdings,
    actors: &mut Vec<Seated>,
    hosted: &mut Hosted<'_>,
) -> Result<(Result<(), Fault>, TurnEnds), Broke> {
    let was_run = next < head.ran;
    if !was_run {
        return Err(Broke::Garbled);
    }
    let ends = record(data, head.turns_at, next, TurnEnds::BYTES).and_then(TurnEnds::read).ok_or(Broke::Garbled)?;
    let spawns = ends.spawns > before.spawns;
    if spawns {
        spawn(data, code, head, before, ends, given, hosted)?;
    }
    // Each child of other code is handed over before the turn's sends are checked, which may be to it.
    let has_births = !given.births.is_empty();
    if has_births {
        hand_births(given, next, hosted)?;
    }
    for index in before.sends..ends.sends {
        let send =
            record(data, head.sends_at, index, SendRecord::BYTES).and_then(SendRecord::read).ok_or(Broke::Garbled)?;
        let bytes = lying(data, send.bytes).ok_or(Broke::Garbled)?;
        // Most mail refers to no blob, and a call whose mail refers to none wrote no reference.
        let pins = if head.refs == 0 {
            Some(Vec::new())
        } else {
            send_pins(data, head, index, bytes, holdings, given)
        };
        let is_refused = pins.is_none_or(|pins| hosted.send(send.by, send.to, send.kind, bytes, &pins).is_err());
        if is_refused {
            return Ok((Err(Fault::new(&FalseSend).unwrap_or_else(|_| Broke::Garbled.fault())), ends));
        }
    }
    for index in before.lines..ends.lines {
        let line =
            record(data, head.lines_at, index, LineRecord::BYTES).and_then(LineRecord::read).ok_or(Broke::Garbled)?;
        let text = lying(data, line.text).and_then(|text| str::from_utf8(text).ok()).ok_or(Broke::Garbled)?;
        hosted.log(line.by, line.level, text).ok_or(Broke::Garbled)?;
    }
    let stands = ends.stood > before.stood;
    if stands {
        stand(data, code, head, before, ends, given, holdings, actors, hosted)?;
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
/// seat taken from those `given` holds: of the guest's own code, or of other code the host began for it.
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
) -> Result<(), Broke> {
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
        hosted.spawn(spawn.by, from, key, spawn.seat).ok_or(Broke::Garbled)?;
    }

    Ok(())
}

/// Stands up, in `hosted`, the children that began in a turn, from the first after those `before` ends at to the
/// last `ends` does, each kept in the instance added to `actors`, and the seat of each the guest dropped put back in
/// `handed`.
///
/// # Errors
///
/// Why the call broke, as a record does not read back or names what the call did not make, or a child taken out
/// refers to a blob the call was not given.
#[allow(clippy::too_many_arguments, reason = "what a replay reads and what it hands back to, each its own")]
fn stand(
    data: &[u8],
    code: &Code,
    head: &OutHead,
    before: TurnEnds,
    ends: TurnEnds,
    given: &mut Given,
    holdings: &Holdings,
    actors: &mut Vec<Seated>,
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
                Lives::In { handle }
            }
            Kept::Out { state } => {
                let state = lying(data, state).ok_or(Broke::Garbled)?;
                let carried = carried_pins(state, holdings, &given.offered)?;
                Lives::Out { state: state.to_vec(), carried }
            }
        };
        let runs = Guest {
            code: code.clone(),
            pin: BlobRef::pin(code.blob),
            me,
            lives,
            declared,
            ran: Ran::Nothing,
            given: Given::default(),
        };
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
fn carried_pins(bytes: &[u8], holdings: &Holdings, offered: &[BlobRef]) -> Result<Vec<BlobRef>, Broke> {
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

/// A count the guest's 32-bit memory holds.
fn count(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
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
        Actor, ActorRef, Answer, Answered, Beginning, BlobRef, Carries, Ending, Forged, Handles, Links, Protocol,
        Seated, To, ToAny, ToSender, ToType, ToWithoutMail, Trapped, Turn,
    };
    use aethereal_data::{ActorPath, Kind, KindName, PathSegment, Plain};
    use aethereal_guest_echo as echo;
    use aethereal_substrate::Engine;
    use std::num::NonZeroUsize;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// The echo guest the build script built.
    const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

    /// `engine`, running guests with wasmtime, and the echo guest put into it as code.
    fn with_echo(engine: Engine) -> (Engine, BlobRef) {
        let mut engine = engine.with_guests(Arc::new(Runtime::default()));
        let Ok(code) = engine.put_code(ECHO.to_vec()) else {
            panic!("the echo guest is fit to run");
        };

        (engine, code)
    }

    /// A ping, as the echo guest declares it: the same name and layout, so the same kind.
    #[derive(Plain)]
    struct Ping {
        n: u64,
    }

    impl Kind for Ping {
        const NAME: KindName = KindName::new("aether.guest.ping");
    }

    #[derive(Plain)]
    struct Pong {
        n: u64,
    }

    impl Kind for Pong {
        const NAME: KindName = KindName::new("aether.guest.pong");
    }

    /// How many pings a pinger sends.
    #[derive(Plain)]
    struct Volley {
        pings: u64,
    }

    impl Kind for Volley {
        const NAME: KindName = KindName::new("aether.test.volley");
    }

    /// The pongs every pinger has been answered with.
    static PONGS: AtomicU64 = AtomicU64::new(0);

    /// A native actor that pings the echo guest, and pings it again on each pong while it has pings left.
    #[derive(aethereal_data::Bound)]
    struct Pinger;

    impl Kind for Pinger {
        const NAME: KindName = KindName::new("aether.test.pinger");
    }

    impl Actor for Pinger {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(volley: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(echo) = turn.resolve(&echo) {
                turn.send_to_any(ToWithoutMail::Held(echo), &Ping { n: volley.pings })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Pinger {
        type Sends = (ToSender<Ping>,);

        fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            PONGS.fetch_add(1, Ordering::Relaxed);
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
        let Ok(_) = engine.add_root::<Pinger>(&(), Volley { pings: 9 }) else {
            panic!("the pinger begins");
        };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        assert_eq!(PONGS.load(Ordering::Relaxed), 10);
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
    struct Which {
        at: u64,
    }

    impl Kind for Which {
        const NAME: KindName = KindName::new("aether.test.which");
    }

    /// What the questioner was answered, each pong with what it kept for its ping.
    static QUESTIONED: Mutex<Vec<(u64, u64)>> = Mutex::new(Vec::new());

    /// A native actor that spawns the echo guest from its code and asks it two pings, keeping which each was.
    #[derive(aethereal_data::Bound)]
    struct Questioner {
        code: BlobRef,
    }

    impl Kind for Questioner {
        const NAME: KindName = KindName::new("aether.test.questioner");
    }

    impl Actor for Questioner {
        type Config = Self;
        type Key = ();
        type Handles = (Volley, Answer<echo::Pong, Which>);
        type BeginSends = (ToType<Self, Volley>,);
        type EndSends = ();

        fn begin(questioner: Self, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let me = turn.me();
            turn.send(ToWithoutMail::Held(me), &Volley { pings: 2 })?;

            Ok(questioner)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Volley> for Questioner {
        type Sends = (aethereal_actor::Asks<Pings, echo::Ping, Which>,);

        fn handle(&mut self, volley: Volley, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            let Ok(echo) = turn.spawn_from::<echo::Echo>(&self.code, &(), &echo::EchoConfig) else {
                return Ok(());
            };
            for at in 0..volley.pings {
                turn.ask_to(echo.protocol::<Pings, _>(), &echo::Ping { n: at * 10 }, &Which { at })?;
            }

            Ok(())
        }
    }

    impl Handles<Answer<echo::Pong, Which>> for Questioner {
        type Sends = ();

        fn handle(&mut self, (pong, which): (Answered<echo::Pong>, Which), _: &mut Turn<'_, ()>) -> Result<(), Fault> {
            let Answered::Yes(pong) = pong else {
                return Ok(());
            };
            if let Ok(mut questioned) = QUESTIONED.lock() {
                questioned.push((pong.n, which.at));
            }

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
        let Ok(_) = engine.add_root::<Questioner>(&(), Questioner { code }) else {
            panic!("the questioner begins");
        };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        let questioned = QUESTIONED.lock().map(|questioned| questioned.clone()).unwrap_or_default();
        assert_eq!(questioned, [(0, 0), (10, 1)]);
    }

    /// The word to close, as the echo guest declares it.
    #[derive(Plain)]
    struct Shut;

    impl Kind for Shut {
        const NAME: KindName = KindName::new("aether.guest.shut");
    }

    /// The pongs the closer has been answered with.
    static CLOSER_PONGS: AtomicU64 = AtomicU64::new(0);

    /// A native actor that sends the echo guest a ping, the word to close, and another ping, all in its `begin`.
    #[derive(aethereal_data::Bound)]
    struct Closer;

    impl Kind for Closer {
        const NAME: KindName = KindName::new("aether.test.closer");
    }

    impl Actor for Closer {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping, Shut), Ping>, ToAny<(Ping, Shut), Shut>);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(echo) = turn.resolve(&echo) {
                turn.send_to_any(ToWithoutMail::Held(echo), &Ping { n: 0 })?;
                turn.send_to_any(ToWithoutMail::Held(echo), &Shut)?;
                turn.send_to_any(ToWithoutMail::Held(echo), &Ping { n: 0 })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Closer {
        type Sends = ();

        fn handle(&mut self, _: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            CLOSER_PONGS.fetch_add(1, Ordering::Relaxed);

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
        let Ok(_) = engine.add_root::<Closer>(&(), Volley { pings: 0 }) else {
            panic!("the closer begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        let failed = engine.log(echo).any(|entry| matches!(entry, aethereal_substrate::Entry::Failed(_)));
        assert!(!failed, "the echo closed, and did not fail");
        assert_eq!(CLOSER_PONGS.load(Ordering::Relaxed), 1);
    }

    /// Which callers have been answered, a bit for each, and how many answers there were.
    static CALLERS_ANSWERED: AtomicU64 = AtomicU64::new(0);
    static CALLER_ANSWERS: AtomicU64 = AtomicU64::new(0);

    /// A native actor that pings the echo guest once as it begins, and notes its answer under its key.
    #[derive(aethereal_data::Bound)]
    struct Caller {
        key: u64,
    }

    impl Kind for Caller {
        const NAME: KindName = KindName::new("aether.test.caller");
    }

    impl Actor for Caller {
        type Config = Volley;
        type Key = u64;
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(volley: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(echo) = turn.resolve(&echo) {
                turn.send_to_any(ToWithoutMail::Held(echo), &Ping { n: volley.pings })?;
            }

            Ok(Self { key: volley.pings })
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Caller {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            let is_own = pong.n == self.key;
            if is_own {
                CALLERS_ANSWERED.fetch_or(1 << self.key, Ordering::Relaxed);
            }
            CALLER_ANSWERS.fetch_add(1, Ordering::Relaxed);

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
        let callers = 50;
        for key in 0..callers {
            let Ok(_) = engine.add_root::<Caller>(&key, Volley { pings: key }) else {
                panic!("a caller begins");
            };
        }
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        assert_eq!(CALLERS_ANSWERED.load(Ordering::Relaxed), (1 << callers) - 1);
        assert_eq!(CALLER_ANSWERS.load(Ordering::Relaxed), callers);
    }

    /// The pongs the volleyer has been answered with.
    static VOLLEYER_PONGS: AtomicU64 = AtomicU64::new(0);

    /// A native actor that tells the mirror guest to close, and volleys with the echo guest, both of one blob.
    #[derive(aethereal_data::Bound)]
    struct Volleyer;

    impl Kind for Volleyer {
        const NAME: KindName = KindName::new("aether.test.volleyer");
    }

    impl Actor for Volleyer {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping, Shut), Ping>, ToAny<(Ping, Shut), Shut>);
        type EndSends = ();

        fn begin(volley: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let mirror = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.mirror"), &()));
            let echo = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.echo"), &()));
            if let Some(mirror) = turn.resolve(&mirror) {
                turn.send_to_any(ToWithoutMail::Held(mirror), &Shut)?;
            }
            if let Some(echo) = turn.resolve(&echo) {
                turn.send_to_any(ToWithoutMail::Held(echo), &Ping { n: volley.pings })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Volleyer {
        type Sends = (ToSender<Ping>,);

        fn handle(&mut self, pong: Pong, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            VOLLEYER_PONGS.fetch_add(1, Ordering::Relaxed);
            if pong.n > 0 {
                turn.reply(&Ping { n: pong.n - 1 })?;
            }

            Ok(())
        }
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
        let Ok(_) = engine.add_root::<Volleyer>(&(), Volley { pings: 3 }) else {
            panic!("the volleyer begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        let failed = engine.log(echo).any(|entry| matches!(entry, aethereal_substrate::Entry::Failed(_)));
        assert!(!failed, "the echo ran on in its instance");
        assert_eq!(VOLLEYER_PONGS.load(Ordering::Relaxed), 4);
    }

    /// The word to crash, as the echo guest declares it.
    #[derive(Plain)]
    struct Crash;

    impl Kind for Crash {
        const NAME: KindName = KindName::new("aether.guest.crash");
    }

    /// A native actor that tells the mirror guest to crash as it begins.
    #[derive(aethereal_data::Bound)]
    struct Crasher;

    impl Kind for Crasher {
        const NAME: KindName = KindName::new("aether.test.crasher");
    }

    impl Actor for Crasher {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Crash,), Crash>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let mirror = ActorPath::from_root(&PathSegment::new(KindName::new("aether.guest.mirror"), &()));
            if let Some(mirror) = turn.resolve(&mirror) {
                turn.send_to_any(ToWithoutMail::Held(mirror), &Crash)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Crasher {
        type Sends = ();

        fn handle(&mut self, _: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
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
            engine.step(&clock);
        }

        let trapped = |actor| {
            let is_trap = |entry: &aethereal_substrate::Entry| match entry {
                aethereal_substrate::Entry::Failed(fault) => fault.kind() == KindId::of::<Trapped>(),
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
            engine.step(&clock);
        }

        let is_trap = |entry: &aethereal_substrate::Entry| match entry {
            aethereal_substrate::Entry::Failed(fault) => fault.kind() == KindId::of::<Trapped>(),
            _ => false,
        };
        let refused_trapped = matches!(&doomed, Err(Refused::Began(fault)) if fault.kind() == KindId::of::<Trapped>());
        assert!(refused_trapped, "the doomed root is refused with its trap");
        assert!(engine.log(echo).any(is_trap), "the echo, in the same instance, ended with it");
        assert!(!engine.table().is_live(echo.id));
    }

    /// The word to a brood to spawn one more child, as the echo guest declares it.
    #[derive(Plain)]
    struct Grow;

    impl Kind for Grow {
        const NAME: KindName = KindName::new("aether.guest.grow");
    }

    /// What an asker pings, and which of [`ANSWERS`] it keeps its answers in.
    #[derive(Plain)]
    struct Asks {
        paths: Vec<ActorPath>,
        tally: u64,
    }

    impl Kind for Asks {
        const NAME: KindName = KindName::new("aether.test.asks");
    }

    /// The answers each test's askers were given, by the tally the test keeps them in.
    static ANSWERS: [Mutex<Vec<u64>>; 12] = [const { Mutex::new(Vec::new()) }; 12];

    /// A native actor that pings each actor at the paths it is given with a ping of zero, and keeps what each answers.
    #[derive(aethereal_data::Bound)]
    struct Asker {
        tally: u64,
    }

    impl Kind for Asker {
        const NAME: KindName = KindName::new("aether.test.asker");
    }

    impl Actor for Asker {
        type Config = Asks;
        type Key = u64;
        type Handles = (Pong,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(asks: Asks, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            for path in &asks.paths {
                if let Some(asked) = turn.resolve(path) {
                    turn.send_to_any(ToWithoutMail::Held(asked), &Ping { n: 0 })?;
                }
            }

            Ok(Self { tally: asks.tally })
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Asker {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            if let Ok(mut answers) = ANSWERS[usize::try_from(self.tally).unwrap_or_default()].lock() {
                answers.push(pong.n);
            }

            Ok(())
        }
    }

    /// A native actor that tells the brood to grow, as it begins.
    #[derive(aethereal_data::Bound)]
    struct Grower;

    impl Kind for Grower {
        const NAME: KindName = KindName::new("aether.test.grower");
    }

    impl Actor for Grower {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(Grow,), Grow>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            if let Some(brood) = turn.resolve(&brood_path()) {
                turn.send_to_any(ToWithoutMail::Held(brood), &Grow)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
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
            engine.step(&clock);
        }
    }

    /// The answers kept in `tally`, in order.
    fn answers(tally: usize) -> Vec<u64> {
        let Ok(answers) = ANSWERS[tally].lock() else {
            panic!("no asker panicked holding the answers");
        };
        let mut answers = answers.clone();
        answers.sort_unstable();

        answers
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
        let Ok(_) = engine.add_root::<Asker>(&0, Asks { paths, tally: 0 }) else {
            panic!("the asker begins");
        };
        settle(&mut engine);

        assert_eq!(answers(0), [1, 11, 21, 31, 1001]);
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
        let Ok(_) = engine.add_root::<Asker>(&1, Asks { paths, tally: 1 }) else {
            panic!("the asker begins");
        };
        settle(&mut engine);

        assert_eq!(answers(1), [1, 11, 21, 31, 41, 51, 61, 71, 1001]);
    }

    /// What a keeper is made from: its key, and which of [`ANSWERS`] it keeps the pings it is sent in.
    #[derive(Plain)]
    struct Keeps {
        key: u64,
        tally: u64,
    }

    impl Kind for Keeps {
        const NAME: KindName = KindName::new("aether.test.keeps");
    }

    /// A native actor that keeps each ping it is sent in its tally, with a hundred for each of its key.
    #[derive(aethereal_data::Bound)]
    struct Keeper {
        key: u64,
        tally: u64,
    }

    impl Kind for Keeper {
        const NAME: KindName = KindName::new("aether.test.keeper");
    }

    impl Actor for Keeper {
        type Config = Keeps;
        type Key = u64;
        type Handles = (Ping,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(keeps: Keeps, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self { key: keeps.key, tally: keeps.tally })
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Ping> for Keeper {
        type Sends = ();

        fn handle(&mut self, ping: Ping, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            if let Ok(mut answers) = ANSWERS[usize::try_from(self.tally).unwrap_or_default()].lock() {
                answers.push(100 * self.key + ping.n);
            }

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
        for key in [0, 1] {
            let Ok(_) = engine.add_root::<Keeper>(&key, Keeps { key, tally: 2 }) else {
                panic!("a keeper begins");
            };
        }
        let keeper = |key: u64| ActorPath::from_root(&PathSegment::new(Keeper::NAME, &key));
        let seek = echo::Seek { paths: vec![keeper(0), keeper(9), keeper(1)] };
        let Ok(_) = engine.add_root_from::<echo::Seeker>(&code, &(), &seek) else {
            panic!("the seeker begins");
        };
        settle(&mut engine);

        assert_eq!(answers(2), [0, 102]);
    }

    #[test]
    fn a_guest_casts_a_reference_to_a_protocol_by_what_its_host_reads_of_the_type() {
        // Catches a guest's cast that answers by its own table, which holds no other code's types, one its host
        // answers for a type that lists only some of the protocol's kinds, and ids that do not cross intact.
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        for key in [0, 1] {
            let Ok(_) = engine.add_root::<Keeper>(&key, Keeps { key, tally: 9 }) else {
                panic!("a keeper begins");
            };
        }
        let keeper = |key: u64| ActorPath::from_root(&PathSegment::new(Keeper::NAME, &key));
        let seek = echo::Seek { paths: vec![keeper(0), keeper(1)] };
        let Ok(_) = engine.add_root_from::<echo::Caster>(&code, &(), &seek) else {
            panic!("the caster begins");
        };
        settle(&mut engine);

        assert_eq!(answers(9), [0, 101]);
    }

    /// What a shiner is made from: the tally it keeps the light of each lamp it is handed in.
    #[derive(Plain)]
    struct Shining {
        tally: u64,
    }

    impl Kind for Shining {
        const NAME: KindName = KindName::new("aether.test.shining");
    }

    /// A native actor that takes each lamp it is handed and keeps its light in its tally: what reads that a lamp came
    /// through a guest whole, as only native code opens one.
    #[derive(aethereal_data::Bound)]
    struct Shiner {
        tally: u64,
    }

    impl Kind for Shiner {
        const NAME: KindName = KindName::new("aether.test.shiner");
    }

    impl Actor for Shiner {
        type Config = Shining;
        type Key = ();
        type Handles = (echo::Hand,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(shining: Shining, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self { tally: shining.tally })
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Hand> for Shiner {
        type Sends = ();

        fn handle(&mut self, hand: echo::Hand, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            for lamp in hand.lamps {
                let mut lamp = turn.take(lamp)?;
                let light = turn.open(&mut lamp)?.light;
                if let Ok(mut answers) = ANSWERS[usize::try_from(self.tally).unwrap_or_default()].lock() {
                    answers.push(light);
                }
            }

            Ok(())
        }
    }

    /// What a lighter is made from: the actor it hands its lamps to, and the light of each.
    #[derive(Plain)]
    struct Lighting {
        to: ActorPath,
        lights: Vec<u64>,
    }

    impl Kind for Lighting {
        const NAME: KindName = KindName::new("aether.test.lighting");
    }

    /// A native actor that makes a lamp of each light it is made with as it begins, keeps it, and hands each in a hand
    /// of its own to the actor `to` names, so they arrive as turns of one batch.
    #[derive(aethereal_data::Bound)]
    struct Lighter;

    impl Kind for Lighter {
        const NAME: KindName = KindName::new("aether.test.lighter");
    }

    impl Actor for Lighter {
        type Config = Lighting;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(echo::Hand,), echo::Hand>,);
        type EndSends = ();

        fn begin(lighting: Lighting, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let Lighting { to, lights } = lighting;
            let to = turn.resolve(&to).and_then(|found| turn.cast::<(echo::Hand,)>(found));
            let Some(to) = to else {
                return Err(Fault::new(&Doom)?);
            };
            for light in lights {
                let lamp = turn.keep(echo::Lamp { light })?;
                let lamps = vec![turn.give(lamp)?];
                turn.send_to_any(ToWithoutMail::Held(to.erase()), &echo::Hand { lamps })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    /// An engine running the echo blob, keeping the bearer for it, with a shiner keeping its lights in tally `tally`.
    fn lit(tally: u64) -> (Engine, BlobRef, ActorPath) {
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let engine = Engine::new(parts, 1024, 0, 1 << 20, 0).with_links(Links::default().with::<echo::Bearer>());
        let (mut engine, code) = with_echo(engine);
        let Ok(_) = engine.add_root::<Shiner>(&(), Shining { tally }) else {
            panic!("the shiner begins");
        };

        (engine, code, ActorPath::from_root(&PathSegment::new(Shiner::NAME, &())))
    }

    #[test]
    fn a_guest_takes_resources_in_mail_and_passes_them_on_to_native_code() {
        // Catches a guest's take, give, or send of a resource its host does not carry out, one carried with the wrong
        // piece of mail of a batch of several, and a resource a guest's mail does not carry to its receiver.
        let (mut engine, code, shiner) = lit(10);
        let porting = echo::Porting { to: shiner, lamps: Vec::new(), how: 0 };
        let Ok(_) = engine.add_root_from::<echo::Porter>(&code, &0, &porting) else {
            panic!("the porter begins");
        };
        let porter = ActorPath::from_root(&PathSegment::new(echo::Porter::NAME, &0));
        let Ok(_) = engine.add_root::<Lighter>(&(), Lighting { to: porter, lights: vec![3, 4] }) else {
            panic!("the lighter begins");
        };
        settle(&mut engine);

        assert_eq!(answers(10), [3, 4]);
    }

    #[test]
    fn a_guest_takes_resources_from_its_config_and_hands_them_to_children_of_its_own_and_other_code() {
        // Catches a guest root that cannot take what the chassis handed in for it, a guest's child of its own code
        // that cannot take what its parent gave it, and a native child its host begins for a guest that is not handed
        // what its config holds, or does not keep what it took once it is seated.
        let (mut engine, code, shiner) = lit(11);
        for how in [0, 1, 2] {
            let lamps = vec![engine.hand(echo::Lamp { light: 10 + how })];
            let porting = echo::Porting { to: shiner.clone(), lamps, how };
            let began = engine.add_root_from::<echo::Porter>(&code, &(10 + how), &porting);
            let Ok(_) = began else {
                panic!("the porter begins: {began:?}");
            };
        }
        settle(&mut engine);

        assert_eq!(answers(11), [10, 11, 12]);
    }

    /// The reason a mortal fails.
    #[derive(Plain)]
    struct Doom;

    impl Kind for Doom {
        const NAME: KindName = KindName::new("aether.test.doom");
    }

    /// A native actor that ends on its first ping: the one keyed zero closes, and any other fails.
    #[derive(aethereal_data::Bound)]
    struct Mortal {
        key: u64,
    }

    impl Kind for Mortal {
        const NAME: KindName = KindName::new("aether.test.mortal");
    }

    impl Actor for Mortal {
        type Config = Volley;
        type Key = u64;
        type Handles = (Ping,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(volley: Volley, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self { key: volley.pings })
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Ping> for Mortal {
        type Sends = ();

        fn handle(&mut self, _: Ping, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
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
        let Ok(_) = engine.add_root::<Keeper>(&0, Keeps { key: 0, tally: 3 }) else {
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

        let Ok(_) = engine.add_root::<Asker>(&2, Asks { paths: vec![mortal(0), mortal(1)], tally: 0 }) else {
            panic!("the asker begins");
        };
        settle(&mut engine);

        assert_eq!(answers(3), [1, 12]);
    }

    /// What a nest is made from: the blob its reporters are spawned from, and how many.
    #[derive(aethereal_data::Bound)]
    struct Nesting {
        code: BlobRef,
        reporters: u64,
    }

    impl Kind for Nesting {
        const NAME: KindName = KindName::new("aether.test.nesting");
    }

    /// A word to a nest to ping each of its reporters.
    #[derive(Plain)]
    struct Poke;

    impl Kind for Poke {
        const NAME: KindName = KindName::new("aether.test.poke");
    }

    /// A native actor that spawns reporters of the echo guest's blob as it begins, pings each when poked, and keeps
    /// what each tells it in the fifth of [`ANSWERS`].
    #[derive(aethereal_data::Bound)]
    struct Nest {
        reporters: Vec<ActorRef<echo::Reporter>>,
    }

    impl Kind for Nest {
        const NAME: KindName = KindName::new("aether.test.nest");
    }

    impl Actor for Nest {
        type Config = Nesting;
        type Key = ();
        type Handles = (Poke, Pong);
        type BeginSends = ();
        type EndSends = ();

        fn begin(nesting: Nesting, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let mut reporters = Vec::new();
            for key in 0..nesting.reporters {
                let config = echo::AddBy { by: 10 * key, deeper: 0 };
                let Ok(reporter) = turn.spawn_from::<echo::Reporter>(&nesting.code, &key, &config) else {
                    return Err(Fault::new(&Doom)?);
                };
                reporters.push(reporter);
            }
            let unmade = turn.spawn_from::<Keeper>(&nesting.code, &0, &Keeps { key: 0, tally: 4 }).err();
            let impostor = turn.spawn_from::<Impostor>(&nesting.code, &9, &echo::AddBy { by: 0, deeper: 0 }).err();
            let both_refused = unmade == Some(Refused::NotMade) && impostor == Some(Refused::NotMade);
            if !both_refused {
                return Err(Fault::new(&Doom)?);
            }

            Ok(Self { reporters })
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Poke> for Nest {
        type Sends = (ToType<echo::Reporter, echo::Ping>,);

        fn handle(&mut self, _: Poke, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            for reporter in &self.reporters {
                turn.send(To::Held(reporter.erase()), &echo::Ping { n: 1 })?;
            }

            Ok(())
        }
    }

    impl Handles<Pong> for Nest {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            if let Ok(mut answers) = ANSWERS[4].lock() {
                answers.push(pong.n);
            }

            Ok(())
        }
    }

    /// A native type named as the echo guest's reporter is, which declares other handlers: a blob that makes a reporter
    /// does not make this.
    #[derive(aethereal_data::Bound)]
    struct Impostor;

    impl Kind for Impostor {
        const NAME: KindName = KindName::new("aether.guest.reporter");
    }

    impl Actor for Impostor {
        type Config = echo::AddBy;
        type Key = u64;
        type Handles = (Poke,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(_: echo::AddBy, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Poke> for Impostor {
        type Sends = ();

        fn handle(&mut self, _: Poke, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    /// A native actor that pokes the nest as it begins.
    #[derive(aethereal_data::Bound)]
    struct Poker;

    impl Kind for Poker {
        const NAME: KindName = KindName::new("aether.test.poker");
    }

    impl Actor for Poker {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(Poke,), Poke>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let nest = ActorPath::from_root(&PathSegment::new(Nest::NAME, &()));
            if let Some(nest) = turn.resolve(&nest) {
                turn.send_to_any(ToWithoutMail::Held(nest), &Poke)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
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
        let Ok(_) = engine.add_root::<Nest>(&(), Nesting { code, reporters: 4 }) else {
            panic!("the nest begins, with its reporters, and is refused the keeper");
        };
        settle(&mut engine);
        let Ok(_) = engine.add_root::<Poker>(&(), Volley { pings: 0 }) else {
            panic!("the poker begins");
        };
        settle(&mut engine);

        assert_eq!(answers(4), [1, 11, 21, 31]);
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
    struct Henning {
        home: BlobRef,
        other: BlobRef,
    }

    impl Kind for Henning {
        const NAME: KindName = KindName::new("aether.test.henning");
    }

    /// A native actor that spawns a hatcher of the echo guest's blob as it begins, and keeps what it tells it in the
    /// sixth of [`ANSWERS`].
    #[derive(aethereal_data::Bound)]
    struct Hen;

    impl Kind for Hen {
        const NAME: KindName = KindName::new("aether.test.hen");
    }

    impl Actor for Hen {
        type Config = Henning;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(henning: Henning, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let hatch = echo::Hatch { other: henning.other, home: henning.home.clone() };
            let Ok(_) = turn.spawn_from::<echo::Hatcher>(&henning.home, &(), &hatch) else {
                return Err(Fault::new(&Doom)?);
            };

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Hen {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            if let Ok(mut answers) = ANSWERS[5].lock() {
                answers.push(pong.n);
            }

            Ok(())
        }
    }

    /// A native actor that tells the hen's hatcher to grow twice, as it begins, so both words are one batch's.
    #[derive(aethereal_data::Bound)]
    struct Rouser;

    impl Kind for Rouser {
        const NAME: KindName = KindName::new("aether.test.rouser");
    }

    impl Actor for Rouser {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(Grow,), Grow>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let hen = ActorPath::from_root(&PathSegment::new(Hen::NAME, &()));
            let Ok(hatcher) = hen.child(&PathSegment::new(echo::Hatcher::NAME, &())) else {
                return Err(Fault::new(&Doom)?);
            };
            if let Some(hatcher) = turn.resolve(&hatcher) {
                turn.send_to_any(ToWithoutMail::Held(hatcher), &Grow)?;
                turn.send_to_any(ToWithoutMail::Held(hatcher), &Grow)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
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
        let engine = Engine::new(parts, 1024, 0, 1 << 20, 0).with_links(Links::default().with::<echo::Chick>());
        let (mut engine, home) = with_echo(engine);
        let Ok(other) = engine.put_code(salted(ECHO)) else {
            panic!("the echo guest, salted, is fit to run");
        };
        let Ok(_) = engine.add_root::<Hen>(&(), Henning { home, other }) else {
            panic!("the hen begins, with its hatcher");
        };
        settle(&mut engine);
        let Ok(_) = engine.add_root::<Rouser>(&(), Volley { pings: 0 }) else {
            panic!("the rouser begins");
        };
        settle(&mut engine);

        assert_eq!(answers(5), [20, 102, 112, 306, 400, 505, 505, 605, 605]);
    }

    /// A native actor that keeps each pong it is sent in the seventh of [`ANSWERS`].
    #[derive(aethereal_data::Bound)]
    struct Ledger;

    impl Kind for Ledger {
        const NAME: KindName = KindName::new("aether.test.ledger");
    }

    impl Actor for Ledger {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(_: Volley, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Ledger {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            if let Ok(mut answers) = ANSWERS[6].lock() {
                answers.push(pong.n);
            }

            Ok(())
        }
    }

    /// The path of the librarian root.
    fn librarian() -> ActorPath {
        ActorPath::from_root(&PathSegment::new(echo::Librarian::NAME, &()))
    }

    /// A native actor that pings the librarian for a book of five as it begins, keeps in the seventh of [`ANSWERS`]
    /// how long the book it is sent is and what its first byte is, read from the store, and sends the book back; and
    /// keeps the pong it is answered with there too.
    #[derive(aethereal_data::Bound)]
    struct Borrower;

    impl Kind for Borrower {
        const NAME: KindName = KindName::new("aether.test.borrower");
    }

    impl Actor for Borrower {
        type Config = Volley;
        type Key = ();
        type Handles = (echo::Book, Pong);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            if let Some(librarian) = turn.resolve(&librarian()) {
                turn.send_to_any(ToWithoutMail::Held(librarian), &Ping { n: 5 })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Book> for Borrower {
        type Sends = (ToSender<echo::Book>,);

        fn handle(&mut self, book: echo::Book, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            let bytes = turn.read(&book.blob);
            let read = [bytes.len() as u64, bytes.first().copied().map_or(0, u64::from)];
            if let Ok(mut answers) = ANSWERS[6].lock() {
                answers.extend(read);
            }
            turn.reply(&book)?;

            Ok(())
        }
    }

    impl Handles<Pong> for Borrower {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            if let Ok(mut answers) = ANSWERS[6].lock() {
                answers.push(pong.n);
            }

            Ok(())
        }
    }

    /// A native actor that tells the librarian to shut, as it begins.
    #[derive(aethereal_data::Bound)]
    struct Shutter;

    impl Kind for Shutter {
        const NAME: KindName = KindName::new("aether.test.shutter");
    }

    impl Actor for Shutter {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(echo::Shut,), echo::Shut>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            if let Some(librarian) = turn.resolve(&librarian()) {
                turn.send_to_any(ToWithoutMail::Held(librarian), &echo::Shut)?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
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
        let Ok(_) = engine.add_root::<Ledger>(&(), Volley { pings: 0 }) else {
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

        let Ok(_) = engine.add_root::<Borrower>(&(), Volley { pings: 0 }) else {
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

        assert_eq!(answers(6), [5, 7, 11, 105]);
        // The blob lent out goes once the mail that carried it is gone, by the time the librarian has closed.
        assert_eq!((kept_by_librarian, after_closing), (2, 1));
        assert!(matches!(forged, Err(Refused::Began(_))));
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
        let ended_forged = |entry: &aethereal_substrate::Entry| match entry {
            aethereal_substrate::Entry::Failed(fault) => is_forged(fault),
            _ => false,
        };
        assert!(refused_forged, "the hoarder is refused for forging");
        assert!(engine.log(echo).any(ended_forged), "the echo, in the same instance, ended for the forging");
        assert!(!engine.table().is_live(echo.id));
    }

    /// A native actor that pings the spendthrift for a book of three, as it begins.
    #[derive(aethereal_data::Bound)]
    struct Dun;

    impl Kind for Dun {
        const NAME: KindName = KindName::new("aether.test.dun");
    }

    impl Actor for Dun {
        type Config = Volley;
        type Key = ();
        type Handles = (echo::Book,);
        type BeginSends = (ToAny<(Ping,), Ping>,);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let spendthrift = ActorPath::from_root(&PathSegment::new(echo::Spendthrift::NAME, &()));
            if let Some(spendthrift) = turn.resolve(&spendthrift) {
                turn.send_to_any(ToWithoutMail::Held(spendthrift), &Ping { n: 3 })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<echo::Book> for Dun {
        type Sends = ();

        fn handle(&mut self, _: echo::Book, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
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

        let failed = engine.log(spendthrift).any(|entry| matches!(entry, aethereal_substrate::Entry::Failed(_)));
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

    /// A native actor that keeps in the eighth of [`ANSWERS`] each pong it is told.
    #[derive(aethereal_data::Bound)]
    struct Mourner;

    impl Kind for Mourner {
        const NAME: KindName = KindName::new("aether.test.mourner");
    }

    impl Actor for Mourner {
        type Config = Volley;
        type Key = ();
        type Handles = (Pong,);
        type BeginSends = ();
        type EndSends = ();

        fn begin(_: Volley, _: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
            Ok(())
        }
    }

    impl Handles<Pong> for Mourner {
        type Sends = ();

        fn handle(&mut self, pong: Pong, _: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
            if let Ok(mut answers) = ANSWERS[7].lock() {
                answers.push(pong.n);
            }

            Ok(())
        }
    }

    /// A native actor that, as it begins, pings the first parting twice and tells it to shut, and pings the second
    /// once and then with a zero, which fails it.
    #[derive(aethereal_data::Bound)]
    struct Dismisser;

    impl Kind for Dismisser {
        const NAME: KindName = KindName::new("aether.test.dismisser");
    }

    impl Actor for Dismisser {
        type Config = Volley;
        type Key = ();
        type Handles = ();
        type BeginSends = (ToAny<(Ping,), Ping>, ToAny<(echo::Shut,), echo::Shut>);
        type EndSends = ();

        fn begin(_: Volley, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
            let parting = |key: u64| ActorPath::from_root(&PathSegment::new(echo::Parting::NAME, &key));
            if let Some(first) = turn.resolve(&parting(1)) {
                turn.send_to_any(ToWithoutMail::Held(first), &Ping { n: 1 })?;
                turn.send_to_any(ToWithoutMail::Held(first), &Ping { n: 1 })?;
                turn.send_to_any(ToWithoutMail::Held(first), &echo::Shut)?;
            }
            if let Some(second) = turn.resolve(&parting(2)) {
                turn.send_to_any(ToWithoutMail::Held(second), &Ping { n: 1 })?;
                turn.send_to_any(ToWithoutMail::Held(second), &Ping { n: 0 })?;
            }

            Ok(Self)
        }

        fn end(self, _: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
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
        let Ok(_) = engine.add_root::<Mourner>(&(), Volley { pings: 0 }) else {
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

        assert_eq!(answers(7), [611, 620]);
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

        let Ok(_) = engine.add_root::<Asker>(&8, Asks { paths: readers, tally: 8 }) else {
            panic!("the asker begins");
        };
        settle(&mut engine);

        assert_eq!(answers(8), [11; 8]);
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
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).with_guests(Arc::clone(&runtime) as Arc<dyn Guests>);
        let (Ok(first), Ok(second)) = (engine.put_code(ECHO.to_vec()), engine.put_code(ECHO.to_vec())) else {
            panic!("the echo guest is fit to run");
        };
        assert_ne!(first.id(), second.id(), "each put is a blob of its own");
        let compiled = || runtime.compiled.read().map_or(usize::MAX, |compiled| compiled.len());
        let clock = Monotonic::new();

        drop(first);
        engine.step(&clock);
        let with_one = compiled();
        drop(second);
        engine.step(&clock);

        assert_eq!((with_one, compiled()), (1, 0));
    }

    #[test]
    fn code_whose_blob_is_freed_makes_nothing() {
        // Catches code kept once the engine has let go of its blob, which would run actors from bytes no longer held.
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let (mut engine, code) = with_echo(Engine::new(parts, 1024, 0, 1 << 20, 0));
        let id = code.id();
        drop(code);
        engine.step(&Monotonic::new());

        let code = BlobRef::pin(id);
        let refused = engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig);

        assert_eq!(refused.err(), Some(Refused::NotMade));
    }
}
