//! The page's clipboard (ADR-0024: The clipboard): `navigator.clipboard`, read and written as plain text by its
//! promises, which the page's event loop drives. The browser asks its reader before a page reads it, and a page may
//! write it only while it has the reader's focus, so either may be refused: a refusal is answered as one, never a
//! fault. A page served where the browser offers no clipboard, not over HTTPS nor from the local machine, answers that
//! it has none.

use aethereal_window::{Clipboard, ClipboardRead, ClipboardReading, ClipboardWriting, ClipboardWritten};
use js_sys::{Array, Function, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// The page's clipboard, served to the browser's jobs.
pub struct PageClipboard;

impl Clipboard for PageClipboard {
    fn read(&self) -> ClipboardReading<'_> {
        Box::pin(async {
            let Some(promise) = asked("readText", &[]) else {
                return ClipboardRead::Unavailable;
            };
            match JsFuture::from(promise).await.map(|read| read.as_string()) {
                Ok(Some(text)) => ClipboardRead::Text(text),
                Ok(None) => ClipboardRead::Empty,
                Err(_) => ClipboardRead::Refused,
            }
        })
    }

    fn write(&self, text: String) -> ClipboardWriting<'_> {
        Box::pin(async move {
            let Some(promise) = asked("writeText", &[JsValue::from_str(&text)]) else {
                return ClipboardWritten::Unavailable;
            };
            match JsFuture::from(promise).await {
                Ok(_) => ClipboardWritten::Written,
                Err(_) => ClipboardWritten::Refused,
            }
        })
    }
}

/// The promise `navigator.clipboard`'s `method` answers called with `arguments`: none when the page has no clipboard,
/// or the call threw before it answered one.
fn asked(method: &str, arguments: &[JsValue]) -> Option<Promise> {
    let navigator = web_sys::window()?.navigator();
    let clipboard = Reflect::get(&navigator, &JsValue::from_str("clipboard")).ok().filter(JsValue::is_object)?;
    let call = Reflect::get(&clipboard, &JsValue::from_str(method)).ok()?.dyn_into::<Function>().ok()?;
    let handed: Array = arguments.iter().collect();

    call.apply(&clipboard, &handed).ok()?.dyn_into::<Promise>().ok()
}
