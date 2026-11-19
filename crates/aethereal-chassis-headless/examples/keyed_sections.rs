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
//! And what dropping a section at its default costs, by its section's size: `rb64-*`, `rb1k-*`, and `rb16k-*`,
//! uniform, a put stirring one word of a section of that size; `-skip` compares the section with its default after
//! each run, and `-bare` does not. (The first spike's `-keep`, a copy kept to put back on a fault, is gone: a fault
//! ends the actor.)
//!
//! And requests: `ask-*`, where each sender asks `BATCH` questions of the dictionary at once, each stirring its key's
//! entry as a put does and answered at once, and asks the next batch once all are answered; and `hold-*`, where the
//! dictionary holds each question, asks a stamper actor for the value to stir with, and stirs and answers the question
//! it held when the stamp comes back. One is a round trip through the dictionary; the other is two, the second asked
//! from it, with a held request between. `-keyed` is one actor split into a section for each key, whose sections
//! answer, hold, and ask.

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to two places loses nothing it shows.
#![allow(clippy::cast_possible_truncation)] // A key below `KEYS` is an index.

use aethereal_actor::{
    ActorRef, Answer, Answered, Beginning, Fault, Held, Item, KeyOf, Later, Root, RunsSections, SectionRunner,
    Sectioned, Turn, Turning, actor, hand_section, is_of,
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
            type From = Put;

            fn key(put: &Put) -> u64 {
                put.key
            }
        }

        impl Sectioned for $dictionary {
            type Section = Block<$words>;

            fn section(&self, section: &mut Block<$words>, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
                hand_section::<Self, Put, ()>(self, section, item, turning, Self::put_into)
            }

            fn is_unused(section: &Block<$words>) -> bool {
                *section == Block::default()
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

/// A question for a key: the dictionary stirs the key's entry with `value`, as a put does, and answers at once.
#[derive(Plain)]
#[kind(name = "aether.spike.get")]
struct Get {
    key: u64,
    value: u64,
}

/// The answer to a [`Get`]: how many puts the key has taken.
#[derive(Plain)]
#[kind(name = "aether.spike.got")]
struct Got {
    puts: u64,
}

/// What an asker keeps for each question it waits on: nothing.
#[derive(Plain)]
#[kind(name = "aether.spike.pending")]
struct Pending;

/// A question for a key that the dictionary holds, to stir with what a stamper answers and answer then.
#[derive(Plain)]
#[kind(name = "aether.spike.fetch")]
struct Fetch {
    key: u64,
    value: u64,
}

/// What a dictionary asks the stamper for each question it holds: the value it was asked with.
#[derive(Plain)]
#[kind(name = "aether.spike.stamp")]
struct Stamp {
    value: u64,
}

/// The stamper's answer: the value to stir with.
#[derive(Plain)]
#[kind(name = "aether.spike.stamped")]
struct Stamped {
    value: u64,
}

/// What a dictionary keeps for each stamp it waits on: the key it is for, and the question it holds.
#[derive(Bound)]
#[kind(name = "aether.spike.stamping")]
struct Stamping {
    key: u64,
    held: Held<Got>,
}

/// Why a round trip of the benchmark went wrong: an answer did not come.
#[derive(Plain)]
#[kind(name = "aether.spike.unanswered")]
struct NoAnswer;

/// The fault of an answer that did not come.
fn no_answer() -> Fault {
    Fault::new(&NoAnswer).unwrap_or_else(|_| unreachable!("a unit encodes"))
}

/// An actor that answers each stamp at once, with its value mixed.
#[derive(Plain)]
#[kind(name = "aether.spike.stamper")]
struct Stamper;

#[actor(key = u64)]
impl Stamper {
    fn begin(_: Unkeyed) -> Result<Self, Fault> {
        Ok(Self)
    }

    #[request]
    fn stamp(&mut self, stamp: Stamp) -> Result<Stamped, Fault> {
        Ok(Stamped { value: mix(stamp.value) })
    }
}

/// What an ordinary dictionary or child is made from: the stamper it asks for what it holds, when it holds.
#[derive(Bound)]
#[kind(name = "aether.spike.store")]
struct Store {
    stamper: Vec<ActorRef<Stamper>>,
}

/// A dictionary that is one ordinary actor, holding every key's entry, 64 bytes each.
#[derive(Bound)]
#[kind(name = "aether.spike.whole")]
struct Whole {
    entries: Vec<u64>,
    stamper: Vec<ActorRef<Stamper>>,
}

#[actor(key = u64)]
impl Whole {
    fn begin(store: Store) -> Result<Self, Fault> {
        Ok(Self { entries: vec![0; (KEYS * 8) as usize], stamper: store.stamper })
    }

    #[tell]
    fn put(&mut self, put: Put) -> Result<(), Fault> {
        self.stir(put.key, put.value);

        Ok(())
    }

    #[request]
    fn get(&mut self, get: Get) -> Result<Got, Fault> {
        Ok(Got { puts: self.stir(get.key, get.value) })
    }

    #[request]
    fn fetch(&mut self, fetch: Fetch, turn: &mut Turn<'_, Self>) -> Result<Later<Got>, Fault> {
        let Some(stamper) = self.stamper.first().copied() else {
            return Err(no_answer());
        };
        let (held, later) = turn.hold();
        turn.to(stamper).ask(&Stamp { value: fetch.value }, &Stamping { key: fetch.key, held })?;

        Ok(later)
    }

    #[answer]
    fn stamped(
        &mut self,
        (answered, stamping): (Answered<Stamped>, Stamping),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Answered::Yes(stamped) = answered else {
            return Err(no_answer());
        };
        let puts = self.stir(stamping.key, stamped.value);
        turn.answer(stamping.held, &Got { puts })?;

        Ok(())
    }
}

impl Whole {
    /// Stirs the entry of `key` with `value` and counts it: what a put does. Answers how many it has taken.
    fn stir(&mut self, key: u64, value: u64) -> u64 {
        let at = (key * 8) as usize;
        stir(&mut self.entries[at..at + STIRRED], value);
        self.entries[at + STIRRED] += 1;

        self.entries[at + STIRRED]
    }
}

/// One of today's children, holding the keys that fall to it, each key at its index among them.
#[derive(Bound)]
#[kind(name = "aether.spike.shard")]
struct Shard {
    entries: Vec<u64>,
    stamper: Vec<ActorRef<Stamper>>,
}

#[actor(key = u64, parent = Sharded)]
impl Shard {
    fn begin(store: Store) -> Result<Self, Fault> {
        let entries = vec![0; (KEYS.div_ceil(CHILDREN.load(Ordering::Relaxed)) * 8) as usize];

        Ok(Self { entries, stamper: store.stamper })
    }

    #[tell]
    fn put(&mut self, put: Put) -> Result<(), Fault> {
        self.stir(put.key, put.value);

        Ok(())
    }

    #[request]
    fn get(&mut self, get: Get) -> Result<Got, Fault> {
        Ok(Got { puts: self.stir(get.key, get.value) })
    }

    #[request]
    fn fetch(&mut self, fetch: Fetch, turn: &mut Turn<'_, Self>) -> Result<Later<Got>, Fault> {
        let Some(stamper) = self.stamper.first().copied() else {
            return Err(no_answer());
        };
        let (held, later) = turn.hold();
        turn.to(stamper).ask(&Stamp { value: fetch.value }, &Stamping { key: fetch.key, held })?;

        Ok(later)
    }

    #[answer]
    fn stamped(
        &mut self,
        (answered, stamping): (Answered<Stamped>, Stamping),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Answered::Yes(stamped) = answered else {
            return Err(no_answer());
        };
        let puts = self.stir(stamping.key, stamped.value);
        turn.answer(stamping.held, &Got { puts })?;

        Ok(())
    }
}

impl Shard {
    /// Stirs the entry of `key` with `value` and counts it: what a put does. Answers how many it has taken.
    fn stir(&mut self, key: u64, value: u64) -> u64 {
        let at = (key / CHILDREN.load(Ordering::Relaxed) * 8) as usize;
        stir(&mut self.entries[at..at + STIRRED], value);
        self.entries[at + STIRRED] += 1;

        self.entries[at + STIRRED]
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

/// What the senders of a run do: put, ask what is answered at once, or ask what is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Load {
    Puts,
    Asks,
    Holds,
}

/// The root of the other two ways: it spawns the children, when they are the way, and the senders.
#[derive(Plain)]
#[kind(name = "aether.spike.sharded")]
struct Sharded;

/// What that root is made from: the one actor, when it is the way, whether keys are hot, what the senders do, and the
/// stamper the children ask for what they hold, when they hold.
#[derive(Bound)]
#[kind(name = "aether.spike.sharding")]
struct Sharding {
    whole: Vec<ActorRef<Whole>>,
    hot: bool,
    load: Load,
    stamper: Vec<ActorRef<Stamper>>,
}

#[actor(key = u64, spawns(Shard, Sender, Asker))]
impl Sharded {
    fn begin(sharding: Sharding, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let shards = if sharding.whole.is_empty() {
            let spawned = (0..CHILDREN.load(Ordering::Relaxed))
                .map(|number| turn.spawn::<Shard>(&number, Store { stamper: sharding.stamper.clone() }));
            spawned.collect::<Result<Vec<_>, _>>().unwrap_or_else(|_| panic!("a child begins"))
        } else {
            Vec::new()
        };
        for number in 0..CLIENTS {
            let sending = Sending { whole: sharding.whole.clone(), shards: shards.clone(), number, hot: sharding.hot };
            match sharding.load {
                Load::Puts => {
                    let Ok(sender) = turn.spawn::<Sender>(&number, sending) else {
                        panic!("a sender begins");
                    };
                    turn.to(sender).send(&Go)?;
                }
                Load::Asks | Load::Holds => {
                    let asking = Asking { sending, holds: sharding.load == Load::Holds };
                    let Ok(asker) = turn.spawn::<Asker>(&number, asking) else {
                        panic!("an asker begins");
                    };
                    turn.to(asker).send(&Go)?;
                }
            }
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

/// What an asker is made from: where its questions go, and whether they are held.
#[derive(Bound)]
#[kind(name = "aether.spike.asking")]
struct Asking {
    sending: Sending,
    holds: bool,
}

/// An asker of the other two ways: it asks a batch of questions at once, and the next batch once all are answered.
#[derive(Bound)]
#[kind(name = "aether.spike.asker")]
struct Asker {
    sending: Sending,
    holds: bool,
    asked: u64,
    answered: u64,
}

#[actor(key = u64, parent = Sharded)]
impl Asker {
    fn begin(asking: Asking) -> Result<Self, Fault> {
        Ok(Self { sending: asking.sending, holds: asking.holds, asked: 0, answered: 0 })
    }

    #[tell]
    fn go(&mut self, _: Go, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.ask_batch(turn)
    }

    #[answer]
    fn got(&mut self, (answered, _): (Answered<Got>, Pending), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Answered::Yes(_) = answered else {
            return Err(no_answer());
        };
        self.answered += 1;
        let is_batch_done = self.answered == self.asked;
        let is_more = self.asked < PUTS;
        if is_batch_done && is_more {
            self.ask_batch(turn)?;
        }

        Ok(())
    }
}

impl Asker {
    /// Asks the next batch of questions.
    fn ask_batch(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let end = (self.asked + BATCH).min(PUTS);
        for index in self.asked..end {
            let key = key_of(self.sending.number, index, self.sending.hot);
            let value = (self.sending.number << 32) | index;
            match (self.sending.whole.first(), self.holds) {
                (Some(whole), false) => {
                    turn.to(*whole).ask(&Get { key, value }, &Pending)?;
                }
                (Some(whole), true) => {
                    turn.to(*whole).ask(&Fetch { key, value }, &Pending)?;
                }
                (None, false) => {
                    let shard = self.sending.shards[(key % self.sending.shards.len() as u64) as usize];
                    turn.to(shard).ask(&Get { key, value }, &Pending)?;
                }
                (None, true) => {
                    let shard = self.sending.shards[(key % self.sending.shards.len() as u64) as usize];
                    turn.to(shard).ask(&Fetch { key, value }, &Pending)?;
                }
            }
        }
        self.asked = end;

        Ok(())
    }
}

/// The last ordinary dictionary or root of children started, whose state a check reads.
static ORDINARY: AtomicU64 = AtomicU64::new(0);

/// The reference to the root `id` of type `T`, which `engine` has just seated.
fn found<T>(engine: &Engine, id: ActorId) -> ActorRef<T> {
    let Some(seat) = engine.table().seat(id) else {
        panic!("a root is seated");
    };

    ActorRef::found(aethereal_actor::Seated { id, seat })
}

/// A stamper for the run `round`, in a list, when the dictionary holds what it is asked: an empty list when it does not.
fn stamper(engine: &mut Engine, round: u64, load: Load) -> Vec<ActorRef<Stamper>> {
    if load != Load::Holds {
        return Vec::new();
    }
    let Ok(id) = engine.add_root::<Stamper>(&round, Unkeyed) else {
        panic!("the stamper begins");
    };

    vec![found(engine, id)]
}

/// How a run of the other two ways goes: whether its keys are hot, and what its senders do.
#[derive(Clone, Copy)]
struct Way {
    hot: bool,
    load: Load,
}

/// Puts, uniform or hot.
const fn puts(hot: bool) -> Way {
    Way { hot, load: Load::Puts }
}

/// Starts the one-actor way for the run `round`.
fn start_one(engine: &mut Engine, round: u64, way: Way) {
    DRAWN.store(KEYS, Ordering::Relaxed);
    let stamper = stamper(engine, round, way.load);
    let Ok(id) = engine.add_root::<Whole>(&round, Store { stamper }) else {
        panic!("the dictionary begins");
    };
    let whole = found(engine, id);
    ORDINARY.store(id.bits(), Ordering::Relaxed);
    let sharding = Sharding { whole: vec![whole], hot: way.hot, load: way.load, stamper: Vec::new() };
    let Ok(_) = engine.add_root::<Sharded>(&round, sharding) else {
        panic!("the senders begin");
    };
}

/// Starts today's way, children, for the run `round`.
fn start_children(engine: &mut Engine, round: u64, way: Way) {
    start_children_of(engine, round, way, (KEYS, SHARDS));
}

/// Starts today's way for the run `round`, drawing from and keeping as many keys and children as `sizes` says.
fn start_children_of(engine: &mut Engine, round: u64, way: Way, (keys, children): (u64, u64)) {
    DRAWN.store(keys, Ordering::Relaxed);
    CHILDREN.store(children, Ordering::Relaxed);
    let stamper = stamper(engine, round, way.load);
    let sharding = Sharding { whole: Vec::new(), hot: way.hot, load: way.load, stamper };
    let Ok(root) = engine.add_root::<Sharded>(&round, sharding) else {
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

/// A dictionary split into a section for each key, whose sections answer questions, or hold them and ask the stamper.
#[derive(Bound)]
#[kind(name = "aether.spike.asked")]
struct Asked64 {
    stamper: Vec<ActorRef<Stamper>>,
}

/// A whole-actor handler of a keyed actor: what reaches it came without its key, and is refused.
fn unkeyed<T>() -> Result<T, Fault> {
    Err(Fault::new(&Unkeyed).unwrap_or_else(|_| unreachable!("a unit encodes")))
}

#[actor(key = u64)]
impl Asked64 {
    fn begin(asked: Asked64) -> Result<Self, Fault> {
        Ok(asked)
    }

    #[request]
    fn get(&mut self, _: Get) -> Result<Got, Fault> {
        unkeyed()
    }

    #[request]
    fn fetch(&mut self, _: Fetch) -> Result<Got, Fault> {
        unkeyed()
    }

    #[answer]
    fn stamped(&mut self, _: (Answered<Stamped>, Stamping)) -> Result<(), Fault> {
        unkeyed()
    }
}

impl KeyOf<Get> for Asked64 {
    type From = Get;

    fn key(get: &Get) -> u64 {
        get.key
    }
}

impl KeyOf<Fetch> for Asked64 {
    type From = Fetch;

    fn key(fetch: &Fetch) -> u64 {
        fetch.key
    }
}

/// A stamp's answer goes back to the section of the key its context names.
impl KeyOf<Answer<Stamped, Stamping>> for Asked64 {
    type From = Stamping;

    fn key(stamping: &Stamping) -> u64 {
        stamping.key
    }
}

impl Sectioned for Asked64 {
    type Section = Block<7>;

    fn section(&self, block: &mut Block<7>, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        if is_of::<Get>(&item) {
            return hand_section::<Self, Get, Got>(self, block, item, turning, Self::get_from);
        }
        if is_of::<Fetch>(&item) {
            return hand_section::<Self, Fetch, Later<Got>>(self, block, item, turning, Self::fetch_into);
        }
        hand_section::<Self, Answer<Stamped, Stamping>, ()>(self, block, item, turning, Self::stamped_into)
    }

    fn is_unused(block: &Block<7>) -> bool {
        *block == Block::default()
    }
}

#[allow(clippy::unnecessary_wraps, reason = "a section's handler returns a turn's result")]
#[allow(clippy::needless_pass_by_value, reason = "a handler is handed its mail by value")]
#[allow(clippy::unused_self, reason = "a section's handler is handed its actor to read, whether it reads it or not")]
impl Asked64 {
    /// Stirs the section and answers at once.
    fn get_from(&self, block: &mut Block<7>, get: Get, _: &mut Turn<'_, Self>) -> Result<Got, Fault> {
        stir(&mut block.words, get.value);
        block.puts += 1;

        Ok(Got { puts: block.puts })
    }

    /// Holds the question, and asks the stamper what to stir with: the answer comes back to this section.
    fn fetch_into(&self, _: &mut Block<7>, fetch: Fetch, turn: &mut Turn<'_, Self>) -> Result<Later<Got>, Fault> {
        let Some(stamper) = self.stamper.first().copied() else {
            return Err(no_answer());
        };
        let (held, later) = turn.hold();
        turn.to(stamper).ask_returning(&Stamp { value: fetch.value }, &Stamping { key: fetch.key, held })?;

        Ok(later)
    }

    /// Stirs the section with what the stamper answered, and answers the question it held.
    fn stamped_into(
        &self,
        block: &mut Block<7>,
        (answered, stamping): (Answered<Stamped>, Stamping),
        turn: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        let Answered::Yes(stamped) = answered else {
            return Err(no_answer());
        };
        stir(&mut block.words, stamped.value);
        block.puts += 1;
        turn.answer(stamping.held, &Got { puts: block.puts })?;

        Ok(())
    }
}

/// What the root of the keyed dictionary's askers is made from: the dictionary, and whether its sections hold.
#[derive(Bound)]
#[kind(name = "aether.spike.keyed_asking")]
struct KeyedAsking {
    to: ActorRef<Asked64>,
    number: u64,
    holds: bool,
}

/// The root of the keyed dictionary's askers.
#[derive(Plain)]
#[kind(name = "aether.spike.keyed_askers")]
struct KeyedAskers;

#[actor(key = u64, spawns(KeyedAsker))]
impl KeyedAskers {
    fn begin(asking: KeyedAsking, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        for number in 0..CLIENTS {
            let Ok(asker) = turn.spawn::<KeyedAsker>(&number, KeyedAsking { number, ..asking }) else {
                panic!("an asker begins");
            };
            turn.to(asker).send(&Go)?;
        }

        Ok(Self)
    }
}

/// An asker of the keyed dictionary, as the other askers are.
#[derive(Bound)]
#[kind(name = "aether.spike.keyed_asker")]
struct KeyedAsker {
    asking: KeyedAsking,
    asked: u64,
    answered: u64,
}

#[actor(key = u64, parent = KeyedAskers)]
impl KeyedAsker {
    fn begin(asking: KeyedAsking) -> Result<Self, Fault> {
        Ok(Self { asking, asked: 0, answered: 0 })
    }

    #[tell]
    fn go(&mut self, _: Go, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.ask_batch(turn)
    }

    #[answer]
    fn got(&mut self, (answered, _): (Answered<Got>, Pending), turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Answered::Yes(_) = answered else {
            return Err(no_answer());
        };
        self.answered += 1;
        let is_batch_done = self.answered == self.asked;
        let is_more = self.asked < PUTS;
        if is_batch_done && is_more {
            self.ask_batch(turn)?;
        }

        Ok(())
    }
}

impl KeyedAsker {
    /// Asks the next batch of questions, each of the section of its key.
    fn ask_batch(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let end = (self.asked + BATCH).min(PUTS);
        for index in self.asked..end {
            let key = key_of(self.asking.number, index, false);
            let value = (self.asking.number << 32) | index;
            if self.asking.holds {
                turn.to(self.asking.to).ask_keyed(&Fetch { key, value }, &Pending)?;
            } else {
                turn.to(self.asking.to).ask_keyed(&Get { key, value }, &Pending)?;
            }
        }
        self.asked = end;

        Ok(())
    }
}

/// Starts the keyed way of asking for the run `round`: its sections hold what they are asked when `holds`.
fn start_asked(engine: &mut Engine, round: u64, holds: bool) {
    DRAWN.store(KEYS, Ordering::Relaxed);
    let load = if holds {
        Load::Holds
    } else {
        Load::Asks
    };
    let stamper = stamper(engine, round, load);
    let runs = Box::new(SectionRunner::<Asked64, true>::new(Asked64 { stamper: stamper.clone() }));
    let filling = |to| KeyedAsking { to, number: 0, holds };
    start_keyed::<Asked64, KeyedAskers>(engine, round, (Asked64 { stamper }, runs), filling);
}

/// A program of the spike: its name, what starts it for a run, and what checks it took every put.
struct Program {
    name: &'static str,
    start: fn(&mut Engine, u64),
    check: fn(&Engine),
}

/// A keyed program of sections `$block`, whose runner drops as `$drops` says.
macro_rules! keyed_program {
    ($name:literal, $dictionary:ident, $filler:ident, $filling:ident, $drops:literal, $hot:literal, $words:literal) => {
        Program {
            name: $name,
            start: |engine, round| {
                DRAWN.store(KEYS, Ordering::Relaxed);
                let runs = Box::new(SectionRunner::<$dictionary, $drops>::new($dictionary));
                let filling = |to| $filling { to, number: 0, hot: $hot };
                start_keyed::<$dictionary, $filler>(engine, round, ($dictionary, runs), filling);
            },
            check: check_keyed::<$words>,
        }
    };
}

const PROGRAMS: [Program; 21] = [
    keyed_program!("keyed-uniform", Keyed64, KeyedFill64, KeyedFilling64, true, false, 7),
    keyed_program!("keyed-hot", Keyed64, KeyedFill64, KeyedFilling64, true, true, 7),
    Program { name: "one-uniform", start: |engine, round| start_one(engine, round, puts(false)), check: check_one },
    Program { name: "one-hot", start: |engine, round| start_one(engine, round, puts(true)), check: check_one },
    Program {
        name: "children-uniform",
        start: |engine, round| start_children(engine, round, puts(false)),
        check: check_children,
    },
    Program {
        name: "children-hot",
        start: |engine, round| start_children(engine, round, puts(true)),
        check: check_children,
    },
    Program {
        name: "keyed-16keys",
        start: |engine, round| {
            DRAWN.store(SHARDS, Ordering::Relaxed);
            let runs = Box::new(SectionRunner::<Keyed64, true>::new(Keyed64));
            let filling = |to| KeyedFilling64 { to, number: 0, hot: false };
            start_keyed::<Keyed64, KeyedFill64>(engine, round, (Keyed64, runs), filling);
        },
        check: check_keyed::<7>,
    },
    Program {
        name: "children-16keys",
        start: |engine, round| start_children_of(engine, round, puts(false), (SHARDS, SHARDS)),
        check: check_children,
    },
    Program {
        name: "children-4096",
        start: |engine, round| start_children_of(engine, round, puts(false), (KEYS, KEYS)),
        check: check_children,
    },
    keyed_program!("rb64-skip", Keyed64, KeyedFill64, KeyedFilling64, true, false, 7),
    keyed_program!("rb64-bare", Keyed64, KeyedFill64, KeyedFilling64, false, false, 7),
    keyed_program!("rb1k-skip", Keyed1k, KeyedFill1k, KeyedFilling1k, true, false, 127),
    keyed_program!("rb1k-bare", Keyed1k, KeyedFill1k, KeyedFilling1k, false, false, 127),
    keyed_program!("rb16k-skip", Keyed16k, KeyedFill16k, KeyedFilling16k, true, false, 2047),
    keyed_program!("rb16k-bare", Keyed16k, KeyedFill16k, KeyedFilling16k, false, false, 2047),
    Program {
        name: "ask-one",
        start: |engine, round| start_one(engine, round, Way { hot: false, load: Load::Asks }),
        check: check_one,
    },
    Program {
        name: "ask-children",
        start: |engine, round| start_children(engine, round, Way { hot: false, load: Load::Asks }),
        check: check_children,
    },
    Program {
        name: "hold-one",
        start: |engine, round| start_one(engine, round, Way { hot: false, load: Load::Holds }),
        check: check_one,
    },
    Program {
        name: "hold-children",
        start: |engine, round| start_children(engine, round, Way { hot: false, load: Load::Holds }),
        check: check_children,
    },
    Program { name: "ask-keyed", start: |engine, round| start_asked(engine, round, false), check: check_keyed::<7> },
    Program { name: "hold-keyed", start: |engine, round| start_asked(engine, round, true), check: check_keyed::<7> },
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
