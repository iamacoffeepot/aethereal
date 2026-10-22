//! The page's entry points, and an actor that subscribes to the canvas's window and to the controls, and notes each
//! event and control it is sent.

use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Path, Tells, Top, Turn, path};
use aethereal_chassis_browser::controls::{self, Control, Controls};
use aethereal_chassis_browser::{Browser, Started};
use aethereal_data::{Bound, Plain};
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{self as window, Event, InitialWindows, Window, WindowId, Windows};
use core::cell::RefCell;
use core::mem;
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;

thread_local! {
    /// What the reporter was sent, for the page to show.
    static SAID: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The program the page runs, for its `requestAnimationFrame` to call.
#[wasm_bindgen]
pub struct Program {
    started: Started,
}

/// Bootstraps the program on the page with the window root, which opens the page's canvas as the one window, the
/// controls root, and the reporter, the browser listening to the page for what each is fed: the pointer and the wheel
/// over the canvas, the keys, the focus, and resizes, and the controls the page marks.
///
/// # Errors
///
/// When the canvas will not open, the page will not be listened to, or a root does not begin.
#[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
pub async fn start() -> Result<Program, JsError> {
    let program = Browser::program().with_cycle::<()>().root::<Windows>().root::<Controls>().root::<Reporter>();

    Ok(Program { started: program.run()?.await? })
}

#[wasm_bindgen]
impl Program {
    /// Runs one frame, at the timestamp `requestAnimationFrame` handed its callback, in milliseconds.
    pub fn frame(&mut self, timestamp_millis: f64) {
        self.started.frame(timestamp_millis);
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
#[kind(name = "aether.example.watches")]
struct Watches {
    window: WindowId,
}

/// The reporter watches the one window the window root opened.
impl<H: Host> Bootstrap<H> for Reporter {
    const SECTION: &'static str = "reporter";
    type Settings = ();
    type Needs = (InitialWindows,);
    type Provides = ();

    async fn bootstrap(
        (): (),
        (windows,): (InitialWindows,),
        cx: &mut Bootstrapping<'_, H, Self>,
    ) -> Result<(), Unbuilt> {
        cx.begin(&(), Watches { window: windows.first() })
    }
}

/// The reporter: it subscribes to the canvas's window and to the controls as it begins, and notes each event and
/// control it is sent.
#[derive(Bound)]
#[kind(name = "aether.example.reporter")]
struct Reporter;

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
