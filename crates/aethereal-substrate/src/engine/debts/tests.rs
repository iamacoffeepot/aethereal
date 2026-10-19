//! The debts the engine keeps beside an actor, as values hold them and turns answer them (ADR-0017).

use super::*;
use aethereal_actor::{Held, MailId, SeatNumber, Unanswered, Within, WithinReader, WithinWriter};
use aethereal_data::{ActorId, Budget, Decode, Encode, Reader, Writer};

/// The actor numbered `n`, in the seat of the same number.
const fn actor(n: u64) -> Seated {
    Seated { id: ActorId::new(n), seat: SeatNumber::new(n) }
}

/// The debt the board, `1`, owes the pointer, `2`, for its request of the count `3`.
const OWED: Owed = Owed { holder: actor(1), asker: actor(2), request: 3 };

/// The chain the request was handled at.
const CHAIN: Context = Context { root: MailId { sender: ActorId::new(2), count: 1 }, depth: 2 };

/// The kind the debt is answered with, and another.
const ANSWER: KindId = KindId::of::<Unanswered>();
const OTHER: KindId = KindId::of::<NotOwed>();

/// Debts that owe [`OWED`], held by one value, to be answered with [`ANSWER`].
fn owing() -> Debts {
    let debt = Debt { asker: OWED.asker, request: OWED.request, answer: ANSWER, holders: 1, chain: CHAIN };

    Debts { held: Holds::Owes(Box::new(Owing { debts: vec![debt] })) }
}

/// A held request read back from `bytes`, as its holder reads one out of what it kept.
fn read(bytes: &[u8]) -> Held<Unanswered> {
    let mut budget = Budget::new(64);
    let mut reader = Reader::<Within>::new(bytes, &mut budget, WithinReader::new());
    let Ok(held) = Held::decode(&mut reader) else {
        panic!("a held request reads back");
    };

    held
}

/// `held` written into bytes, as its holder keeps it for a request of its own.
fn written(held: &Held<Unanswered>) -> Vec<u8> {
    let mut out = Writer::<Within>::new(WithinWriter::new());
    let Ok(()) = held.encode(&mut out) else {
        panic!("a held request is written");
    };

    out.into_bytes()
}

/// [`OWED`] in bytes, as a held request writes it.
fn written_owed() -> Vec<u8> {
    let mut out = Writer::<Within>::new(WithinWriter::new());
    for word in [1_u64, 1, 2, 2, 3] {
        let Ok(()) = word.encode(&mut out) else {
            panic!("a word is written");
        };
    }

    out.into_bytes()
}

/// The changes noted since the last time, settled against `debts`: each debt let go of.
fn settled(debts: &mut Debts, changes: &mut Vec<holders::Change>) -> Vec<Debt> {
    holders::take(changes);

    summed(changes).into_iter().filter_map(|(owed, by)| debts.change(owed, by)).collect()
}

#[test]
fn a_debt_handed_through_bytes_is_let_go_of_only_once_no_value_holds_it() {
    // Catches a held request written into what its holder keeps counted as dropped, which would tell the asker its
    // request went unanswered while it is still held; and one read back and dropped never let go of.
    let mut debts = owing();
    let mut changes = Vec::new();
    holders::open(Vec::new());
    let first = read(&written_owed());
    let kept = written(&first);
    drop(first);

    let while_kept = settled(&mut debts, &mut changes);
    let again = read(&kept);
    let while_read = settled(&mut debts, &mut changes);
    drop(again);
    let let_go = settled(&mut debts, &mut changes);
    let _ = holders::close();

    assert!(while_kept.is_empty(), "the bytes hold it");
    assert!(while_read.is_empty(), "the value read back holds it");
    assert_eq!(let_go.len(), 1, "no value holds it");
    assert_eq!(let_go[0].chain, CHAIN, "what the engine says of it goes on the request's chain");
}

#[test]
fn a_turn_answers_only_a_debt_its_actor_holds_of_the_kind_it_answers_with() {
    // Catches an answer the engine takes for one its actor owes when it is another's, of another kind, or answered
    // already, which would answer a request twice or with a kind its asker does not handle.
    let debts = owing();
    let answers = |owed, answer| [Owes::Answers { owed, answer }];
    let others = Owed { holder: actor(4), ..OWED };
    let twice = [Owes::Answers { owed: OWED, answer: ANSWER }, Owes::Answers { owed: OWED, answer: ANSWER }];

    let owed = matches!(debts.filed(actor(1), &answers(OWED, ANSWER), CHAIN), Filed::Owes(left) if left.is_empty());
    let not_mine = matches!(debts.filed(actor(1), &answers(others, ANSWER), CHAIN), Filed::NotOwed);
    let not_kind = matches!(debts.filed(actor(1), &answers(OWED, OTHER), CHAIN), Filed::NotOwed);
    let not_twice = matches!(debts.filed(actor(1), &twice, CHAIN), Filed::NotOwed);

    assert!(owed, "the debt it holds is answered");
    assert!(not_mine, "another's debt is not");
    assert!(not_kind, "nor one answered with another kind");
    assert!(not_twice, "nor one answered already");
}
