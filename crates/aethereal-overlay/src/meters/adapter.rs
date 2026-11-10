//! The GPU adapter's line beneath the meters (ADR-0053: Frame statistics): the adapter the renderer draws on, asked
//! once as the overlay begins and kept, in stone, `name · backend · driver driver_info`, cut at its end to the right
//! column's width and set flush right in it, so it ends where the meters' values do.

use super::figures::cut_back;
use crate::colour::STONE;
use aethereal_data::Plain;
use aethereal_render::stats::GpuDescribed;
use aethereal_render::text::Run;

/// What the overlay knows of the adapter the renderer draws on.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub enum Adapter {
    /// Asked, as the overlay began, and not yet answered: the line is blank.
    Asked,
    /// Answered.
    Described(GpuDescribed),
    /// The renderer did not answer: the line says so.
    Unanswered,
}

impl Adapter {
    /// The runs of the adapter's line on a right column `columns` characters wide, flush right in it: a space while it
    /// is asked, as a text holds something.
    pub fn runs(&self, columns: usize) -> Vec<Run> {
        let line = match self {
            Self::Asked => String::from(" "),
            Self::Described(described) => cut_back(&written(described), columns),
            Self::Unanswered => cut_back("adapter unknown: the renderer did not say", columns),
        };

        vec![Run::new(&format!("{line:>columns$}"), STONE.opaque())]
    }
}

/// The adapter as its line writes it: its name, its backend, and its driver and the driver's description of itself,
/// parted by dots, a part the platform leaves empty left out.
pub(super) fn written(described: &GpuDescribed) -> String {
    let driver = [described.driver.as_str(), described.driver_info.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let parts = [described.name.as_str(), described.backend.as_str(), driver.as_str()];

    parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" · ")
}
