//! The cursor each canvas shows (ADR-0055), as the window root asks for it: set as the canvas's CSS `cursor`, the
//! page's name for each icon, and an image as a URL of its pixels, made the first time it is shown and kept. A native
//! build of the chassis opens no canvas, and shows no cursor.

use aethereal_substrate::Engine;
use aethereal_window::{Cursor, WindowId};
#[cfg(target_arch = "wasm32")]
use {
    aethereal_window::{CursorImage, MadeCursors},
    wasm_bindgen::{Clamped, JsCast, JsValue},
    web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData},
};

/// The canvases opened as windows, each beside its window, which a cursor is shown over, and the CSS of each image
/// shown, made once.
#[cfg(target_arch = "wasm32")]
pub struct Cursors {
    canvases: Vec<(WindowId, HtmlCanvasElement)>,
    made: MadeCursors<String>,
}

/// No canvas a cursor is shown over: a native build of the chassis opens none.
#[cfg(not(target_arch = "wasm32"))]
pub struct Cursors;

#[cfg(target_arch = "wasm32")]
impl Cursors {
    /// Showing cursors over `canvases`, each beside the window it was opened as.
    #[must_use]
    pub fn over(canvases: Vec<(WindowId, HtmlCanvasElement)>) -> Self {
        Self { canvases, made: MadeCursors::default() }
    }

    /// Showing cursors over no canvas, as an engine on no page does.
    #[must_use]
    pub fn none() -> Self {
        Self::over(Vec::new())
    }

    /// Shows `cursor` over the canvas opened as `window`, as the window root asked: an icon by the page's name for it,
    /// none, or an image whose pixels `engine` holds, as a URL made of them the first time it is shown, and the one
    /// made then each time after. An image the page would not draw shows the default. A canvas not opened as a window
    /// shows nothing.
    pub(crate) fn show(&mut self, engine: &mut Engine, window: WindowId, cursor: &Cursor) {
        let Some((_, canvas)) = self.canvases.iter().find(|(opened, _)| *opened == window) else {
            return;
        };
        let css = match cursor {
            Cursor::Default => "default",
            Cursor::Hidden => "none",
            Cursor::Text => "text",
            Cursor::Hand => "pointer",
            Cursor::Grab => "grab",
            Cursor::Grabbing => "grabbing",
            Cursor::NotAllowed => "not-allowed",
            Cursor::Image(image) => {
                let made = self.made.of(engine, image, |image, pixels| image_css(canvas, image, pixels));
                made.map_or("default", String::as_str)
            }
        };
        // A page whose style will not take a cursor shows the one it had, and nothing waits on it.
        let _ = canvas.style().set_property("cursor", css);
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Cursors {
    /// Showing cursors over no canvas.
    #[must_use]
    pub const fn none() -> Self {
        Self
    }

    /// Shows nothing: no canvas is opened.
    #[expect(clippy::unused_self, reason = "the page's build shows the cursor over its canvas")]
    pub(crate) const fn show(&mut self, _: &mut Engine, _: WindowId, _: &Cursor) {}
}

/// The CSS `cursor` of `image`, whose pixels are `pixels`, shown over `canvas`: a URL of its pixels drawn into a canvas
/// of its own, its hotspot, and the default, which a page that will not draw it shows.
///
/// # Errors
///
/// What the page said when it would not draw it.
#[cfg(target_arch = "wasm32")]
fn image_css(canvas: &HtmlCanvasElement, image: &CursorImage, pixels: &[u8]) -> Result<String, JsValue> {
    let unmade = |what: &str| JsValue::from_str(what);
    let document = canvas.owner_document().ok_or_else(|| unmade("the canvas is in no document"))?;
    let drawn: HtmlCanvasElement = document.create_element("canvas")?.dyn_into().map_err(JsValue::from)?;
    let (width, height) = (u32::from(image.width), u32::from(image.height));
    drawn.set_width(width);
    drawn.set_height(height);
    let context = drawn.get_context("2d")?.ok_or_else(|| unmade("the canvas draws in no 2D context"))?;
    let context: CanvasRenderingContext2d = context.dyn_into().map_err(JsValue::from)?;
    context.put_image_data(&ImageData::new_with_u8_clamped_array_and_sh(Clamped(pixels), width, height)?, 0.0, 0.0)?;

    Ok(format!("url({}) {} {}, default", drawn.to_data_url()?, image.hotspot_x, image.hotspot_y))
}
