//! The keys a window's main frame declares (ADR-0055): the `Keys` entries its walk holds, the last called first, as
//! the renderer sends them to the window's actor, and only when they have changed.

use super::lists::{CHILD, OTHER, called};
use super::*;
use aethereal_actor::{ProtocolRef, SeatNumber, Seated};
use aethereal_window::KeySet;

/// The entry `entry` of `actor`'s list, hearing `keys` in the window, blocking them as `blocks` says.
fn heeds(actor: ActorId, entry: u32, keys: KeySet, blocks: bool) -> KeysEntry {
    let actor = ProtocolRef::found(Seated { id: actor, seat: SeatNumber::new(1) });

    KeysEntry { actor, entry, keys, blocks }
}

/// What `kept` keeps of a `Keys` entry of `frame`, `entry`.
fn declares(frame: Frame, entry: &KeysEntry) -> Listed {
    Listed::Keys { frame, entry: entry.clone() }
}

#[test]
fn a_window_is_sent_the_keys_its_walk_declares_the_last_called_first_and_again_only_when_they_change() {
    // Catches keys sent in the order the lists were kept, or the first called on top, so a game called beneath the
    // console hears its keys first; a parent's new list that reorders its own declaration and its child's left as it
    // was; another window's declarations sent to this one; and the entries sent again each frame though nothing
    // changed, or not sent again once a list reorders them.
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut kept = kept_for(&device, &[window(), nth_window(1)]);
    let main = Frame::Main(window());
    let (game, hud) = (heeds(MAKER, 0, KeySet::All, false), heeds(CHILD, 0, KeySet::of(["Tab"]), false));
    let console = heeds(OTHER, 1, KeySet::All, true);
    let elsewhere = heeds(OTHER, 0, KeySet::All, true);

    assert_eq!(kept.listed(vec![declares(main, &hud)], CHILD), Ok(()));
    assert_eq!(kept.listed(vec![declares(main, &game), called(CHILD, Area::CALLER)], MAKER), Ok(()));
    let other = vec![declares(Frame::Main(nth_window(1)), &elsewhere), declares(main, &console)];
    assert_eq!(kept.listed(other, OTHER), Ok(()));
    let first = kept.relisted([window()]);
    let again = kept.relisted([window()]);
    assert_eq!(kept.listed(vec![called(CHILD, Area::CALLER), declares(main, &game)], MAKER), Ok(()));
    let reordered = kept.relisted([window()]);

    let listed =
        |entries: &[&KeysEntry]| vec![(window(), KeysListed { entries: entries.iter().copied().cloned().collect() })];
    assert_eq!(first, listed(&[&console, &hud, &game]));
    assert_eq!(again, vec![]);
    assert_eq!(reordered, listed(&[&console, &game, &hud]));
}
