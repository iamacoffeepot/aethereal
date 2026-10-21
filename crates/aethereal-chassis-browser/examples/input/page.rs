//! The page's entry points, and an actor that subscribes to the canvas's window and to the controls, and notes each
//! event and control it is sent.

use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Path, Tells, Top, Turn, path};
use aethereal_chassis_browser::controls::{self, Control, Controls, ControlsConfig};
use aethereal_chassis_browser::{Browser, Listening, Mappings, Performance, canvas};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_substrate::Engine;
use aethereal_window::{self as window, Event, Heard, Opening, Window, WindowId, Windows};
use core::cell::RefCell;
use core::mem;
use core::num::NonZeroUsize;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;
use web_sys::HtmlCanvasElement;

thread_local! {
    /// What the reporter was sent, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The program the page runs, for its `requestAnimationFrame` to call, and the page's listeners, kept while it runs.
#[wasm_bindgen]
pub struct Program {
    browser: Browser<Performance>,
    _canvas: Listening,
    _controls: Listening,
}

/// Makes the engine with the window root, the controls root, and the reporter as its roots, the canvas the one window,
/// and listens to the page for what each is fed: the pointer over `canvas`, the keys, the focus, and resizes, and the
/// controls the page marks.
///
/// # Errors
///
/// When a root does not begin, or the page will not be listened to.
#[wasm_bindgen]
pub fn start(canvas: &HtmlCanvasElement) -> Result<Program, JsError> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0)
        .with_cycle::<()>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let mut opening = Opening::new(setup.source::<Heard>());
    let shown = canvas::open(canvas, &mut opening)?;
    let mut setup = setup
        .add_root::<Windows>(&(), opening.config())
        .map_err(|refused| JsError::new(&format!("the window root did not begin: {refused:?}")))?;
    let (source, feed) = setup.source::<Control>();
    let mut engine = setup
        .add_root::<Controls>(&(), ControlsConfig { source })
        .map_err(|refused| JsError::new(&format!("the controls root did not begin: {refused:?}")))?
        .add_root::<Reporter>(&(), Watches { window: shown.window() })
        .map_err(|refused| JsError::new(&format!("the reporter did not begin: {refused:?}")))?
        .ready();
    let mappings = Mappings::unasked(&mut engine);
    let program = Program {
        browser: Browser::new(engine, Performance, mappings),
        _canvas: canvas::listen(canvas, &shown)?,
        _controls: controls::listen(&feed)?,
    };

    Ok(program)
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame.
    pub fn frame(&mut self) {
        self.browser.frame();
    }

    /// What the reporter was sent since this was last asked, a line each.
    #[must_use]
    #[expect(clippy::unused_self, reason = "the page asks it of the program it started")]
    pub fn said(&self) -> Vec<String> {
        SAID.with_borrow_mut(mem::take)
    }
}

/// What the reporter watches: the window it subscribes to.
#[derive(Plain)]
struct Watches {
    window: WindowId,
}

impl Kind for Watches {
    const NAME: KindName = KindName::new("aether.example.watches");
}

/// The reporter: it subscribes to the canvas's window and to the controls as it begins, and notes each event and
/// control it is sent.
#[derive(Bound)]
struct Reporter;

impl Kind for Reporter {
    const NAME: KindName = KindName::new("aether.example.reporter");
}

impl ChildOf for Reporter {
    type Parent = Top;
}

impl Actor for Reporter {
    type Config = Watches;
    type Key = ();
    type Handles = (Event, Control);
    type BeginSends = (Tells<Path<Window>, window::Subscribe>, Tells<Path<Controls>, controls::Subscribe>);
    type EndSends = ();

    fn begin(watches: Watches, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(watches.window)).send(&window::Subscribe)?;
        turn.to(path::<Controls>(())).send(&controls::Subscribe)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Event> for Reporter {
    type Sends = ();

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        SAID.with_borrow_mut(|said| said.push(format!("window {event:?}")));

        Ok(())
    }
}

impl Handles<Control> for Reporter {
    type Sends = ();

    fn handle(&mut self, control: Control, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        SAID.with_borrow_mut(|said| said.push(format!("control {control:?}")));

        Ok(())
    }
}
