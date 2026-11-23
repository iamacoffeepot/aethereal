//! What each running guest is relied on for (ADR-0054): every proof made at run time, a cast or a guard proving a
//! sender, relies on the guest it proves for what its protocol takes, by the code that proved it. A native turn's
//! proofs rely as the program, and a guest's as its module; a native actor is relied on for nothing. What each turn
//! asked for is read in the engine's record of its turns, and what each guest is relied on for in its table.
#![cfg(not(target_arch = "wasm32"))]

use aethereal_actor::{
    ActorRef, AnsweredWith, Bytes, ErasedActorRef, Fault, ProtocolRef, Prover, Reliance, Relied, Speaks, Turn, actor,
};
use aethereal_chassis_headless::guest::Runtime;
use aethereal_data::{ActorId, ActorPath, Bound, Kind, KindId, PathSegment, Plain};
use aethereal_guest_echo as echo;
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Turned, Unheard};
use std::num::NonZeroUsize;
use std::sync::Arc;

/// The echo guest the build script built.
const ECHO: &[u8] = include_bytes!(env!("AETHEREAL_GUEST_ECHO"));

/// A word to a surveyor to prove the actor it is made for, by the proof it names.
#[derive(Plain)]
#[kind(name = "aether.test.survey")]
struct Survey {
    by: echo::Proof,
}

/// A native root that proves the actor at its path on each word to, as the word says, and tells it a ping.
#[derive(Bound)]
#[kind(name = "aether.test.surveyor")]
struct Surveyor {
    at: ActorPath,
}

#[actor]
impl Surveyor {
    fn begin(surveyor: Self) -> Result<Self, Fault> {
        Ok(surveyor)
    }

    #[tell]
    fn survey(&mut self, survey: Survey, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Some(found) = turn.resolve(&self.at) else {
            return Ok(());
        };
        let proven: Option<ProtocolRef<(echo::Ping,)>> = match survey.by {
            echo::Proof::Told => turn.cast(found),
            echo::Proof::Asked => turn.cast::<echo::Pings>(found).map(ProtocolRef::narrow),
        };
        if let Some(proven) = proven {
            turn.to(proven).send(&echo::Ping { n: 0 })?;
        }

        Ok(())
    }
}

/// A native root that takes pings only from an actor that speaks pongs, and tells it one back: its guard proves the
/// sender.
#[derive(Bound)]
#[kind(name = "aether.test.listener")]
struct Listener;

#[actor]
impl Listener {
    fn begin(listener: Self) -> Result<Self, Fault> {
        Ok(listener)
    }

    #[tell]
    fn ping(&mut self, _: echo::Ping, turn: &mut Turn<'_, Self, Speaks<(echo::Pong,)>>) -> Result<(), Fault> {
        turn.to(*turn.from()).send(&echo::Pong { n: 0 })?;

        Ok(())
    }
}

/// The path of the root singleton of the type `K`.
fn root_path<K: Kind>() -> ActorPath {
    ActorPath::from_root(&PathSegment::new(K::NAME, &()))
}

/// An engine running guests, the echo guest put in it as code, with an echo, a timekeeper, a listener, a surveyor made
/// for the actor at `surveyed`, and a relier made for the actor at `relied_on`, each a root, and a harness over it:
/// with the echo's code.
fn program(surveyed: ActorPath, relied_on: ActorPath) -> Result<(Harness<Bench>, Bytes), Unheard> {
    let mut engine =
        Engine::new(NonZeroUsize::MIN, 1024, 1 << 20, 1 << 23, 0).with_guests(Arc::new(Runtime::default())).ready();
    let Ok(code) = engine.put_code(ECHO.to_vec()) else {
        panic!("the echo guest is fit to run");
    };
    engine.add_root_from::<echo::Echo>(&code, &(), &echo::EchoConfig)?;
    engine.add_root_from::<echo::Timekeeper>(&code, &(), &echo::EchoConfig)?;
    engine.add_root_from::<echo::Relier>(&code, &(), &echo::RelyingOn { at: relied_on })?;
    engine.add_root::<Listener>(&(), Listener)?;
    engine.add_root::<Surveyor>(&(), Surveyor { at: surveyed })?;

    Ok((Harness::bare(engine)?, code))
}

/// What the actor `actor` refers to is relied on for, as of the last step, when it is a live guest.
///
/// # Panics
///
/// When it is not.
fn relied_on(harness: &mut Harness<Bench>, actor: impl Into<ErasedActorRef>) -> Vec<Reliance> {
    let Relied::Guest(reliances) = harness.relied(actor) else {
        panic!("the actor is a live guest");
    };

    reliances.all().to_vec()
}

/// What the turns of `by` among `turned` asked that a guest be relied on for, with the guest, in the order asked.
fn asked_by<A>(turned: &[Turned], by: ActorRef<A>) -> Vec<(ActorId, Reliance)> {
    let by = by.seated();

    turned.iter().filter(|turn| turn.actor == by).flat_map(|turn| turn.relied.iter().copied()).collect()
}

/// That `kind` is relied on, answered as `answer` says, by `prover`.
const fn reliance(kind: KindId, answer: AnsweredWith, prover: Prover) -> Reliance {
    Reliance { kind, answer, prover }
}

#[test]
fn a_native_cast_of_a_guest_relies_on_it_for_its_protocol_as_the_program() -> Result<(), Unheard> {
    // Catches a cast that writes no reliance, one that loses the answer a named protocol carries for its kind, and one
    // that names a guest's module, or nothing, where the program proved it.
    let (mut harness, _) = program(root_path::<echo::Echo>(), root_path::<Listener>())?;
    let (surveyor, echo) = (harness.root::<Surveyor>(), harness.root::<echo::Echo>());

    harness.tell(surveyor, &Survey { by: echo::Proof::Asked })?;
    let turned = harness.settle_turns();

    let pinged = reliance(KindId::of::<echo::Ping>(), AnsweredWith::Kind(KindId::of::<echo::Pong>()), Prover::Program);
    assert_eq!(asked_by(&turned, surveyor), vec![(echo.id(), pinged)]);
    assert_eq!(relied_on(&mut harness, echo), vec![pinged]);

    Ok(())
}

#[test]
fn a_proof_already_relied_on_asks_for_nothing_and_another_answer_is_kept_beside_it() -> Result<(), Unheard> {
    // Catches a proof that asks for its reliance again each time, which a guest proven every turn would pay for in
    // every turn, and a second proof of a kind by one prover that drops or overwrites the answer of the first.
    let (mut harness, _) = program(root_path::<echo::Echo>(), root_path::<Listener>())?;
    let (surveyor, echo) = (harness.root::<Surveyor>(), harness.root::<echo::Echo>());
    harness.tell(surveyor, &Survey { by: echo::Proof::Asked })?;
    harness.settle();

    harness.tell(surveyor, &Survey { by: echo::Proof::Asked })?;
    let again = harness.settle_turns();
    harness.tell(surveyor, &Survey { by: echo::Proof::Told })?;
    let told = harness.settle_turns();

    let ping = KindId::of::<echo::Ping>();
    let pinged = reliance(ping, AnsweredWith::Kind(KindId::of::<echo::Pong>()), Prover::Program);
    let told_ping = reliance(ping, AnsweredWith::Nothing, Prover::Program);
    assert_eq!(asked_by(&again, surveyor), vec![]);
    assert_eq!(asked_by(&told, surveyor), vec![(echo.id(), told_ping)]);
    assert_eq!(relied_on(&mut harness, echo), vec![told_ping, pinged]);

    Ok(())
}

#[test]
fn a_proof_of_a_native_actor_relies_on_nothing() -> Result<(), Unheard> {
    // Catches a native actor relied on, whose code never moves, which every cast of it would pay an effect for, by a
    // native turn's cast or a guest's.
    let (mut harness, _) = program(root_path::<Listener>(), root_path::<Listener>())?;
    let (surveyor, relier, listener) =
        (harness.root::<Surveyor>(), harness.root::<echo::Relier>(), harness.root::<Listener>());

    harness.tell(surveyor, &Survey { by: echo::Proof::Told })?;
    harness.tell(relier, &echo::Prove { by: echo::Proof::Told })?;
    let turned = harness.settle_turns();

    assert_eq!(asked_by(&turned, surveyor), vec![]);
    assert_eq!(asked_by(&turned, relier), vec![]);
    assert!(matches!(harness.relied(listener), Relied::Native));

    Ok(())
}

#[test]
fn a_guests_cast_and_guard_rely_on_the_guests_they_prove_as_its_module() -> Result<(), Unheard> {
    // Catches a guest's proof that its host writes no reliance for, by a cast or by a guard, and one written as the
    // program's where a module proved it.
    let (mut harness, code) = program(root_path::<Listener>(), root_path::<echo::Timekeeper>())?;
    let (relier, timekeeper) = (harness.root::<echo::Relier>(), harness.root::<echo::Timekeeper>());

    harness.tell(relier, &echo::Prove { by: echo::Proof::Told })?;
    let turned = harness.settle_turns();

    let module = Prover::module(code.id());
    let by_cast = reliance(KindId::of::<echo::Ping>(), AnsweredWith::Nothing, module);
    let by_guard = reliance(KindId::of::<echo::Pong>(), AnsweredWith::Nothing, module);
    assert_eq!(asked_by(&turned, relier), vec![(timekeeper.id(), by_cast)]);
    assert_eq!(asked_by(&turned, timekeeper), vec![(relier.id(), by_guard)]);
    assert_eq!(relied_on(&mut harness, timekeeper), vec![by_cast]);
    assert_eq!(relied_on(&mut harness, relier), vec![by_guard]);

    Ok(())
}

#[test]
fn a_native_guard_relies_on_a_guest_sender_as_the_program() -> Result<(), Unheard> {
    // Catches a guard that proves a guest sender and writes no reliance, and one that names the sender's own module
    // where the receiver's code, the program's, proved it.
    let (mut harness, _) = program(root_path::<Listener>(), root_path::<Listener>())?;
    let (relier, listener) = (harness.root::<echo::Relier>(), harness.root::<Listener>());

    harness.tell(relier, &echo::Prove { by: echo::Proof::Told })?;
    let turned = harness.settle_turns();

    let ponged = reliance(KindId::of::<echo::Pong>(), AnsweredWith::Nothing, Prover::Program);
    assert_eq!(asked_by(&turned, listener), vec![(relier.id(), ponged)]);
    assert_eq!(relied_on(&mut harness, relier), vec![ponged]);

    Ok(())
}

#[test]
fn a_guests_reliances_end_with_it() -> Result<(), Unheard> {
    // Catches reliances kept for a guest that has ended, which a guest seated in its place later would inherit.
    let (mut harness, _) = program(root_path::<echo::Echo>(), root_path::<Listener>())?;
    let (surveyor, echo) = (harness.root::<Surveyor>(), harness.root::<echo::Echo>());
    harness.tell(surveyor, &Survey { by: echo::Proof::Asked })?;
    harness.settle();
    let was_relied_on = !relied_on(&mut harness, echo).is_empty();

    harness.tell(echo, &echo::Shut)?;
    harness.settle();

    assert!(was_relied_on);
    assert!(matches!(harness.relied(echo), Relied::Unheld));

    Ok(())
}
