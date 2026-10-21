//! The page's entry points, and an actor that subscribes to the input root and notes each event it is sent.

use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Path, Tells, Top, Turn, path};
use aethereal_chassis_browser::input::{Event, Input, InputConfig, Listening, Subscribe, listen};
use aethereal_chassis_browser::{Browser, Performance};
use aethereal_data::{Bound, Kind, KindName, Plain};
use aethereal_substrate::Engine;
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
    _listening: Listening,
}

/// Makes the engine with the input root and the reporter as its roots, and listens to the page for what the input
/// root is fed: the pointer over `canvas`, the keys, and the controls the page marks.
///
/// # Errors
///
/// When a root does not begin, or the page will not be listened to.
#[wasm_bindgen]
pub fn start(canvas: &HtmlCanvasElement) -> Result<Program, JsError> {
    let mut setup = Engine::new(NonZeroUsize::MIN, 1024, 0, 1 << 20, 0)
        .with_cycle::<()>()
        .map_err(|unordered| JsError::new(&format!("the cycle is out of order: {unordered:?}")))?;
    let (source, feed) = setup.source::<Event>();
    let engine = setup
        .add_root::<Input>(&(), InputConfig { source })
        .map_err(|refused| JsError::new(&format!("the input root did not begin: {refused:?}")))?
        .add_root::<Reporter>(&(), Nothing)
        .map_err(|refused| JsError::new(&format!("the reporter did not begin: {refused:?}")))?
        .ready();
    let listening = listen(canvas, &feed)?;

    Ok(Program { browser: Browser::new(engine, Performance), _listening: listening })
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

/// An empty config.
#[derive(Plain)]
struct Nothing;

impl Kind for Nothing {
    const NAME: KindName = KindName::new("aether.example.nothing");
}

/// The reporter: it subscribes to the input root as it begins, and notes each event it is sent.
#[derive(Bound)]
struct Reporter;

impl Kind for Reporter {
    const NAME: KindName = KindName::new("aether.example.reporter");
}

impl ChildOf for Reporter {
    type Parent = Top;
}

impl Actor for Reporter {
    type Config = Nothing;
    type Key = ();
    type Handles = (Event,);
    type BeginSends = (Tells<Path<Input>, Subscribe>,);
    type EndSends = ();

    fn begin(_: Nothing, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Input>(())).send(&Subscribe)?;

        Ok(Self)
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

impl Handles<Event> for Reporter {
    type Sends = ();

    fn handle(&mut self, event: Event, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        SAID.with_borrow_mut(|said| said.push(format!("{event:?}")));

        Ok(())
    }
}
