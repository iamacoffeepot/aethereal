//! A list's check and what is kept of lists: a list refused leaves the last standing, and what is kept follows its
//! sender's releases, its ending, and its frame's going.

use super::*;
use crate::frame::FrameId;
use aethereal_data::{Budget, from_bytes};
use aethereal_window::WindowId;

/// The actor whose lists these are, and another.
const MAKER: ActorId = ActorId::new(1);
const OTHER: ActorId = ActorId::new(2);

/// The pass the placements join listed.
#[derive(Plain)]
#[kind(name = "aether.test.blended")]
struct Blended;

impl Pass for Blended {}

/// The pass the placements join loose.
#[derive(Plain)]
#[kind(name = "aether.test.depths")]
struct Depths;

impl Pass for Depths {}

/// A pass none of the placements joins.
#[derive(Plain)]
#[kind(name = "aether.test.elsewhere")]
struct Elsewhere;

impl Pass for Elsewhere {}

/// The first window's main frame.
fn main() -> Frame {
    let Ok(window) = from_bytes::<WindowId>(&0_u64.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a window's id is its number");
    };

    Frame::Main(window)
}

/// A frame that ends on a texture.
const TEXTURE: Frame = Frame::Texture(FrameId { id: 40 });

/// One placement a test keeps: its maker, and how it is drawn in each pass of a frame it joins.
struct Placed {
    maker: ActorId,
    joins: Vec<((Frame, PassName), Listing)>,
}

/// The placements a test keeps, each by its id.
struct Kept {
    placements: HashMap<u64, Placed>,
}

impl Kept {
    /// Placements 1 to 4 made by [`MAKER`], each joining [`Depths`] of the main frame loose and [`Blended`] of the main
    /// frame and of [`TEXTURE`] listed; and placement 5 made by [`OTHER`], joined alike.
    fn new() -> Self {
        let joins = || {
            vec![
                ((main(), PassName::of::<Depths>()), Listing::Loose),
                ((main(), PassName::of::<Blended>()), Listing::Listed { stamp: 0 }),
                ((TEXTURE, PassName::of::<Blended>()), Listing::Listed { stamp: 0 }),
            ]
        };
        let mut placements: HashMap<_, _> = (1..=4).map(|id| (id, Placed { maker: MAKER, joins: joins() })).collect();
        placements.insert(5, Placed { maker: OTHER, joins: joins() });

        Self { placements }
    }
}

impl Placements for Kept {
    fn listing(
        &mut self,
        placement: u64,
        by: ActorId,
        pass: (Frame, PassName),
    ) -> Result<(usize, &mut Listing), EntryRefused> {
        let Placed { maker, joins } = self.placements.get_mut(&placement).ok_or(EntryRefused::Placement)?;
        if *maker != by {
            return Err(EntryRefused::NotOwner);
        }
        let at = joins.iter().position(|(joined, _)| *joined == pass).ok_or(EntryRefused::Unjoined)?;

        Ok((at, &mut joins[at].1))
    }
}

/// A list of the placements `ids` in the pass `P` of `frame`.
fn list<P: Pass>(frame: Frame, ids: &[u64]) -> DrawList {
    DrawList::of::<P>(frame, ids.iter().map(|id| PlacementId { id: *id }).collect())
}

/// The placements of each list kept for the pass `P` of `frame`, each list in its order, the lists sorted.
fn kept_of<P: Pass>(lists: &Lists, frame: Frame) -> Vec<Vec<u64>> {
    let mut kept: Vec<Vec<u64>> =
        lists.of(frame, PassName::of::<P>()).map(|list| list.iter().map(|entry| entry.placement).collect()).collect();
    kept.sort_unstable();

    kept
}

#[test]
fn a_list_refused_for_any_of_its_ids_leaves_the_last_list_standing_whole() {
    // Catches a list kept up to the id that failed, an order nobody declared, and a refused list that blanks or
    // replaces what its sender drew in the pass; and each of the check's refusals let through: a placement never made,
    // one another actor made, one that joins another pass, one that joins the pass loose, and one named twice.
    let mut kept = Kept::new();
    let mut lists = Lists::default();
    assert_eq!(lists.check(&list::<Blended>(main(), &[3, 1]), MAKER, &mut kept), Ok(()));

    let refused = |at, why| Err(ListRefused::Entry { at, why });
    let cases = [
        (list::<Blended>(main(), &[1, 9]), refused(1, EntryRefused::Placement)),
        (list::<Blended>(main(), &[2, 5]), refused(1, EntryRefused::NotOwner)),
        (list::<Elsewhere>(main(), &[1]), refused(0, EntryRefused::Unjoined)),
        (list::<Depths>(main(), &[1]), refused(0, EntryRefused::Loose)),
        (list::<Blended>(main(), &[1, 2, 1]), refused(2, EntryRefused::Twice)),
    ];
    for (refusing, why) in cases {
        assert_eq!(lists.check(&refusing, MAKER, &mut kept), why, "{refusing:?}");
        assert_eq!(kept_of::<Blended>(&lists, main()), vec![vec![3, 1]], "the last list stands after {refusing:?}");
    }
}

#[test]
fn a_placement_named_by_an_earlier_list_is_not_named_twice_in_the_next() {
    // Catches a stamp that marks a join as listed once and for all, rather than by the number of the list checked:
    // every list after the first would be refused, and the order could never change.
    let mut kept = Kept::new();
    let mut lists = Lists::default();
    assert_eq!(lists.check(&list::<Blended>(main(), &[1, 2]), MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(&list::<Blended>(main(), &[2, 1]), MAKER, &mut kept), Ok(()));
    let refused = lists.check(&list::<Blended>(main(), &[2, 2]), MAKER, &mut kept);
    assert_eq!(refused, Err(ListRefused::Entry { at: 1, why: EntryRefused::Twice }));

    assert_eq!(kept_of::<Blended>(&lists, main()), vec![vec![2, 1]]);
}

#[test]
fn each_actor_keeps_one_list_for_each_pass_of_each_frame_and_a_later_one_replaces_its_own_alone() {
    // Catches lists kept by pass alone, so one actor's list replaces another's, or one frame's the other's; and lists
    // appended, so an earlier list keeps drawing beside the later one. An empty list is kept, drawing none.
    let mut kept = Kept::new();
    let mut lists = Lists::default();
    assert_eq!(lists.check(&list::<Blended>(main(), &[1, 2]), MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(&list::<Blended>(main(), &[5]), OTHER, &mut kept), Ok(()));
    assert_eq!(lists.check(&list::<Blended>(TEXTURE, &[4]), MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(&list::<Blended>(main(), &[3]), MAKER, &mut kept), Ok(()));
    assert_eq!(kept_of::<Blended>(&lists, main()), vec![vec![3], vec![5]]);
    assert_eq!(kept_of::<Blended>(&lists, TEXTURE), vec![vec![4]]);

    assert_eq!(lists.check(&list::<Blended>(main(), &[]), MAKER, &mut kept), Ok(()));
    assert_eq!(kept_of::<Blended>(&lists, main()), vec![vec![], vec![5]]);
}

#[test]
fn a_placement_released_leaves_its_makers_lists_and_the_rest_keep_their_order() {
    // Catches a released placement's entry left in a list, drawn after it is gone or found missing at `Render`, or
    // its list cut short or reordered as it leaves.
    let mut kept = Kept::new();
    let mut lists = Lists::default();
    assert_eq!(lists.check(&list::<Blended>(main(), &[4, 2, 3, 1]), MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(&list::<Blended>(TEXTURE, &[2, 1]), MAKER, &mut kept), Ok(()));

    lists.released(2, MAKER, [(main(), PassName::of::<Blended>()), (TEXTURE, PassName::of::<Blended>())]);
    assert_eq!(kept_of::<Blended>(&lists, main()), vec![vec![4, 3, 1]]);
    assert_eq!(kept_of::<Blended>(&lists, TEXTURE), vec![vec![1]]);
}

#[test]
fn an_actor_that_ends_takes_its_lists_and_a_frame_that_goes_takes_the_lists_for_it() {
    // Catches lists kept after their sender ended, whose placements are gone, or after their frame went, which no
    // list names again; and another actor's lists, or another frame's, dropped with them.
    let mut kept = Kept::new();
    let mut lists = Lists::default();
    assert_eq!(lists.check(&list::<Blended>(main(), &[1]), MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(&list::<Blended>(TEXTURE, &[2]), MAKER, &mut kept), Ok(()));
    assert_eq!(lists.check(&list::<Blended>(main(), &[5]), OTHER, &mut kept), Ok(()));
    assert_eq!(lists.check(&list::<Blended>(TEXTURE, &[5]), OTHER, &mut kept), Ok(()));

    lists.ended(MAKER);
    assert_eq!(kept_of::<Blended>(&lists, main()), vec![vec![5]]);
    assert_eq!(kept_of::<Blended>(&lists, TEXTURE), vec![vec![5]]);
    lists.unframed(TEXTURE);
    assert_eq!(kept_of::<Blended>(&lists, main()), vec![vec![5]]);
    assert_eq!(kept_of::<Blended>(&lists, TEXTURE), Vec::<Vec<u64>>::new());
}
