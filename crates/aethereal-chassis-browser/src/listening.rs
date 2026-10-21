//! The page's callbacks the chassis registers, kept while the page listens: each pushes what it heard onto a feed and
//! returns, and nothing reaches the engine until the next frame brings it in (ADR-0010).

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsError};
use web_sys::{EventTarget, MutationObserver};

/// The callbacks the page runs as it hears what the chassis feeds the engine, which the page keeps for as long as it
/// listens. Dropped, it takes each off what it listens to.
pub struct Listening {
    listeners: Vec<Listener>,
    observers: Vec<Observer>,
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

/// A callback the page runs as it sees an element change, and the observer it runs through.
struct Observer {
    observer: MutationObserver,
    _callback: Closure<dyn FnMut()>,
}

impl Drop for Observer {
    fn drop(&mut self) {
        self.observer.disconnect();
    }
}

impl Listening {
    /// Listening to nothing yet.
    pub(crate) const fn new() -> Self {
        Self { listeners: Vec::new(), observers: Vec::new() }
    }

    /// Runs `heard` each time `target` hears `event`.
    pub(crate) fn on(
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

    /// Runs `seen` each time the page sees one of `attributes` of `element` set.
    pub(crate) fn watch(
        &mut self,
        element: &web_sys::Element,
        attributes: &[&str],
        seen: impl FnMut() + 'static,
    ) -> Result<(), JsError> {
        let callback = Closure::<dyn FnMut()>::new(seen);
        let observer = MutationObserver::new(callback.as_ref().unchecked_ref())
            .map_err(|_| JsError::new("the page would not observe an element"))?;
        let filter: js_sys::Array = attributes.iter().map(|name| wasm_bindgen::JsValue::from_str(name)).collect();
        let init = web_sys::MutationObserverInit::new();
        init.set_attribute_filter(&filter);
        observer
            .observe_with_options(element, &init)
            .map_err(|_| JsError::new("the page would not observe the element's attributes"))?;
        self.observers.push(Observer { observer, _callback: callback });

        Ok(())
    }
}
