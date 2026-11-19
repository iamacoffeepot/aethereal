//! What the spike of keyed sections must show: a key's mail in the order sent, a failed turn put back, and two keys
//! of one actor run on two workers in one step.

use super::super::{Engine, Split, SplitCost, Workers};
use crate::Clock;
use crate::stream;
use aethereal_actor::Entry as Logged;
use aethereal_actor::{
    ActorRef, Beginning, Fault, Item, KeyOf, SectionKey, SectionRunner, Sectioned, Time, Turn, Turning, actor,
    hand_section,
};
use aethereal_data::{ActorId, Bound, Plain};
use std::num::NonZeroUsize;

/// A value for a key of the dictionary.
#[derive(Plain)]
#[kind(name = "aether.spike.put")]
struct Put {
    key: u64,
    value: u64,
}

/// Why a put was refused: it came with no key, or with the value the dictionary was made to fail on.
#[derive(Plain)]
#[kind(name = "aether.spike.refused_put")]
enum RefusedPut {
    Unkeyed,
    Poisoned,
}

/// A dictionary split into a section for each key. What it holds itself is fixed as it begins, and every section reads
/// it: the value whose put fails.
#[derive(Plain)]
#[kind(name = "aether.spike.dictionary")]
struct Dictionary {
    poison: u64,
}

/// One key's section: the last value put, how many puts it took, and how many came out of the order they were sent in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Words {
    last: u64,
    puts: u64,
    out_of_order: u64,
}

#[actor]
impl Dictionary {
    fn begin(config: Dictionary) -> Result<Self, Fault> {
        Ok(config)
    }

    /// Mail sent without its key reaches the whole actor, which holds no section: refused.
    #[tell]
    fn put(&mut self, _: Put) -> Result<(), Fault> {
        Err(Fault::new(&RefusedPut::Unkeyed).unwrap_or_else(|_| unreachable!("a refusal encodes")))
    }
}

impl KeyOf<Put> for Dictionary {
    fn key(put: &Put) -> u64 {
        put.key
    }
}

impl Sectioned for Dictionary {
    type Section = Words;

    fn section(&self, section: &mut Words, item: Item<'_>, turning: Turning<'_>) -> Result<u64, Fault> {
        hand_section::<Self, Put>(self, section, item, turning, Self::put_into)
    }
}

impl Dictionary {
    /// The section's handler of a put: it changes the section, then fails on the poisoned value.
    #[allow(clippy::unnecessary_wraps, reason = "a section's handler returns a turn's result, as any handler does")]
    #[allow(clippy::needless_pass_by_value, reason = "a handler is handed its mail by value")]
    fn put_into(&self, words: &mut Words, put: Put, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if put.value != words.last + 1 {
            words.out_of_order += 1;
        }
        words.last = put.value;
        words.puts += 1;
        if put.value == self.poison {
            return Err(Fault::new(&RefusedPut::Poisoned).unwrap_or_else(|_| unreachable!("a refusal encodes")));
        }

        Ok(())
    }
}

/// What a filler puts as it begins, in order, and where.
#[derive(Bound)]
#[kind(name = "aether.spike.filling")]
struct Filling {
    to: ActorRef<Dictionary>,
    puts: Vec<Put>,
}

/// A root that puts what its config lists into the dictionary as it begins.
#[derive(Plain)]
#[kind(name = "aether.spike.filler")]
struct Filler;

#[actor(key = u64)]
impl Filler {
    fn begin(filling: Filling, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        for put in &filling.puts {
            turn.to(filling.to).send_keyed(put)?;
        }

        Ok(Self)
    }
}

/// A clock that reads nought: the tests reason about no time.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// An engine of four parts with a dictionary failing on `poison`, whose turns put a section back when they fail.
fn dictionary(poison: u64) -> (Engine, ActorRef<Dictionary>) {
    let Some(parts) = NonZeroUsize::new(4) else {
        unreachable!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let runs = Box::new(SectionRunner::<Dictionary, true, true>::new(Dictionary { poison }));
    let Ok(reference) = engine.add_sectioned_root::<Dictionary>(&(), Dictionary { poison }, runs) else {
        panic!("the dictionary begins");
    };

    (engine, reference)
}

/// Adds the filler `number`, which puts `puts` into the dictionary `to` as it begins.
fn fill(engine: &mut Engine, number: u64, to: ActorRef<Dictionary>, puts: Vec<Put>) {
    let Ok(_) = engine.add_root::<Filler>(&number, Filling { to, puts }) else {
        panic!("the filler begins");
    };
}

/// Steps `engine` until nothing is left to run.
fn settle(engine: &mut Engine) {
    while !engine.is_idle() {
        engine.step(Time::ZERO, &Stopped);
    }
}

/// The section of `key` of the dictionary `of`.
fn words(engine: &Engine, of: ActorRef<Dictionary>, key: u64) -> Option<Words> {
    engine.section::<Words>(of.id(), key).copied()
}

/// The part the section of `key` of `actor` falls in, of four.
fn part_of(actor: ActorId, key: u64) -> u64 {
    stream::lane(actor.bits(), SectionKey::of(key)) & 3
}

#[test]
fn a_keys_mail_is_handled_in_the_order_its_sender_sent_it() {
    // Catches a part's items put in order by their lane alone, or two sections that share a part handed as one run:
    // each would hand a key's puts out of the order sent, once two keys' puts lie interleaved in one part.
    let (mut engine, dictionary) = dictionary(u64::MAX);
    let first = 1;
    let Some(second) = (2..64).find(|key| part_of(dictionary.id(), *key) == part_of(dictionary.id(), first)) else {
        unreachable!("some key falls in the same part");
    };
    let puts = (1..=200).flat_map(|value| [Put { key: first, value }, Put { key: second, value }]).collect();
    fill(&mut engine, 1, dictionary, puts);

    settle(&mut engine);

    let all_in_order = Some(Words { last: 200, puts: 200, out_of_order: 0 });
    assert_eq!(words(&engine, dictionary, first), all_in_order);
    assert_eq!(words(&engine, dictionary, second), all_in_order);
    assert_eq!(words(&engine, dictionary, second + 1), None, "a key never sent has no section");
}

#[test]
fn a_failed_turn_leaves_its_section_as_it_was_and_the_actor_running() {
    // Catches a section left as the failed turn changed it, and a failure that ends the whole actor as an ordinary
    // failed turn does (ADR-0009), so the puts after it are never handled.
    let (mut engine, dictionary) = dictionary(5);
    fill(&mut engine, 1, dictionary, (1..=10).map(|value| Put { key: 3, value }).collect());

    settle(&mut engine);

    // The fifth put was taken back, so the sixth came after the fourth.
    assert_eq!(words(&engine, dictionary, 3), Some(Words { last: 10, puts: 9, out_of_order: 1 }));
    assert!(engine.table().seat(dictionary.id()).is_some(), "the dictionary is live");
    let failures = engine.log(dictionary.seated()).filter(|entry| matches!(entry, Logged::EndFailed(_))).count();
    assert_eq!(failures, 1, "the failure is logged");
}

#[test]
fn two_keys_of_one_actor_run_on_two_workers_in_one_step() {
    // Catches a keyed actor's mail kept under its seat, so all of its sections run on the worker that holds its part,
    // one after another.
    let (mut engine, dictionary) = dictionary(u64::MAX);
    let first = 1;
    let Some(second) = (2..64).find(|key| part_of(dictionary.id(), *key) != part_of(dictionary.id(), first)) else {
        unreachable!("some key falls in another part");
    };
    fill(&mut engine, 1, dictionary, vec![Put { key: first, value: 1 }, Put { key: second, value: 1 }]);

    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::ZERO).into_split() else {
        panic!("a step of two parts with a turn each is split between two workers");
    };
    let mut parts: Vec<Vec<u64>> = shares
        .iter()
        .map(|share| share.parcels.iter().map(|parcel| u64::try_from(parcel.part).unwrap_or(u64::MAX)).collect())
        .collect();
    parts.sort();
    let mut expected = vec![vec![part_of(dictionary.id(), first)], vec![part_of(dictionary.id(), second)]];
    expected.sort();
    assert_eq!(parts, expected, "each worker runs one key's part");
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &Stopped)).collect();
    engine.apply(done);

    assert_eq!(engine.step_number(), 1);
    let one_put = Some(Words { last: 1, puts: 1, out_of_order: 0 });
    assert_eq!(words(&engine, dictionary, first), one_put);
    assert_eq!(words(&engine, dictionary, second), one_put);
}
