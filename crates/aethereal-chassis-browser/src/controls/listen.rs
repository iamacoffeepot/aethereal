//! The page's listeners for its controls: a callback on each element the page marks, which pushes what it holds onto
//! the controls root's feed as it changes and returns.

use super::read::value_of;
use super::{Control, Value};
use crate::Listening;
use aethereal_substrate::Feed;
use wasm_bindgen::{JsCast, JsError};
use web_sys::{Element, HtmlInputElement, HtmlSelectElement, HtmlTextAreaElement};

/// Listens to each element of the page marked `data-control` that has an id, an input, a select, or a text area,
/// pushing what it holds onto `feed` once now and again each time it changes.
///
/// What is pushed once the controls root has been dropped reaches nothing, and is refused, which a callback has no use
/// for: the page stops listening when it drops what this returns.
///
/// # Errors
///
/// When the page has no window or document, or will not listen to one of the controls.
pub fn listen(feed: &Feed<Control>) -> Result<Listening, JsError> {
    let window = web_sys::window().ok_or_else(|| JsError::new("the page has no window"))?;
    let document = window.document().ok_or_else(|| JsError::new("the page has no document"))?;
    let mut listening = Listening::new();

    let marked = document.query_selector_all("[data-control]").map_err(|_| JsError::new("no controls were found"))?;
    for control in (0..marked.length()).filter_map(|at| marked.get(at)?.dyn_into::<Element>().ok()) {
        let name = control.id();
        if name.is_empty() {
            continue;
        }
        let Some(value) = read(&control) else {
            continue;
        };
        feed.push(Control { name: name.clone(), value }).ok();

        let (changed, feed) = (control.clone(), feed.clone());
        listening.on(&control, "input", move |_| {
            if let Some(value) = read(&changed) {
                feed.push(Control { name: name.clone(), value }).ok();
            }
        })?;
    }

    Ok(listening)
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
