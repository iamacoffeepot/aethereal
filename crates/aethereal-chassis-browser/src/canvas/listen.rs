//! The canvas opened as a window, and the page's listeners that feed it: callbacks the page runs as it hears the
//! pointer and the wheel over the canvas, the keys, its focus, and a resize, which push each event onto the window's
//! feed and return.

use super::dom::{Canvas, Density, Measured, Pointing};
use crate::Listening;
use aethereal_window::{Event, Opening, Surface, WindowFeed, WindowId};
use core::cell::Cell;
use js_sys::Reflect;
use std::rc::Rc;
use wasm_bindgen::{JsCast, JsError, JsValue};
use web_sys::{
    HtmlCanvasElement, KeyboardEvent, MouseEvent, PointerEvent, ResizeObserverBoxOptions, ResizeObserverEntry,
    ResizeObserverSize, WheelEvent,
};

/// The longest side of the canvas, in its pixels: the longest side of a texture of the device the chassis opens,
/// which asks WebGPU for its default limits, and so is given those.
const LONGEST: u32 = 8192;

/// The canvas opened as a window: the window's feed, and its size as last pushed, in the canvas's pixels, which the
/// pointer is read in and the chassis configures the canvas's surface to as each frame begins.
pub struct Shown {
    feed: WindowFeed,
    size: Rc<Cell<(u32, u32)>>,
}

impl Shown {
    /// The window, by its id.
    #[must_use]
    pub const fn window(&self) -> WindowId {
        self.feed.window()
    }

    /// The window's size as last pushed, shared with the listeners that push it.
    pub(crate) fn size(&self) -> Rc<Cell<(u32, u32)>> {
        Rc::clone(&self.size)
    }
}

/// Opens `canvas` as a window, standing as the page has it now: its backing store as its surface, the page's device
/// pixels to each CSS pixel as its scale, and focused when the page has the keyboard. A page with several canvases
/// opens each, and each is the next window, `0`, `1`, … in the order opened.
///
/// # Errors
///
/// When the page has no window or document, or will not say whether it has the keyboard.
pub fn open(canvas: &HtmlCanvasElement, opening: &mut Opening) -> Result<Shown, JsError> {
    Ok(Page::now()?.open(canvas, opening))
}

/// The page as it stands as its canvases are opened: its window, and whether it has the keyboard.
pub struct Page {
    window: web_sys::Window,
    focused: bool,
}

impl Page {
    /// The page as it stands now.
    ///
    /// # Errors
    ///
    /// When the page has no window or document, or will not say whether it has the keyboard.
    pub fn now() -> Result<Self, JsError> {
        let window = web_sys::window().ok_or_else(|| JsError::new("the page has no window"))?;
        let document = window.document().ok_or_else(|| JsError::new("the page has no document"))?;
        let focused = document.has_focus().map_err(|_| JsError::new("the page would not say whether it is focused"))?;

        Ok(Self { window, focused })
    }

    /// Opens `canvas` as the next window, standing as the page has it now.
    pub fn open(&self, canvas: &HtmlCanvasElement, opening: &mut Opening) -> Shown {
        let feed = opening.open(surface(canvas, &self.window), self.focused);
        let size = Rc::new(Cell::new((canvas.width(), canvas.height())));

        Shown { feed, size }
    }
}

/// Listens to the page for what the canvas's window hears, pushing each event onto `window` as the page hears it: the
/// pointer and the wheel over `canvas`, in its pixels; the keys, the focus, and the page's resizes, on the page's
/// window; and the canvas laid out at another size.
///
/// The wheel over the canvas stops the page's default, so the page does not scroll while the wheel turns over it.
///
/// The keys and the focus are the page's, heard on its window: each canvas listened to hears every key and each change
/// of the page's focus, as the page has one keyboard.
///
/// **The canvas is drawn at the size the page lays it out at**, at `density`: whenever the page lays it out at
/// another size, which it measures in device pixels where it can, the window is resized to it, and the chassis
/// configures its surface to it as its next frame begins, which sets the canvas's backing store. The page lays the
/// canvas out once a frame at most, so a window dragged through many sizes is resized once a frame. The pointer and
/// the wheel are read in the pixels of the size last pushed, the window's as its actors were told it.
///
/// A resize is pushed as the window's surface reads then, whether or not it changed: the window's actor sends on only
/// one that did. What is pushed once the window root has been dropped reaches nothing, and is refused, which a
/// callback has no use for: the page stops listening when it drops what this returns.
///
/// # Errors
///
/// When the page has no window, or will not listen for one of the events.
pub fn listen(canvas: &HtmlCanvasElement, shown: &Shown, density: Density) -> Result<Listening, JsError> {
    let page = web_sys::window().ok_or_else(|| JsError::new("the page has no window"))?;
    let mut listening = Listening::new();
    let (window, sized) = (&shown.feed, shown.size());

    for (event, pointing) in
        [("pointermove", Pointing::Moved), ("pointerdown", Pointing::Down), ("pointerup", Pointing::Up)]
    {
        let (shown, feed, size) = (canvas.clone(), window.clone(), Rc::clone(&sized));
        listening.on(canvas, event, move |event| {
            let Some(pointer) = event.dyn_ref::<PointerEvent>() else {
                return;
            };
            if let Some(event) = pointing.event(at(&shown, size.get(), pointer), pointer.button()) {
                feed.push(event).ok();
            }
        })?;
    }

    let (shown, turned, size) = (canvas.clone(), window.clone(), Rc::clone(&sized));
    listening.on_preventing(canvas, "wheel", move |event| {
        let Some(wheel) = event.dyn_ref::<WheelEvent>() else {
            return;
        };
        wheel.prevent_default();
        let (laid_out, from) = laid_out(&shown, size.get(), wheel);
        if let Some(event) = laid_out.wheel(from, (wheel.delta_x(), wheel.delta_y()), wheel.delta_mode()) {
            turned.push(event).ok();
        }
    })?;

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

    // The page's resize says its device pixels to each CSS pixel may have changed, as when it is zoomed, which the
    // observer does not see when the canvas's device pixels stay as they were.
    let (resized, size, scaled) = (window.clone(), Rc::clone(&sized), page.clone());
    listening.on(&page, "resize", move |_| {
        let (width, height) = size.get();
        resized.push(Event::Resized(Surface { width, height, scale: density.scale(scaled.device_pixel_ratio()) })).ok();
    })?;
    let in_device_pixels = measures_device_pixels();
    let measured = if in_device_pixels {
        ResizeObserverBoxOptions::DevicePixelContentBox
    } else {
        ResizeObserverBoxOptions::ContentBox
    };
    let resized = window.clone();
    listening.resized(canvas, measured, move |entries| {
        let Some(entry) = entries.iter().last().and_then(|entry| entry.dyn_into::<ResizeObserverEntry>().ok()) else {
            return;
        };
        let ratio = page.device_pixel_ratio();
        let (width, height) = laid_out_at(&entry, in_device_pixels).pixels(density, ratio, LONGEST);
        sized.set((width, height));
        resized.push(Event::Resized(Surface { width, height, scale: density.scale(ratio) })).ok();
    })?;

    Ok(listening)
}

/// The canvas's surface as the page has it now: its backing store, on a screen of the page's device pixels to each
/// CSS pixel.
fn surface(canvas: &HtmlCanvasElement, page: &web_sys::Window) -> Surface {
    Surface { width: canvas.width(), height: canvas.height(), scale: page.device_pixel_ratio() }
}

/// Whether the page measures an element's box in device pixels, as `devicePixelContentBoxSize`, which not every
/// browser gives: observing for a box the page has no name for throws, so it is asked first.
fn measures_device_pixels() -> bool {
    let global = js_sys::global();
    Reflect::get(&global, &JsValue::from_str("ResizeObserverEntry"))
        .and_then(|entry| Reflect::get(&entry, &JsValue::from_str("prototype")))
        .and_then(|prototype| Reflect::has(&prototype, &JsValue::from_str("devicePixelContentBoxSize")))
        .unwrap_or(false)
}

/// The size `entry` says the canvas is laid out at: in device pixels when the page measures them, and in CSS pixels
/// otherwise, as every page gives them. Its sides are read as a canvas whose lines run across, its inline size its
/// width.
fn laid_out_at(entry: &ResizeObserverEntry, in_device_pixels: bool) -> Measured {
    // Read only where the page gives it: it is no array elsewhere.
    let device = in_device_pixels
        .then(|| entry.device_pixel_content_box_size().get(0))
        .and_then(|size| size.dyn_into::<ResizeObserverSize>().ok());
    if let Some(size) = device {
        return Measured::DevicePixels { width: size.inline_size(), height: size.block_size() };
    }
    let content = entry.content_rect();

    Measured::CssPixels { width: content.width(), height: content.height() }
}

/// Where `pointer` is on `canvas`, drawn at `size`, in the canvas's pixels.
fn at(canvas: &HtmlCanvasElement, size: (u32, u32), pointer: &PointerEvent) -> (f32, f32) {
    let (laid_out, (from_left, from_top)) = laid_out(canvas, size, pointer);

    laid_out.pixel(from_left, from_top)
}

/// `canvas`, drawn at `size`, as the page lays it out now, and where `pointer` is from its top left, in CSS pixels:
/// measured from the canvas's box now, so a canvas moved or resized since is read where it is.
fn laid_out(canvas: &HtmlCanvasElement, (width, height): (u32, u32), pointer: &MouseEvent) -> (Canvas, (f64, f64)) {
    let shown = canvas.get_bounding_client_rect();
    let laid_out = Canvas::new(width, height, shown.width(), shown.height());
    let from = (f64::from(pointer.client_x()) - shown.left(), f64::from(pointer.client_y()) - shown.top());

    (laid_out, from)
}
