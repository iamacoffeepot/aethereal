//! The canvas opened as a window, and the page's listeners that feed it: callbacks the page runs as it hears the
//! pointer over the canvas, the keys, its focus, and a resize, which push each event onto the window's feed and
//! return.

use super::dom::{Canvas, Pointing};
use crate::Listening;
use aethereal_window::{Event, Opening, Surface, WindowFeed};
use wasm_bindgen::{JsCast, JsError};
use web_sys::{HtmlCanvasElement, KeyboardEvent, PointerEvent};

/// Opens `canvas` as a window, standing as the page has it now: its backing store as its surface, the page's device
/// pixels to each CSS pixel as its scale, and focused when the page has the keyboard.
///
/// # Errors
///
/// When the page has no window or document, or will not say whether it has the keyboard.
pub fn open(canvas: &HtmlCanvasElement, opening: &mut Opening) -> Result<WindowFeed, JsError> {
    let window = web_sys::window().ok_or_else(|| JsError::new("the page has no window"))?;
    let document = window.document().ok_or_else(|| JsError::new("the page has no document"))?;
    let focused = document.has_focus().map_err(|_| JsError::new("the page would not say whether it is focused"))?;

    Ok(opening.open(surface(canvas, &window), focused))
}

/// Listens to the page for what the canvas's window hears, pushing each event onto `window` as the page hears it: the
/// pointer over `canvas`, in its pixels; the keys, the focus, and the page's resizes, on the page's window; and the
/// canvas's backing store set to another size.
///
/// A resize is pushed as the canvas's surface reads then, whether or not it changed: the window's actor sends on only
/// one that did. What is pushed once the window root has been dropped reaches nothing, and is refused, which a
/// callback has no use for: the page stops listening when it drops what this returns.
///
/// # Errors
///
/// When the page has no window, or will not listen for one of the events.
pub fn listen(canvas: &HtmlCanvasElement, window: &WindowFeed) -> Result<Listening, JsError> {
    let page = web_sys::window().ok_or_else(|| JsError::new("the page has no window"))?;
    let mut listening = Listening::new();

    for (event, pointing) in
        [("pointermove", Pointing::Moved), ("pointerdown", Pointing::Down), ("pointerup", Pointing::Up)]
    {
        let (shown, feed) = (canvas.clone(), window.clone());
        listening.on(canvas, event, move |event| {
            let Some(pointer) = event.dyn_ref::<PointerEvent>() else {
                return;
            };
            if let Some(event) = pointing.event(at(&shown, pointer), pointer.button()) {
                feed.push(event).ok();
            }
        })?;
    }

    let pressed = window.clone();
    listening.on(&page, "keydown", move |event| {
        if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
            pressed.push(Event::KeyDown { code: key.code(), key: key.key(), repeat: key.repeat() }).ok();
        }
    })?;
    let released = window.clone();
    listening.on(&page, "keyup", move |event| {
        if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
            released.push(Event::KeyUp { code: key.code(), key: key.key() }).ok();
        }
    })?;

    let gained = window.clone();
    listening.on(&page, "focus", move |_| {
        gained.push(Event::FocusGained).ok();
    })?;
    let lost = window.clone();
    listening.on(&page, "blur", move |_| {
        lost.push(Event::FocusLost).ok();
    })?;

    // The page's resize says its device pixels to each CSS pixel may have changed, as when it is zoomed; a canvas's
    // backing store changes only when its width or height is set, which the observer sees.
    let (resized, shown, scaled) = (window.clone(), canvas.clone(), page.clone());
    listening.on(&page, "resize", move |_| {
        resized.push(Event::Resized(surface(&shown, &scaled))).ok();
    })?;
    let (resized, shown) = (window.clone(), canvas.clone());
    listening.watch(canvas, &["width", "height"], move || {
        resized.push(Event::Resized(surface(&shown, &page))).ok();
    })?;

    Ok(listening)
}

/// The canvas's surface as the page has it now: its backing store, on a screen of the page's device pixels to each
/// CSS pixel.
fn surface(canvas: &HtmlCanvasElement, page: &web_sys::Window) -> Surface {
    Surface { width: canvas.width(), height: canvas.height(), scale: page.device_pixel_ratio() }
}

/// Where `pointer` is on `canvas`, in the canvas's pixels: measured from the canvas's box as the page lays it out
/// now, so a canvas moved or resized since is read where it is.
fn at(canvas: &HtmlCanvasElement, pointer: &PointerEvent) -> (f32, f32) {
    let shown = canvas.get_bounding_client_rect();
    let canvas_at = Canvas::new(canvas.width(), canvas.height(), shown.width(), shown.height());

    canvas_at.pixel(f64::from(pointer.client_x()) - shown.left(), f64::from(pointer.client_y()) - shown.top())
}
