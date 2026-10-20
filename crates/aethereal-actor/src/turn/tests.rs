//! The turn's tests: what a spawn, a send, and a failed `begin` leave in a turn's effect buffers.

use super::*;
use crate::{
    Beginning, Carries, Claim, Claimed, Ended, Ending, Fault, How, Item, Kept, Mail, MailId, NotHeld, Payload,
    Protocol, Resource, Subscribers,
};
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

    fn child(&self, parent: ActorId, segment: &PathSegment) -> ActorId {
        ActorId::new(Fnv::resume(parent.bits()).bytes(segment.to_string().as_bytes()).finish())
    }

    fn is_live(&self, id: ActorId) -> bool {
        self.0.iter().any(|(made, _)| made.id == id)
    }

    fn held(&self, id: ActorId) -> bool {
        self.is_live(id)
    }

    fn child_seat(&self, id: ActorId) -> Option<SeatNumber> {
        self.seat(id)
    }

    fn blob(&self, _: crate::BlobId) -> Option<&[u8]> {
        None
    }

    fn blob_hash(&self, _: crate::BlobId) -> Option<BlobHash> {
        None
    }

    fn now(&self) -> Time {
        Time::ZERO
    }

    fn runs_stage(&self, _: KindId) -> bool {
        true
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

    fn begin(config: LeafConfig, turn: &mut Beginning<'_, ()>) -> Result<Self, Fault> {
        if config.fails {
            turn.log(Level::Warn, "told to fail");
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
        Ok(turn.send(To::Held(other), &Poke)?)
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

/// A singleton that spawns leaves in its `begin`, and takes their answers to its pokes.
#[derive(Plain)]
struct Branch;

impl Kind for Branch {
    const NAME: KindName = KindName::new("aether.test.branch");
}

impl Actor for Branch {
    type Config = BranchConfig;
    type Key = ();
    type Handles = (Poked,);
    type BeginSends = (ToType<Leaf, Poke>,);
    type EndSends = ();

    /// Spawns its leaves, poking each that begins.
    fn begin(config: BranchConfig, turn: &mut Beginning<'_, Self::BeginSends>) -> Result<Self, Fault> {
        for key in 0..config.leaves {
            let is_last = key + 1 == config.leaves;
            let fails = is_last && config.last_fails;
            match turn.spawn::<Leaf>(&key, LeafConfig { fails }) {
                Ok(leaf) => turn.send_to(leaf, &Poke)?,
                Err(Refused::Began(fault)) => return Err(fault),
                Err(_) => {}
            }
        }

        Ok(Self)
    }

    fn end(self, _turn: &mut Ending<'_, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A branch takes the answer to a poke, and does nothing with it.
impl Handles<Poked> for Branch {
    type Sends = ();

    fn handle(&mut self, _: Poked, _: &mut Turn<'_, ()>) -> Result<(), Fault> {
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
        turn.send(ToWithoutMail::Parent, &Poked)?;

        Ok(Self)
    }

    fn end(self, turn: &mut Ending<'_, Self::EndSends>) -> Result<(), Fault> {
        if turn.why() == Why::Closed {
            turn.send(ToWithoutMail::Parent, &Shut)?;
        }

        Ok(())
    }
}

/// Where a turn's first piece of mail with no bytes is written: at the start of an arena of its own.
const NOTHING: Payload = Payload::InArena { slot: 0, start: 0, len: 0, pieces: 1 };

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

    // A turn built for a test lends no instance its handler reads, so it is lent a part's empty instances for as long
    // as the turn lives.
    let instances = Box::leak(Box::default());

    Turning::new(me, path, above, table, &(), asked, instances, Keeps::Claimed, 0, source, CHAIN, Tie::TELL)
        .declaring::<Leaf, S>(poke)
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

    let (_, stood, _, _, _) = asked.into_parts();
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
    // Catches a failed child held aside as if it began, the children it stood up outliving it, a key or a seat that a
    // failed begin used up, mail it or a child of it sent kept, and a line it logged lost.
    let table = Listed::default();
    let mut asked = Asked::new();
    let mut turn = opened::<()>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let refused = turn.spawn::<Branch>(&(), BranchConfig { leaves: 2, last_fails: true });
    let Err(Refused::Began(fault)) = refused else {
        panic!("the branch's last leaf fails, and the branch with it");
    };
    assert_eq!(fault.read(&mut Budget::new(0)), Ok(Some(ToldToFail)));

    assert_eq!(asked.effects(), [], "the failed branch's spawn, and its leaves', are taken back");
    assert_eq!(asked.mailed(), [], "the poke of the leaf that began is taken back with the branch");
    let lines: Vec<&str> = asked.lines().iter().map(|line| asked.text(line)).collect();
    assert_eq!(lines, ["told to fail"]);

    let mut turn = opened::<()>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    assert!(turn.spawn::<Branch>(&(), BranchConfig { leaves: 0, last_fails: false }).is_ok());
    let (buffers, stood, _, seats, _) = asked.into_parts();
    assert_eq!(buffers.effects, [Effect::new(ME, branch_spawn(0))]);
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
    let to_parent = |kind| Mailed { by: herald.seated(), to: ME, count: 1, kind, bytes: NOTHING, tie: Tie::TELL };
    let spawn = Effect::new(ME, What::Spawn { from: Code::Linked(Declared::of::<Herald>()), seat: SeatNumber::new(0) });
    assert_eq!(asked.effects(), [spawn]);
    assert_eq!(asked.mailed(), [to_parent(KindId::of::<Poked>())]);

    let Ok(path) = ROOT.child(&PathSegment::new(Herald::NAME, &())) else {
        panic!("a child of the root fits");
    };
    let end = |why| {
        let mut asked = Asked::new();
        let above = Above::Actor(ME, Declared::of::<Leaf>());
        let mut instances = Instances::default();
        let closing =
            Closing::new(herald.seated(), &path, above, &table, &mut asked, &mut instances, Keeps::Claimed, 0, why);
        let ended = Runs::end(Box::new(Herald), closing);

        (ended, asked.into_parts().0.mailed)
    };
    assert_eq!(end(Why::Closed), (Ok(()), vec![to_parent(KindId::of::<Shut>())]));
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
        let mut instances = Instances::default();
        let turning = Turning::new(
            ME,
            &ROOT,
            Above::Chassis,
            table,
            &(),
            &mut asked,
            &mut instances,
            Keeps::Claimed,
            0,
            other,
            CHAIN,
            Tie::TELL,
        );
        let mail =
            Mail::new(kind, bytes, &crate::PinSource::NOTHING, other, MailId { sender: other.id, count: 1 }, 1, 64);
        let handled = leaf.turn(Item::Mail(mail), turning);

        let Buffers { mailed, effects, .. } = asked.into_parts().0;

        (handled, mailed, effects)
    };

    let poked = Mailed { by: ME, to: other, count: 1, kind: KindId::of::<Poked>(), bytes: NOTHING, tie: Tie::TELL };
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Poke>(), &[]), (Ok(()), vec![poked], Vec::new()));
    let closed = vec![Effect::new(ME, What::Close)];
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Shut>(), &[]), (Ok(()), Vec::new(), closed));
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Poked>(), &[]), (Ok(()), Vec::new(), Vec::new()));

    let meet = Written::of(&ErasedActorRef::new(other)).map(|written| written.bytes().to_vec());
    let Ok(meet) = meet else {
        panic!("a reference is written");
    };
    let poke = Mailed { by: ME, to: other, count: 1, kind: KindId::of::<Poke>(), bytes: NOTHING, tie: Tie::TELL };
    assert_eq!(handle(&mut leaf, &table, KindId::of::<Meet>(), &meet), (Ok(()), vec![poke], Vec::new()));
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
        let sent = turn.send(To::Parent, &Poke);

        (sent.is_ok(), asked.into_parts().0.mailed)
    };

    let live = Listed(vec![(parent, ROOT.clone())]);
    let to_parent = Mailed { by: ME, to: parent, count: 1, kind: KindId::of::<Poke>(), bytes: NOTHING, tie: Tie::TELL };
    assert_eq!(send(&live), (true, vec![to_parent]));
    assert_eq!(send(&Listed::default()), (true, Vec::new()));
}

#[test]
fn a_reference_casts_to_a_protocol_only_when_its_type_lists_every_kind() {
    // Catches a cast that answers when its actor's type lists some kinds of the protocol and not all, one that answers
    // for an actor the table does not hold, and one that loses the seat it was handed.
    let leaf = seated(7, 3);
    let table = Listed(vec![(leaf, ROOT.clone())]);
    let mut asked = Asked::new();
    let turn = opened::<()>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let held = ErasedActorRef::new(leaf);

    let cast = turn.cast::<(Shut, Poke)>(held);
    let converted: ProtocolRef<(Poke, Shut)> = ActorRef::<Leaf>::new(leaf).protocol();
    assert_eq!(cast, Some(converted.narrow()));
    assert_eq!(turn.cast::<(Poke, ToldToFail)>(held), None);
    assert_eq!(turn.cast::<(Poke,)>(ErasedActorRef::new(seated(8, 4))), None);
}

#[test]
fn mail_sent_to_all_is_a_piece_for_each_receiver_with_its_bytes_written_once() {
    // Catches a send to all that writes its bytes again for each receiver, sends one of them nothing or two pieces,
    // gives two pieces one count, or sends them out of the order given.
    let receivers = [seated(9, 5), seated(8, 6), seated(7, 7)];
    let table = Listed(Vec::new());
    let mut asked = Asked::new();
    let mut turn = opened::<(ToType<Leaf, Meet>,)>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let references = receivers.map(ActorRef::<Leaf>::new);
    let meeting = Meet { other: ErasedActorRef::new(receivers[0]) };

    assert_eq!(turn.send_to_all(&references, &meeting), Ok(()));
    assert_eq!(turn.send_to_all(&references[..0], &meeting), Ok(()));

    let (buffers, _, _, _, _) = asked.into_parts();
    let seen: Vec<(Seated, u64)> = buffers.mailed.iter().map(|mailed| (mailed.to, mailed.count)).collect();
    assert_eq!(seen, [(receivers[0], 1), (receivers[1], 2), (receivers[2], 3)]);
    let first = buffers.mailed[0].bytes;
    let is_shared = buffers.mailed.iter().all(|mailed| mailed.bytes == first);
    assert!(is_shared, "every piece reads the one copy of the bytes");
}

#[test]
fn mail_sent_through_a_protocol_reference_reaches_the_actor_it_refers_to() {
    // Catches a protocol reference that sends to another seat than the one it was made with, alone or in a send to
    // all.
    let receivers = [seated(9, 5), seated(8, 6), seated(7, 7)];
    let table = Listed(Vec::new());
    let mut asked = Asked::new();
    let mut turn = opened::<(ToAny<(Poke,), Poke>,)>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let references: [ProtocolRef<(Poke,)>; 3] = receivers.map(|receiver| ActorRef::<Leaf>::new(receiver).protocol());

    assert_eq!(turn.send_to(references[0], &Poke), Ok(()));
    assert_eq!(turn.send_to_all(&references[1..], &Poke), Ok(()));

    let (buffers, _, _, _, _) = asked.into_parts();
    let seen: Vec<(Seated, KindId)> = buffers.mailed.iter().map(|mailed| (mailed.to, mailed.kind)).collect();
    let poke = KindId::of::<Poke>();
    assert_eq!(seen, [(receivers[0], poke), (receivers[1], poke), (receivers[2], poke)]);
}

/// What a leaf is poked, shut, and introduced by, as a named protocol: a poke is answered, a shut is not, and nor is
/// a meeting, though a leaf sends a poke on to the actor it meets, which is no reply.
struct Poking;

impl Protocol for Poking {
    type Kinds = (Poke, Shut, Meet);
}

impl Carries<Poke> for Poking {
    type Answers = (Poked,);
}

impl Carries<Shut> for Poking {
    type Answers = ();
}

impl Carries<Meet> for Poking {
    type Answers = ();
}

#[test]
fn mail_sent_through_a_named_protocol_reaches_the_actor_it_refers_to() {
    // Catches a reference to a named protocol that sends to another seat than the one it was made with, and one that
    // does not narrow to the tuple of a kind it takes.
    let receivers = [seated(9, 5), seated(8, 6)];
    let table = Listed(Vec::new());
    let mut asked = Asked::new();
    let mut turn =
        opened::<(ToAny<Poking, Poke>, ToAny<(Shut,), Shut>)>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let references: [ProtocolRef<Poking>; 2] = receivers.map(|receiver| ActorRef::<Leaf>::new(receiver).protocol());

    assert_eq!(turn.send_to(references[0], &Poke), Ok(()));
    assert_eq!(turn.send_to(references[1].narrow(), &Shut), Ok(()));

    let (buffers, _, _, _, _) = asked.into_parts();
    let seen: Vec<(Seated, KindId)> = buffers.mailed.iter().map(|mailed| (mailed.to, mailed.kind)).collect();
    assert_eq!(seen, [(receivers[0], KindId::of::<Poke>()), (receivers[1], KindId::of::<Shut>())]);
}

#[test]
fn subscribers_are_held_once_watched_once_and_leave_by_ending_or_unsubscribing() {
    // Catches a subscriber held or watched twice, one that does not leave when it ends or unsubscribes, and the
    // last subscriber moved into a leaver's place but still found at its old one.
    let [first, second, third] = [seated(9, 5), seated(8, 6), seated(7, 7)];
    let table = Listed(Vec::new());
    let mut asked = Asked::new();
    let mut turn = opened::<(Watches,)>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let listener = |at: Seated| -> ProtocolRef<(Poke,)> { ActorRef::<Leaf>::new(at).protocol() };
    let mut subscribers = Subscribers::new();

    let added = [first, second, third, second].map(|at| subscribers.subscribe(&mut turn, listener(at)));
    assert_eq!(added, [true, true, true, false]);
    assert!(subscribers.unsubscribe(listener(first)));
    assert_eq!(subscribers.references(), [listener(third), listener(second)]);
    assert!(subscribers.ended(&Ended::new(third, How::Closed)));
    assert!(!subscribers.ended(&Ended::new(first, How::Closed)));
    assert!(!subscribers.unsubscribe(listener(third)));
    assert_eq!(subscribers.references(), [listener(second)]);

    let watched: Vec<Seated> = asked
        .effects()
        .iter()
        .filter_map(|effect| match effect.what() {
            What::Watch { of, .. } => Some(of.seated()),
            _ => None,
        })
        .collect();
    assert_eq!(watched, [first, second, third]);
}

#[test]
fn back_to_back_mail_of_one_kind_to_one_receiver_is_one_run() {
    // Catches a run joining mail to another receiver or of another kind, one counted from the wrong piece, and a
    // piece of a run read from the bytes of another.
    let (other, third) = (seated(9, 5), seated(8, 6));
    let table =
        Listed(vec![(other, ActorPath::new("aether.test.other")), (third, ActorPath::new("aether.test.third"))]);
    let mut asked = Asked::new();
    let mut turn =
        opened::<(ToType<Leaf, Meet>, ToType<Leaf, Poke>)>(ME, &ROOT, Above::Chassis, &table, &mut asked, ME);
    let meet = |to: Seated| Meet { other: ErasedActorRef::new(to) };
    let sends = [(other, true), (other, true), (other, true), (other, false), (third, true), (other, true)];
    for (to, meets) in sends {
        let target = To::Held(ErasedActorRef::new(to));
        let sent = if meets {
            turn.send(target, &meet(third))
        } else {
            turn.send(target, &Poke)
        };
        assert_eq!(sent, Ok(()));
    }

    let (buffers, _, arena, _, _) = asked.into_parts();
    let seen: Vec<(Seated, u64, KindId, u32)> =
        buffers.mailed.iter().map(|mailed| (mailed.to, mailed.count, mailed.kind, mailed.bytes.pieces())).collect();
    let (meets, pokes) = (KindId::of::<Meet>(), KindId::of::<Poke>());
    assert_eq!(seen, [(other, 1, meets, 3), (other, 4, pokes, 1), (third, 5, meets, 1), (other, 6, meets, 1)]);

    let run = buffers.mailed[0].bytes;
    let alone = buffers.mailed[2].bytes;
    assert_eq!(arena.read(run.piece(2, run.each())), arena.read(alone), "each piece holds a meeting with the third");
}

/// A resource, which a guest's host holds a guest to the claims of.
struct Lamp;

impl Resource for Lamp {}

#[test]
fn a_host_holds_a_guest_to_what_it_claims_of_resources() {
    // Catches a host that takes a guest's word for what it took, gave, or carried: a resource taken twice, one taken
    // that came from another sender, one given that it never held, and one carried by mail it did not send.
    let (other, stranger) = (seated(9, 5), seated(8, 4));
    let table = Listed::default();
    let mut asked = Asked::new();
    let mut instances = Instances::default();
    let mut keeping = Keeping::default();
    keeping.arrive(other, MailId { sender: other.id, count: 1 }, Kept::new(7, Lamp));
    keeping.arrive(stranger, MailId { sender: stranger.id, count: 1 }, Kept::new(8, Lamp));
    let keeps = Keeps::Here(&mut keeping);
    let turning = Turning::new(
        ME,
        &ROOT,
        Above::Chassis,
        &table,
        &(),
        &mut asked,
        &mut instances,
        keeps,
        0,
        other,
        CHAIN,
        Tie::TELL,
    );
    let (mut hosted, _) = turning.hosted(Declared::of::<Leaf>(), Role::Others);
    let claim = |ticket, what| Claim { by: ME, ticket, what };

    assert_eq!(hosted.claim(claim(7, Claimed::Took)), Ok(()));
    assert_eq!(hosted.claim(claim(7, Claimed::Took)), Err(NotHeld), "a resource is taken once");
    assert_eq!(hosted.claim(claim(8, Claimed::Took)), Err(NotHeld), "it came from another sender");
    assert_eq!(hosted.claim(claim(9, Claimed::Gave)), Err(NotHeld), "it was never held");
    assert_eq!(hosted.claim(claim(7, Claimed::Gave)), Ok(()));
    assert_eq!(hosted.claim(claim(7, Claimed::Carried { send: 0 })), Err(NotHeld), "no mail was sent");
}
