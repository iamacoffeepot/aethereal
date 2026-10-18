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

use aethereal_actor::records::{
    Gone, Handed, ItemHead, Kept, Keying, LineRecord, Naming, OutHead, SendRecord, Span, SpawnRecord, StoodRecord,
    Stopped, TurnEnds, WatchRecord,
};
use aethereal_actor::section::{self, Made};
use aethereal_actor::{
    Batch, Children, Declared, FalseSend, FalseWatch, Fault, Hosted, How, Instances, Item, Preparing, Reads, Refused,
    Role, Runs, SeatNumber, Seated, Seats, Table, Trapped, Turning,
};
use aethereal_data::{ActorId, ActorKey, ActorPath, Budget, KeyName, PathSegment, from_bytes, to_bytes};
use aethereal_substrate::Engine;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use wasmtime::{Caller, Extern, InstancePre, Linker, Memory, Module, Store, TypedFunc};

/// What runs guests' code: one compiler, for every blob.
#[derive(Default)]
pub struct Guests {
    engine: wasmtime::Engine,
}

/// A blob of guest code, compiled, and what each actor type it makes declares: what every part makes its instance
/// of the blob from. Cloning it shares the compiled code.
#[derive(Clone)]
pub struct Code {
    /// What its instances are kept under in a part's [`Instances`]: a number no other code compiled in the process
    /// has.
    key: u64,
    /// The module, with the calls out it imports linked in.
    module: InstancePre<Lending>,
    made: Vec<&'static Declared>,
}

/// The key the next code compiled takes.
static CODES: AtomicU64 = AtomicU64::new(0);

/// Why a blob of code was refused as it entered.
#[derive(Debug)]
pub enum Unfit {
    /// It is not a module the runtime compiles.
    Uncompiled(String),
    /// Its section of what it makes is missing, or does not read back.
    Undeclared,
    /// It lacks an export a guest must have, or has it with the wrong signature.
    Unexported(&'static str),
    /// It imports what its host does not offer.
    Unlinked(String),
}

/// One instance of a blob, a part's, and the calls its host makes of it.
struct Instance {
    store: Store<Lending>,
    memory: Memory,
    inbox: TypedFunc<u32, u32>,
    out: TypedFunc<(), u64>,
    begin: TypedFunc<(u32, u64, u64, u32), u32>,
    take_in: TypedFunc<(u32, u64, u64, u32), u32>,
    turns: TypedFunc<(u32, u32, u32), u64>,
    /// Where the guest's inbox is, and how many bytes it has room for: asked for again only when it must grow.
    inbox_at: usize,
    inbox_room: usize,
    /// The actors that live in it: the part drops it when the last ends, and a trap in it ends them all.
    actors: Vec<Seated>,
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

/// What a call's calls out are answered from: what its batch, or its root's `begin`, was lent, and the seats it gave.
struct Answering<'a> {
    table: &'a dyn Table,
    made: &'a dyn Children,
    seats: &'a mut Seats,
    /// The seats it gave the guest's children and has not had back, in the order given.
    handed: Vec<SeatNumber>,
}

/// An actor whose code runs in a guest's instance, as the engine holds it: the instance is its part's, which it
/// takes from what its part lends each batch and turn.
struct Guest {
    code: Code,
    me: Seated,
    lives: Lives,
    declared: &'static Declared,
    /// What the guest's turns of the batch prepared last asked for, and how far its host has handed them back.
    ran: Ran,
    /// The seats the batch prepared last gave the guest's children, not yet handed to the engine with a child.
    handed: Vec<SeatNumber>,
}

/// Where a guest actor's state is.
enum Lives {
    /// In its part's instance, under the handle the guest knows it by.
    In { handle: u32 },
    /// Taken out of the instance it began in, which is another part's, as its path and state written at `Within`:
    /// taken into its own part's instance on its first batch.
    Out { state: Vec<u8> },
}

/// What a guest's batch left to hand back.
#[derive(Clone, Copy)]
enum Ran {
    /// No batch yet.
    Nothing,
    /// Its turns' records lie where `head` says in the guest's memory: the `next` is to be handed back, and the turns
    /// before it end where `before` says.
    At { head: OutHead, next: u32, before: TurnEnds },
    /// The call that ran them trapped, or wrote what does not read back: every turn of the batch fails.
    Trapped,
}

/// The top bit of a seat given to a guest's child, set when the seat is in another part than its parent's.
const ELSEWHERE_BIT: u64 = 1 << 63;

/// The handle a guest writes for a child it did not keep.
const NO_HANDLE: u32 = u32::MAX;

impl Guests {
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

        Self { engine }
    }

    /// Compiles `bytes` as guest code, and reads what it makes: refused when it is not fit to run.
    ///
    /// # Errors
    ///
    /// Why it is not fit.
    pub fn code(&self, bytes: &[u8]) -> Result<Code, Unfit> {
        let module = Module::new(&self.engine, bytes).map_err(|error| Unfit::Uncompiled(error.to_string()))?;
        let section = declared_section(bytes).ok_or(Unfit::Undeclared)?;
        let mut budget = Budget::new(1 << 20);
        let made = from_bytes::<Vec<Made>>(section, &mut budget).map_err(|_| Unfit::Undeclared)?;
        let exports = ["memory", "aethereal_inbox", "aethereal_out", "aethereal_begin", "aethereal_take_in"];
        for export in exports.into_iter().chain(["aethereal_turns"]) {
            let has = module.get_export(export).is_some();
            if !has {
                return Err(Unfit::Unexported(export));
            }
        }
        let module = calls_out(&self.engine)
            .and_then(|linker| linker.instantiate_pre(&module))
            .map_err(|error| Unfit::Unlinked(error.to_string()))?;

        let key = CODES.fetch_add(1, Ordering::Relaxed);

        Ok(Code { key, module, made: made.iter().map(Made::leak).collect() })
    }
}

/// The calls out a guest makes, each answered from what the call under way was lent (ADR-0015).
fn calls_out(engine: &wasmtime::Engine) -> wasmtime::Result<Linker<Lending>> {
    let mut linker = Linker::new(engine);
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
    linker.func_wrap("aethereal", "aethereal_take_seat", |mut caller: Caller<'_, Lending>| {
        answer(&mut caller, |answering| {
            let seat = answering.seats.take();
            answering.handed.push(seat);
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
            let given = answering.handed.iter().position(|handed| *handed == seat);
            if let Some(given) = given {
                answering.handed.remove(given);
                answering.seats.give_back(seat);
            }
        })
    })?;

    Ok(linker)
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

/// The bytes `span` says lie in the memory of the guest that makes a call out: nothing when they lie outside it.
fn guest_bytes(caller: &mut Caller<'_, Lending>, span: Span) -> Option<Vec<u8>> {
    let memory = caller.get_export("memory")?.into_memory()?;

    lying(memory.data(&*caller), span).map(<[u8]>::to_vec)
}

impl<'a> Answering<'a> {
    /// What answers a call's calls out, from what its batch was lent.
    fn of(Reads { table, made, seats }: Reads<'a>) -> Self {
        Self { table, made, seats, handed: Vec::new() }
    }
}

impl Code {
    /// What the `of`th type it makes declares.
    #[must_use]
    pub fn declared(&self, of: usize) -> Option<&'static Declared> {
        self.made.get(of).copied()
    }

    /// Which of the types it makes is the one of id `kind`, and what it declares.
    fn made_of(&self, kind: aethereal_data::KindId) -> Option<(u32, &'static Declared)> {
        let of = self.made.iter().position(|declared| declared.id() == kind)?;

        Some((u32::try_from(of).ok()?, self.made[of]))
    }

    /// Its instance in the part whose `instances` are lent: loaded there by the first of its actors to want it.
    fn instance<'i>(&self, instances: &'i mut Instances) -> &'i mut Instance {
        instances.get_or_load(self.key, || Instance::load(self))
    }
}

impl Instance {
    /// A new instance of `code`.
    ///
    /// # Panics
    ///
    /// Panics when the code cannot be instantiated, which its checks as it entered rule out.
    fn load(code: &Code) -> Self {
        let mut store = Store::new(code.module.module().engine(), Lending::Between);
        let Ok(instance) = code.module.instantiate(&mut store) else {
            panic!("guest code that entered is instantiated");
        };
        let memory = instance.get_memory(&mut store, "memory");
        let inbox = instance.get_typed_func(&mut store, "aethereal_inbox");
        let out = instance.get_typed_func(&mut store, "aethereal_out");
        let begin = instance.get_typed_func(&mut store, "aethereal_begin");
        let take_in = instance.get_typed_func(&mut store, "aethereal_take_in");
        let turns = instance.get_typed_func(&mut store, "aethereal_turns");
        let (Some(memory), Ok(inbox), Ok(out), Ok(begin), Ok(take_in), Ok(turns)) =
            (memory, inbox, out, begin, take_in, turns)
        else {
            panic!("guest code that entered has its exports");
        };

        Self { store, memory, inbox, out, begin, take_in, turns, inbox_at: 0, inbox_room: 0, actors: Vec::new() }
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

/// Makes a root of the `of`th type `code` makes, under `key`, from the bytes of its config, in its part's instance
/// of `code`: `()` for a singleton type, as a native root is keyed.
///
/// # Errors
///
/// Refused as [`Engine::add_root_made`] refuses.
///
/// # Panics
///
/// Panics when `code` makes no `of`th type, or when the type is keyed otherwise than `key` is.
pub fn add_root<K: ActorKey>(
    engine: &mut Engine,
    code: &Code,
    of: usize,
    key: &K,
    config: &[u8],
) -> Result<ActorId, Refused> {
    let Some(declared) = code.declared(of) else {
        panic!("the code makes no type {of}");
    };
    let is_keyed_so = declared.keyed() == K::KEYED;
    assert!(is_keyed_so, "the code's type {of} is keyed as {:?}", declared.keyed());
    let segment = PathSegment::new(declared.name().clone(), key);
    let Ok(naming) = to_bytes(&Naming::of(&segment)) else {
        panic!("a segment's naming is written");
    };
    let handed: Vec<u8> = count(naming.len()).to_le_bytes().into_iter().chain(naming).chain(config.to_vec()).collect();

    engine.add_root_made(&segment, declared, |me, path, table, asked, instances| {
        let held = code.instance(instances);
        let of = u32::try_from(of).unwrap_or(u32::MAX);
        let mut answering = Answering { table, made: &(), seats: asked.seats_mut(), handed: Vec::new() };
        let began = held.begin(of, me, &handed, &mut answering);
        let mut handing = answering.handed;
        let mut hosted = Hosted::beginning(me, path, table, asked, declared);
        let replayed = began.and_then(|(handle, wrote)| {
            let Instance { store, memory, actors, .. } = &mut *held;
            let data = memory.data(&*store);
            let replayed = replay(data, code, &wrote, 0, TurnEnds::default(), &mut handing, actors, &mut hosted)?;
            Some((handle, replayed))
        });
        for seat in handing {
            hosted.give_back(seat);
        }
        let Some((handle, (began, _))) = replayed else {
            // A trap in a `begin` ends every actor in the instance, as one in a turn does (ADR-0015).
            hosted.trapped(held.actors.iter().copied());
            instances.drop_instance::<Instance>(code.key);
            return Err(trapped());
        };
        began?;
        held.actors.push(me);

        let lives = Lives::In { handle };
        Ok(Box::new(Guest { code: code.clone(), me, lives, declared, ran: Ran::Nothing, handed: Vec::new() }))
    })
}

impl Runs for Guest {
    fn declared(&self) -> &'static Declared {
        self.declared
    }

    /// Runs every turn of the batch in the guest, in one call into its instance, its calls out answered from what the
    /// batch was lent, and keeps what they asked for for [`Self::turn`] to hand back. The guest stops after a turn that
    /// fails or closes it, as the engine does. An actor taken out of the instance it began in, another part's, is
    /// first taken into this part's.
    fn prepare(&mut self, preparing: Preparing<'_>) {
        let (batch, instances, reads) = preparing.hosted();
        let held = self.code.instance(instances);
        if let Lives::Out { state } = &self.lives {
            let taken = self.code.made_of(self.declared.id()).and_then(|(of, _)| held.take_in(of, self.me, state));
            let Some(handle) = taken else {
                self.ran = Ran::Trapped;
                return;
            };
            held.actors.push(self.me);
            self.lives = Lives::In { handle };
        }
        let Lives::In { handle } = self.lives else {
            return;
        };
        let (items, len) = held.hand_in(batch);
        let mut answering = Answering::of(reads);
        let Instance { store, turns, .. } = &mut *held;
        let called = lending(store, &mut answering, |store| turns.call(store, (handle, items, len)));
        let wrote = called.ok().and_then(|out| held.out_head(out));
        if wrote.is_none() {
            // Nothing the trapped call stood up will be handed to the engine, so every seat it took goes back now.
            for seat in answering.handed.drain(..) {
                answering.seats.give_back(seat);
            }
        }
        self.handed = answering.handed;
        self.ran = wrote.map_or(Ran::Trapped, |head| Ran::At { head, next: 0, before: TurnEnds::default() });
    }

    /// Hands back the next turn [`Self::prepare`] ran: what it asked for, each send checked against what the guest
    /// declares, and each child it stood up. When the call that ran it trapped, or wrote what does not read back, the
    /// turn fails, every other actor in the instance ends with it, and the part drops the instance (ADR-0015). A turn
    /// that fails gives back the seats the batch took for children it never handed to the engine.
    fn turn(&mut self, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        let role = match &item {
            Item::Mail(mail) if self.declared.lists(mail.kind()) => Role::Kind(mail.kind()),
            Item::Mail(_) => Role::Others,
            Item::Ended(_) => Role::Ended,
        };
        let (mut hosted, instances) = turning.hosted(self.declared, role);
        let replayed = match self.ran {
            Ran::At { head, next, before } => {
                let Instance { store, memory, actors, .. } = self.code.instance(instances);
                let data = memory.data(&*store);
                replay(data, &self.code, &head, next, before, &mut self.handed, actors, &mut hosted)
                    .map(|(replayed, ends)| (replayed, Ran::At { head, next: next + 1, before: ends }))
            }
            Ran::Nothing | Ran::Trapped => None,
        };
        let Some((replayed, ran)) = replayed else {
            self.ran = Ran::Trapped;
            for seat in self.handed.drain(..) {
                hosted.give_back(seat);
            }
            // What the trap left in the instance's memory cannot be trusted, so nothing runs in it again.
            if let Some(held) = instances.get::<Instance>(self.code.key) {
                hosted.trapped(held.actors.iter().copied());
            }
            instances.drop_instance::<Instance>(self.code.key);
            return Err(trapped());
        };
        self.ran = ran;
        let is_last = matches!(ran, Ran::At { head, next, .. } if next == head.ran);
        let is_failed = replayed.is_err();
        let gives_back = !self.handed.is_empty() && (is_failed || is_last);
        if gives_back {
            for seat in self.handed.drain(..) {
                hosted.give_back(seat);
            }
        }

        replayed
    }

    /// Ends the actor in its part's instance, and drops the instance when it was the last actor in it. An instance a
    /// trap dropped is not loaded again for it, and an actor never taken into one leaves none.
    fn end(self: Box<Self>, closing: aethereal_actor::Closing<'_>) -> Result<(), Fault> {
        let instances = closing.hosted();
        let Some(held) = instances.get::<Instance>(self.code.key) else {
            return Ok(());
        };
        held.actors.retain(|actor| *actor != self.me);
        let is_last = held.actors.is_empty();
        if is_last {
            instances.drop_instance::<Instance>(self.code.key);
        }

        Ok(())
    }
}

impl Instance {
    /// Runs the `begin` of the `of`th type the code makes, for `me`, from `handed`, its segment's naming and config,
    /// its calls out answered from `answering`: the handle the guest keeps the actor under, and the head of what it
    /// asked for. Nothing when the call trapped, or wrote what does not read back.
    fn begin(&mut self, of: u32, me: Seated, handed: &[u8], answering: &mut Answering<'_>) -> Option<(u32, OutHead)> {
        let len = self.write_inbox(handed);
        let Self { store, begin, out, .. } = self;
        let handle = lending(store, answering, |store| begin.call(store, (of, me.id.bits(), me.seat.number(), len)));
        let handle = handle.ok()?;
        let out = out.call(&mut *store, ()).ok()?;
        let wrote = self.out_head(out).filter(|wrote| wrote.ran == 1)?;

        Some((handle, wrote))
    }

    /// Takes `me`, an actor of the `of`th type the code makes, into the instance from `state`, its path and state as
    /// the guest that took it out wrote them: the handle the guest keeps it under. Nothing when the call trapped, or
    /// the state did not read back.
    fn take_in(&mut self, of: u32, me: Seated, state: &[u8]) -> Option<u32> {
        let len = self.write_inbox(state);
        let handle = self.take_in.call(&mut self.store, (of, me.id.bits(), me.seat.number(), len)).ok()?;
        let is_taken = handle != NO_HANDLE;

        is_taken.then_some(handle)
    }
    /// Writes the items of `batch` straight into the guest's inbox, each an [`ItemHead`] and then its bytes, and
    /// answers with how many items and how many bytes.
    fn hand_in(&mut self, batch: &dyn Batch) -> (u32, u32) {
        let mut items = 0;
        let mut len = 0;
        batch.each(&mut |peeked| {
            let (handed, bytes) = match &peeked.item {
                Item::Mail(mail) => (Handed::Mail { kind: mail.kind() }, mail.bytes()),
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

    /// The head of what the guest's last call asked for, which `out` says where it lies: nothing when it does not read
    /// back.
    fn out_head(&self, out: u64) -> Option<OutHead> {
        let at = usize::try_from(out >> 32).ok()?;
        let len = usize::try_from(out & u64::from(u32::MAX)).ok()?;

        OutHead::read(self.memory.data(&self.store).get(at..at + len)?)
    }
}

/// Hands the `next` turn of those `head` says the guest ran into `hosted`, read from the guest's memory `data` where
/// it wrote them, the turns before it ending where `before` says: the children it spawned, their seats taken from
/// `handed`, those the host gave the call; its sends, each checked against what its sender declares; its lines; the
/// children that began, each kept in the instance added to `actors` unless the turn failed; and how it ended. Answers
/// with that, and where it ends: nothing when the guest ran no such turn, or its records do not read back.
#[allow(clippy::too_many_arguments, reason = "what a replay reads and what it hands back to, each its own")]
fn replay(
    data: &[u8],
    code: &Code,
    head: &OutHead,
    next: u32,
    before: TurnEnds,
    handed: &mut Vec<SeatNumber>,
    actors: &mut Vec<Seated>,
    hosted: &mut Hosted<'_>,
) -> Option<(Result<(), Fault>, TurnEnds)> {
    let actors_before = actors.len();
    let replayed = replay_records(data, code, head, next, before, handed, actors, hosted);
    // A failed turn's children are dropped by the engine, and never live in the instance; nor do those of a turn whose
    // records do not read back.
    let has_more = actors.len() > actors_before;
    let is_kept = matches!(replayed, Some((Ok(()), _)));
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
    handed: &mut Vec<SeatNumber>,
    actors: &mut Vec<Seated>,
    hosted: &mut Hosted<'_>,
) -> Option<(Result<(), Fault>, TurnEnds)> {
    let was_run = next < head.ran;
    if !was_run {
        return None;
    }
    let ends = TurnEnds::read(record(data, head.turns_at, next, TurnEnds::BYTES)?)?;
    let spawns = ends.spawns > before.spawns;
    if spawns {
        spawn(data, code, head, before, ends, handed, hosted)?;
    }
    for index in before.sends..ends.sends {
        let send = SendRecord::read(record(data, head.sends_at, index, SendRecord::BYTES)?)?;
        let is_refused = hosted.send(send.by, send.to, send.kind, lying(data, send.bytes)?).is_err();
        if is_refused {
            return Some((Err(Fault::new(&FalseSend).unwrap_or_else(|_| trapped())), ends));
        }
    }
    for index in before.lines..ends.lines {
        let line = LineRecord::read(record(data, head.lines_at, index, LineRecord::BYTES)?)?;
        hosted.log(line.by, line.level, str::from_utf8(lying(data, line.text)?).ok()?)?;
    }
    let stands = ends.stood > before.stood;
    if stands {
        stand(data, code, head, before, ends, handed, actors, hosted)?;
    }
    for index in before.watches..ends.watches {
        let watch = WatchRecord::read(record(data, head.watches_at, index, WatchRecord::BYTES)?)?;
        let is_false = hosted.watch(watch.by, watch.of).is_err();
        if is_false {
            return Some((Err(Fault::new(&FalseWatch).unwrap_or_else(|_| trapped())), ends));
        }
    }
    let is_last = next + 1 == head.ran;
    let ended = match head.stopped {
        _ if !is_last => Ok(()),
        Stopped::No => Ok(()),
        // A `begin` cannot close its actor, so a record that says one did does not read back.
        Stopped::Closed if !hosted.may_close() => return None,
        Stopped::Closed => {
            hosted.close();
            Ok(())
        }
        Stopped::Failed { kind, bytes } => Err(Fault::of_parts(kind, lying(data, bytes)?.to_vec())),
    };
    Some((ended, ends))
}

/// Hands `hosted` the spawns a turn recorded, from the first after those `before` ends at to the last `ends` does, each
/// seat taken from `handed`. Nothing when a record does not read back.
fn spawn(
    data: &[u8],
    code: &Code,
    head: &OutHead,
    before: TurnEnds,
    ends: TurnEnds,
    handed: &mut Vec<SeatNumber>,
    hosted: &mut Hosted<'_>,
) -> Option<()> {
    for index in before.spawns..ends.spawns {
        let spawn = SpawnRecord::read(record(data, head.spawns_at, index, SpawnRecord::BYTES)?)?;
        let (_, declared) = code.made_of(spawn.kind)?;
        let key = match spawn.key {
            Keying::Singleton => None,
            Keying::Keyed(key) => Some(from_bytes::<KeyName>(lying(data, key)?, &mut Budget::new(1 << 10)).ok()?),
        };
        let given = handed.iter().position(|seat| *seat == spawn.seat)?;
        handed.remove(given);
        hosted.spawn(spawn.by, declared, key, spawn.seat)?;
    }

    Some(())
}

/// Stands up, in `hosted`, the children that began in a turn, from the first after those `before` ends at to the
/// last `ends` does, each kept in the instance added to `actors`, and the seat of each the guest dropped put back in
/// `handed`. Nothing when a record does not read back.
#[allow(clippy::too_many_arguments, reason = "what a replay reads and what it hands back to, each its own")]
fn stand(
    data: &[u8],
    code: &Code,
    head: &OutHead,
    before: TurnEnds,
    ends: TurnEnds,
    handed: &mut Vec<SeatNumber>,
    actors: &mut Vec<Seated>,
    hosted: &mut Hosted<'_>,
) -> Option<()> {
    for index in before.stood..ends.stood {
        let stood = StoodRecord::read(record(data, head.stood_at, index, StoodRecord::BYTES)?)?;
        let me = stood.me;
        let (by, declared) = hosted.spawned(me)?;
        let is_above = by == stood.above;
        if !is_above {
            return None;
        }
        let lives = match stood.held {
            // A child the guest dropped, as the turn that stood it up failed, is not handed to the engine.
            Kept::Here { handle: NO_HANDLE } => {
                handed.push(me.seat);
                continue;
            }
            Kept::Here { handle } => {
                actors.push(me);
                Lives::In { handle }
            }
            Kept::Out { state } => Lives::Out { state: lying(data, state)?.to_vec() },
        };
        let runs = Guest { code: code.clone(), me, lives, declared, ran: Ran::Nothing, handed: Vec::new() };
        hosted.stand(me, Box::new(runs))?;
    }

    Some(())
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

/// The fault of a turn that trapped.
fn trapped() -> Fault {
    let Ok(fault) = Fault::new(&Trapped) else {
        panic!("a reason that holds nothing is written");
    };

    fault
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
    use aethereal_actor::{Actor, Beginning, Ending, Handles, Seated, ToAny, ToSender, ToWithoutMail, Turn};
    use aethereal_data::{ActorPath, Kind, KindName, PathSegment, Plain};
    use std::num::NonZeroUsize;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// The echo guest the build script built.
    const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

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
                turn.to_any::<(Ping,)>(ToWithoutMail::Held(echo)).send(&Ping { n: volley.pings })?;
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
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(4) else {
            panic!("four is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 64);
        let Ok(_) = add_root(&mut engine, &code, 0, &(), &[]) else {
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
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(echo)).send(&Ping { n: 0 })?;
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(echo)).send(&Shut)?;
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(echo)).send(&Ping { n: 0 })?;
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
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(echo) = add_root(&mut engine, &code, 0, &(), &[]) else {
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
                turn.to_any::<(Ping,)>(ToWithoutMail::Held(echo)).send(&Ping { n: volley.pings })?;
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
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(_) = add_root(&mut engine, &code, 0, &(), &[]) else {
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
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(mirror)).send(&Shut)?;
            }
            if let Some(echo) = turn.resolve(&echo) {
                turn.to_any::<(Ping, Shut)>(ToWithoutMail::Held(echo)).send(&Ping { n: volley.pings })?;
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
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(echo) = add_root(&mut engine, &code, 0, &(), &[]) else {
            panic!("the echo begins");
        };
        let Ok(_) = add_root(&mut engine, &code, 1, &(), &[]) else {
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
                turn.to_any::<(Crash,)>(ToWithoutMail::Held(mirror)).send(&Crash)?;
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
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(echo) = add_root(&mut engine, &code, 0, &(), &[]) else {
            panic!("the echo begins");
        };
        let Ok(mirror) = add_root(&mut engine, &code, 1, &(), &[]) else {
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
                aethereal_substrate::Entry::Failed(fault) => fault.kind() == aethereal_data::KindId::of::<Trapped>(),
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
        let guests = Guests::default();
        let Ok(code) = guests.code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(1) else {
            panic!("one is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(echo) = add_root(&mut engine, &code, 0, &(), &[]) else {
            panic!("the echo begins");
        };
        let Some(seat) = engine.table().seat(echo) else {
            panic!("the echo is seated");
        };
        let echo = Seated { id: echo, seat };

        let doomed = add_root(&mut engine, &code, 3, &(), &[]);
        let clock = Monotonic::new();
        while !engine.is_idle() {
            engine.step(&clock);
        }

        let is_trap = |entry: &aethereal_substrate::Entry| match entry {
            aethereal_substrate::Entry::Failed(fault) => fault.kind() == aethereal_data::KindId::of::<Trapped>(),
            _ => false,
        };
        let refused_trapped =
            matches!(&doomed, Err(Refused::Began(fault)) if fault.kind() == aethereal_data::KindId::of::<Trapped>());
        assert!(refused_trapped, "the doomed root is refused with its trap");
        assert!(engine.log(echo).any(is_trap), "the echo, in the same instance, ended with it");
        assert!(!engine.table().is_live(echo.id));
    }

    /// Which of the echo guest's types a brood is.
    const BROOD: usize = 4;

    /// What a brood is made from, as the echo guest declares it.
    #[derive(Plain)]
    struct BroodConfig {
        children: u64,
    }

    impl Kind for BroodConfig {
        const NAME: KindName = KindName::new("aether.guest.brood_config");
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
    static ANSWERS: [Mutex<Vec<u64>>; 4] = [const { Mutex::new(Vec::new()) }; 4];

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
                    turn.to_any::<(Ping,)>(ToWithoutMail::Held(asked)).send(&Ping { n: 0 })?;
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
                turn.to_any::<(Grow,)>(ToWithoutMail::Held(brood)).send(&Grow)?;
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
    fn brood(code: &Code, parts: usize, children: u64) -> Engine {
        let Some(parts) = NonZeroUsize::new(parts) else {
            panic!("a brood's engine has parts");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        let Ok(config) = to_bytes(&BroodConfig { children }) else {
            panic!("a brood's config is written");
        };
        let Ok(_) = add_root(&mut engine, code, BROOD, &(), &config) else {
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
        let Ok(code) = Guests::default().code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let mut engine = brood(&code, 1, 3);
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
        let Ok(code) = Guests::default().code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let parts = 2;
        let mut engine = brood(&code, parts, 8);
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

    /// Which of the blob's types the seeker is.
    const SEEKER: usize = 6;

    /// What a seeker is made from, as the echo guest declares it.
    #[derive(Plain)]
    struct Seek {
        paths: Vec<ActorPath>,
    }

    impl Kind for Seek {
        const NAME: KindName = KindName::new("aether.guest.seek");
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
        let Ok(code) = Guests::default().code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
        for key in [0, 1] {
            let Ok(_) = engine.add_root::<Keeper>(&key, Keeps { key, tally: 2 }) else {
                panic!("a keeper begins");
            };
        }
        let keeper = |key: u64| ActorPath::from_root(&PathSegment::new(Keeper::NAME, &key));
        let Ok(config) = to_bytes(&Seek { paths: vec![keeper(0), keeper(9), keeper(1)] }) else {
            panic!("a seeker's config is written");
        };
        let Ok(_) = add_root(&mut engine, &code, SEEKER, &(), &config) else {
            panic!("the seeker begins");
        };
        settle(&mut engine);

        assert_eq!(answers(2), [0, 102]);
    }

    /// Which of the blob's types the watcher is.
    const WATCHER: usize = 7;

    /// What a watcher is made from, as the echo guest declares it.
    #[derive(Plain)]
    struct Watching {
        paths: Vec<ActorPath>,
        tell: ActorPath,
    }

    impl Kind for Watching {
        const NAME: KindName = KindName::new("aether.guest.watching");
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
        let Ok(code) = Guests::default().code(ECHO) else {
            panic!("the echo guest is fit to run");
        };
        let Some(parts) = NonZeroUsize::new(2) else {
            panic!("two is not zero");
        };
        let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0);
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
        let Ok(config) = to_bytes(&Watching { paths: vec![mortal(0), mortal(1)], tell }) else {
            panic!("a watcher's config is written");
        };
        let Ok(_) = add_root(&mut engine, &code, WATCHER, &(), &config) else {
            panic!("the watcher begins");
        };
        settle(&mut engine);

        let Ok(_) = engine.add_root::<Asker>(&2, Asks { paths: vec![mortal(0), mortal(1)], tally: 0 }) else {
            panic!("the asker begins");
        };
        settle(&mut engine);

        assert_eq!(answers(3), [1, 12]);
    }
}
