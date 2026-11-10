//! The renderer's answer to which adapter it draws on (ADR-0053: Frame statistics), asked once as the overlay begins
//! and kept for the line beneath the band's meters.

use super::Overlay;
use super::texts::TextRole;
use crate::meters::Adapter;
use aethereal_actor::{Answer, Answered, Fault, Handles, Level, Path, Tells, Turn};
use aethereal_data::Plain;
use aethereal_render::Renderer;
use aethereal_render::stats::GpuDescribed;
use aethereal_render::text::WriteText;

/// The renderer was asked which adapter it draws on.
#[derive(Plain)]
#[kind(name = "aether.overlay.adapter_asked")]
pub struct AdapterAsked;

/// Keeps the adapter, or that the renderer did not say it, and writes the adapter's line when it is placed.
impl Handles<Answer<GpuDescribed, AdapterAsked>> for Overlay {
    type Sends = (Tells<Path<Renderer>, WriteText>,);

    fn handle(
        &mut self,
        (answered, AdapterAsked): (Answered<GpuDescribed>, AdapterAsked),
        turn: &mut Turn<'_, Self, Self::Sends>,
    ) -> Result<(), Fault> {
        self.adapter = match answered {
            Answered::Yes(described) => Adapter::Described(described),
            Answered::No(why) => {
                turn.log(Level::Warn, format_args!("the renderer did not say its adapter: {why:?}"));
                Adapter::Unanswered
            }
        };

        self.write_text(TextRole::Adapter, turn)
    }
}
