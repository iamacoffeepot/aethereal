//! A list's check, what is kept of lists, and the walk: a list refused leaves the last standing, what is kept follows
//! its sender's releases and its ending, and the walk draws each child where its parent calls it, from the roots.

use super::*;

/// The actor whose lists these are, another root, and actors that stand as children of theirs.
const MAKER: ActorId = ActorId::new(1);
const OTHER: ActorId = ActorId::new(2);
const HUD: ActorId = ActorId::new(3);
const MAP: ActorId = ActorId::new(4);
const STRAY: ActorId = ActorId::new(5);

/// One placement a test keeps: its maker, and how each of its joins is drawn.
struct Placed {
    maker: ActorId,
    joins: Vec<Listing>,
}

/// The placements a test keeps, each by its id.
struct Kept {
    placements: HashMap<u64, Placed>,
}

impl Kept {
    /// Placements 1 to 4 made by [`MAKER`], each joining one pass loose and two listed; placement 5 made by [`OTHER`],
    /// joined alike; and placement 6 made by [`MAKER`], joining every pass it joins loose.
    fn new() -> Self {
        let joins = || vec![Listing::Loose, Listing::Listed { stamp: 0 }, Listing::Listed { stamp: 0 }];
        let mut placements: HashMap<_, _> = (1..=4).map(|id| (id, Placed { maker: MAKER, joins: joins() })).collect();
        placements.insert(5, Placed { maker: OTHER, joins: joins() });
        placements.insert(6, Placed { maker: MAKER, joins: vec![Listing::Loose] });

        Self { placements }
    }
}

impl Placements for Kept {
    fn stamp(&mut self, placement: u64, by: ActorId, number: u64) -> Result<(), EntryRefused> {
        let Placed { maker, joins } = self.placements.get_mut(&placement).ok_or(EntryRefused::Placement)?;
        if *maker != by {
            return Err(EntryRefused::NotOwner);
        }

        stamp(joins, number)
    }
}

/// An entry drawing the placement `id`, which the pointer does not reach.
const fn draw(id: u64) -> Listed {
    Listed::Draw { placement: id, picks: Picks::Ignores }
}

/// A list drawing the placements `ids`, in order.
fn draws(ids: &[u64]) -> Vec<Listed> {
    ids.iter().map(|id| draw(*id)).collect()
}

/// What the lists draw from `roots`, in the order of the walk.
fn walked(lists: &Lists, roots: &[ActorId]) -> Vec<u64> {
    lists
        .walk(roots.to_vec())
        .filter_map(|entry| match entry {
            Listed::Draw { placement, .. } => Some(*placement),
            Listed::Call(_) | Listed::Hit { .. } | Listed::Keys { .. } => None,
        })
        .collect()
}

/// Lists whose root list is [`MAKER`] then [`OTHER`].
fn lists() -> Lists {
    Lists::new(vec![MAKER, OTHER])
}

#[test]
fn a_list_refused_for_any_of_its_entries_leaves_the_last_list_standing_whole() {
    // Catches a list kept up to the entry that failed, an order nobody declared, and a refused list that blanks or
    // replaces what its sender drew; and each of the check's refusals let through: a placement never made, one another
    // actor made, one joined only loose, one named twice, and a child called twice, which would draw it twice.
    let mut kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(draws(&[3, 1]), MAKER, &mut kept), Ok(()));

    let refused = |at, why| Err(ListRefused { at, why });
    let calls_twice = vec![Listed::Call(HUD), draw(2), Listed::Call(HUD)];
    let cases = [
        (draws(&[1, 9]), refused(1, EntryRefused::Placement)),
        (draws(&[2, 5]), refused(1, EntryRefused::NotOwner)),
        (draws(&[6]), refused(0, EntryRefused::Loose)),
        (draws(&[1, 2, 1]), refused(2, EntryRefused::Twice)),
        (calls_twice, refused(2, EntryRefused::Twice)),
    ];
    for (refusing, why) in cases {
        assert_eq!(lists.check(refusing.clone(), MAKER, &mut kept), why, "{refusing:?}");
        assert_eq!(walked(&lists, &[MAKER]), vec![3, 1], "the last list stands after {refusing:?}");
    }
}

#[test]
fn a_placement_named_by_an_earlier_list_is_not_named_twice_in_the_next() {
    // Catches a stamp that marks a join as listed once and for all, rather than by the number of the list checked:
    // every list after the first would be refused, and the order could never change.
    let mut kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(draws(&[1, 2]), MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(draws(&[2, 1]), MAKER, &mut kept), Ok(()));
    let refused = lists.check(draws(&[2, 2]), MAKER, &mut kept);
    assert_eq!(refused, Err(ListRefused { at: 1, why: EntryRefused::Twice }));

    assert_eq!(walked(&lists, &[MAKER]), vec![2, 1]);
}

#[test]
fn each_actor_keeps_one_list_and_a_later_one_replaces_its_own_alone() {
    // Catches one actor's list replacing another's, and lists appended, so an earlier list keeps drawing beside the
    // later one. An empty list is kept, drawing none.
    let mut kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(draws(&[1, 2]), MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(draws(&[5]), OTHER, &mut kept), Ok(()));
    assert_eq!(lists.check(draws(&[3]), MAKER, &mut kept), Ok(()));
    assert_eq!(walked(&lists, lists.roots()), vec![3, 5]);

    assert_eq!(lists.check(draws(&[]), MAKER, &mut kept), Ok(()));
    assert_eq!(walked(&lists, lists.roots()), vec![5]);
}

#[test]
fn a_placement_released_leaves_its_makers_list_and_an_actor_that_ends_takes_its_list() {
    // Catches a released placement's entry left in a list, drawn after it is gone or found missing at `Render`, or its
    // list cut short or reordered as it leaves; and an ended actor's list kept, or another actor's dropped with it.
    let mut kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(draws(&[4, 2, 3, 1]), MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(draws(&[5]), OTHER, &mut kept), Ok(()));

    lists.released(2, MAKER);
    assert_eq!(walked(&lists, lists.roots()), vec![4, 3, 1, 5]);
    lists.ended(MAKER);
    assert_eq!(walked(&lists, lists.roots()), vec![5]);
}

#[test]
fn the_walk_draws_each_child_where_its_parent_calls_it_and_no_actor_no_list_calls() {
    // Catches lists drawn back to back in any order of their own, as the hash map of them holds them: a child drawn
    // after its parent's whole list rather than where it is called, a grandchild drawn outside its parent, roots out
    // of the root list's order, and an actor drawn that no list calls.
    let mut kept = Kept::new();
    let mut lists = lists();
    let parent = vec![draw(1), Listed::Call(HUD), draw(4)];
    assert_eq!(lists.check(parent, MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(vec![draw(2), Listed::Call(MAP)], HUD, &mut make(HUD, 2)), Ok(()));
    assert_eq!(lists.check(draws(&[3]), MAP, &mut make(MAP, 3)), Ok(()));
    assert_eq!(lists.check(draws(&[5]), OTHER, &mut kept), Ok(()));
    assert_eq!(lists.check(draws(&[7]), STRAY, &mut make(STRAY, 7)), Ok(()));

    assert_eq!(walked(&lists, lists.roots()), vec![1, 2, 3, 4, 5]);
    assert_eq!(walked(&lists, &[OTHER, MAKER]), vec![5, 1, 2, 3, 4]);
}

#[test]
fn a_parent_reorders_its_children_by_sending_one_list() {
    // Catches an order between children taken from anything but the parent's last list: when they sent theirs, or
    // which of them was called first in an earlier list.
    let mut kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(draws(&[2]), HUD, &mut make(HUD, 2)), Ok(()));
    assert_eq!(lists.check(draws(&[3]), MAP, &mut make(MAP, 3)), Ok(()));
    let hud_on_top = vec![draw(1), Listed::Call(MAP), Listed::Call(HUD)];
    assert_eq!(lists.check(hud_on_top, MAKER, &mut kept), Ok(()));
    assert_eq!(walked(&lists, &[MAKER]), vec![1, 3, 2]);

    let map_on_top = vec![Listed::Call(HUD), Listed::Call(MAP), draw(1)];
    assert_eq!(lists.check(map_on_top, MAKER, &mut kept), Ok(()));
    assert_eq!(walked(&lists, &[MAKER]), vec![2, 3, 1]);
}

#[test]
fn a_walk_ends_where_lists_call_round_as_only_colliding_ids_could() {
    // Catches a walk with no bound on its depth: lists that call each other round, which a child's id colliding with
    // an actor's above it would write, walked forever.
    let mut kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(vec![draw(1), Listed::Call(MAKER)], MAKER, &mut kept), Ok(()));

    let drawn = walked(&lists, &[MAKER]);
    assert_eq!(drawn.len(), ActorPath::MAX_DEPTH);
}

/// The placements of a child `by` makes: placement `id`, joined listed.
fn make(by: ActorId, id: u64) -> Kept {
    let placed = Placed { maker: by, joins: vec![Listing::Listed { stamp: 0 }] };

    Kept { placements: HashMap::from([(id, placed)]) }
}

#[test]
fn an_entry_the_pointer_reaches_names_its_place_and_is_refused_from_a_sender_that_hears_no_pointer() {
    // Catches an entry told to its sender by a number other than its place in the list, so a click on the second
    // button presses the first; one the pointer reaches let through from an actor that handles no pointer event, whose
    // events would be dropped as it blocks everything beneath it; and an entry the pointer ignores refused for it.
    let hearer = ProtocolRef::found(aethereal_actor::Seated { id: MAKER, seat: aethereal_actor::SeatNumber::new(1) });
    let placement = PlacementId { id: 3 };
    let blocks = Entry::Draw { placement, pointer: Pointer::Blocks };
    let ignores = Entry::Draw { placement, pointer: Pointer::Ignores };
    let child = |_: &PathSegment| HUD;

    let reached = Picks::Reaches(Reach { actor: hearer, entry: 2, blocks: true });
    assert_eq!(blocks.listed(2, (Some(hearer), None), child), Ok(Listed::Draw { placement: 3, picks: reached }));
    assert_eq!(blocks.listed(2, (None, None), child), Err(EntryRefused::Unheard));
    assert_eq!(ignores.listed(2, (None, None), child), Ok(draw(3)));
}

#[test]
fn an_entry_that_declares_keys_names_its_place_and_is_refused_from_a_sender_that_hears_no_keys() {
    // Catches keys told to their sender by a number other than the entry's place in the list; and a declaration let
    // through from an actor that handles no key, whose keys would be dropped as it blocks every key beneath it.
    let hearer = ProtocolRef::found(aethereal_actor::Seated { id: MAKER, seat: aethereal_actor::SeatNumber::new(1) });
    let Ok(window) = aethereal_data::from_bytes(&0_u64.to_le_bytes(), &mut aethereal_data::Budget::new(8)) else {
        panic!("a window's id is its number");
    };
    let frame = Frame::Main(window);
    let keys = Entry::Keys { frame, keys: KeySet::All, blocks: true };
    let child = |_: &PathSegment| HUD;

    let declared = KeysEntry { actor: hearer, entry: 4, keys: KeySet::All, blocks: true };
    assert_eq!(keys.listed(4, (None, Some(hearer)), child), Ok(Listed::Keys { frame, entry: declared }));
    assert_eq!(keys.listed(4, (None, None), child), Err(EntryRefused::KeysUnheard));
}
