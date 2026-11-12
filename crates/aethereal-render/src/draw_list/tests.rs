//! A list's check, what is kept of lists, and the walk: a list refused leaves the last standing, what is kept follows
//! its sender's releases and its ending, and the walk draws and runs each child's where its parent calls it, from the
//! roots.

use super::*;

/// The actor whose lists these are, another root, and actors that stand as children of theirs.
const MAKER: ActorId = ActorId::new(1);
const OTHER: ActorId = ActorId::new(2);
const HUD: ActorId = ActorId::new(3);
const MAP: ActorId = ActorId::new(4);
const STRAY: ActorId = ActorId::new(5);

/// The placements and dispatches a test keeps, each by its id beside its maker.
struct Kept {
    placements: HashMap<u64, ActorId>,
    dispatches: HashMap<u64, ActorId>,
}

impl Kept {
    /// Placements 1 to 4 made by [`MAKER`], and placement 5 made by [`OTHER`]; dispatches 11 and 12 made by
    /// [`MAKER`], and dispatch 13 made by [`OTHER`].
    fn new() -> Self {
        let mut placements: HashMap<_, _> = (1..=4).map(|id| (id, MAKER)).collect();
        placements.insert(5, OTHER);
        let dispatches = HashMap::from([(11, MAKER), (12, MAKER), (13, OTHER)]);

        Self { placements, dispatches }
    }
}

impl Listables for Kept {
    fn placement_maker(&self, placement: u64) -> Option<ActorId> {
        self.placements.get(&placement).copied()
    }

    fn dispatch_maker(&self, dispatch: u64) -> Option<ActorId> {
        self.dispatches.get(&dispatch).copied()
    }
}

/// An entry drawing the placement `id`, which the pointer does not reach.
const fn draw(id: u64) -> Listed {
    Listed::Draw { placement: id, picks: Picks::Ignores }
}

/// A call of `child`, placed where its caller is.
const fn call(child: ActorId) -> Listed {
    Listed::Call { child, area: Area::CALLER }
}

/// A list drawing the placements `ids`, in order.
fn draws(ids: &[u64]) -> Vec<Listed> {
    ids.iter().map(|id| draw(*id)).collect()
}

/// What the lists draw and run from `roots`, in the order of the walk: each placement and dispatch by its id.
fn walked(lists: &Lists, roots: &[ActorId]) -> Vec<u64> {
    lists
        .walk(roots.to_vec())
        .filter_map(|walking| match walking.entry {
            Listed::Draw { placement, .. } => Some(*placement),
            Listed::Run(dispatch) => Some(*dispatch),
            Listed::Call { .. } | Listed::Hit { .. } | Listed::Keys { .. } => None,
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
    // replaces what its sender drew; and each of the check's refusals let through: a placement or dispatch never made,
    // or released, one another actor made, which would let one actor draw or run another's where it chose, one named
    // twice, and a child called twice, which would draw it twice; and a placement's id taken for a dispatch's.
    let kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(draws(&[3, 1]), MAKER, &kept), Ok(()));

    let refused = |at, why| Err(ListRefused { at, why });
    let calls_twice = vec![call(HUD), draw(2), call(HUD)];
    let cases = [
        (draws(&[1, 9]), refused(1, EntryRefused::Placement)),
        (draws(&[2, 5]), refused(1, EntryRefused::NotOwner)),
        (draws(&[1, 2, 1]), refused(2, EntryRefused::Twice)),
        (calls_twice, refused(2, EntryRefused::Twice)),
        (vec![Listed::Run(11), Listed::Run(19)], refused(1, EntryRefused::Dispatch)),
        (vec![Listed::Run(1)], refused(0, EntryRefused::Dispatch)),
        (vec![Listed::Run(11), Listed::Run(13)], refused(1, EntryRefused::NotOwner)),
        (vec![Listed::Run(12), draw(1), Listed::Run(12)], refused(2, EntryRefused::Twice)),
    ];
    for (refusing, why) in cases {
        assert_eq!(lists.check(refusing.clone(), MAKER, &kept), why, "{refusing:?}");
        assert_eq!(walked(&lists, &[MAKER]), vec![3, 1], "the last list stands after {refusing:?}");
    }
}

#[test]
fn a_placement_named_by_an_earlier_list_is_not_named_twice_in_the_next() {
    // Catches a check that remembers what earlier lists named: every list after the first would be refused as naming
    // its placements twice, and the order could never change.
    let kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(draws(&[1, 2]), MAKER, &kept), Ok(()));
    assert_eq!(lists.check(draws(&[2, 1]), MAKER, &kept), Ok(()));
    let refused = lists.check(draws(&[2, 2]), MAKER, &kept);
    assert_eq!(refused, Err(ListRefused { at: 1, why: EntryRefused::Twice }));

    assert_eq!(walked(&lists, &[MAKER]), vec![2, 1]);
}

#[test]
fn each_actor_keeps_one_list_and_a_later_one_replaces_its_own_alone() {
    // Catches one actor's list replacing another's, and lists appended, so an earlier list keeps drawing beside the
    // later one. An empty list is kept, drawing none.
    let kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(draws(&[1, 2]), MAKER, &kept), Ok(()));
    assert_eq!(lists.check(draws(&[5]), OTHER, &kept), Ok(()));
    assert_eq!(lists.check(draws(&[3]), MAKER, &kept), Ok(()));
    assert_eq!(walked(&lists, lists.roots()), vec![3, 5]);

    assert_eq!(lists.check(draws(&[]), MAKER, &kept), Ok(()));
    assert_eq!(walked(&lists, lists.roots()), vec![5]);
}

#[test]
fn a_placement_or_dispatch_released_leaves_its_makers_list_and_an_actor_that_ends_takes_its_list() {
    // Catches a released placement's or dispatch's entry left in a list, drawn or run after it is gone or found missing
    // at `Render`, or its list cut short or reordered as it leaves; and an ended actor's list kept, or another actor's
    // dropped with it.
    let kept = Kept::new();
    let mut lists = lists();
    let list = vec![draw(4), Listed::Run(11), draw(2), draw(3), Listed::Run(12), draw(1)];
    assert_eq!(lists.check(list, MAKER, &kept), Ok(()));
    assert_eq!(lists.check(draws(&[5]), OTHER, &kept), Ok(()));

    lists.released(2, MAKER);
    lists.released(11, MAKER);
    assert_eq!(walked(&lists, lists.roots()), vec![4, 3, 12, 1, 5]);
    lists.ended(MAKER);
    assert_eq!(walked(&lists, lists.roots()), vec![5]);
}

#[test]
fn the_walk_draws_each_child_where_its_parent_calls_it_and_no_actor_no_list_calls() {
    // Catches lists drawn back to back in any order of their own, as the hash map of them holds them: a child drawn
    // after its parent's whole list rather than where it is called, a grandchild drawn outside its parent, roots out
    // of the root list's order, and an actor drawn that no list calls.
    let kept = Kept::new();
    let mut lists = lists();
    let parent = vec![draw(1), call(HUD), draw(4)];
    assert_eq!(lists.check(parent, MAKER, &kept), Ok(()));
    assert_eq!(lists.check(vec![draw(2), call(MAP)], HUD, &make(HUD, 2)), Ok(()));
    assert_eq!(lists.check(draws(&[3]), MAP, &make(MAP, 3)), Ok(()));
    assert_eq!(lists.check(draws(&[5]), OTHER, &kept), Ok(()));
    assert_eq!(lists.check(draws(&[7]), STRAY, &make(STRAY, 7)), Ok(()));

    assert_eq!(walked(&lists, lists.roots()), vec![1, 2, 3, 4, 5]);
    assert_eq!(walked(&lists, &[OTHER, MAKER]), vec![5, 1, 2, 3, 4]);
}

/// The rect from `min` to `max`.
const fn rect(min: [f32; 2], max: [f32; 2]) -> Rect2 {
    Rect2 { min: Vec2::new(min[0], min[1]), max: Vec2::new(max[0], max[1]) }
}

#[test]
fn nested_areas_compose_from_the_window_down_each_clip_cut_to_its_caller_s_whatever_its_mode() {
    // Catches an inheriting child's area taken as given, as if its caller's coordinates were the window's, so a list
    // inside a panel draws at the list's offset from the window's corner; a clip left where it was given, not moved
    // by its caller's origin; an absolute child moved by its ancestors' origins, so a scene placed in the window's
    // pixels lands off by its parent's offset, and its own inheriting children not added from the window's corner; a
    // clip, of either mode, that widens what the caller was cut to, so a child draws out of its parent's box; and a
    // list's entries drawn where its parent's are, or a sibling's after the child's (ADR-0055).
    let kept = Kept::new();
    let mut lists = lists();
    let panel = rect([100.0, 50.0], [300.0, 250.0]);
    let scrolled = Area::Inherit { origin: Vec2::new(10.0, -40.0), clip: Clip::To(rect([0.0, 0.0], [400.0, 150.0])) };
    let on_screen = Area::Absolute { clip: Clip::To(rect([0.0, 0.0], [150.0, 500.0])) };
    let nudged = Area::Inherit { origin: Vec2::new(7.0, 8.0), clip: Clip::Whole };
    let parent = vec![draw(1), Listed::Call { child: HUD, area: Area::rect(panel) }, draw(4)];
    assert_eq!(lists.check(parent, MAKER, &kept), Ok(()));
    let hud =
        vec![draw(2), Listed::Call { child: MAP, area: scrolled }, Listed::Call { child: OTHER, area: on_screen }];
    assert_eq!(lists.check(hud, HUD, &make(HUD, 2)), Ok(()));
    assert_eq!(lists.check(draws(&[3]), MAP, &make(MAP, 3)), Ok(()));
    assert_eq!(lists.check(vec![draw(5), Listed::Call { child: STRAY, area: nudged }], OTHER, &kept), Ok(()));
    assert_eq!(lists.check(draws(&[7]), STRAY, &make(STRAY, 7)), Ok(()));

    let placed: Vec<(u64, Within)> = lists
        .walk(vec![MAKER])
        .filter_map(|walking| match walking.entry {
            Listed::Draw { placement, .. } => Some((*placement, walking.within)),
            Listed::Run(_) | Listed::Call { .. } | Listed::Hit { .. } | Listed::Keys { .. } => None,
        })
        .collect();

    let at = |x, y, clip| Within { origin: Vec2::new(x, y), clip: Clip::To(clip) };
    let in_panel = at(100.0, 50.0, panel);
    let in_scroll = at(110.0, 10.0, rect([100.0, 50.0], [300.0, 200.0]));
    let on_screen_in_panel = at(0.0, 0.0, rect([100.0, 50.0], [150.0, 250.0]));
    let nudged_on_screen = at(7.0, 8.0, rect([100.0, 50.0], [150.0, 250.0]));
    let expected = vec![
        (1, Within::WINDOW),
        (2, in_panel),
        (3, in_scroll),
        (5, on_screen_in_panel),
        (7, nudged_on_screen),
        (4, Within::WINDOW),
    ];
    assert_eq!(placed, expected);
}

#[test]
fn a_clip_cuts_a_target_s_pixels_at_their_middles_scaled_to_the_target_and_within_it() {
    // Catches a scissor taken from a clip in the surface's pixels on a target drawn at a share of its size, which cuts
    // a half-size bloom at twice the place; one that reaches past the target, which WebGPU refuses, failing the whole
    // frame, or that wraps below zero; one that rounds a clip's edge to take a pixel whose middle is outside; and a
    // clip cut to nothing given a scissor of no pixels, which WebGPU refuses, rather than its draw skipped (ADR-0055).
    let full = [1.0, 1.0];
    let size = [64, 32];

    assert_eq!(Clip::Whole.pixels(full, size), Cut::Pixels { x: 0, y: 0, width: 64, height: 32 });
    let inside = Clip::To(rect([10.0, 4.0], [20.0, 12.0]));
    assert_eq!(inside.pixels(full, size), Cut::Pixels { x: 10, y: 4, width: 10, height: 8 });
    assert_eq!(inside.pixels([0.5, 0.5], size), Cut::Pixels { x: 5, y: 2, width: 5, height: 4 });
    let middles = Clip::To(rect([10.4, 4.6], [20.4, 12.6]));
    assert_eq!(middles.pixels(full, size), Cut::Pixels { x: 10, y: 5, width: 10, height: 8 });
    let past = Clip::To(rect([-30.0, -5.0], [100.0, 50.0]));
    assert_eq!(past.pixels(full, size), Cut::Pixels { x: 0, y: 0, width: 64, height: 32 });
    let outside = Clip::To(rect([70.0, 0.0], [90.0, 10.0]));
    assert_eq!(outside.pixels(full, size), Cut::Nothing);
    let beside = Within::WINDOW
        .called(Area::rect(rect([20.0, 0.0], [30.0, 10.0])))
        .called(Area::rect(rect([15.0, 0.0], [25.0, 10.0])));
    assert_eq!(beside.clip.pixels(full, size), Cut::Nothing, "a child placed past its caller's clip");
}

#[test]
fn a_parent_reorders_its_children_by_sending_one_list() {
    // Catches an order between children taken from anything but the parent's last list: when they sent theirs, or
    // which of them was called first in an earlier list.
    let kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(draws(&[2]), HUD, &make(HUD, 2)), Ok(()));
    assert_eq!(lists.check(draws(&[3]), MAP, &make(MAP, 3)), Ok(()));
    let hud_on_top = vec![draw(1), call(MAP), call(HUD)];
    assert_eq!(lists.check(hud_on_top, MAKER, &kept), Ok(()));
    assert_eq!(walked(&lists, &[MAKER]), vec![1, 3, 2]);

    let map_on_top = vec![call(HUD), call(MAP), draw(1)];
    assert_eq!(lists.check(map_on_top, MAKER, &kept), Ok(()));
    assert_eq!(walked(&lists, &[MAKER]), vec![2, 3, 1]);
}

#[test]
fn a_walk_ends_where_lists_call_round_as_only_colliding_ids_could() {
    // Catches a walk with no bound on its depth: lists that call each other round, which a child's id colliding with
    // an actor's above it would write, walked forever.
    let kept = Kept::new();
    let mut lists = lists();
    assert_eq!(lists.check(vec![draw(1), call(MAKER)], MAKER, &kept), Ok(()));

    let drawn = walked(&lists, &[MAKER]);
    assert_eq!(drawn.len(), ActorPath::MAX_DEPTH);
}

/// The placements of a child `by` makes: placement `id`.
fn make(by: ActorId, id: u64) -> Kept {
    Kept { placements: HashMap::from([(id, by)]), dispatches: HashMap::new() }
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

    let reached = Picks::Reaches(Reaching { actor: hearer, entry: 2, blocks: true });
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
