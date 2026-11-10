//! The chart's columns at one span, kept as the cycles come (ADR-0053: The three stops): each column whose span has
//! passed as its fastest, median, and slowest cycles, worked out once as it closed, and the newest, still open, as the cycles it
//! has so far. A chart cut into another count of columns, as the window is resized, is binned again from the cycles
//! kept.

use super::Column;
use super::bin::Bin;
use aethereal_data::Plain;
use std::collections::VecDeque;

/// The newest column, while a cycle has fallen in one.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
enum Open {
    /// No cycle has been taken in since the columns were cut.
    Nothing,
    /// The cycles of the newest column so far.
    Filling(Bin),
}

/// The columns of one span: those closed, oldest first, and the open one.
#[derive(Plain, Debug, Clone, PartialEq, Eq)]
pub struct Bins {
    span_nanos: u64,
    closed: VecDeque<Column>,
    open: Open,
}

impl Bins {
    /// No columns yet, each `span_nanos` of the program's time.
    pub const fn new(span_nanos: u64) -> Self {
        Self { span_nanos, closed: VecDeque::new(), open: Open::Nothing }
    }

    /// How long each column's span is.
    pub const fn span_nanos(&self) -> u64 {
        self.span_nanos
    }

    /// Takes in a cycle `delta_nanos` long that ended `elapsed_nanos` into the program: into the open column when it
    /// falls in its span, or else into a new one, the open column closed as what it came to.
    pub fn take(&mut self, elapsed_nanos: u64, delta_nanos: u64) {
        let span = elapsed_nanos / self.span_nanos.max(1);
        if let Open::Filling(bin) = &mut self.open {
            if bin.span() == span {
                bin.take(delta_nanos);
                return;
            }
            self.closed.push_back(bin.column());
        }
        self.open = Open::Filling(Bin::first(span, delta_nanos));
    }

    /// Lets go of every closed column before `oldest`.
    pub fn keep_from(&mut self, oldest: u64) {
        while let Some(column) = self.closed.front() {
            let kept = column.span >= oldest;
            if kept {
                break;
            }
            self.closed.pop_front();
        }
    }

    /// The columns from the newest, the open one, back to the oldest kept.
    pub fn newest_first(&self) -> impl Iterator<Item = Column> + '_ {
        let open = match &self.open {
            Open::Nothing => None,
            Open::Filling(bin) => Some(bin.column()),
        };

        open.into_iter().chain(self.closed.iter().rev().copied())
    }
}
