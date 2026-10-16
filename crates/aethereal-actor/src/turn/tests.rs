//! The turn's tests: what a spawn, a send, and a failed `begin` leave in a turn's list.

use super::*;
use crate::{Beginning, Ending, Fault, Item, Mail, MailId, Payload, Span, Within};
use aethereal_data::{Bound, Budget, Fnv, KeyName, KindName, Plain};
use alloc::string::ToString;
use alloc::vec;

/// A table that holds the actors listed, and computes a child's id as some fold of its parent's and its segment.
#[derive(Default)]
struct Listed(Vec<(Seated, ActorPath)>);

impl Listed {
    /// The seat of the actor listed under `id`.
    fn seat(&self, id: ActorId) -> Option<SeatNumber> {
        self.0.iter().find(|(made, _)| made.id == id).map(|(made, _)| made.seat)
    }
}

impl Table for Listed {
    fn resolve(&self, path: &ActorPath) -> Option<Seated> {
        self.0.iter().find(|(_, held)| held == path).map(|(made, _)| *made)
    }

    fn path(&self, id: ActorId) -> Option<ActorPath> {
        self.0.iter().find(|(made, _)| made.id == id).map(|(_, path)| path.clone())
    }

    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        ActorId::new(Fnv::resume(parent.bits()).bytes(segment.to_string().as_bytes()).finish())
    }

    fn is_live(&self, id: ActorId) -> bool {
        self.0.iter().any(|(made, _)| made.id == id)
    }

    fn child_seat(&self, id: ActorId) -> Option<SeatNumber> {
        self.seat(id)
    }

    /// Every actor listed is a leaf.
    fn declared(&self, id: ActorId) -> Option<&'static Declared> {
        let is_listed = self.is_live(id);

        is_listed.then(Declared::of::<Leaf>)
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
    other: ErasedActorRef,
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
    type Handles = (Poke, Poked, Shut, Meet);
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: LeafConfig, _turn: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
        if config.fails {
            return Err(Fault::new(&ToldToFail)?);
        }

        Ok(Self)
    }

    fn end(self, _turn: &mut Ending<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Poke> for Leaf {
    type Sends = (ToSender<Poked>,);

    fn handle(&mut self, _: Poke, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        Ok(turn.reply(&Poked)?)
    }
}

/// A leaf takes the answer to a poke, and does nothing with it.
impl Handles<Poked> for Leaf {
    type Sends = ();

    fn handle(&mut self, _: Poked, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Shut> for Leaf {
    type Sends = ();

    fn handle(&mut self, _: Shut, turn: &mut Turn<'_, ()>) -> Result<(), Fault> {
        turn.close();

        Ok(())
    }
}

impl Handles<Meet> for Leaf {
    type Sends = (ToType<Self, Poke>,);

    fn handle(&mut self, Meet { other }: Meet, turn: &mut Turn<'_, Self::Sends>) -> Result<(), Fault> {
        Ok(turn.to::<Self>(To::Held(other)).send(&Poke)?)
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
    type Handles = ();
    type BeginSends = ();
    type EndSends = ();

    fn begin(config: BranchConfig, turn: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
        for key in 0..config.leaves {
            let is_last = key + 1 == config.leaves;
            let fails = is_last && config.last_fails;
            if let Err(Refused::Began(fault)) = turn.spawn::<Leaf>(&key, LeafConfig { fails }) {
                return Err(fault);
            }
        }

        Ok(Self)
    }

    fn end(self, _turn: &mut Ending<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A singleton that tells its parent it began, and on ending tells it why.
#[derive(Plain)]
struct Herald;

impl Kind for Herald {
    const NAME: KindName = KindName::new("aether.test.herald");
}

/// A herald's parent is a leaf.
impl Actor for Herald {
    type Config = Poke;
    type Key = ();
    type Handles = ();
    type BeginSends = (ToType<Leaf, Poked>,);
    type EndSends = (ToType<Leaf, Shut>,);

    fn begin(_config: Poke, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to::<Leaf>(ToWithoutMail::Parent).send(&Poked)?;

        Ok(Self)
    }

    fn end(self, turn: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        if turn.why() == Why::Closed {
            turn.to::<Leaf>(ToWithoutMail::Parent).send(&Shut)?;
        }

        Ok(())
    }
}

/// Where a turn's first piece of mail with no bytes is written: at the start of an arena of its own.
const NOTHING: Payload = Payload::InArena(Span { slot: 0, start: 0, len: 0 });

static ROOT: ActorPath = ActorPath::new("aether.test.root");
const ME: Seated = seated(1, 100);
/// Where in its chain the item a test's turn handles came.
const CHAIN: Chain = Chain { root: MailId { sender: ME.id, count: 1 }, depth: 1 };

/// The turn of the leaf `me` names, at `path`, reading `table` and adding to `asked`, handling a poke from
/// `source`, through a handler that may send `S`.
fn opened<'a, S: Sends>(
    me: Seated,
    path: &'a ActorPath,
    above: Above,
    table: &'a dyn Table,
    asked: &'a mut Asked,
    source: Seated,
) -> Turn<'a, S> {
    let poke = Role::Kind(KindId::of::<Poke>());

    Turning::new(me, path, above, table, asked, 0, source, CHAIN).declaring::<Leaf, S>(poke)
}

/// The actor `bits` names, in the seat numbered `seat`.
const fn seated(bits: u64, seat: u64) -> Seated {
    Seated { id: ActorId::new(bits), seat: SeatNumber::new(seat) }
}

/// The spawn effect of the leaf under `key`, in the seat numbered `seat`.
fn leaf_spawn(key: u64, seat: u64) -> What {
    let key = KeyName::new(["0", "1"][usize::from(key == 1)]);

    What::SpawnInstanced { from: Code::Linked(Declared::of::<Leaf>()), key, seat: SeatNumber::new(seat) }
}

/// The spawn effect of the branch, in the seat numbered `seat`.
fn branch_spawn(seat: u64) -> What {
    What::Spawn { from: Code::Linked(Declared::of::<Branch>()), seat: SeatNumber::new(seat) }
}

#[test]
fn child_begins_inside_its_parents_turn() {
    // Catches a child's effects placed before its own spawn or signed by its parent, a child that is not held aside
    // under the id, path, and seat its spawn gives it, and two children given one seat.
    let table = Listed::default();
    let mut asked = Asked::new();
    let mut turn = opened::<()>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let Ok(branch) = turn.spawn::<Branch>(&(), BranchConfig { leaves: 2, last_fails: false }) else {
        panic!("the branch and its leaves begin");
    };

    let branch_segment = PathSegment::new(Branch::NAME, &());
    assert_eq!(branch.id(), table.child(ME.id, &branch_segment));
    let by_branch = |what| Effect::new(branch.seated(), what);
    let spawn_branch = Effect::new(ME, branch_spawn(0));
    assert_eq!(asked.effects(), [spawn_branch, by_branch(leaf_spawn(0, 1)), by_branch(leaf_spawn(1, 2))]);

    let (_, stood, _, _) = asked.into_parts();
    let paths: Vec<_> = stood.iter().map(|child| child.path().clone()).collect();
    let leaves = [
        "aether.test.root/aether.test.branch/aether.test.leaf:0",
        "aether.test.root/aether.test.branch/aether.test.leaf:1",
    ];
    assert_eq!(
        paths,
        [ActorPath::new(leaves[0]), ActorPath::new(leaves[1]), ActorPath::new("aether.test.root/aether.test.branch")]
    );
    assert_eq!(stood[2].seated(), branch.seated());
    let seats: Vec<u64> = stood.iter().map(|child| child.seated().seat.number()).collect();
    assert_eq!(seats, [1, 2, 0]);
}

#[test]
fn used_key_is_refused_and_asks_for_nothing() {
    // Catches a second child at a key the turn has used, and one at a key the table holds from an earlier step.
    let held = ActorPath::new("aether.test.root/aether.test.leaf:1");
    let table = Listed::default();
    let held_id = table.child(ME.id, &held.last());
    let table = Listed(vec![(Seated { id: held_id, seat: SeatNumber::new(7) }, held)]);
    let mut asked = Asked::new();
    let mut turn = opened::<()>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);

    assert!(turn.spawn::<Leaf>(&0, LeafConfig { fails: false }).is_ok());
    assert_eq!(turn.spawn::<Leaf>(&0, LeafConfig { fails: false }), Err(Refused::KeyTaken));
    assert_eq!(turn.spawn::<Leaf>(&1, LeafConfig { fails: false }), Err(Refused::KeyTaken));
    assert_eq!(asked.effects(), [Effect::new(ME, leaf_spawn(0, 0))]);
}

#[test]
fn failed_begin_leaves_its_fault_and_no_child() {
    // Catches a failed child held aside as if it began, the children it stood up outliving it, and a key or a seat
    // that a failed begin used up.
    let table = Listed::default();
    let mut asked = Asked::new();
    let mut turn = opened::<()>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let refused = turn.spawn::<Branch>(&(), BranchConfig { leaves: 2, last_fails: true });
    let Err(Refused::Began(fault)) = refused else {
        panic!("the branch's last leaf fails, and the branch with it");
    };
    assert_eq!(fault.read(&mut Budget::new(0)), Ok(Some(ToldToFail)));

    let branch = Seated { id: table.child(ME.id, &PathSegment::new(Branch::NAME, &())), seat: SeatNumber::new(0) };
    let last_leaf =
        Seated { id: table.child(branch.id, &PathSegment::new(Leaf::NAME, &1u64)), seat: SeatNumber::new(2) };
    let spawn_branch = Effect::new(ME, branch_spawn(0));
    let expected = [
        spawn_branch.clone(),
        Effect::new(branch, leaf_spawn(0, 1)),
        Effect::new(branch, leaf_spawn(1, 2)),
        Effect::new(last_leaf, What::Faulted),
        Effect::new(branch, What::Faulted),
    ];
    assert_eq!(asked.effects(), expected);

    let mut turn = opened::<()>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    assert!(turn.spawn::<Branch>(&(), BranchConfig { leaves: 0, last_fails: false }).is_ok());
    let (effects, stood, _, seats) = asked.into_parts();
    assert_eq!(effects.last(), Some(&spawn_branch));
    assert_eq!(stood.len(), 1);
    assert_eq!(seats.into_parts().0, [SeatNumber::new(2), SeatNumber::new(1)]);
}

#[test]
fn begin_and_end_send_as_their_own_actor() {
    // Catches a `begin`'s mail signed by the parent whose turn it runs in or placed before its spawn, and a
    // closing turn that does not learn why it runs.
    let table = Listed(vec![(ME, ROOT.clone())]);
    let mut asked = Asked::new();
    let mut turn = opened::<()>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let Ok(herald) = turn.spawn::<Herald>(&(), Poke) else {
        panic!("the herald begins");
    };
    let to_parent = |kind| What::Send { to: ME, count: 1, kind, bytes: NOTHING };
    let spawn = Effect::new(ME, What::Spawn { from: Code::Linked(Declared::of::<Herald>()), seat: SeatNumber::new(0) });
    assert_eq!(asked.effects(), [spawn, Effect::new(herald.seated(), to_parent(KindId::of::<Poked>()))]);

    let Ok(path) = ROOT.child(&PathSegment::new(Herald::NAME, &())) else {
        panic!("a child of the root fits");
    };
    let end = |why| {
        let mut asked = Asked::new();
        let above = Above::Actor(ME, Declared::of::<Leaf>());
        let closing = Closing::new(herald.seated(), &path, above, &table, &mut asked, 0, why);
        let ended = Runs::end(Box::new(Herald), closing);

        (ended, asked.into_parts().0)
    };
    assert_eq!(end(Why::Closed), (Ok(()), vec![Effect::new(herald.seated(), to_parent(KindId::of::<Shut>()))]));
    assert_eq!(end(Why::Faulted), (Ok(()), Vec::new()));
}

#[test]
fn child_past_the_deepest_path_is_refused() {
    // Catches a spawn that asks for a child the table could never name.
    let Ok(deepest) =
        (1..ActorPath::MAX_DEPTH).try_fold(ROOT.clone(), |path, _| path.child(&PathSegment::new(Leaf::NAME, &0u64)))
    else {
        panic!("a path of the greatest depth fits");
    };
    let table = Listed::default();
    let mut asked = Asked::new();
    let mut turn = opened::<()>(ME, &deepest, Above::Chassis, &table, &mut asked, ME);

    assert_eq!(turn.spawn::<Leaf>(&0, LeafConfig { fails: false }), Err(Refused::TooDeep));
    assert!(asked.effects().is_empty());
}

#[test]
fn handler_asks_by_the_kind_of_its_mail() {
    // Catches mail read as a kind it is not, a send that loses its kind or its target, a reply left naming the
    // sender instead of the actor the mail came from, and a reference in mail read as another actor.
    let other = seated(9, 5);
    let table = Listed(vec![(other, ActorPath::new("aether.test.other"))]);
    let mut leaf: Box<dyn Runs> = Box::new(Leaf);
    let handle = |leaf: &mut Box<dyn Runs>, table: &Listed, kind, bytes: &[u8]| {
        let mut asked = Asked::new();
        let turning = Turning::new(ME, &ROOT, Above::Chassis, table, &mut asked, 0, other, CHAIN);
        let mail = Mail::new(kind, bytes, other, MailId { sender: other.id, count: 1 }, 1, 64);
        let handled = leaf.handle(Item::Mail(mail), turning);

        (handled, asked.into_parts().0)
    };

    let poked = What::Send { to: other, count: 1, kind: KindId::of::<Poked>(), bytes: NOTHING };
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Poke>(), &[]), (Ok(()), vec![Effect::new(ME, poked)]));
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Shut>(), &[]), (Ok(()), vec![Effect::new(ME, What::Close)]));
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Poked>(), &[]), (Ok(()), Vec::new()));

    let mut out = Writer::<Within>::new(crate::WithinWriter::new());
    assert_eq!(ErasedActorRef::new(other).encode(&mut out), Ok(()));
    let meet = out.into_bytes();
    let poke = What::Send { to: other, count: 1, kind: KindId::of::<Poke>(), bytes: NOTHING };
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Meet>(), &meet), (Ok(()), vec![Effect::new(ME, poke)]));
}

#[test]
fn mail_to_a_parent_goes_only_while_it_is_live() {
    // Catches a send to the parent that skips its liveness, or names another actor than the one that spawned it.
    let parent = seated(7, 3);
    let Ok(path) = ROOT.child(&PathSegment::new(Leaf::NAME, &0u64)) else {
        panic!("a child of the root fits");
    };
    let send = |table: &Listed| {
        let mut asked = Asked::new();
        let above = Above::Actor(parent, Declared::of::<Leaf>());
        let mut turn = opened::<<Leaf as Handles<Meet>>::Sends>(ME, &path, above, table, &mut asked, ME);
        let sent = turn.to::<Leaf>(To::Parent).send(&Poke);

        (sent.is_ok(), asked.into_parts().0)
    };

    let live = Listed(vec![(parent, ROOT.clone())]);
    let to_parent = What::Send { to: parent, count: 1, kind: KindId::of::<Poke>(), bytes: NOTHING };
    assert_eq!(send(&live), (true, vec![Effect::new(ME, to_parent)]));
    assert_eq!(send(&Listed::default()), (true, Vec::new()));
}
