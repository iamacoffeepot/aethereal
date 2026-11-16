//! A screen's selection (ADR-0056: Copy and paste): the text its reader selects, across the rows of a scroll, kept in
//! its table as an anchor and a caret, each the key path of a text and a byte offset into it.
//!
//! ```text
//! press on a Selectable, or on a panel over a text of a scroll that selects every text
//!       ──> anchor and caret at the boundary under the pointer (a word, or a line, for a double or triple click)
//! drag  ──> caret at the text under the pointer, within the scope; past the scroll's view, the scroll slides on at
//!           each Prepare, and the caret follows
//! layout──> each text of the scope between them in tree order: the part it holds, drawn beneath its text
//! Ctrl+C──> the view built again with every row between them, its texts' parts joined by newlines ──> the clipboard
//! Ctrl+A──> from the start to the end of the scroll at or around the focus
//! ```
//!
//! A text's scope is the innermost scroll around it that selects it, a `Selectable` by any scroll and a `Text` or a
//! `Rich` by one that selects every text ([`Selects::Texts`]), or a `Selectable` standing in no scroll itself; a
//! selection runs within one scope. A list in a scroll that windows its items builds the rows its two ends stand in
//! wherever they stand, as it does its focused row, so tree order places them, and a copy builds every row between, so
//! the rows a window did not build are copied as the screen's view builds them.

#[cfg(test)]
mod tests;

use super::input::Heard;
use super::popups::spans;
use super::{Fresh, Last, Screen, Worked, parents};
use crate::fonts::Asker;
use crate::table::{Dragging, Focus, Made, Mark, Selection};
use crate::tree::{Flat, KeyPath, Shape, Tree};
use crate::widget::{Arrange, Built, Reads, Selected, Selects, contains};
use crate::widgets::text_input::edit::{Edit, boundary};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use core::iter;

/// What a screen's view builds of a list in a scroll: the rows that can show, or every row its selection runs through,
/// to copy it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Building {
    Shown,
    Selection,
}

/// Whether a turn of input asked for the screen's selection to be copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Copies {
    #[default]
    Nothing,
    Selection,
}

/// Where a node of a tree stands with a selection: the place in tree order of the widget a selection of its text runs
/// within, or not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scoped {
    Unread,
    Within(usize),
}

/// The scroll around a node, as its children see it: the innermost of any kind, and the innermost that selects every
/// text.
#[derive(Debug, Clone, Copy, Default)]
struct Around {
    any: Option<usize>,
    texts: Option<usize>,
}

/// Where each of `flat`, a tree's nodes in tree order, stands with a selection: a text that reads, within the innermost
/// scroll around it that selects it, or a `Selectable` in no scroll within itself; any other, unread. A popup's content
/// stands apart from the scrolls around the popup, as it shows in the screen's top layer, or not at all while it is
/// shut, so a tooltip's text is never selected with the rows beneath it.
fn scoped(flat: &[Flat<'_>]) -> Vec<Scoped> {
    let parents = parents(flat);
    let mut around = vec![Around::default(); flat.len()];
    let mut scoped = vec![Scoped::Unread; flat.len()];
    for (at, node) in flat.iter().enumerate() {
        let widget = node.node.widget();
        let parent = &flat[parents[at]];
        let popped = matches!(parent.node.widget().arrange(), Arrange::Popup(_));
        let content = popped && contains(parent.path, node.path);
        let rooted = at == 0;
        let outer = if rooted || content {
            Around::default()
        } else {
            around[parents[at]]
        };
        scoped[at] = match (widget.reads(), outer) {
            (Reads::Selectable, Around { any: Some(scroll), .. })
            | (Reads::Shown, Around { texts: Some(scroll), .. }) => Scoped::Within(scroll),
            (Reads::Selectable, Around { any: None, .. }) => Scoped::Within(at),
            (Reads::Shown, Around { texts: None, .. }) | (Reads::Nothing, _) => Scoped::Unread,
        };
        around[at] = match widget.arrange() {
            Arrange::Scroll(_) => {
                let texts = widget.selects() == Selects::Texts;
                let selecting = if texts {
                    Some(at)
                } else {
                    outer.texts
                };
                Around { any: Some(at), texts: selecting }
            }
            Arrange::Flex(_) | Arrange::Grid | Arrange::Stack | Arrange::Leaf | Arrange::Popup(_) => outer,
        };
    }

    scoped
}

/// Where `mark` stands among `flat`, in tree order, with its offset: the start of the scope at `scope`, before its
/// first text; past its last; or at its text. Nothing when its text is not among them.
fn position(mark: &Mark, flat: &[Flat<'_>], scope: usize, ends: &[usize]) -> Option<(usize, u32)> {
    match mark {
        Mark::Start => Some((scope, 0)),
        Mark::End => Some((ends[scope], 0)),
        Mark::At { path, offset } => flat.iter().position(|node| node.path == path).map(|at| (at, *offset)),
    }
}

/// The part of each text of `flat` that `made` holds, by its place in tree order: from its start's offset in the text it
/// starts in, through each text of its scope between, to its end's offset in the text it ends in; none when its scope
/// or an end is not among them, or it holds nothing.
fn spanned(flat: &[Flat<'_>], made: &Made) -> Vec<(usize, u32, u32)> {
    let Some(scope) = flat.iter().position(|node| *node.path == made.scope) else {
        return Vec::new();
    };
    let ends = spans(flat);
    let (Some(anchor), Some(caret)) =
        (position(&made.anchor, flat, scope, &ends), position(&made.caret, flat, scope, &ends))
    else {
        return Vec::new();
    };
    let (from, to) = (anchor.min(caret), anchor.max(caret));
    let last = to.0.min(flat.len().saturating_sub(1));
    let scoped = scoped(flat);
    let mut parts = Vec::new();
    for (at, scoped) in scoped.iter().enumerate().take(last + 1).skip(from.0) {
        if *scoped != Scoped::Within(scope) {
            continue;
        }
        let start = if at == from.0 {
            from.1
        } else {
            0
        };
        let end = if at == to.0 {
            to.1
        } else {
            u32::MAX
        };
        if start < end {
            parts.push((at, start, end));
        }
    }

    parts
}

/// What part of each of `flat`'s texts `selection` holds as it is drawn, by place in tree order, in the colour of its
/// scope's style.
pub(super) fn parts(flat: &[Flat<'_>], selection: &Selection) -> Vec<Selected> {
    let mut parts = vec![Selected::Nothing; flat.len()];
    let Selection::Made(made) = selection else {
        return parts;
    };
    let colour = flat.iter().find(|node| *node.path == made.scope).map(|scope| scope.node.style().colour);
    let Some(colour) = colour else {
        return parts;
    };
    for (at, start, end) in spanned(flat, made) {
        parts[at] = Selected::Part { start, end, colour };
    }

    parts
}

/// The text `made` holds of `flat`'s texts: each one's part, joined by newlines in tree order.
fn copied(flat: &[Flat<'_>], made: &Made) -> String {
    let at = |offset: u32| usize::try_from(offset).unwrap_or(usize::MAX);
    let texts: Vec<String> = spanned(flat, made)
        .into_iter()
        .map(|(place, start, end)| {
            let read = flat[place].node.widget().read();
            let (start, end) = (boundary(&read, at(start)), boundary(&read, at(end)));
            String::from(&read[start..end.max(start)])
        })
        .collect();

    texts.join("\n")
}

impl Screen {
    /// Whether its reader has a selection standing with some text in it, or is dragging one out: what a screen that
    /// moves its focus of its own accord leaves be, so its selection is not left behind.
    #[must_use]
    pub fn selects(&self) -> bool {
        self.table.selection().selects()
    }

    /// Takes in a press of the main button through the hit over the widget at `path`, at `at` in the screen's pixels,
    /// the `presses`th in a row, which reaches it as a panel when `panel` (a hit that blocks the pointer with the
    /// default cursor): on a `Selectable`, or as a panel over a text that reads within its scope, it puts the anchor and
    /// the caret at the boundary under the pointer, or around the word or the line there, and holds the drag; a press
    /// on a panel gives the focus to the scope when it can hold it, so its keys reach it. A press that reaches no text
    /// clears the selection, unless it is on its scope's bar, outside the scope's content box.
    pub(super) fn press_select(&mut self, path: &KeyPath, at: Vec2, (presses, panel): (u32, bool), heard: &mut Heard) {
        let found = self.pressed_text(path, at, panel);
        let offset = found.as_ref().and_then(|(text, _)| self.boundary_at(text, at));
        let (Some((text, scope)), Some(offset)) = (found, offset) else {
            let scoped = matches!(self.table.selection(), Selection::Made(made) if made.scope == *path);
            let inside = self.laid(path).is_some_and(|laid| laid.content.contains_point(at));
            let on_bar = scoped && !inside;
            if !on_bar {
                self.select(Selection::Nothing);
            }
            return;
        };
        let read = self.widget(&text).map(Built::read).unwrap_or_default();
        let edit = Edit::new(&read, usize::try_from(offset).unwrap_or(usize::MAX), 0);
        let (start, end) = match presses {
            0 | 1 => (edit.caret, edit.caret),
            2 => edit.word_around(edit.caret),
            _ => edit.line_around(edit.caret),
        };
        let mark = |offset: usize| Mark::At { path: text.clone(), offset: u32::try_from(offset).unwrap_or(u32::MAX) };
        let made = Made { scope: scope.clone(), anchor: mark(start), caret: mark(end), drag: Dragging::Held { at } };
        self.select(Selection::Made(made));
        let focusable = self.widget(&scope).is_some_and(Built::focusable);
        let on_panel = text != *path;
        let focuses = focusable && on_panel;
        if focuses {
            self.refocus(Focus::On(scope), heard);
        }
    }

    /// Takes the drag of the selection to `at`, in the screen's pixels: the caret goes to the boundary of the text of
    /// its scope under the pointer, held within the scope's view, while the press that makes it is held. Only a caret
    /// that moves lays the screen out at the next `Prepare`; the pointer's place is kept either way.
    pub(super) fn drag_select(&mut self, at: Vec2) {
        let Selection::Made(made) = self.table.selection() else {
            return;
        };
        let Dragging::Held { .. } = made.drag else {
            return;
        };
        let made = made.clone();
        let under = self.caret_at(&made.scope, at);
        let offset = under.as_ref().and_then(|(path, held)| self.boundary_at(path, *held));
        let caret = match (under, offset) {
            (Some((path, _)), Some(offset)) => Mark::At { path, offset },
            _ => made.caret.clone(),
        };
        let moved = caret != made.caret;
        self.table.select(Selection::Made(Made { caret, drag: Dragging::Held { at }, ..made }));
        if moved {
            self.fresh = Fresh::Stale;
        }
    }

    /// Lets go of the drag of the selection, as the press that makes it comes up.
    pub(super) fn let_select(&mut self) {
        let Selection::Made(made) = self.table.selection() else {
            return;
        };
        let made = Made { drag: Dragging::Let, ..made.clone() };
        self.select(Selection::Made(made));
    }

    /// Selects all that the scroll at or around the widget at `path` holds, or that widget whole when no scroll is
    /// around it.
    pub(super) fn select_all(&mut self, path: &KeyPath) {
        let scrolls =
            |at: &KeyPath| self.widget(at).is_some_and(|widget| matches!(widget.arrange(), Arrange::Scroll(_)));
        let scope = iter::once(path.clone()).chain(path.ancestors()).find(|at| scrolls(at));
        let scope = scope.unwrap_or_else(|| path.clone());
        let made = Made { scope, anchor: Mark::Start, caret: Mark::End, drag: Dragging::Let };
        self.select(Selection::Made(made));
    }

    /// Slides the scroll a held drag of the selection runs within as far as the pointer is past its view's edge along
    /// its axis, so a drag past the view selects on through what it holds: what each `Prepare` does before it lays out.
    pub(super) fn follow_drag(&mut self) {
        let Selection::Made(Made { scope, drag: Dragging::Held { at }, .. }) = self.table.selection() else {
            return;
        };
        let (scope, at) = (scope.clone(), *at);
        let (Some(axis), Some(view)) = (self.scrolls(&scope), self.laid(&scope).map(|laid| laid.content)) else {
            return;
        };
        let before = axis.along(at) - axis.along(view.min);
        let after = axis.along(at) - axis.along(view.max);
        let past = before.min(0.0) + after.max(0.0);
        if past != 0.0 {
            self.slide(&scope, past);
        }
    }

    /// Takes the caret of a held drag of the selection to where the pointer was last, as the layout just made places
    /// what is under it: a scroll slid under a still pointer moves the caret on. A caret moved lays the screen out at the
    /// next `Prepare`.
    pub(super) fn redrag(&mut self) {
        let Selection::Made(Made { drag: Dragging::Held { at }, .. }) = self.table.selection() else {
            return;
        };
        let at = *at;
        self.drag_select(at);
    }

    /// Notes that the screen's view is about to be built to copy its selection: a list in a scroll its selection runs
    /// through builds every row between its ends.
    pub fn building_selection(&mut self) {
        self.building = Building::Selection;
    }

    /// Takes in `tree`, the view built to copy the screen's selection, and asks the clipboard, in `heard`, to hold the
    /// text the selection holds of it, each text's part joined by newlines; the view builds the rows that can show
    /// again from then on.
    pub fn copy_selection(&mut self, tree: &Tree, heard: &mut Heard) {
        self.building = Building::Shown;
        let Selection::Made(made) = self.table.selection() else {
            return;
        };
        let mut shape = Shape::default();
        let Ok(flattened) = tree.flat(&mut shape) else {
            return;
        };
        let text = copied(&flattened.nodes, made);
        heard.copy(text);
    }

    /// Keeps `selection` as what the reader selected: a change lays the screen out at the next `Prepare`.
    fn select(&mut self, selection: Selection) {
        if *self.table.selection() == selection {
            return;
        }
        self.table.select(selection);
        self.fresh = Fresh::Stale;
    }

    /// The text a press of the main button through the hit over the widget at `path`, at `at` in the screen's pixels,
    /// starts a selection in, with its scope: that widget, when it is a `Selectable`; or, when it reaches it as a panel,
    /// the topmost text within it under the pointer that reads within its scope. Nothing when the press starts none.
    fn pressed_text(&self, path: &KeyPath, at: Vec2, panel: bool) -> Option<(KeyPath, KeyPath)> {
        let Last::Built(tree) = &self.unkept.last else {
            return None;
        };
        let mut shape = Shape::default();
        let flattened = tree.flat(&mut shape).ok()?;
        let flat = flattened.nodes.as_slice();
        let pressed = flat.iter().position(|node| node.path == path)?;
        let scoped = scoped(flat);
        let ends = spans(flat);
        let selectable = flat[pressed].node.widget().reads() == Reads::Selectable;
        let within = if selectable {
            pressed..pressed + 1
        } else if panel {
            pressed + 1..ends[pressed]
        } else {
            return None;
        };
        let under = within.rev().find(|place| {
            let read = matches!(scoped[*place], Scoped::Within(_));
            let laid = self.laid(flat[*place].path);
            read && laid.is_some_and(|laid| laid.rect.contains_point(at))
        })?;
        let Scoped::Within(scope) = scoped[under] else {
            return None;
        };

        Some((flat[under].path.clone(), flat[scope].path.clone()))
    }

    /// The text a drag to `at`, in the screen's pixels, puts the caret of a selection within the scope at `scope` in,
    /// and the place in it the caret goes nearest: the pointer held within the scope's view, in the text of the scope
    /// under it; or, past them, held within the last text that starts above it, or the first.
    fn caret_at(&self, scope: &KeyPath, at: Vec2) -> Option<(KeyPath, Vec2)> {
        let Last::Built(tree) = &self.unkept.last else {
            return None;
        };
        let mut shape = Shape::default();
        let flattened = tree.flat(&mut shape).ok()?;
        let flat = flattened.nodes.as_slice();
        let place = flat.iter().position(|node| node.path == scope)?;
        let scoped = scoped(flat);
        let ends = spans(flat);
        let view = self.laid(scope).map_or(Rect2 { min: at, max: at }, |laid| laid.content);
        let held = clamped(at, view);
        let texts: Vec<(&KeyPath, Rect2)> = (place..ends[place])
            .filter(|at| scoped[*at] == Scoped::Within(place))
            .filter_map(|at| self.laid(flat[at].path).map(|laid| (flat[at].path, laid.rect)))
            .collect();
        let under = texts.iter().find(|(_, rect)| rect.contains_point(held));
        let above = texts.iter().rfind(|(_, rect)| rect.min.y <= held.y);
        let (path, rect) = under.or(above).or_else(|| texts.first())?;

        Some(((*path).clone(), clamped(held, *rect)))
    }

    /// The boundary of the text of the widget at `path` nearest `at`, in the screen's pixels, as its last layout drew it.
    fn boundary_at(&mut self, path: &KeyPath, at: Vec2) -> Option<u32> {
        let laid = self.laid(path)?;
        let Worked { last: Last::Built(tree), fonts, texts, .. } = &mut *self.unkept else {
            return None;
        };
        let node = tree.find(path)?;
        let mut measuring = fonts.measuring(texts, self.viewport.scale);
        measuring.at(path, Asker::Drawing);

        Some(node.widget().boundary(at, node.style(), &laid, &mut measuring))
    }
}

/// `at`, held within `rect`.
fn clamped(at: Vec2, rect: Rect2) -> Vec2 {
    Vec2::new(at.x.clamp(rect.min.x, rect.max.x.max(rect.min.x)), at.y.clamp(rect.min.y, rect.max.y.max(rect.min.y)))
}
