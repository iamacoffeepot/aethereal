//! Dispatches as the lists name them (ADR-0055), on the device that draws nothing: one runs only where its maker's
//! list names it, in the list's order, whether it runs until released or once, and a list that names another's is
//! refused.

use super::lists::OTHER;
use super::once::{counting, counts, run_once};
use super::*;
use crate::dispatch::CreateDispatch;

/// Makes the dispatch `create` for `maker`, as the renderer does: checked and kept until released or run. Its id.
pub(super) fn dispatched(device: &wgpu::Device, kept: &mut Kept, create: &CreateDispatch, maker: ActorId) -> u64 {
    let made = kept.checked_dispatch(device, create, maker);
    let Ok(made) = made else {
        panic!("the dispatch is refused: {:?}", made.err());
    };
    let id = kept.ids.next();
    kept.dispatches.keep(id, maker, made);
    kept.reorder();

    id
}

/// A list running `dispatches`, in order.
pub(super) fn runs(dispatches: &[u64]) -> Vec<Listed> {
    dispatches.iter().map(|dispatch| Listed::Run(*dispatch)).collect()
}

/// The dispatches the window's main frame's pass [`Compute`] runs, in order, each by its id.
fn run_in_compute(kept: &Kept) -> Vec<u64> {
    kept.computed(Frame::Main(window()), PassName::of::<Compute>()).into_iter().map(|(id, _)| id).collect()
}

#[test]
fn a_dispatch_runs_only_while_a_list_names_it_in_the_list_s_order_whether_it_runs_until_released_or_once() {
    // Catches a dispatch run before any list names it, as every dispatch once was, or still run once a newer list
    // leaves it out; the dispatches of a pass run in the order of their ids or grouped by compute program, as the
    // renderer once sorted them, rather than as listed; and a dispatch that runs once run where no list puts it, ahead
    // of those listed, as one once was.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue);
    let mut kept = kept(&device);
    let main = Frame::Main(window());
    let [first, second, third, fourth] = [(); 4].map(|()| counting(&gpu, &mut kept, MAKER));
    let (first_program, _) = first;
    let (_, third_field) = third;
    let a = dispatched(&device, &mut kept, &counts(main, first), MAKER);
    let b = dispatched(&device, &mut kept, &counts(main, second), MAKER);
    let c = dispatched(&device, &mut kept, &counts(main, (first_program, third_field)), MAKER);
    assert_eq!(run_in_compute(&kept), Vec::<u64>::new(), "no list names them");

    assert_eq!(kept.listed(runs(&[c, b, a]), MAKER), Ok(()));
    assert_eq!(run_in_compute(&kept), vec![c, b, a]);
    let once = dispatched(&device, &mut kept, &run_once(counts(main, fourth)), MAKER);
    assert_eq!(run_in_compute(&kept), vec![c, b, a], "the one that runs once is listed nowhere");
    assert_eq!(kept.listed(runs(&[b, once, c]), MAKER), Ok(()));
    assert_eq!(run_in_compute(&kept), vec![b, once, c], "a left out, and the one that runs once between");
    assert_eq!(kept.listed(runs(&[b, c, once]), MAKER), Ok(()));
    assert_eq!(run_in_compute(&kept), vec![b, c, once], "the one that runs once after both");
}

#[test]
fn a_list_naming_another_s_dispatch_is_refused_and_a_released_one_leaves_its_maker_s_list() {
    // Catches one actor running another's dispatch where it chose, a refused list that replaces the last, and a
    // released dispatch left in its maker's list, run after its release or found missing at `Render`.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue);
    let mut kept = kept(&device);
    let main = Frame::Main(window());
    let [mine, also] = [(); 2].map(|()| counting(&gpu, &mut kept, MAKER));
    let theirs = counting(&gpu, &mut kept, OTHER);
    let mine = dispatched(&device, &mut kept, &counts(main, mine), MAKER);
    let also = dispatched(&device, &mut kept, &counts(main, also), MAKER);
    let theirs = dispatched(&device, &mut kept, &counts(main, theirs), OTHER);
    assert_eq!(kept.listed(runs(&[mine, also]), MAKER), Ok(()));

    let not_owned = Err(ListRefused { at: 1, why: EntryRefused::NotOwner });
    assert_eq!(kept.listed(runs(&[mine, theirs]), MAKER), not_owned);
    assert_eq!(run_in_compute(&kept), vec![mine, also]);
    assert_eq!(kept.listed(runs(&[theirs]), OTHER), Ok(()));
    assert_eq!(run_in_compute(&kept), vec![mine, also, theirs], "each root's list in the root list's order");

    assert_eq!(kept.release(Release::Dispatch(DispatchId { id: mine }), MAKER), Owned::Mine(()));
    assert_eq!(run_in_compute(&kept), vec![also, theirs]);
    let named_released = Err(ListRefused { at: 0, why: EntryRefused::Dispatch });
    assert_eq!(kept.listed(runs(&[mine]), MAKER), named_released);
}
