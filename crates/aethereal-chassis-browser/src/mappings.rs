//! What the chassis serves the renderer between steps: the buffers it asks to have mapped for reading, mapped, and
//! word of each pushed onto its source once the GPU has (ADR-0019).

use aethereal_actor::Requests;
use aethereal_render::mapping::{Mapped, Mapping};
use aethereal_render::{Gpu, RendererConfig};
use aethereal_substrate::{Engine, Feed, Setup};

/// The chassis's ends of the renderer's controller and source.
pub struct Mappings {
    requests: Requests<Mapping>,
    feed: Feed<Mapped>,
}

/// The renderer's config, drawing with `gpu`, made in `setup`, and what the chassis serves it between steps.
pub fn renderer<S>(setup: &mut Setup<S>, gpu: Gpu) -> (RendererConfig, Mappings) {
    let gpu = setup.hand(gpu);
    let (mapping, requests) = setup.controller();
    let (mapped, feed) = setup.source();

    (RendererConfig { gpu, mapping, mapped }, Mappings { requests, feed })
}

impl Mappings {
    /// The ends of a controller and a source no renderer holds, for an engine that has none, such as a page that
    /// draws nothing: nothing is asked.
    pub fn unasked(engine: &mut Engine) -> Self {
        let (_, requests) = engine.controller();
        let (_, feed) = engine.source();

        Self { requests, feed }
    }

    /// Starts mapping each buffer the renderer asked for since the last: the page's promise pushes what became of it
    /// when the GPU has mapped it, and nothing waits for it.
    pub fn serve(&self) {
        for mapping in self.requests.take() {
            let feed = self.feed.clone();
            mapping.start(move |mapped| {
                // Refused only once the renderer has gone, and nothing is then owed what it read.
                let _ = feed.push(mapped);
            });
        }
    }
}
