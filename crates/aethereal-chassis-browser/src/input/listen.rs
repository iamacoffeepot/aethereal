//! The page's listeners: callbacks the page runs as it hears the pointer, the keys, and its controls, which push each
//! event onto the input root's feed and return. Nothing reaches the engine until the next frame brings it in
//! (ADR-0010).

use super::Event;
use super::Value;
use super::dom::{Canvas, Pointing, value_of};
use aethereal_substrate::Feed;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsError};
use web_sys::{
    Element, EventTarget, HtmlCanvasElement, HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement, KeyboardEvent,
    PointerEvent,
};

/// The callbacks the page runs as it hears what the input root is fed, which the page keeps for as long as it listens.
/// Dropped, it takes each off what it listens to.
pub struct Listening {
    listeners: Vec<Listener>,
}

/// One callback, and what it listens to for which event.
struct Listener {
    target: EventTarget,
    event: &'static str,
    callback: Closure<dyn FnMut(web_sys::Event)>,
}

impl Drop for Listener {
    fn drop(&mut self) {
        // A callback that would not come off is left to throw when the page calls it, which runs nothing of the
        // engine's: there is nothing else to do with it here.
        self.target.remove_event_listener_with_callback(self.event, self.callback.as_ref().unchecked_ref()).ok();
    }
}

impl Listening {
    /// Runs `heard` each time `target` hears `event`.
    fn on(
        &mut self,
        target: &EventTarget,
        event: &'static str,
        heard: impl FnMut(web_sys::Event) + 'static,
    ) -> Result<(), JsError> {
        let callback = Closure::<dyn FnMut(web_sys::Event)>::new(heard);
        target
            .add_event_listener_with_callback(event, callback.as_ref().unchecked_ref())
            .map_err(|_| JsError::new(&format!("the page would not listen for {event}")))?;
        self.listeners.push(Listener { target: target.clone(), event, callback });

        Ok(())
    }
}

/// Listens to the page for what the input root is fed, pushing each event onto `feed` as the page hears it: the
/// pointer over `canvas`, in its pixels; the keys, anywhere on the page; and each element marked `data-control` that
/// has an id, an input, a select, or a text area, whose value is pushed once now and again each time it changes.
///
/// What is pushed once the input root has been dropped reaches nothing, and is refused, which a callback has no use
/// for: the page stops listening when it drops what this returns.
///
/// # Errors
///
/// When the page has no window or document, or will not listen for one of the events.
pub fn listen(canvas: &HtmlCanvasElement, feed: &Feed<Event>) -> Result<Listening, JsError> {
    let window = web_sys::window().ok_or_else(|| JsError::new("the page has no window"))?;
    let document = window.document().ok_or_else(|| JsError::new("the page has no document"))?;
    let mut listening = Listening { listeners: Vec::new() };

    for (event, pointing) in
        [("pointermove", Pointing::Moved), ("pointerdown", Pointing::Down), ("pointerup", Pointing::Up)]
    {
        let (shown, feed) = (canvas.clone(), feed.clone());
        listening.on(canvas, event, move |event| {
            if let Some(pointer) = event.dyn_ref::<PointerEvent>() {
                feed.push(pointing.event(at(&shown, pointer), pointer.button())).ok();
            }
        })?;
    }

    let pressed = feed.clone();
    listening.on(&window, "keydown", move |event| {
        if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
            pressed.push(Event::KeyDown { code: key.code(), key: key.key(), repeat: key.repeat() }).ok();
        }
    })?;
    let released = feed.clone();
    listening.on(&window, "keyup", move |event| {
        if let Some(key) = event.dyn_ref::<KeyboardEvent>() {
            released.push(Event::KeyUp { code: key.code(), key: key.key() }).ok();
        }
    })?;

    let marked = document.query_selector_all("[data-control]").map_err(|_| JsError::new("no controls were found"))?;
    for control in (0..marked.length()).filter_map(|at| marked.get(at)?.dyn_into::<Element>().ok()) {
        let name = control.id();
        if name.is_empty() {
            continue;
        }
        let Some(value) = read(&control) else {
            continue;
        };
        feed.push(Event::Control { name: name.clone(), value }).ok();

        let (changed, feed) = (control.clone(), feed.clone());
        listening.on(&control, "input", move |_| {
            if let Some(value) = read(&changed) {
                feed.push(Event::Control { name: name.clone(), value }).ok();
            }
        })?;
    }

    Ok(listening)
}

/// Where `pointer` is on `canvas`, in the canvas's pixels: measured from the canvas's box as the page lays it out
/// now, so a canvas moved or resized since is read where it is.
fn at(canvas: &HtmlCanvasElement, pointer: &PointerEvent) -> (f32, f32) {
    let shown = canvas.get_bounding_client_rect();
    let canvas_at = Canvas::new(canvas.width(), canvas.height(), shown.width(), shown.height());

    canvas_at.pixel(f64::from(pointer.client_x()) - shown.left(), f64::from(pointer.client_y()) - shown.top())
}

/// What the control `element` holds: none when it is no input, select, or text area.
fn read(element: &Element) -> Option<Value> {
    if let Some(input) = element.dyn_ref::<HtmlInputElement>() {
        return Some(value_of(&input.type_(), input.value(), input.checked()));
    }
    if let Some(select) = element.dyn_ref::<HtmlSelectElement>() {
        return Some(Value::Text(select.value()));
    }

    element.dyn_ref::<HtmlTextAreaElement>().map(|area| Value::Text(area.value()))
}
