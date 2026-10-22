//! The window root bootstrapped on the browser: each canvas its settings name opened as a window, and listened to
//! (ADR-0024, ADR-0029).

use super::unopened;
use crate::canvas::{self, Density, Page};
use crate::{Browser, Performance};
use aethereal_data::Plain;
use aethereal_substrate::{Bootstrap, Bootstrapping, Unbuilt};
use aethereal_window::{InitialWindows, Opening, Windows};
use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;

/// The window root's settings on the browser: its section, `windows` (ADR-0028).
#[derive(Plain, Debug, Clone, PartialEq)]
#[kind(name = "aether.browser.windows")]
#[settings]
pub struct WindowsSettings {
    /// The ids of the page's canvases opened as windows, in order, at least one: the renderer opens WebGPU on them.
    pub canvases: Vec<String>,
    /// The most of a canvas's pixels drawn to each of the page's CSS pixels: none for every device pixel of the screen.
    #[flag]
    pub density: f64,
}

impl Default for WindowsSettings {
    fn default() -> Self {
        Self { canvases: vec![String::from("canvas")], density: 0.0 }
    }
}

/// Opens each canvas the settings name as a window, standing as the page has it now, `0`, `1`, … in the order named,
/// begins the window root with them, and gives the browser its end, which answers each window the root asks to open or
/// close `NoHost`, as a page owns its canvases (ADR-0027). Each canvas is drawn at the size the page lays it out at,
/// at the settings' density, and the browser keeps the page's listeners that feed each window what it hears. Provides
/// the windows opened.
impl Bootstrap<Browser<Performance>> for Windows {
    const SECTION: &'static str = "windows";
    type Settings = WindowsSettings;
    type Needs = ();
    type Provides = (InitialWindows,);

    async fn bootstrap(
        settings: WindowsSettings,
        (): (),
        cx: &mut Bootstrapping<'_, Browser<Performance>, Self>,
    ) -> Result<(InitialWindows,), Unbuilt> {
        let page = Page::now().map_err(unopened)?;
        let density = Density::at_most(settings.density);
        let mut opening = Opening::new(cx.source(), cx.controller());
        let mut opened = Vec::with_capacity(settings.canvases.len());
        for id in &settings.canvases {
            let canvas = canvas_of(id)?;
            let shown = page.open(&canvas, &mut opening);
            cx.serve(canvas::listen(&canvas, &shown, density).map_err(unopened)?);
            opened.push(shown.window());
            cx.host().opened(canvas, shown);
        }
        let mut opened = opened.into_iter();
        let Some(first) = opened.next() else {
            return Err(Unbuilt::Failed("the window root's settings name no canvas to open".into()));
        };

        let (config, hosting) = opening.config();
        cx.begin(&(), config)?;
        cx.serve(hosting);

        Ok((InitialWindows::new(first, opened.collect()),))
    }
}

/// The page's canvas of the id `id`.
fn canvas_of(id: &str) -> Result<HtmlCanvasElement, Unbuilt> {
    let document = web_sys::window().and_then(|window| window.document());
    let element = document.and_then(|document| document.get_element_by_id(id));
    let canvas = element.and_then(|element| element.dyn_into::<HtmlCanvasElement>().ok());

    canvas.ok_or_else(|| Unbuilt::Unopened(format!("the page has no canvas of the id {id}").into()))
}
