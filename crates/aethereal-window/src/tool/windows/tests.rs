//! The window tools' tests: each call is asked of the window root as the tool it names, and answered with what the
//! root answered.

use super::*;
use crate::event::{Mode, Surface};
use crate::host::Opening;
use crate::window::WindowId;
use crate::windows::{Heard, Listed, Logical, Place, Unplaced, Windowing};
use aethereal_substrate::Engine;
use aethereal_substrate::harness::{Bench, Harness, Unheard};
use aethereal_tools::{CommandLine, FlagsFault};
use std::num::NonZeroUsize;

/// The surface the window is opened at.
const SURFACE: Surface = Surface { width: 640, height: 480, scale: 1.0 };

/// A harness over an engine with one window open, maximized, and the tool.
fn tooled() -> Result<Harness<Bench>, Unheard> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0).with_cycle::<()>();
    let mut opening = Opening::new(setup.source::<Heard>(), setup.controller::<Windowing>());
    let _feed = opening.open("main", SURFACE, Mode::Maximized, true);
    let (config, _hosting) = opening.config();
    let engine = setup.add_root::<Windows>(&(), config)?.add_root::<WindowTool>(&(), WindowToolConfig)?.ready();

    Harness::bare(engine)
}

/// Calls `tool` with `input`, and answers what it answered: the bytes of its value, or why it refused.
fn call<K: Plain>(harness: &mut Harness<Bench>, tool: &str, input: &K) -> Result<Result<Vec<u8>, String>, Unheard> {
    let input = harness.put(to_bytes(input)?)?;
    let worker = WindowTool::worker(harness.root::<WindowTool>());
    let asked = harness.ask(worker, &Call { tool: String::from(tool), input })?;
    let called = match harness.answer(asked)? {
        Answered::Yes(Called::Output(output)) => Ok(harness.read(&output).map(<[u8]>::to_vec).unwrap_or_default()),
        Answered::Yes(Called::Refused(why)) => Err(why),
        Answered::Yes(Called::Image { .. }) | Answered::No(_) => Err(String::from("not an answer the tool gives")),
    };

    Ok(called)
}

#[test]
fn each_call_is_asked_of_the_window_root_as_named_and_answered_with_its_answer() -> Result<(), Unheard> {
    // Catches a call asked of the root as another tool, the root's answer not passed back, and a name the worker does
    // not serve left unanswered.
    let mut harness = tooled()?;
    let main = WindowId::nth(0);
    let listed =
        Listed { window: main, title: String::from("main"), surface: SURFACE, mode: Mode::Maximized, focused: true };
    let list = WindowList { windows: vec![listed] };
    assert_eq!(call(&mut harness, "list_windows", &ListWindows)?, Ok(to_bytes(&list)?));

    let resize = ResizeWindow { window: main, size: Logical { width: 10.0, height: 10.0 } };
    let refused = Resized::Refused(Unplaced::NotWindowed);
    assert_eq!(call(&mut harness, "resize_window", &resize)?, Ok(to_bytes(&refused)?));
    let moving = MoveWindow { window: WindowId::nth(3), at: Place { x: 0.0, y: 0.0 } };
    let refused = Moved::Refused(Unplaced::NotOpen);
    assert_eq!(call(&mut harness, "move_window", &moving)?, Ok(to_bytes(&refused)?));
    let set = SetMode { window: main, mode: Mode::Maximized };
    let shown = ModeSet::Set { surface: SURFACE };
    assert_eq!(call(&mut harness, "set_window_mode", &set)?, Ok(to_bytes(&shown)?));
    assert!(call(&mut harness, "close_window", &ListWindows)?.is_err());

    Ok(())
}

#[test]
fn every_sheet_s_flags_fit_its_input() {
    // Catches a `#[flag]` an `Offer` refuses (`Offered::BadFlags`): the tool would be missing from a running engine,
    // and nothing would answer when it is called.
    let sheets = WindowTool::sheets();
    let refused: Vec<(&str, FlagsFault)> = sheets
        .iter()
        .filter_map(|sheet| CommandLine::of(sheet).err().map(|fault| (sheet.name.as_str(), fault)))
        .collect();

    assert!(refused.is_empty(), "{refused:?}");
}
