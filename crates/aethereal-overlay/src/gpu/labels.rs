//! The passes' labels, each given a place in the GPU's stack, and with it a colour, in the order it was first heard,
//! as a program's passes are few and run in the same order frame after frame (ADR-0053: The three stops).

use super::STACKED;
use aethereal_data::Plain;

/// What the renderer's labels begin with, which every pass of its own shares, and which the figures leave out.
const RENDER_PREFIX: &str = "aether.render.";

/// Where a pass of a label is stacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// At this place in the stack, from its foot.
    Pass(usize),
    /// Past the stack's room: counted in the frame's total, and stacked nowhere.
    Past,
}

/// The labels heard, in the order they were first heard, at most a stack's room of them.
#[derive(Plain, Debug, Default, Clone, PartialEq, Eq)]
pub struct Labels {
    labels: Vec<String>,
}

impl Labels {
    /// Where a pass of `label` is stacked: at its place when it has been heard, or at the next when there is room.
    pub fn slot(&mut self, label: &str) -> Slot {
        if let Some(index) = self.labels.iter().position(|heard| heard == label) {
            return Slot::Pass(index);
        }
        let room = self.labels.len() < STACKED;
        if !room {
            return Slot::Past;
        }
        self.labels.push(String::from(label));

        Slot::Pass(self.labels.len() - 1)
    }

    /// Each label heard, in the stack's order, as the figures show it: the renderer's own prefix left out.
    pub fn shown(&self) -> impl Iterator<Item = &str> {
        self.labels.iter().map(|label| label.strip_prefix(RENDER_PREFIX).unwrap_or(label))
    }
}
