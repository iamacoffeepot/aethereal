//! The page's entry points, and an actor that subscribes to the canvas's window and to the controls, declares every
//! key of the window in its draw list, and notes each event, key, and control it is sent.

use crate::said::{Program, say};
use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Handles, Path, Render, Tells, Tick, Top, Turn, path};
use aethereal_chassis_browser::Browser;
use aethereal_chassis_browser::controls::{self, Control, Controls};
use aethereal_data::{Bound, Plain};
use aethereal_render::Renderer;
use aethereal_render::draw_list::{DrawList, Entry};
use aethereal_render::frame::Frame;
use aethereal_substrate::{Bootstrap, Bootstrapping, Host, Unbuilt};
use aethereal_window::{self as window, Event, InitialWindows, KeySet, Keyed, Window, WindowId, Windows};
use wasm_bindgen::JsError;
use wasm_bindgen::prelude::wasm_bindgen;

/// Bootstraps the program on the page with the window root, which opens the page's canvas as the one window, the
/// renderer, which draws nothing there but walks the reporter's list for the keys it declares (ADR-0055), the controls
/// root, and the reporter, the root list, the browser listening to the page for what each is fed: the pointer and the
/// wheel over the canvas, the keys, the focus, and resizes, and the controls the page marks.
///
/// # Errors
///
/// When the canvas will not open, the page will not be listened to, or a root does not begin.
#[wasm_bindgen(wasm_bindgen_futures = aethereal_chassis_browser::wasm_bindgen_futures)]
pub async fn start() -> Result<Program, JsError> {
    let program = Browser::program()
        .with_cycle::<(Tick, Render)>()
        .root::<Windows>()
        .root::<Renderer>()
        .root::<Controls>()
        .root::<Reporter>()
        .draws::<(Reporter,)>();

    Ok(Program::from(program.run()?.await?))
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

/// The reporter: it subscribes to the canvas's window and to the controls as it begins, declares every key of the
/// window in its draw list, and notes each event, key, and control it is sent.
#[derive(Bound)]
#[kind(name = "aether.example.reporter")]
struct Reporter;

impl ChildOf for Reporter {
    type Parent = Top;
}

impl Actor for Reporter {
    type Config = Watches;
    type Key = ();
    type Handles = (Event, Keyed, Control);
    type BeginSends = (
        Tells<Path<Window>, window::Subscribe>,
        Tells<Path<Renderer>, DrawList>,
        Tells<Path<Controls>, controls::Subscribe>,
    );
    type EndSends = ();

    fn begin(watches: Watches, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        turn.to(path::<Window>(watches.window)).send(&window::Subscribe)?;
        let hears = Entry::Keys { frame: Frame::Main(watches.window), keys: KeySet::All, blocks: true };
        turn.to(path::<Renderer>(())).send(&DrawList { entries: vec![hears] })?;
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
        say(format!("window {event:?}"));

        Ok(())
    }
}

impl Handles<Keyed> for Reporter {
    type Sends = ();

    fn handle(&mut self, keyed: Keyed, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        say(format!("window {:?}", keyed.event));

        Ok(())
    }
}

impl Handles<Control> for Reporter {
    type Sends = ();

    fn handle(&mut self, control: Control, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        say(format!("control {control:?}"));

        Ok(())
    }
}
