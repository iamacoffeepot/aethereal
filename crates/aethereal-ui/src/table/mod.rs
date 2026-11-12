//! A screen's widget table (ADR-0056: Widget state is kept by key): each widget's state, under its key path, made with
//! `Default` the first time the key appears, kept across rebuilds, and dropped when the key goes. No widget makes or
//! frees its own. It also keeps which widget has the screen's focus, at most one, by its key path, cleared when that
//! key goes or its widget can no longer hold focus (ADR-0056: Focus and keys).
//!
//! The table is part of the screen's state, so it moves onto new code with the screen (ADR-0054). Each state is kept
//! as its bytes beside its kind, so a table of every widget's state is one plain value, and a state whose type
//! changed, a different widget at the same key or the same widget on new code whose state's layout changed, is made
//! anew rather than read as something else (ADR-0006).

#[cfg(test)]
mod tests;

use crate::tree::{Flat, KeyPath};
use crate::widget::Widget;
use aethereal_data::{Budget, KindId, Plain, from_bytes, to_bytes};

/// The most bytes a state read out of the table may decode into.
const MOST_STATE_BYTES: usize = 64 * 1024;

/// Each widget's state, by its key path, in the paths' order, and the widget that has the screen's focus.
#[derive(Debug, Clone, PartialEq, Default, Plain)]
pub struct Table {
    kept: Vec<Kept>,
    focus: Focus,
}

/// Which widget has a screen's focus: none until one is pressed or tabbed to.
#[derive(Debug, Clone, PartialEq, Eq, Default, Plain)]
pub enum Focus {
    /// None has.
    #[default]
    Nowhere,
    /// The widget at this key path has.
    On(KeyPath),
}

/// One widget's state: its key path, its state's kind, and the state's bytes.
#[derive(Debug, Clone, PartialEq, Plain)]
struct Kept {
    path: KeyPath,
    kind: KindId,
    state: Vec<u8>,
}

impl Table {
    /// Keeps a state for each node of `flat`, a tree's nodes: a state kept under the node's path, of its widget's
    /// state's kind, stays as it is; any other is made with `Default`; and a state no node's path names is dropped.
    /// Focus held by a path no node names, or by a widget that cannot hold it, is cleared.
    pub(crate) fn refresh(&mut self, flat: &[Flat<'_>]) {
        if let Focus::On(path) = &self.focus {
            let holds = flat.iter().any(|flat| flat.path == *path && flat.node.widget().focusable());
            if !holds {
                self.focus = Focus::Nowhere;
            }
        }
        let mut kept: Vec<Kept> = flat
            .iter()
            .map(|flat| {
                let widget = flat.node.widget();
                let kind = widget.state_kind();
                let found = self.found(&flat.path).filter(|kept| kept.kind.same(kind));
                let state = found.map_or_else(|| widget.first_state(), |kept| kept.state.clone());

                Kept { path: flat.path.clone(), kind, state }
            })
            .collect();
        kept.sort_unstable_by(|left, right| left.path.cmp(&right.path));

        self.kept = kept;
    }

    /// The state kept under `path`, if any.
    fn found(&self, path: &KeyPath) -> Option<&Kept> {
        self.kept.binary_search_by(|kept| kept.path.cmp(path)).ok().map(|at| &self.kept[at])
    }

    /// The bytes of the state kept under `path`, if any.
    pub(crate) fn bytes(&self, path: &KeyPath) -> Option<&[u8]> {
        self.found(path).map(|kept| kept.state.as_slice())
    }

    /// Keeps `state`, of the kind `kind`, under `path`, in place of what was kept there.
    pub(crate) fn write(&mut self, path: &KeyPath, kind: KindId, state: Vec<u8>) {
        let kept = Kept { path: path.clone(), kind, state };
        match self.kept.binary_search_by(|kept| kept.path.cmp(path)) {
            Ok(at) => self.kept[at] = kept,
            Err(at) => self.kept.insert(at, kept),
        }
    }

    /// Keeps `state`, the state of a widget `W`, under `path`, in place of what was kept there: as the screen's library
    /// opens and shuts the stock widgets whose content it shows in its top layer.
    pub(crate) fn put<W: Widget>(&mut self, path: &KeyPath, state: &W::State) {
        // A state encodes as any plain value does: only a count past four billion fails, which no widget's state holds.
        if let Ok(bytes) = to_bytes(state) {
            self.write(path, KindId::of::<W::State>(), bytes);
        }
    }

    /// Which widget has the screen's focus.
    #[must_use]
    pub const fn focus(&self) -> &Focus {
        &self.focus
    }

    /// Gives the screen's focus to `focus`.
    pub(crate) fn focus_on(&mut self, focus: Focus) {
        self.focus = focus;
    }

    /// The state of the widget `W` kept under `path`: none when no state is kept there, or the one kept is another
    /// type's.
    #[must_use]
    pub fn state<W: Widget>(&self, path: &KeyPath) -> Option<W::State> {
        let kept = self.found(path)?;
        let ours = kept.kind.same(KindId::of::<W::State>());
        if !ours {
            return None;
        }

        from_bytes(&kept.state, &mut Budget::new(MOST_STATE_BYTES)).ok()
    }

    /// How many states it keeps.
    #[must_use]
    pub fn len(&self) -> usize {
        self.kept.len()
    }

    /// Whether it keeps none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}
