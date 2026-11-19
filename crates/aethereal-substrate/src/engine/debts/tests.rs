//! The debts the engine keeps beside an actor, as values hold them and turns answer them (ADR-0017).

use super::*;
use aethereal_actor::{Due, Held, MailId, SeatNumber, Unanswered, Written};
use aethereal_data::{ActorId, to_bytes};

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

/// The debt [`OWED`] names, held by one value, to be answered with [`ANSWER`].
const DEBT: Debt = Debt { asker: OWED.asker, request: OWED.request, answer: ANSWER, holders: 1, chain: CHAIN };

/// Debts that owe [`DEBT`].
fn owing() -> Debts {
    let mut debts = ByNumber::default();
    debts.insert(request_id(OWED.asker, OWED.request), DEBT);

    Debts { held: Holds::Owes(Box::new(Owing { debts, answered: Vec::new() })) }
}

/// A held request read back from `written`, as its holder reads one out of what it kept.
fn read(written: &Written) -> Held<Unanswered> {
    let Ok(held) = written.read(64) else {
        panic!("a held request reads back");
    };

    held
}

/// `held` written, as its holder keeps it for a request of its own.
fn written(held: &Held<Unanswered>) -> Written {
    let Ok(written) = Written::of(held) else {
        panic!("a held request is written");
    };

    written
}

/// [`OWED`] in bytes, of a request due never, as a held request writes it.
fn written_owed() -> Written {
    let words = [1_u64, 1, 2, 2, 3].iter().flat_map(|word| to_bytes(word).unwrap_or_default());
    let due = to_bytes(&Due::Never).unwrap_or_default();

    Written::new(words.chain(due).collect(), Vec::new())
}

/// The changes noted since the last time, settled against `debts`: each debt let go of.
fn settled(debts: &mut Debts, changes: &mut Vec<holders::Change>) -> Vec<Debt> {
    holders::take(changes);
    sum(changes);

    changes.drain(..).filter_map(|(owed, by)| debts.change(owed, by)).collect()
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
    let answers = |owed, answer| [Owes::Answers { owed, answer }];
    let others = Owed { holder: actor(4), ..OWED };
    let twice = [Owes::Answers { owed: OWED, answer: ANSWER }, Owes::Answers { owed: OWED, answer: ANSWER }];
    let mut answered = owing();

    let owed = answered.file(actor(1), &answers(OWED, ANSWER), CHAIN).is_ok();
    let not_mine = owing().file(actor(1), &answers(others, ANSWER), CHAIN).is_err();
    let not_kind = owing().file(actor(1), &answers(OWED, OTHER), CHAIN).is_err();
    let not_twice = owing().file(actor(1), &twice, CHAIN).is_err();

    assert!(owed, "the debt it holds is answered");
    assert!(!answered.owes(OWED.asker, OWED.request), "and is no longer owed");
    assert!(not_mine, "another's debt is not");
    assert!(not_kind, "nor one answered with another kind");
    assert!(not_twice, "nor one answered already");
}

#[test]
fn a_turn_that_answers_what_it_does_not_owe_changes_no_debt() {
    // Catches a turn that fails partway keeping what it filed before the debt it did not owe: a debt it answered, whose
    // answer is dropped with its mail, would be answered by no one, and a request it held would be owed by an actor
    // whose turn never held it.
    let mut debts = owing();
    let handled = Owed { asker: actor(5), request: 7, ..OWED };
    let turn = [
        Owes::Answers { owed: OWED, answer: ANSWER },
        Owes::Holds { owed: handled, answer: ANSWER },
        Owes::Answers { owed: handled, answer: ANSWER },
        Owes::Answers { owed: OWED, answer: ANSWER },
    ];
    let later = Context { depth: CHAIN.depth + 1, ..CHAIN };

    let filed = debts.file(actor(1), &turn, later);

    assert!(filed.is_err(), "the turn answered a debt it had answered already");
    assert_eq!(debts.take_all(), [DEBT], "the debts are as they were before the turn");
}
