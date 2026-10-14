//! The turn's tests: what a spawn, a send, and a failed `begin` leave in a turn's list.

use super::*;
use crate::{Beginning, Ending, Fault, Made, Mail, Unreadable, Within};
use aethereal_data::{Bound, Budget, Fnv, KeyName, KindName, Plain};
use alloc::string::ToString;
use alloc::vec;

/// A table that holds the actors listed, and computes a child's id as some fold of its parent's and its segment.
#[derive(Default)]
struct Listed(Vec<(ActorId, ActorPath)>);

impl Made for Listed {
    fn made(&self, id: ActorId) -> bool {
        self.0.iter().any(|(made, _)| *made == id)
    }
}

impl Table for Listed {
    fn resolve(&self, path: &ActorPath) -> Option<ActorId> {
        self.0.iter().find(|(_, held)| held == path).map(|(id, _)| *id)
    }

    fn path(&self, id: ActorId) -> Option<ActorPath> {
        self.0.iter().find(|(made, _)| *made == id).map(|(_, path)| path.clone())
    }

    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        ActorId::new(Fnv::resume(parent.bits()).bytes(segment.to_string().as_bytes()).finish())
    }
}

/// Mail that asks a leaf to say it was poked.
#[derive(Plain)]
struct Poke;

impl Kind for Poke {
    const NAME: KindName = KindName::new("aether.test.poke");
}

/// A leaf's answer to a poke.
#[derive(Plain)]
struct Poked;

impl Kind for Poked {
    const NAME: KindName = KindName::new("aether.test.poked");
}

/// Mail that asks a leaf to close.
#[derive(Plain)]
struct Shut;

impl Kind for Shut {
    const NAME: KindName = KindName::new("aether.test.shut");
}

/// Mail that hands a leaf a reference.
#[derive(Bound)]
struct Meet {
    other: ActorRef,
}

impl Kind for Meet {
    const NAME: KindName = KindName::new("aether.test.meet");
}

/// Why a leaf told to fail did not begin.
#[derive(Debug, PartialEq, Plain)]
struct ToldToFail;

impl Kind for ToldToFail {
    const NAME: KindName = KindName::new("aether.test.told_to_fail");
}

/// Whether a leaf's `begin` fails.
#[derive(Plain)]
struct LeafConfig {
    fails: bool,
}

impl Kind for LeafConfig {
    const NAME: KindName = KindName::new("aether.test.leaf_config");
}

/// An actor with no children.
#[derive(Plain)]
struct Leaf;

impl Kind for Leaf {
    const NAME: KindName = KindName::new("aether.test.leaf");
}

impl Actor for Leaf {
    type Config = LeafConfig;
    type Key = u64;

    fn begin(config: LeafConfig, _turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        if config.fails {
            return Err(Fault::new(&ToldToFail)?);
        }

        Ok(Self)
    }

    fn handle(&mut self, mail: Mail<'_>, turn: &mut Turn<'_>) -> Result<(), Fault> {
        if mail.read::<Poke>()?.is_some() {
            turn.send(To::Sender, &Poked)?;
        } else if mail.read::<Shut>()?.is_some() {
            turn.close();
        } else if let Some(Meet { other }) = mail.read()? {
            turn.send(To::Held(other), &Poke)?;
        }

        Ok(())
    }

    fn end(self, _turn: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// How many leaves a branch spawns, and whether the last of them fails to begin.
#[derive(Plain)]
struct BranchConfig {
    leaves: u64,
    last_fails: bool,
}

impl Kind for BranchConfig {
    const NAME: KindName = KindName::new("aether.test.branch_config");
}

/// A singleton that spawns leaves in its `begin`.
#[derive(Plain)]
struct Branch;

impl Kind for Branch {
    const NAME: KindName = KindName::new("aether.test.branch");
}

impl Actor for Branch {
    type Config = BranchConfig;
    type Key = ();

    fn begin(config: BranchConfig, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        for key in 0..config.leaves {
            let is_last = key + 1 == config.leaves;
            let fails = is_last && config.last_fails;
            if let Err(Refused::Began(fault)) = turn.spawn::<Leaf>(&key, LeafConfig { fails }) {
                return Err(fault);
            }
        }

        Ok(Self)
    }

    fn handle(&mut self, _mail: Mail<'_>, _turn: &mut Turn<'_>) -> Result<(), Fault> {
        Ok(())
    }

    fn end(self, _turn: &mut Ending<'_>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A singleton that tells its parent it began, and on ending tells it why.
#[derive(Plain)]
struct Herald;

impl Kind for Herald {
    const NAME: KindName = KindName::new("aether.test.herald");
}

impl Actor for Herald {
    type Config = Poke;
    type Key = ();

    fn begin(_config: Poke, turn: &mut Beginning<'_>) -> Result<Self, Fault> {
        turn.send(ToWithoutMail::Parent, &Poked)?;

        Ok(Self)
    }

    fn handle(&mut self, _mail: Mail<'_>, _turn: &mut Turn<'_>) -> Result<(), Fault> {
        Ok(())
    }

    fn end(self, turn: &mut Ending<'_>) -> Result<(), Fault> {
        if turn.why() == Why::Closed {
            turn.send(ToWithoutMail::Parent, &Shut)?;
        }

        Ok(())
    }
}

const ROOT: ActorPath = ActorPath::new("aether.test.root");
const ME: ActorId = ActorId::new(1);

/// The spawn effect of the leaf under `key`.
fn leaf_spawn(key: u64) -> What {
    What::SpawnInstanced { from: Code::Linked(Leaf::NAME), key: KeyName::new(["0", "1"][usize::from(key == 1)]) }
}

#[test]
fn child_begins_inside_its_parents_turn() {
    // Catches a child's effects placed before its own spawn or signed by its parent, and a child that is not
    // held aside under the id and path its spawn gives it.
    let table = Listed::default();
    let mut asked = Asked::new();
    let mut turn = Turn::new(ME, ROOT, &table, &mut asked, &[]);
    let Ok(branch) = turn.spawn::<Branch>(&(), BranchConfig { leaves: 2, last_fails: false }) else {
        panic!("the branch and its leaves begin");
    };

    let branch_segment = PathSegment::new(Branch::NAME, &());
    assert_eq!(branch.id(), table.child(ME, &branch_segment));
    let by_branch = |what| Effect::new(branch.id(), what);
    let spawn_branch = Effect::new(ME, What::Spawn { from: Code::Linked(Branch::NAME) });
    assert_eq!(asked.effects(), [spawn_branch, by_branch(leaf_spawn(0)), by_branch(leaf_spawn(1))]);

    let (_, stood) = asked.into_parts();
    let paths: Vec<_> = stood.iter().map(|child| child.path().clone()).collect();
    let leaves = [
        "aether.test.root/aether.test.branch/aether.test.leaf:0",
        "aether.test.root/aether.test.branch/aether.test.leaf:1",
    ];
    assert_eq!(
        paths,
        [ActorPath::new(leaves[0]), ActorPath::new(leaves[1]), ActorPath::new("aether.test.root/aether.test.branch")]
    );
    assert_eq!(stood[2].id(), branch.id());
}

#[test]
fn used_key_is_refused_and_asks_for_nothing() {
    // Catches a second child at a key the turn has used, and one at a key the table holds from an earlier step.
    let held = ActorPath::new("aether.test.root/aether.test.leaf:1");
    let table = Listed::default();
    let held_id = table.child(ME, &held.last());
    let table = Listed(vec![(held_id, held)]);
    let mut asked = Asked::new();
    let mut turn = Turn::new(ME, ROOT, &table, &mut asked, &[]);

    assert!(turn.spawn::<Leaf>(&0, LeafConfig { fails: false }).is_ok());
    assert_eq!(turn.spawn::<Leaf>(&0, LeafConfig { fails: false }), Err(Refused::KeyTaken));
    assert_eq!(turn.spawn::<Leaf>(&1, LeafConfig { fails: false }), Err(Refused::KeyTaken));
    assert_eq!(asked.effects(), [Effect::new(ME, leaf_spawn(0))]);
}

#[test]
fn failed_begin_leaves_its_fault_and_no_child() {
    // Catches a failed child held aside as if it began, the children it stood up outliving it, and a key that a
    // failed begin used up.
    let table = Listed::default();
    let mut asked = Asked::new();
    let mut turn = Turn::new(ME, ROOT, &table, &mut asked, &[]);
    let refused = turn.spawn::<Branch>(&(), BranchConfig { leaves: 2, last_fails: true });
    let Err(Refused::Began(fault)) = refused else {
        panic!("the branch's last leaf fails, and the branch with it");
    };
    assert_eq!(fault.read(&mut Budget::new(0)), Ok(Some(ToldToFail)));

    let branch = table.child(ME, &PathSegment::new(Branch::NAME, &()));
    let last_leaf = table.child(branch, &PathSegment::new(Leaf::NAME, &1u64));
    let spawn_branch = Effect::new(ME, What::Spawn { from: Code::Linked(Branch::NAME) });
    let expected = [
        spawn_branch.clone(),
        Effect::new(branch, leaf_spawn(0)),
        Effect::new(branch, leaf_spawn(1)),
        Effect::new(last_leaf, What::Faulted),
        Effect::new(branch, What::Faulted),
    ];
    assert_eq!(asked.effects(), expected);

    let mut turn = Turn::new(ME, ROOT, &table, &mut asked, &[]);
    assert!(turn.spawn::<Branch>(&(), BranchConfig { leaves: 0, last_fails: false }).is_ok());
    let (effects, stood) = asked.into_parts();
    assert_eq!(effects.last(), Some(&spawn_branch));
    assert_eq!(stood.len(), 1);
}

#[test]
fn begin_and_end_send_as_their_own_actor() {
    // Catches a `begin`'s mail signed by the parent whose turn it runs in or placed before its spawn, and a
    // closing turn that does not learn why it runs.
    let table = Listed::default();
    let mut asked = Asked::new();
    let mut turn = Turn::new(ME, ROOT, &table, &mut asked, &[]);
    let Ok(herald) = turn.spawn::<Herald>(&(), Poke) else {
        panic!("the herald begins");
    };
    let to_parent = |kind| What::Send { to: To::Parent, kind, bytes: Vec::new() };
    let spawn = Effect::new(ME, What::Spawn { from: Code::Linked(Herald::NAME) });
    assert_eq!(asked.effects(), [spawn, Effect::new(herald.id(), to_parent(KindId::of::<Poked>()))]);

    let end = |why| {
        let mut asked = Asked::new();
        let mut ending = Ending::new(herald.id(), ROOT, &table, &mut asked, why);
        let ended = Box::new(Herald).end(&mut ending);

        (ended, asked.into_parts().0)
    };
    assert_eq!(end(Why::Closed), (Ok(()), vec![Effect::new(herald.id(), to_parent(KindId::of::<Shut>()))]));
    assert_eq!(end(Why::Faulted), (Ok(()), Vec::new()));
}

#[test]
fn child_past_the_deepest_path_is_refused() {
    // Catches a spawn that asks for a child the table could never name.
    let Ok(deepest) =
        (1..ActorPath::MAX_DEPTH).try_fold(ROOT, |path, _| path.child(&PathSegment::new(Leaf::NAME, &0u64)))
    else {
        panic!("a path of the greatest depth fits");
    };
    let table = Listed::default();
    let mut asked = Asked::new();
    let mut turn = Turn::new(ME, deepest, &table, &mut asked, &[]);

    assert_eq!(turn.spawn::<Leaf>(&0, LeafConfig { fails: false }), Err(Refused::TooDeep));
    assert!(asked.effects().is_empty());
}

#[test]
fn handler_asks_by_the_kind_of_its_mail() {
    // Catches mail read as a kind it is not, a send that loses its kind or its target, and a reference in mail
    // taken without the table's proof.
    let other = ActorId::new(9);
    let table = Listed(vec![(other, ActorPath::new("aether.test.other"))]);
    let mut leaf: Box<dyn Runs> = Box::new(Leaf);
    let handle = |leaf: &mut Box<dyn Runs>, table: &Listed, kind, bytes: &[u8]| {
        let mut asked = Asked::new();
        let mut turn = Turn::new(ME, ROOT, table, &mut asked, &[]);
        let handled = leaf.handle(Mail::new(kind, bytes, other, table, 64), &mut turn);

        (handled, asked.into_parts().0)
    };

    let poked = What::Send { to: To::Sender, kind: KindId::of::<Poked>(), bytes: Vec::new() };
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Poke>(), &[]), (Ok(()), vec![Effect::new(ME, poked)]));
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Shut>(), &[]), (Ok(()), vec![Effect::new(ME, What::Close)]));
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Poked>(), &[]), (Ok(()), Vec::new()));

    let mut out = Writer::<Within>::new(crate::WithinWriter::new());
    assert_eq!(ActorRef::new(other).encode(&mut out), Ok(()));
    let meet = out.into_bytes();
    let poke = What::Send { to: To::Held(ActorRef::new(other)), kind: KindId::of::<Poke>(), bytes: Vec::new() };
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Meet>(), &meet), (Ok(()), vec![Effect::new(ME, poke)]));

    let (handled, effects) = handle(&mut leaf, &Listed::default(), KindId::of::<Meet>(), &meet);
    let Err(fault) = handled else {
        panic!("the mail holds a reference to an actor this table has not made");
    };
    assert_eq!(fault.read(&mut Budget::new(0)), Ok(Some(Unreadable)));
    assert!(effects.is_empty());
}
