//! The overlay's list at each stop (ADR-0055): the keys it declares, the console's hit, and its placements, in order.

use super::drawing::listing;
use crate::layout::Layout;
use crate::stop::Stop;
use aethereal_data::{Budget, from_bytes};
use aethereal_render::draw_list::{Entry, Pointer};
use aethereal_render::frame::Frame;
use aethereal_render::pass::Interface;
use aethereal_render::placement::PlacementId;
use aethereal_window::{Cursor, KeySet, Surface, WindowId};

/// The window the overlay draws on.
fn window() -> WindowId {
    let Ok(window) = from_bytes(&0_u64.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a window's id is its number");
    };

    window
}

/// The placement the renderer made under `id`.
fn placement(id: u64) -> PlacementId {
    let Ok(placement) = from_bytes(&id.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a placement's id is its number");
    };

    placement
}

#[test]
fn the_overlay_hears_the_backtick_alone_until_the_console_opens_and_then_every_key_and_the_wheel_over_it() {
    // Catches a hidden overlay that lists nothing, and so never hears the backtick again; a closed console that still
    // takes every key, so the game beneath it hears none; an open console that lets keys through to the game, so
    // typing walks the character; and a console whose panel lets the pointer through, so the wheel over it zooms the
    // game as well as scrolling it.
    let surface = Surface { width: 800, height: 600, scale: 1.0 };
    let main = Frame::Main(window());
    let placements = vec![placement(7), placement(9)];
    let drawn = |id| Entry::Draw { placement: placement(id), pointer: Pointer::Ignores, cursor: Cursor::Default };
    let backtick = Entry::Keys { frame: main, keys: KeySet::of(["Backquote"]), blocks: true };

    let hidden = listing(window(), Stop::Hidden, surface, Vec::new());
    let band = listing(window(), Stop::Band, surface, placements.clone());
    let console = listing(window(), Stop::Console, surface, placements);

    assert_eq!(hidden, vec![backtick.clone()]);
    assert_eq!(band, vec![backtick, drawn(7), drawn(9)]);
    let every = Entry::Keys { frame: main, keys: KeySet::All, blocks: true };
    let panel = Entry::hit::<Interface>(main, Layout::of(surface).console(), Pointer::Blocks, Cursor::Default);
    assert_eq!(console, vec![every, panel, drawn(7), drawn(9)]);
}
