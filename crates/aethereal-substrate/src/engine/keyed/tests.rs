//! What the spike of keyed sections must show: a key's mail in the order sent, two keys of one actor run on two
//! workers in one step, and a section that asks, holds, and is asked as a whole actor does, its requests kept beside it
//! in its part and its answers stamped back to it, all answered when its actor ends.

use super::super::{Engine, Split, SplitCost, Workers};
use crate::Clock;
use crate::stream;
use aethereal_actor::Entry as Logged;
use aethereal_actor::{
    ActorRef, Answer, Answered, Beginning, Fault, Held, Item, KeyOf, Later, Seated, SectionKey, SectionRunner,
    Sectioned, Time, Turn, Turning, Unanswered, actor, hand_section, is_answer_of, is_of,
};
use aethereal_data::{ActorId, Bound, Plain};
use std::mem;
use std::num::NonZeroUsize;

/// A value for a key of the dictionary.
#[derive(Plain)]
#[kind(name = "aether.spike.put")]
struct Put {
    key: u64,
    value: u64,
}

/// Tells the section of `key` to ask the oracle about `key`, keeping `at`.
#[derive(Plain)]
#[kind(name = "aether.spike.lookup")]
struct Lookup {
    key: u64,
    at: u64,
}

/// Asks the section of `key` for a reply, which it holds until a [`Release`].
#[derive(Plain)]
#[kind(name = "aether.spike.keep")]
struct Keep {
    key: u64,
}

/// Tells the section of `key` what to do with the requests it holds.
#[derive(Plain)]
#[kind(name = "aether.spike.release")]
struct Release {
    key: u64,
    how: Releasing,
}

/// What a section does with the requests it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Releasing {
    /// Answers each with this value.
    Answers(u64),
    /// Lets each go without an answer.
    LetsGo,
}

/// Tells the section of `key` to go back to its default.
#[derive(Plain)]
#[kind(name = "aether.spike.forget")]
struct Forget {
    key: u64,
}

/// Tells the section of `key` to close its actor.
#[derive(Plain)]
#[kind(name = "aether.spike.close")]
struct Close {
    key: u64,
}

/// What the oracle is asked: about `value`, answered at once with ten times it, or held for ever when `silent`.
#[derive(Plain)]
#[kind(name = "aether.spike.question")]
struct Question {
    value: u64,
    silent: bool,
}

/// What a question, or a kept request, is answered with.
#[derive(Plain)]
#[kind(name = "aether.spike.said")]
struct Said {
    value: u64,
}

/// What an asker keeps for its request: the key it is about, and which of its requests it is.
#[derive(Plain)]
#[kind(name = "aether.spike.looking")]
struct Looking {
    key: u64,
    at: u64,
}

/// What became of a request, as its asker keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Heard {
    /// It was answered with `value`, and what was kept for it is `at`.
    Yes { value: u64, at: u64 },
    /// It went unanswered for `why`, and what was kept for it is `at`.
    No { why: Unanswered, at: u64 },
}

impl Heard {
    /// What `answered` says of the request that kept `looking`.
    fn of(answered: &Answered<Said>, looking: &Looking) -> Self {
        match answered {
            Answered::Yes(reply) => Self::Yes { value: reply.value, at: looking.at },
            Answered::No(why) => Self::No { why: *why, at: looking.at },
        }
    }
}

/// Why the dictionary refused what it was handed: it came with no key, or with the value it was made to fail on.
#[derive(Plain)]
#[kind(name = "aether.spike.refused_put")]
enum RefusedPut {
    Unkeyed,
    Poisoned,
}

/// The fault of a refusal.
fn refused(why: &RefusedPut) -> Fault {
    Fault::new(why).unwrap_or_else(|_| unreachable!("a refusal encodes"))
}

/// An actor that answers each question at once with ten times its value, or holds it for ever when it is silent.
#[derive(Bound)]
#[kind(name = "aether.spike.oracle")]
struct Oracle {
    held: Vec<Held<Said>>,
}

/// What an oracle is made from: nothing.
#[derive(Plain)]
#[kind(name = "aether.spike.blank")]
struct Blank;

#[actor]
impl Oracle {
    fn begin(_: Blank) -> Result<Self, Fault> {
        Ok(Self { held: Vec::new() })
    }

    #[request]
    fn question(
        &mut self,
        question: Question,
        turn: &mut Turn<'_, Self>,
    ) -> Result<aethereal_actor::Reply<Said>, Fault> {
        if question.silent {
            let (held, later) = turn.hold();
            self.held.push(held);

            return Ok(aethereal_actor::Reply::Later(later));
        }

        Ok(aethereal_actor::Reply::Now(Said { value: question.value * 10 }))
    }
}

/// A dictionary split into a section for each key. What it holds itself is fixed as it begins, and every section reads
/// it: the value whose put fails, and the oracle its sections ask.
#[derive(Bound)]
#[kind(name = "aether.spike.dictionary")]
struct Dictionary {
    poison: u64,
    oracle: ActorRef<Oracle>,
}

/// One key's section: the last value put, how many puts it took, how many came out of the order they were sent in,
/// what became of the requests it asked, and the requests it holds.
#[derive(Debug, Default)]
struct Page {
    last: u64,
    puts: u64,
    out_of_order: u64,
    heard: Vec<Heard>,
    held: Vec<Held<Said>>,
}

/// A whole-actor handler of a keyed actor: what reaches it came without its key, and is refused.
macro_rules! unkeyed {
    () => {
        Err(refused(&RefusedPut::Unkeyed))
    };
}

#[actor]
impl Dictionary {
    fn begin(config: Dictionary) -> Result<Self, Fault> {
        Ok(config)
    }

    #[tell]
    fn put(&mut self, _: Put) -> Result<(), Fault> {
        unkeyed!()
    }

    #[tell]
    fn lookup(&mut self, _: Lookup) -> Result<(), Fault> {
        unkeyed!()
    }

    #[request]
    fn keep(&mut self, _: Keep) -> Result<Said, Fault> {
        unkeyed!()
    }

    #[tell]
    fn release(&mut self, _: Release) -> Result<(), Fault> {
        unkeyed!()
    }

    #[tell]
    fn forget(&mut self, _: Forget) -> Result<(), Fault> {
        unkeyed!()
    }

    #[tell]
    fn close(&mut self, _: Close) -> Result<(), Fault> {
        unkeyed!()
    }

    #[answer]
    fn reply(&mut self, _: (Answered<Said>, Looking)) -> Result<(), Fault> {
        unkeyed!()
    }
}

/// The key of each kind a section handles, as `#[tell(key = ..)]`, `#[request(key = ..)]`, and `#[answer(key = ..)]`
/// would declare it.
macro_rules! keyed_by {
    ($($kind:ty => |$mail:ident| $key:expr;)*) => {$(
        impl KeyOf<$kind> for Dictionary {
            type From = $kind;

            fn key($mail: &$kind) -> u64 {
                $key
            }
        }
    )*};
}

keyed_by! {
    Put => |put| put.key;
    Lookup => |lookup| lookup.key;
    Keep => |keep| keep.key;
    Release => |release| release.key;
    Forget => |forget| forget.key;
    Close => |close| close.key;
}

/// The section an answer goes back to: the key in the context its section kept.
impl KeyOf<Answer<Said, Looking>> for Dictionary {
    type From = Looking;

    fn key(looking: &Looking) -> u64 {
        looking.key
    }
}

impl Sectioned for Dictionary {
    type Section = Page;

    fn section(&self, page: &mut Page, item: Item<'_>, turning: Turning<'_>) -> Result<(), Fault> {
        if is_of::<Put>(&item) {
            return hand_section::<Self, Put, ()>(self, page, item, turning, Self::put_into);
        }
        if is_of::<Lookup>(&item) {
            return hand_section::<Self, Lookup, ()>(self, page, item, turning, Self::look_up);
        }
        if is_of::<Keep>(&item) {
            return hand_section::<Self, Keep, Later<Said>>(self, page, item, turning, Self::hold);
        }
        if is_of::<Release>(&item) {
            return hand_section::<Self, Release, ()>(self, page, item, turning, Self::let_out);
        }
        if is_of::<Forget>(&item) {
            return hand_section::<Self, Forget, ()>(self, page, item, turning, Self::clear);
        }
        if is_of::<Close>(&item) {
            return hand_section::<Self, Close, ()>(self, page, item, turning, Self::close_all);
        }
        if is_answer_of::<Said>(&item) {
            return hand_section::<Self, Answer<Said, Looking>, ()>(self, page, item, turning, Self::replied);
        }

        unkeyed!()
    }

    fn is_unused(page: &Page) -> bool {
        let is_blank = page.last == 0 && page.puts == 0 && page.out_of_order == 0;

        is_blank && page.heard.is_empty() && page.held.is_empty()
    }
}

/// A section's handlers, each a turn of one key's section.
#[allow(clippy::unnecessary_wraps, reason = "a section's handler returns a turn's result, as any handler does")]
#[allow(clippy::needless_pass_by_value, reason = "a handler is handed its mail by value")]
#[allow(clippy::unused_self, reason = "a section's handler is handed its actor to read, whether it reads it or not")]
impl Dictionary {
    /// Changes the section, then fails on the poisoned value.
    fn put_into(&self, page: &mut Page, put: Put, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        if put.value != page.last + 1 {
            page.out_of_order += 1;
        }
        page.last = put.value;
        page.puts += 1;
        if put.value == self.poison {
            return Err(refused(&RefusedPut::Poisoned));
        }

        Ok(())
    }

    /// Asks the oracle about the key, and changes nothing until the answer comes.
    fn look_up(&self, _: &mut Page, lookup: Lookup, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let question = Question { value: lookup.key, silent: lookup.at == SILENT };
        turn.to(self.oracle).ask_returning(&question, &Looking { key: lookup.key, at: lookup.at })?;

        Ok(())
    }

    /// Holds the request, to answer it at a release.
    fn hold(&self, page: &mut Page, _: Keep, turn: &mut Turn<'_, Self>) -> Result<Later<Said>, Fault> {
        let (held, later) = turn.hold();
        page.held.push(held);

        Ok(later)
    }

    /// Answers or lets go of what the section holds.
    fn let_out(&self, page: &mut Page, release: Release, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        for held in mem::take(&mut page.held) {
            match release.how {
                Releasing::Answers(value) => turn.answer(held, &Said { value })?,
                Releasing::LetsGo => drop(held),
            }
        }

        Ok(())
    }

    /// Goes back to its default.
    fn clear(&self, page: &mut Page, _: Forget, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        *page = Page::default();

        Ok(())
    }

    /// Closes the whole actor.
    fn close_all(&self, _: &mut Page, _: Close, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }

    /// Keeps what became of a request it asked, with what it kept.
    fn replied(
        &self,
        page: &mut Page,
        (answered, looking): (Answered<Said>, Looking),
        _: &mut Turn<'_, Self>,
    ) -> Result<(), Fault> {
        page.heard.push(Heard::of(&answered, &looking));

        Ok(())
    }
}

/// The `at` of a lookup whose question the oracle holds for ever.
const SILENT: u64 = 99;

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

/// What a teller sends the dictionary as it begins, in order: each a tell to one key.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Told {
    Lookup(u64, u64),
    Release(u64, Releasing),
    Forget(u64),
    Close(u64),
    Put(u64, u64),
}

/// What a teller is made from: where it tells, and what.
#[derive(Bound)]
#[kind(name = "aether.spike.telling")]
struct Telling {
    to: ActorRef<Dictionary>,
    told: Vec<Told>,
}

/// A root that tells the dictionary what its config lists as it begins.
#[derive(Plain)]
#[kind(name = "aether.spike.teller")]
struct Teller;

#[actor(key = u64)]
impl Teller {
    fn begin(telling: Telling, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        let to = telling.to;
        for told in telling.told {
            match told {
                Told::Lookup(key, at) => turn.to(to).send_keyed(&Lookup { key, at })?,
                Told::Release(key, how) => turn.to(to).send_keyed(&Release { key, how })?,
                Told::Forget(key) => turn.to(to).send_keyed(&Forget { key })?,
                Told::Close(key) => turn.to(to).send_keyed(&Close { key })?,
                Told::Put(key, value) => turn.to(to).send_keyed(&Put { key, value })?,
            };
        }

        Ok(Self)
    }
}

/// What an asker is made from: the dictionary, and the keys whose sections it asks to keep a request, keeping its
/// place among them.
#[derive(Bound)]
#[kind(name = "aether.spike.asking")]
struct Asking {
    to: ActorRef<Dictionary>,
    keys: Vec<u64>,
}

/// A root that asks sections of the dictionary to keep a request, as it begins, and keeps what became of each.
#[derive(Bound)]
#[kind(name = "aether.spike.asker")]
struct Asker {
    heard: Vec<Heard>,
}

#[actor(key = u64)]
impl Asker {
    fn begin(asking: Asking, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
        for (at, key) in (0..).zip(asking.keys) {
            turn.to(asking.to).ask_keyed(&Keep { key }, &Looking { key, at })?;
        }

        Ok(Self { heard: Vec::new() })
    }

    #[answer]
    fn reply(&mut self, (answered, looking): (Answered<Said>, Looking)) -> Result<(), Fault> {
        self.heard.push(Heard::of(&answered, &looking));

        Ok(())
    }
}

/// A clock that reads nought: the tests reason about no time.
struct Stopped;

impl Clock for Stopped {
    fn nanos(&self) -> u64 {
        0
    }
}

/// The reference to the root `id` of type `T`, which `engine` seats.
fn found<T>(engine: &Engine, id: ActorId) -> ActorRef<T> {
    let Some(seat) = engine.table().seat(id) else {
        panic!("a root is seated");
    };

    ActorRef::found(Seated { id, seat })
}

/// An engine of four parts with an oracle and a dictionary failing on `poison`.
fn dictionary(poison: u64) -> (Engine, ActorRef<Dictionary>) {
    let Some(parts) = NonZeroUsize::new(4) else {
        unreachable!("four is not zero");
    };
    let mut engine = Engine::new(parts, 1024, 0, 1 << 20, 0).ready();
    let Ok(oracle) = engine.add_root::<Oracle>(&(), Blank) else {
        panic!("the oracle begins");
    };
    let oracle = found(&engine, oracle);
    let runs = Box::new(SectionRunner::<Dictionary, true>::new(Dictionary { poison, oracle }));
    let Ok(reference) = engine.add_sectioned_root::<Dictionary>(&(), Dictionary { poison, oracle }, runs) else {
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

/// Adds the teller `number`, which tells the dictionary `to` what `told` lists as it begins.
fn tell(engine: &mut Engine, number: u64, to: ActorRef<Dictionary>, told: Vec<Told>) {
    let Ok(_) = engine.add_root::<Teller>(&number, Telling { to, told }) else {
        panic!("the teller begins");
    };
}

/// Adds the asker `number`, which asks the sections of `keys` of the dictionary `to` to keep a request.
fn ask(engine: &mut Engine, number: u64, to: ActorRef<Dictionary>, keys: Vec<u64>) -> ActorRef<Asker> {
    let Ok(id) = engine.add_root::<Asker>(&number, Asking { to, keys }) else {
        panic!("the asker begins");
    };

    found(engine, id)
}

/// Steps `engine` until nothing is left to run.
fn settle(engine: &mut Engine) {
    while !engine.is_idle() {
        engine.step(Time::ZERO, &Stopped);
    }
}

/// The section of `key` of the dictionary `of`.
fn page(engine: &Engine, of: ActorRef<Dictionary>, key: u64) -> Option<&Page> {
    engine.section::<Page>(of.id(), key)
}

/// What the asker `asker` heard.
fn heard(engine: &Engine, asker: ActorRef<Asker>) -> Vec<Heard> {
    engine.state::<Asker>(asker.seated()).map(|asker| asker.heard.clone()).unwrap_or_default()
}

/// What the asker `asker` heard, by what it kept: the order in which two sections' askers are told is not the order
/// asked.
fn heard_sorted(engine: &Engine, asker: ActorRef<Asker>) -> Vec<Heard> {
    let mut heard = heard(engine, asker);
    heard.sort_by_key(|heard| match heard {
        Heard::Yes { at, .. } | Heard::No { at, .. } => *at,
    });

    heard
}

/// The part the section of `key` of `actor` falls in, of four.
fn part_of(actor: ActorId, key: u64) -> u64 {
    stream::lane(actor.bits(), SectionKey::of(key)) & 3
}

/// A key above `first` whose section falls in another part than the section of `first` of `actor`.
fn apart_from(actor: ActorId, first: u64) -> u64 {
    let Some(second) = (first + 1..first + 64).find(|key| part_of(actor, *key) != part_of(actor, first)) else {
        unreachable!("some key falls in another part");
    };

    second
}

/// Runs one step of `engine` on two workers, and answers which parts each ran.
fn step_on_two(engine: &mut Engine) -> Vec<Vec<u64>> {
    let workers = Workers { active: 2, max: 2, wake_nanos: 0, split: SplitCost::NONE };
    let Split::Shares(shares) = engine.plan(workers, Time::ZERO).into_split() else {
        panic!("a step of two parts with a turn each is split between two workers");
    };
    let mut parts: Vec<Vec<u64>> = shares
        .iter()
        .map(|share| share.parcels.iter().map(|parcel| u64::try_from(parcel.part).unwrap_or(u64::MAX)).collect())
        .collect();
    parts.sort();
    let reading = engine.reading();
    let done = shares.into_iter().map(|share| reading.run(share, &Stopped)).collect();
    engine.apply(done);

    parts
}

/// Whether the log of `actor` says it failed once.
fn failed_once(engine: &Engine, actor: Seated) -> bool {
    engine.log(actor).filter(|entry| matches!(entry, Logged::Failed(_))).count() == 1
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

    for key in [first, second] {
        let found = page(&engine, dictionary, key).map(|page| (page.last, page.puts, page.out_of_order));
        assert_eq!(found, Some((200, 200, 0)), "key {key}");
    }
    assert!(page(&engine, dictionary, second + 1).is_none(), "a key never sent has no section");
}

#[test]
fn two_keys_of_one_actor_run_on_two_workers_in_one_step() {
    // Catches a keyed actor's mail kept under its seat, so all of its sections run on the worker that holds its part,
    // one after another.
    let (mut engine, dictionary) = dictionary(u64::MAX);
    let first = 1;
    let second = apart_from(dictionary.id(), first);
    fill(&mut engine, 1, dictionary, vec![Put { key: first, value: 1 }, Put { key: second, value: 1 }]);

    let parts = step_on_two(&mut engine);

    let mut expected = vec![vec![part_of(dictionary.id(), first)], vec![part_of(dictionary.id(), second)]];
    expected.sort();
    assert_eq!(parts, expected, "each worker runs one key's part");
    for key in [first, second] {
        let found = page(&engine, dictionary, key).map(|page| (page.last, page.puts));
        assert_eq!(found, Some((1, 1)), "key {key}");
    }
}

#[test]
fn sections_on_two_workers_ask_in_one_step_and_each_answer_comes_back_to_its_own_section() {
    // Catches an answer routed to the actor's seat or to another section, where nothing waits on it and it is dropped;
    // a context filed beside the seat, which a section's turn on another worker cannot reach; and two sections that
    // number their requests from one count, so the oracle drops one of two requests of one id as a repeat.
    let (mut engine, dictionary) = dictionary(u64::MAX);
    let first = 1;
    let second = apart_from(dictionary.id(), first);
    tell(&mut engine, 1, dictionary, vec![Told::Lookup(first, 7), Told::Lookup(second, 8)]);

    let parts = step_on_two(&mut engine);
    settle(&mut engine);

    let mut expected = vec![vec![part_of(dictionary.id(), first)], vec![part_of(dictionary.id(), second)]];
    expected.sort();
    assert_eq!(parts, expected, "each worker runs one key's part, and its section asks there");
    let heard_by = |key| page(&engine, dictionary, key).map(|page| page.heard.clone());
    assert_eq!(heard_by(first), Some(vec![Heard::Yes { value: first * 10, at: 7 }]));
    assert_eq!(heard_by(second), Some(vec![Heard::Yes { value: second * 10, at: 8 }]));
    assert!(!engine.section_holds(dictionary.id(), first), "an answered request is no longer waited on");
}

#[test]
fn a_section_holds_a_request_and_answers_it_in_a_later_turn() {
    // Catches a debt filed beside the seat, which a section's answer in a later turn then does not find and fails on;
    // an answer not stamped back to the asker's section, here the asker's whole; and a debt kept after it is answered.
    let (mut engine, dictionary) = dictionary(u64::MAX);
    let asker = ask(&mut engine, 1, dictionary, vec![5]);
    settle(&mut engine);
    assert!(heard(&engine, asker).is_empty(), "the request is held");
    assert!(engine.section_holds(dictionary.id(), 5), "the section owes it");

    tell(&mut engine, 2, dictionary, vec![Told::Release(5, Releasing::Answers(42))]);
    settle(&mut engine);

    assert_eq!(heard(&engine, asker), vec![Heard::Yes { value: 42, at: 0 }]);
    assert!(!engine.section_holds(dictionary.id(), 5), "an answered debt is no longer owed");
    assert!(engine.table().seat(dictionary.id()).is_some(), "the dictionary is live");
}

#[test]
fn a_held_request_a_section_lets_go_of_is_answered_unreplied() {
    // Catches the count of what holds a section's debt looked for beside the seat, where it is not, so the debt
    // dropped by a section is never let go of and its asker waits for ever.
    let (mut engine, dictionary) = dictionary(u64::MAX);
    let asker = ask(&mut engine, 1, dictionary, vec![5]);
    settle(&mut engine);

    tell(&mut engine, 2, dictionary, vec![Told::Release(5, Releasing::LetsGo)]);
    settle(&mut engine);

    assert_eq!(heard(&engine, asker), vec![Heard::No { why: Unanswered::Unreplied, at: 0 }]);
    assert!(!engine.section_holds(dictionary.id(), 5));
}

/// Sets up a dictionary whose sections of `owing` each hold a request of one asker, and whose section of `waiting`
/// waits on an answer the oracle never gives: answers the engine, the dictionary, and the asker.
fn owing_and_waiting(owing: [u64; 2], waiting: u64) -> (Engine, ActorRef<Dictionary>, ActorRef<Asker>) {
    let (mut engine, dictionary) = dictionary(13);
    let asker = ask(&mut engine, 1, dictionary, owing.to_vec());
    tell(&mut engine, 1, dictionary, vec![Told::Lookup(waiting, SILENT)]);
    settle(&mut engine);
    for key in owing.into_iter().chain([waiting]) {
        assert!(engine.section_holds(dictionary.id(), key), "the section of {key} holds a request");
    }

    (engine, dictionary, asker)
}

#[test]
fn a_fault_in_any_section_ends_the_actor_and_every_section_s_debts_are_answered_ended() {
    // Catches a failed section turn that ends nothing (the first spike's rollback), and an ending that answers only
    // the debts beside the seat, or only those of the failing section's part, leaving the rest held for ever; and the
    // contexts of what a section waits on, or a section's state, left behind in a part after the actor has gone.
    let (mut engine, dictionary, asker) = owing_and_waiting([1, 2], 3);

    tell(&mut engine, 2, dictionary, vec![Told::Put(4, 13)]);
    settle(&mut engine);

    let ended = |at| Heard::No { why: Unanswered::Ended, at };
    assert_eq!(heard_sorted(&engine, asker), vec![ended(0), ended(1)]);
    assert!(engine.table().seat(dictionary.id()).is_none(), "the dictionary is gone");
    assert!(!engine.keeps_sections(dictionary.id()), "no part keeps anything of it");
    assert!(failed_once(&engine, dictionary.seated()), "the failure is logged");
}

#[test]
fn a_section_that_closes_its_actor_ends_it_and_every_section_s_debts_are_answered_ended() {
    // Catches the same as a fault, for an actor that closes itself from a section's turn.
    let (mut engine, dictionary, asker) = owing_and_waiting([1, 2], 3);

    tell(&mut engine, 2, dictionary, vec![Told::Close(4)]);
    settle(&mut engine);

    let ended = |at| Heard::No { why: Unanswered::Ended, at };
    assert_eq!(heard_sorted(&engine, asker), vec![ended(0), ended(1)]);
    assert!(engine.table().seat(dictionary.id()).is_none(), "the dictionary is gone");
    assert!(!engine.keeps_sections(dictionary.id()), "no part keeps anything of it");
}

#[test]
fn a_section_left_at_its_default_while_it_waits_is_kept_and_its_answer_finds_its_context() {
    // Catches a section dropped at its default while it waits on an answer, with what it keeps of the request: the
    // answer then finds no context and is dropped, and the section never hears it. And a section kept for ever once
    // it has sent mail, as the first spike kept it: here it is dropped once it is back at its default.
    let (mut engine, dictionary) = dictionary(u64::MAX);
    tell(&mut engine, 1, dictionary, vec![Told::Lookup(6, 3)]);
    engine.step(Time::ZERO, &Stopped);
    engine.step(Time::ZERO, &Stopped);
    assert!(page(&engine, dictionary, 6).is_some_and(Dictionary::is_unused), "it waits at its default");
    assert!(engine.section_holds(dictionary.id(), 6), "it waits on the oracle's answer");

    settle(&mut engine);
    let heard_by = page(&engine, dictionary, 6).map(|page| page.heard.clone());
    assert_eq!(heard_by, Some(vec![Heard::Yes { value: 60, at: 3 }]));

    tell(&mut engine, 2, dictionary, vec![Told::Forget(6)]);
    settle(&mut engine);
    assert!(page(&engine, dictionary, 6).is_none(), "a section that sent mail is dropped once it is unused");
}
