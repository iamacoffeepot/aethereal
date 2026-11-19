//! The spike of keyed sections (`docs/spikes/keyed-sections.md`): a dictionary many senders write to, kept three ways.
//!
//! Run with `cargo run --release -p aethereal-chassis-headless --example keyed_sections [program...]
//! [workers=N,...]`. Each line is the best of three runs on the given numbers of workers (by default 1, 4, and 16),
//! timed from the roots' `begin` to the last step: the puts handled, nanoseconds a put, and the steps the run took.
//!
//! `CLIENTS` senders each put `PUTS` values, `BATCH` a turn, into a dictionary of `KEYS` keys. A put stirs one word of
//! its key's 64-byte entry and counts itself there. Keys are drawn evenly (`-uniform`), or every other put goes to key
//! nought (`-hot`).
//!
//! - `keyed-*`: one actor split into a section for each key, its sections run side by side;
//! - `one-*`: one ordinary actor holding every key's entry;
//! - `children-*`: today's way, `SHARDS` ordinary child actors each holding the keys that fall to it, and each sender
//!   sending to the child of the key.
//!
//! Beside them, `keyed-16keys` and `children-16keys` draw from 16 keys only, so each key's run is long, and
//! `children-4096` keeps a child for each of the 4,096 keys: what a run costs, apart from what a put costs.
//!
//! And what a keyed turn's rollback costs, by its section's size: `rb64-*`, `rb1k-*`, and `rb16k-*`, uniform, a put
//! stirring one word of a section of that size; `-keep` keeps a copy of the section before each turn and compares it
//! with its default after, `-skip` compares and keeps no copy, and `-bare` does neither.

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to two places loses nothing it shows.
#![allow(clippy::cast_possible_truncation)] // A key below `KEYS` is an index.

use aethereal_actor::{
    ActorRef, Beginning, Fault, Item, KeyOf, Root, RunsSections, SectionRunner, Sectioned, Turn, Turning, actor,
    hand_section,
};
use aethereal_chassis_headless::{Headless, Monotonic};
use aethereal_data::{ActorId, Bound, Kind, PathSegment, Plain};
use aethereal_substrate::{Clock, Engine, child_id};
use std::env;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};

/// How many senders put into the dictionary, how many puts each sends, and how many a turn.
const CLIENTS: u64 = 64;
const PUTS: u64 = 4_096;
const BATCH: u64 = 64;
/// How many keys the dictionary holds, and how many children hold them today's way.
const KEYS: u64 = 4_096;
const SHARDS: u64 = 16;
/// How many keys the program running draws from, and how many children it keeps today's way: `KEYS` and `SHARDS`
/// but for the programs that set them otherwise, before they start. Read by every put, never written while one runs.
static DRAWN: AtomicU64 = AtomicU64::new(KEYS);
static CHILDREN: AtomicU64 = AtomicU64::new(SHARDS);
/// How many rounds of mixing a put stirs a word with.
const STIR: u32 = 8;
/// How many words of a 64-byte entry a put may stir: the eighth counts the puts.
const STIRRED: usize = 7;

/// The puts of a whole run.
const fn total() -> u64 {
    CLIENTS * PUTS
}

/// The finaliser of `SplitMix64`.
const fn mix(bits: u64) -> u64 {
    let mut mixed = bits.wrapping_add(0x9e37_79b9_7f4a_7c15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);

    mixed ^ (mixed >> 31)
}

/// The key of the `index`th put of the sender `client`.
fn key_of(client: u64, index: u64, hot: bool) -> u64 {
    let is_hot = hot && index.is_multiple_of(2);
    if is_hot {
        0
    } else {
        mix(client * PUTS + index) % DRAWN.load(Ordering::Relaxed)
    }
}

/// Stirs one of `words`, picked by `value`, with it: the work of a put.
fn stir(words: &mut [u64], value: u64) {
    let at = (value % words.len() as u64) as usize;
    let mut word = words[at] ^ value;
    for _ in 0..STIR {
        word = mix(word);
    }
    words[at] = word;
}

/// A value for a key.
#[derive(Plain)]
#[kind(name = "aether.spike.put")]
struct Put {
    key: u64,
    value: u64,
}

/// The word to send the next batch.
#[derive(Plain)]
#[kind(name = "aether.spike.go")]
struct Go;

/// A put sent without its key, to a keyed actor's whole.
#[derive(Plain)]
#[kind(name = "aether.spike.unkeyed")]
struct Unkeyed;

/// One key's section of `N` words and a count of its puts, `8 * (N + 1)` bytes, flat.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Block<const N: usize> {
    words: [u64; N],
    puts: u64,
}

impl<const N: usize> Default for Block<N> {
    fn default() -> Self {
        Self { words: [0; N], puts: 0 }
    }
}

/// The last keyed dictionary started, whose sections a check reads.
static DICTIONARY: AtomicU64 = AtomicU64::new(0);

/// A keyed dictionary of sections of `$words` words, the root that spawns its senders, and the senders.
macro_rules! keyed {
    ($dictionary:ident, $filler:ident, $filling:ident, $client:ident, $words:literal, $names:tt) => {
        keyed!(@ $dictionary, $filler, $filling, $client, $words, $names);
    };
    (@ $dictionary:ident, $filler:ident, $filling:ident, $client:ident, $words:literal, [$d:literal, $fg:literal, $fr:literal, $c:literal]) => {
        /// A dictionary split into a section for each key.
        #[derive(Plain)]
        #[kind(name = $d)]
        struct $dictionary;

        #[actor(key = u64)]
        impl $dictionary {
            fn begin(_: $dictionary) -> Result<Self, Fault> {
                Ok(Self)
            }

            /// A put sent without its key reaches the whole, which holds no section.
            #[tell]
            fn put(&mut self, _: Put) -> Result<(), Fault> {
                Err(Fault::new(&Unkeyed).unwrap_or_else(|_| unreachable!("a unit encodes")))
            }
        }

        impl KeyOf<Put> for $dictionary {
            fn key(put: &Put) -> u64 {
                put.key
            }
        }

        impl Sectioned for $dictionary {
            type Section = Block<$words>;

            fn section(&self, section: &mut Block<$words>, item: Item<'_>, turning: Turning<'_>) -> Result<u64, Fault> {
                hand_section::<Self, Put>(self, section, item, turning, Self::put_into)
            }
        }

        impl $dictionary {
            #[allow(clippy::unnecessary_wraps, reason = "a section's handler returns a turn's result")]
            #[allow(clippy::trivially_copy_pass_by_ref, reason = "a section's handler reads its actor by reference")]
            fn put_into(&self, block: &mut Block<$words>, put: Put, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
                stir(&mut block.words, put.value);
                block.puts += 1;

                Ok(())
            }
        }

        /// What the root of senders is made from: the dictionary, and whether its keys are hot.
        #[derive(Bound)]
        #[kind(name = $fg)]
        struct $filling {
            to: ActorRef<$dictionary>,
            number: u64,
            hot: bool,
        }

        /// The root of the senders.
        #[derive(Plain)]
        #[kind(name = $fr)]
        struct $filler;

        #[actor(key = u64, spawns($client))]
        impl $filler {
            fn begin(filling: $filling, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
                for number in 0..CLIENTS {
                    let config = $filling { number, ..filling };
                    let Ok(client) = turn.spawn::<$client>(&number, config) else {
                        panic!("a sender begins");
                    };
                    turn.to(client).send(&Go)?;
                }

                Ok(Self)
            }
        }

        /// A sender: on each `Go` it puts a batch, and tells itself `Go` again until it has put all it puts.
        #[derive(Bound)]
        #[kind(name = $c)]
        struct $client {
            to: ActorRef<$dictionary>,
            number: u64,
            hot: bool,
            sent: u64,
        }

        #[actor(key = u64, parent = $filler)]
        impl $client {
            fn begin(filling: $filling) -> Result<Self, Fault> {
                Ok(Self { to: filling.to, number: filling.number, hot: filling.hot, sent: 0 })
            }

            #[tell]
            fn go(&mut self, _: Go, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
                let end = (self.sent + BATCH).min(PUTS);
                for index in self.sent..end {
                    let put = Put { key: key_of(self.number, index, self.hot), value: (self.number << 32) | index };
                    turn.to(self.to).send_keyed(&put)?;
                }
                self.sent = end;
                if self.sent < PUTS {
                    turn.to(turn.me()).send(&Go)?;
                }

                Ok(())
            }
        }
    };
}

keyed!(
    Keyed64,
    KeyedFill64,
    KeyedFilling64,
    KeyedClient64,
    7,
    ["aether.spike.dictionary_64", "aether.spike.filling_64", "aether.spike.filler_64", "aether.spike.client_64"]
);
keyed!(
    Keyed1k,
    KeyedFill1k,
    KeyedFilling1k,
    KeyedClient1k,
    127,
    ["aether.spike.dictionary_1k", "aether.spike.filling_1k", "aether.spike.filler_1k", "aether.spike.client_1k"]
);
keyed!(
    Keyed16k,
    KeyedFill16k,
    KeyedFilling16k,
    KeyedClient16k,
    2047,
    ["aether.spike.dictionary_16k", "aether.spike.filling_16k", "aether.spike.filler_16k", "aether.spike.client_16k"]
);

/// Starts a keyed dictionary of type `D`, run by `runs`, and its senders, of type `F`, for the run `round`.
fn start_keyed<D, F>(
    engine: &mut Engine,
    round: u64,
    (config, runs): (D::Config, Box<dyn RunsSections>),
    filling: impl FnOnce(ActorRef<D>) -> F::Config,
) where
    D: Root + Sectioned + aethereal_actor::Actor<Key = u64>,
    F: Root + aethereal_actor::Actor<Key = u64>,
{
    let Ok(to) = engine.add_sectioned_root::<D>(&round, config, runs) else {
        panic!("the dictionary begins");
    };
    DICTIONARY.store(to.id().bits(), Ordering::Relaxed);
    let Ok(_) = engine.add_root::<F>(&round, filling(to)) else {
        panic!("the senders begin");
    };
}

/// Stops the benchmark unless the keyed dictionary of sections `S` took every put.
fn check_keyed<const N: usize>(engine: &Engine) {
    let dictionary = ActorId::new(DICTIONARY.load(Ordering::Relaxed));
    let puts: u64 =
        (0..KEYS).filter_map(|key| engine.section::<Block<N>>(dictionary, key)).map(|block| block.puts).sum();
    assert_eq!(puts, total(), "every put reaches its section");
}

/// A dictionary that is one ordinary actor, holding every key's entry, 64 bytes each.
#[derive(Plain)]
#[kind(name = "aether.spike.whole")]
struct Whole {
    entries: Vec<u64>,
}

#[actor(key = u64)]
impl Whole {
    fn begin(_: Unkeyed) -> Result<Self, Fault> {
        Ok(Self { entries: vec![0; (KEYS * 8) as usize] })
    }

    #[tell]
    fn put(&mut self, put: Put) -> Result<(), Fault> {
        let at = (put.key * 8) as usize;
        stir(&mut self.entries[at..at + STIRRED], put.value);
        self.entries[at + STIRRED] += 1;

        Ok(())
    }
}

/// One of today's children, holding the keys that fall to it, each key at its index among them.
#[derive(Plain)]
#[kind(name = "aether.spike.shard")]
struct Shard {
    entries: Vec<u64>,
}

#[actor(key = u64, parent = Sharded)]
impl Shard {
    fn begin(_: Unkeyed) -> Result<Self, Fault> {
        Ok(Self { entries: vec![0; (KEYS.div_ceil(CHILDREN.load(Ordering::Relaxed)) * 8) as usize] })
    }

    #[tell]
    fn put(&mut self, put: Put) -> Result<(), Fault> {
        let at = (put.key / CHILDREN.load(Ordering::Relaxed) * 8) as usize;
        stir(&mut self.entries[at..at + STIRRED], put.value);
        self.entries[at + STIRRED] += 1;

        Ok(())
    }
}

/// What a sender of the other two ways is made from: where its puts go, whether its keys are hot, and its number.
#[derive(Bound)]
#[kind(name = "aether.spike.sending")]
struct Sending {
    whole: Vec<ActorRef<Whole>>,
    shards: Vec<ActorRef<Shard>>,
    number: u64,
    hot: bool,
}

/// The root of the other two ways: it spawns the children, when they are the way, and the senders.
#[derive(Plain)]
#[kind(name = "aether.spike.sharded")]
struct Sharded;

/// What that root is made from: the one actor, when it is the way, and whether keys are hot.
#[derive(Bound)]
#[kind(name = "aether.spike.sharding")]
struct Sharding {
    whole: Vec<ActorRef<Whole>>,
    hot: bool,
}

#[actor(key = u64, spawns(Shard, Sender))]
impl Sharded {
    fn begin(sharding: Sharding, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let shards = if sharding.whole.is_empty() {
            let spawned = (0..CHILDREN.load(Ordering::Relaxed)).map(|number| turn.spawn::<Shard>(&number, Unkeyed));
            spawned.collect::<Result<Vec<_>, _>>().unwrap_or_else(|_| panic!("a child begins"))
        } else {
            Vec::new()
        };
        for number in 0..CLIENTS {
            let sending = Sending { whole: sharding.whole.clone(), shards: shards.clone(), number, hot: sharding.hot };
            let Ok(sender) = turn.spawn::<Sender>(&number, sending) else {
                panic!("a sender begins");
            };
            turn.to(sender).send(&Go)?;
        }

        Ok(Self)
    }
}

/// A sender of the other two ways, as the keyed sender is.
#[derive(Bound)]
#[kind(name = "aether.spike.sender")]
struct Sender {
    sending: Sending,
    sent: u64,
}

#[actor(key = u64, parent = Sharded)]
impl Sender {
    fn begin(sending: Sending) -> Result<Self, Fault> {
        Ok(Self { sending, sent: 0 })
    }

    #[tell]
    fn go(&mut self, _: Go, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let end = (self.sent + BATCH).min(PUTS);
        for index in self.sent..end {
            let key = key_of(self.sending.number, index, self.sending.hot);
            let put = Put { key, value: (self.sending.number << 32) | index };
            if let Some(whole) = self.sending.whole.first() {
                turn.to(*whole).send(&put)?;
            } else {
                turn.to(self.sending.shards[(key % self.sending.shards.len() as u64) as usize]).send(&put)?;
            }
        }
        self.sent = end;
        if self.sent < PUTS {
            turn.to(turn.me()).send(&Go)?;
        }

        Ok(())
    }
}

/// The last ordinary dictionary or root of children started, whose state a check reads.
static ORDINARY: AtomicU64 = AtomicU64::new(0);

/// Starts the one-actor way for the run `round`.
fn start_one(engine: &mut Engine, round: u64, hot: bool) {
    DRAWN.store(KEYS, Ordering::Relaxed);
    let Ok(id) = engine.add_root::<Whole>(&round, Unkeyed) else {
        panic!("the dictionary begins");
    };
    let Some(seat) = engine.table().seat(id) else {
        panic!("the dictionary is seated");
    };
    let whole = ActorRef::found(aethereal_actor::Seated { id, seat });
    ORDINARY.store(id.bits(), Ordering::Relaxed);
    let Ok(_) = engine.add_root::<Sharded>(&round, Sharding { whole: vec![whole], hot }) else {
        panic!("the senders begin");
    };
}

/// Starts today's way, children, for the run `round`.
fn start_children(engine: &mut Engine, round: u64, hot: bool) {
    start_children_of(engine, round, hot, (KEYS, SHARDS));
}

/// Starts today's way for the run `round`, drawing from and keeping as many keys and children as `sizes` says.
fn start_children_of(engine: &mut Engine, round: u64, hot: bool, (keys, children): (u64, u64)) {
    DRAWN.store(keys, Ordering::Relaxed);
    CHILDREN.store(children, Ordering::Relaxed);
    let Ok(root) = engine.add_root::<Sharded>(&round, Sharding { whole: Vec::new(), hot }) else {
        panic!("the children and senders begin");
    };
    ORDINARY.store(root.bits(), Ordering::Relaxed);
}

/// The puts counted in `entries`, an entry of 8 words each.
fn counted(entries: &[u64]) -> u64 {
    entries.chunks(8).map(|entry| entry[STIRRED]).sum()
}

/// Stops the benchmark unless the one actor took every put.
fn check_one(engine: &Engine) {
    let id = ActorId::new(ORDINARY.load(Ordering::Relaxed));
    let whole = engine.table().seat(id).and_then(|seat| engine.state::<Whole>(aethereal_actor::Seated { id, seat }));
    let puts = whole.map_or(0, |whole| counted(&whole.entries));
    assert_eq!(puts, total(), "every put reaches the dictionary");
}

/// Stops the benchmark unless the children took every put.
fn check_children(engine: &Engine) {
    let root = ActorId::new(ORDINARY.load(Ordering::Relaxed));
    let puts: u64 = (0..CHILDREN.load(Ordering::Relaxed))
        .map(|number| child_id(root, &PathSegment::new(Shard::NAME, &number)))
        .filter_map(|id| engine.table().seat(id).map(|seat| aethereal_actor::Seated { id, seat }))
        .filter_map(|seated| engine.state::<Shard>(seated))
        .map(|shard| counted(&shard.entries))
        .sum();
    assert_eq!(puts, total(), "every put reaches its child");
}

/// A program of the spike: its name, what starts it for a run, and what checks it took every put.
struct Program {
    name: &'static str,
    start: fn(&mut Engine, u64),
    check: fn(&Engine),
}

/// A keyed program of sections `$block`, whose runner keeps and drops as `$keeps` and `$drops` say.
macro_rules! keyed_program {
    ($name:literal, $dictionary:ident, $filler:ident, $filling:ident, $keeps:literal, $drops:literal, $hot:literal, $words:literal) => {
        Program {
            name: $name,
            start: |engine, round| {
                DRAWN.store(KEYS, Ordering::Relaxed);
                let runs = Box::new(SectionRunner::<$dictionary, $keeps, $drops>::new($dictionary));
                let filling = |to| $filling { to, number: 0, hot: $hot };
                start_keyed::<$dictionary, $filler>(engine, round, ($dictionary, runs), filling);
            },
            check: check_keyed::<$words>,
        }
    };
}

const PROGRAMS: [Program; 18] = [
    keyed_program!("keyed-uniform", Keyed64, KeyedFill64, KeyedFilling64, true, true, false, 7),
    keyed_program!("keyed-hot", Keyed64, KeyedFill64, KeyedFilling64, true, true, true, 7),
    Program { name: "one-uniform", start: |engine, round| start_one(engine, round, false), check: check_one },
    Program { name: "one-hot", start: |engine, round| start_one(engine, round, true), check: check_one },
    Program {
        name: "children-uniform",
        start: |engine, round| start_children(engine, round, false),
        check: check_children,
    },
    Program { name: "children-hot", start: |engine, round| start_children(engine, round, true), check: check_children },
    Program {
        name: "keyed-16keys",
        start: |engine, round| {
            DRAWN.store(SHARDS, Ordering::Relaxed);
            let runs = Box::new(SectionRunner::<Keyed64, true, true>::new(Keyed64));
            let filling = |to| KeyedFilling64 { to, number: 0, hot: false };
            start_keyed::<Keyed64, KeyedFill64>(engine, round, (Keyed64, runs), filling);
        },
        check: check_keyed::<7>,
    },
    Program {
        name: "children-16keys",
        start: |engine, round| start_children_of(engine, round, false, (SHARDS, SHARDS)),
        check: check_children,
    },
    Program {
        name: "children-4096",
        start: |engine, round| start_children_of(engine, round, false, (KEYS, KEYS)),
        check: check_children,
    },
    keyed_program!("rb64-keep", Keyed64, KeyedFill64, KeyedFilling64, true, true, false, 7),
    keyed_program!("rb64-skip", Keyed64, KeyedFill64, KeyedFilling64, false, true, false, 7),
    keyed_program!("rb64-bare", Keyed64, KeyedFill64, KeyedFilling64, false, false, false, 7),
    keyed_program!("rb1k-keep", Keyed1k, KeyedFill1k, KeyedFilling1k, true, true, false, 127),
    keyed_program!("rb1k-skip", Keyed1k, KeyedFill1k, KeyedFilling1k, false, true, false, 127),
    keyed_program!("rb1k-bare", Keyed1k, KeyedFill1k, KeyedFilling1k, false, false, false, 127),
    keyed_program!("rb16k-keep", Keyed16k, KeyedFill16k, KeyedFilling16k, true, true, false, 2047),
    keyed_program!("rb16k-skip", Keyed16k, KeyedFill16k, KeyedFilling16k, false, true, false, 2047),
    keyed_program!("rb16k-bare", Keyed16k, KeyedFill16k, KeyedFilling16k, false, false, false, 2047),
];

/// One run of `program` on `workers` workers: the nanoseconds it took, from its roots' `begin` to the last step, and
/// the steps. The host is made before the clock starts.
fn run(program: &Program, workers: NonZeroUsize) -> (u64, u64) {
    let parts = NonZeroUsize::MIN.saturating_add(15);
    let engine = Engine::new(parts, 1024, 1 << 30, 1 << 30, 0).ready();
    let mut headless = Headless::new(engine, workers);
    let clock = Monotonic::new();
    let began = clock.nanos();
    (program.start)(headless.engine_mut(), 0);
    headless.run_until_idle();
    let took = clock.nanos() - began;
    (program.check)(headless.engine());

    (took, headless.measured().steps)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let named: Vec<&str> = args.iter().filter(|arg| !arg.contains('=')).map(String::as_str).collect();
    let workers: Vec<usize> = args
        .iter()
        .find_map(|arg| arg.strip_prefix("workers="))
        .map_or_else(|| vec![1, 4, 16], |given| given.split(',').filter_map(|count| count.parse().ok()).collect());

    println!("program            workers      puts   M puts/s  ns/put    steps");
    for program in &PROGRAMS {
        let is_skipped = !named.is_empty() && !named.contains(&program.name);
        if is_skipped {
            continue;
        }
        for &count in &workers {
            let workers = NonZeroUsize::MIN.saturating_add(count.max(1) - 1);
            let runs = (0..3).map(|_| run(program, workers));
            let (took, steps) = runs.min_by_key(|(took, _)| *took).unwrap_or_default();
            println!(
                "{:18} {workers:7} {:9}   {:8.2} {:7} {steps:8}",
                program.name,
                total(),
                total() as f64 * 1e3 / took.max(1) as f64,
                took / total().max(1),
            );
        }
    }
}
