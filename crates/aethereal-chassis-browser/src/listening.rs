//! The page's callbacks the chassis registers, kept while the page listens: each pushes what it heard onto a feed and
//! returns, and nothing reaches the engine until the next frame brings it in (ADR-0010).

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsError};
use web_sys::{AddEventListenerOptions, EventTarget, ResizeObserver, ResizeObserverBoxOptions, ResizeObserverOptions};

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

/// A callback the page runs as it sees an element resized, and the observer it runs through.
struct Observer {
    observer: ResizeObserver,
    _callback: Closure<dyn FnMut(js_sys::Array)>,
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

    /// Runs `heard` each time `target` hears `event`, registered as the page registers a listener by default.
    pub(crate) fn on(
        &mut self,
        target: &EventTarget,
        event: &'static str,
        heard: impl FnMut(web_sys::Event) + 'static,
    ) -> Result<(), JsError> {
        self.register(target, event, heard, &AddEventListenerOptions::new())
    }

    /// Runs `heard` each time `target` hears `event`, registered as not passive, so that `heard` may stop the page's
    /// default with `preventDefault`: a page takes a passive listener's `preventDefault` as nothing, and may make a
    /// listener passive by default, as it does one for `wheel` on its window or document.
    pub(crate) fn on_preventing(
        &mut self,
        target: &EventTarget,
        event: &'static str,
        heard: impl FnMut(web_sys::Event) + 'static,
    ) -> Result<(), JsError> {
        let options = AddEventListenerOptions::new();
        options.set_passive(false);

        self.register(target, event, heard, &options)
    }

    /// Runs `heard` each time `target` hears `event`, registered with `options`.
    fn register(
        &mut self,
        target: &EventTarget,
        event: &'static str,
        heard: impl FnMut(web_sys::Event) + 'static,
        options: &AddEventListenerOptions,
    ) -> Result<(), JsError> {
        let callback = Closure::<dyn FnMut(web_sys::Event)>::new(heard);
        target
            .add_event_listener_with_callback_and_add_event_listener_options(
                event,
                callback.as_ref().unchecked_ref(),
                options,
            )
            .map_err(|_| JsError::new(&format!("the page would not listen for {event}")))?;
        self.listeners.push(Listener { target: target.clone(), event, callback });

        Ok(())
    }

    /// Runs `seen` with the page's entries for `element` each time the page lays it out at another size, measured as
    /// `measured` says, and once as it begins observing it. The page runs it once a frame at most, after its animation
    /// frame's callbacks and before it paints.
    pub(crate) fn resized(
        &mut self,
        element: &web_sys::Element,
        measured: ResizeObserverBoxOptions,
        seen: impl FnMut(js_sys::Array) + 'static,
    ) -> Result<(), JsError> {
        let callback = Closure::<dyn FnMut(js_sys::Array)>::new(seen);
        let observer = ResizeObserver::new(callback.as_ref().unchecked_ref())
            .map_err(|_| JsError::new("the page would not observe an element's size"))?;
        let options = ResizeObserverOptions::new();
        options.set_box(measured);
        observer.observe_with_options(element, &options);
        self.observers.push(Observer { observer, _callback: callback });

        Ok(())
    }
}
