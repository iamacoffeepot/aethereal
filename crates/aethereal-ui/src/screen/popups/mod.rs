//! A screen's popups (ADR-0056: Popups draw in one top layer per screen): where each widget stands as it draws, beneath
//! the top layer or in an open popup's layer of it, or nowhere, inside a shut one; what opens and shuts a popup beside
//! its own press; and a tooltip's wait.
//!
//! ```text
//! tree + table ──> surfaces: each node Base, Top { popup }, or Hidden, and Live or Inert ──> layout, draws, hits, Tab
//! press at a hit or behind a modal ──> every open popup it is outside shuts; every tooltip hovered is dismissed
//! click within content that raises an event ──> a popup that closes on a choice shuts
//! Escape ──> the topmost open popup shuts, focus within it back on it
//! Entered / Left ──> reached ──> Prepare: a tooltip newly hovered waits, a Sleep numbered by its hover ──> Slept ──> open
//! ```
//!
//! A widget claims a layer by arranging its children as a popup, `Arrange::Popup` with its `Layer`, and the library opens
//! and shuts it as its screen's input says through `Widget`'s methods alone, as it slides a scroll for the wheel: one
//! opened by its own input, a `Popup`, is read with `open` and shut with `shut`, and one opened by the pointer resting,
//! a `Tooltip`, keeps where it stands with the pointer through `tip` and `tip_to`. So a program's own primitive claims a
//! layer as a stock one does.

mod layer;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) use layer::place;
pub(super) use layer::{Layers, Pending, lift, past_anchor, spans};

use super::input::Heard;
use super::orders::Orders;
use super::{Fresh, Last, Screen, Worked};
use crate::input::Used;
use crate::table::{Focus, Table};
use crate::tree::{Flat, KeyPath, Shape};
use crate::widget::{Arrange, Built, Inside, Layer, Opens, Side, Tip, anchor, anchors, contains};
use aethereal_actor::{Answered, Level, Slept};
use aethereal_data::Plain;

/// What a tooltip's sleep is run for: the tooltip's path, and the number of the hover it waits on.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.tipping")]
pub struct Tipping {
    pub path: KeyPath,
    pub hover: u64,
}

/// Where a node of a screen's tree stands as it draws, and whether the pointer and Tab reach what it lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Surface {
    pub stands: Stands,
    pub touch: Touch,
}

/// Where a node stands as it draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stands {
    /// Inside a shut popup's content: laid out nowhere, and neither drawn nor reached.
    Hidden,
    /// Beneath its screen's top layer.
    Base,
    /// In the top layer, in the layer of the open popup at this place in the tree.
    Top { popup: usize },
}

/// Whether the pointer and Tab reach the widgets of a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Touch {
    Live,
    /// A tooltip's content: it lists no hit, and Tab passes it.
    Inert,
}

impl Surface {
    /// Whether a widget standing so is shown and reached: it can hold focus, and lists its hit.
    pub(super) fn reaches(self) -> bool {
        let shown = self.stands != Stands::Hidden;
        let live = self.touch == Touch::Live;

        shown && live
    }
}

/// Which popups of a screen are open, as its keys are declared for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Open {
    Nothing,
    /// At least one, none of them modal: Escape shuts the topmost.
    Popup,
    /// A modal one: Escape shuts the topmost, and Tab stays within the topmost modal.
    Modal,
}

/// A widget whose content shows in its screen's top layer, as it arranges its children: the widget, and its layer.
#[derive(Debug, Clone, Copy)]
pub(super) struct Popper<'a> {
    widget: &'a dyn Built,
    layer: Layer,
}

/// `widget` as one whose content shows in its screen's top layer, if it arranges its children as a popup.
pub(super) fn popper(widget: &dyn Built) -> Option<Popper<'_>> {
    match widget.arrange() {
        Arrange::Popup(layer) => Some(Popper { widget, layer }),
        Arrange::Flex(_) | Arrange::Grid | Arrange::Stack | Arrange::Scroll(_) | Arrange::Leaf => None,
    }
}

impl Popper<'_> {
    /// Whether the one at `path` is open, as `table` keeps its state.
    pub(super) fn open(self, table: &Table, path: &KeyPath) -> bool {
        let state = table.held(path, self.widget.state_kind()).unwrap_or_default();
        match self.layer.opens {
            Opens::Pressed { .. } => self.widget.open(state),
            Opens::Rested { .. } => self.widget.tip(state) == Tip::Open,
        }
    }

    /// Where the one at `path` stands with the pointer, as `table` keeps its state: for one opened by the pointer
    /// resting.
    fn tip(self, table: &Table, path: &KeyPath) -> Tip {
        let state = table.held(path, self.widget.state_kind()).unwrap_or_default();

        self.widget.tip(state)
    }

    /// Keeps `tip` as where the one at `path` stands with the pointer, in its state as `table` keeps it.
    fn tip_to(self, table: &mut Table, path: &KeyPath, tip: Tip) {
        self.change(table, path, |widget, state| widget.tip_to(state, tip));
    }

    /// Shuts the one at `path`, as `table` keeps its state, and answers whether it was open: one opened by the pointer
    /// resting goes back to waiting for nothing.
    fn shut(self, table: &mut Table, path: &KeyPath) -> bool {
        let open = self.open(table, path);
        match self.layer.opens {
            Opens::Pressed { .. } => self.change(table, path, |widget, state| widget.shut(state)),
            Opens::Rested { .. } => self.tip_to(table, path, Tip::Away),
        }

        open
    }

    /// Changes the state of the one at `path` with `change`, as `table` keeps it, or as it is first made when none of
    /// its kind is kept there, and keeps it in `table` when it changed.
    fn change(self, table: &mut Table, path: &KeyPath, change: impl FnOnce(&dyn Built, &mut Vec<u8>)) {
        let kind = self.widget.state_kind();
        let before = table.held(path, kind).map_or_else(|| self.widget.first_state(), <[u8]>::to_vec);
        let mut state = before.clone();
        change(self.widget, &mut state);
        if state != before {
            table.write(path, kind, state);
        }
    }

    /// Which side of its anchor its content is placed on.
    pub(super) const fn side(self) -> Side {
        self.layer.side
    }

    /// Whether it blocks the pointer from the rest of its screen while it is open.
    pub(super) const fn modal(self) -> bool {
        match self.layer.opens {
            Opens::Pressed { modal, .. } => modal,
            Opens::Rested { .. } => false,
        }
    }

    /// Whether its own drawing and hit are listed over its anchor's: one opened by its own input is its anchor's button.
    pub(super) const fn over(self) -> bool {
        match self.layer.opens {
            Opens::Pressed { .. } => true,
            Opens::Rested { .. } => false,
        }
    }

    /// How its content is reached, standing within what is reached as `around` says: the content of one opened by the
    /// pointer resting is inert, as the pointer leaving its anchor for it would shut it.
    const fn touch(self, around: Touch) -> Touch {
        match self.layer.opens {
            Opens::Pressed { .. } => around,
            Opens::Rested { .. } => Touch::Inert,
        }
    }
}

/// Where each of `flat`, a tree's nodes in tree order, stands, with each popup's state as `table` keeps it: a popup's
/// anchor where the popup does, and its content in its layer while it is open and shown, and hidden otherwise.
pub(super) fn surfaces(flat: &[Flat<'_>], table: &Table) -> Vec<Surface> {
    let mut surfaces = vec![Surface { stands: Stands::Base, touch: Touch::Live }; flat.len()];
    for (at, node) in flat.iter().enumerate() {
        let here = surfaces[at];
        let Some(popper) = popper(node.node.widget()) else {
            for child in node.children {
                surfaces[*child] = here;
            }
            continue;
        };
        let open = popper.open(table, node.path);
        let shown = here.stands != Stands::Hidden;
        let content = if open && shown {
            Surface { stands: Stands::Top { popup: at }, touch: popper.touch(here.touch) }
        } else {
            Surface { stands: Stands::Hidden, touch: here.touch }
        };
        let first = anchor(node.path);
        for child in node.children {
            let held = *flat[*child].path == first;
            surfaces[*child] = if held {
                here
            } else {
                content
            };
        }
    }

    surfaces
}

/// The places among `flat` of the roots of the content of each open popup, `surfaces` where each stands: each child of
/// a popup laid out in its layer.
pub(super) fn tops(flat: &[Flat<'_>], surfaces: &[Surface]) -> Vec<usize> {
    let mut tops = Vec::new();
    for (at, node) in flat.iter().enumerate() {
        for child in node.children {
            let rooted = surfaces[*child].stands == Stands::Top { popup: at };
            if rooted {
                tops.push(*child);
            }
        }
    }

    tops
}

/// Which of `flat`'s popups are open and shown, `surfaces` where each stands and `table` keeping their states.
pub(super) fn open(flat: &[Flat<'_>], surfaces: &[Surface], table: &Table) -> Open {
    let mut open = Open::Nothing;
    for (node, surface) in flat.iter().zip(surfaces) {
        let Some(popper) = popper(node.node.widget()) else {
            continue;
        };
        let Opens::Pressed { modal, .. } = popper.layer.opens else {
            continue;
        };
        let shown = surface.stands != Stands::Hidden;
        let opened = popper.open(table, node.path);
        let counts = shown && opened;
        if !counts {
            continue;
        }
        open = if modal {
            Open::Modal
        } else {
            open.with_popup()
        };
    }

    open
}

impl Open {
    /// It, with a popup that is not modal open beside what it says.
    const fn with_popup(self) -> Self {
        match self {
            Self::Nothing | Self::Popup => Self::Popup,
            Self::Modal => Self::Modal,
        }
    }
}

/// The place among `flat` of the topmost of the open popups opened by their own input that `wanted` takes by whether
/// each is modal, shown as `surfaces` say, and it: the last in tree order, whose layer draws over the others'.
fn topmost<'flat>(
    flat: &'flat [Flat<'_>],
    surfaces: &[Surface],
    table: &Table,
    wanted: impl Fn(bool) -> bool,
) -> Option<(usize, Popper<'flat>)> {
    let mut topmost = None;
    for (at, (node, surface)) in flat.iter().zip(surfaces).enumerate() {
        let Some(popper) = popper(node.node.widget()) else {
            continue;
        };
        let Opens::Pressed { modal, .. } = popper.layer.opens else {
            continue;
        };
        let shown = surface.stands != Stands::Hidden;
        let opened = popper.open(table, node.path);
        let takes = wanted(modal);
        let counts = shown && opened && takes;
        if counts {
            topmost = Some((at, popper));
        }
    }

    topmost
}

/// Which of `flat` Tab reaches, `surfaces` where each stands and `table` keeping the popups' states: each focusable
/// widget shown and live, and, while a modal popup is open, only those within the topmost's content.
pub(super) fn tabbable(flat: &[Flat<'_>], surfaces: &[Surface], table: &Table) -> Vec<bool> {
    let modal = topmost(flat, surfaces, table, |modal| modal).map(|(at, _)| &flat[at].path);
    flat.iter()
        .zip(surfaces)
        .map(|(node, surface)| {
            let focusable = node.node.widget().focusable();
            let ringed = modal.is_none_or(|modal| contains(modal, node.path));

            focusable && surface.reaches() && ringed
        })
        .collect()
}

/// The path of the popup among `flat` whose content `path` stands in, the innermost, if it stands in one: what the
/// widgets around it that hear keys end at, as a popup's content is a surface of its own.
pub(super) fn boundary<'tree>(flat: &[Flat<'tree>], path: &KeyPath) -> Option<&'tree KeyPath> {
    flat.iter()
        .filter(|node| popper(node.node.widget()).is_some())
        .map(|node| node.path)
        .rfind(|popup| contains(popup, path))
}

/// Keeps a state in `table` for each of `flat`, a tree's nodes in tree order, shuts each popup whose anchor has gone,
/// and clears focus held by a widget that is hidden or inert; and answers where each node stands.
pub(super) fn stood(flat: &[Flat<'_>], table: &mut Table) -> Vec<Surface> {
    table.refresh(flat);
    unanchored(flat, table);
    let surfaces = surfaces(flat, table);
    let reachable = match table.focus() {
        Focus::On(path) => flat.iter().zip(&surfaces).any(|(node, surface)| node.path == path && surface.reaches()),
        Focus::Nowhere => true,
    };
    if !reachable {
        table.focus_on(Focus::Nowhere);
    }

    surfaces
}

/// Shuts each popup among `flat` whose anchor has gone, as `table` keeps it, and sends each tooltip whose anchor has
/// gone away: what was open against it has nothing to stand against.
fn unanchored(flat: &[Flat<'_>], table: &mut Table) {
    for node in flat {
        let Some(popper) = popper(node.node.widget()) else {
            continue;
        };
        let first = anchor(node.path);
        let held = node.children.iter().any(|child| *flat[*child].path == first);
        if held {
            continue;
        }
        popper.shut(table, node.path);
    }
}

/// Whether the pointer reaches, as `reached` says, the tooltip at `tooltip`, or its anchor or a widget within it.
fn hovered(reached: &[KeyPath], tooltip: &KeyPath) -> bool {
    reached.iter().any(|hit| *hit == *tooltip || anchors(tooltip, hit))
}

/// Where a press landed, as the popups it is outside shut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Pressing {
    /// On the hit over the widget at this path.
    At(KeyPath),
    /// On the backdrop behind the modal popup at this path.
    Behind(KeyPath),
}

impl Pressing {
    /// Whether it landed within the popup at `popup`: on it or within it, and, behind a modal, within a popup the modal
    /// stands within.
    fn within(&self, popup: &KeyPath) -> bool {
        match self {
            Self::At(path) => *path == *popup || popup.holds(path),
            Self::Behind(modal) => popup.holds(modal),
        }
    }
}

/// Whether the pointer has come onto or left any hit of a screen since its last `Prepare` looked at its tooltips.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub(super) enum Hover {
    Settled,
    Moved,
}

impl Screen {
    /// Notes that the pointer came onto the hit over the widget at `path`, `entered`, or left it.
    pub(super) fn reach(&mut self, path: &KeyPath, entered: bool) {
        self.reached.retain(|reached| reached != path);
        if entered {
            self.reached.push(path.clone());
        }
        self.hover = Hover::Moved;
    }

    /// Looks at each tooltip of the last tree built as the pointer stands: one whose anchor the pointer came onto
    /// begins to wait, numbered by a hover of its own, with a sleep of its delay; one the pointer has left goes away,
    /// and shuts if it was open.
    pub(super) fn hover(&mut self, orders: &mut Orders) {
        self.hover = Hover::Settled;
        let Worked { last: Last::Built(tree), .. } = &*self.unkept else {
            return;
        };
        let mut shape = Shape::default();
        let Ok(flattened) = tree.flat(&mut shape) else {
            return;
        };
        for node in &flattened.nodes {
            let Some(popper) = popper(node.node.widget()) else {
                continue;
            };
            let Opens::Rested { delay } = popper.layer.opens else {
                continue;
            };
            let on = hovered(&self.reached, node.path);
            let tip = popper.tip(&self.table, node.path);
            let next = match (tip, on) {
                (Tip::Away, true) => {
                    self.hovers += 1;
                    orders.sleep(delay, Tipping { path: node.path.clone(), hover: self.hovers });
                    Tip::Waiting { hover: self.hovers }
                }
                (Tip::Waiting { .. } | Tip::Open | Tip::Dismissed, false) => Tip::Away,
                (Tip::Away, false) | (Tip::Waiting { .. } | Tip::Open | Tip::Dismissed, true) => continue,
            };
            if tip == Tip::Open {
                self.fresh = Fresh::Stale;
            }
            popper.tip_to(&mut self.table, node.path, next);
        }
    }

    /// A tooltip's sleep ended: the tooltip it was run for, in the last tree built, opens if it is still waiting for that
    /// hover. One for a hover that has ended, or that a later one followed, opens nothing; one that did not end in its
    /// time is logged.
    pub fn slept(&mut self, answered: Answered<Slept>, tipping: Tipping) -> Orders {
        let mut orders = Orders::default();
        let Slept = match answered {
            Answered::Yes(slept) => slept,
            Answered::No(unanswered) => {
                orders.log(Level::Debug, format!("a tooltip's wait did not end in its time: {unanswered:?}"));
                return orders;
            }
        };
        let Tipping { path, hover } = tipping;
        let Last::Built(tree) = &self.unkept.last else {
            return orders;
        };
        let Some(popper) = tree.find(&path).and_then(|node| popper(node.widget())) else {
            return orders;
        };
        let waiting = popper.tip(&self.table, &path) == Tip::Waiting { hover };
        if waiting {
            popper.tip_to(&mut self.table, &path, Tip::Open);
            self.fresh = Fresh::Stale;
        }

        orders
    }

    /// Takes in a press `pressing` of any button: each open popup it landed outside of shuts, and each tooltip the
    /// pointer is on is dismissed.
    pub(super) fn pressed_in(&mut self, pressing: &Pressing) {
        let Worked { last: Last::Built(tree), .. } = &*self.unkept else {
            return;
        };
        let mut shape = Shape::default();
        let Ok(flattened) = tree.flat(&mut shape) else {
            return;
        };
        for node in &flattened.nodes {
            let Some(popper) = popper(node.node.widget()) else {
                continue;
            };
            match popper.layer.opens {
                Opens::Pressed { .. } => {
                    let within = pressing.within(node.path);
                    if within {
                        continue;
                    }
                    let was_open = popper.shut(&mut self.table, node.path);
                    if was_open {
                        self.fresh = Fresh::Stale;
                    }
                }
                Opens::Rested { .. } => {
                    let on = hovered(&self.reached, node.path);
                    if !on {
                        continue;
                    }
                    let shown = popper.tip(&self.table, node.path) == Tip::Open;
                    if shown {
                        self.fresh = Fresh::Stale;
                    }
                    popper.tip_to(&mut self.table, node.path, Tip::Dismissed);
                }
            }
        }
    }

    /// Takes in a click on the widget at `path` that raised an event, a choice made, once the widget has: each open
    /// popup around it whose content holds it and that closes on a choice shuts. A click that raised nothing, on a
    /// popup's own anchor to open a popup within, or on a panel's empty space, shuts none.
    pub(super) fn chosen(&mut self, path: &KeyPath) {
        let Worked { last: Last::Built(tree), .. } = &*self.unkept else {
            return;
        };
        for around in path.ancestors() {
            let Some(popper) = tree.find(&around).and_then(|node| popper(node.widget())) else {
                continue;
            };
            let Opens::Pressed { inside, .. } = popper.layer.opens else {
                continue;
            };
            let closes = inside == Inside::Closes;
            let within = contains(&around, path);
            let shuts = closes && within;
            if !shuts {
                continue;
            }
            let was_open = popper.shut(&mut self.table, &around);
            if was_open {
                self.fresh = Fresh::Stale;
            }
        }
    }

    /// Takes in Escape as a popup's: the topmost open popup shuts, and focus within its content goes back to it. Passed
    /// when none is open.
    pub(super) fn shut_topmost(&mut self, heard: &mut Heard) -> Used {
        let Worked { last: Last::Built(tree), .. } = &*self.unkept else {
            return Used::Passed;
        };
        let mut shape = Shape::default();
        let Ok(flattened) = tree.flat(&mut shape) else {
            return Used::Passed;
        };
        let surfaces = surfaces(&flattened.nodes, &self.table);
        let Some((at, popper)) = topmost(&flattened.nodes, &surfaces, &self.table, |_| true) else {
            return Used::Passed;
        };
        let popup = flattened.nodes[at].path.clone();
        popper.shut(&mut self.table, &popup);
        self.fresh = Fresh::Stale;
        let within = match self.table.focus() {
            Focus::On(path) => contains(&popup, path),
            Focus::Nowhere => false,
        };
        if within {
            self.refocus(Focus::On(popup), heard);
        }

        Used::Used
    }

    /// The paths of the widgets the one at `path` stands within on its own surface, its parent's first: up to and not
    /// including the popup whose content it stands in, so the wheel, the keys around the focus, and Tab's bringing
    /// into view stay on the popup's content.
    pub(super) fn surfaced(&self, path: &KeyPath) -> Vec<KeyPath> {
        let Last::Built(tree) = &self.unkept.last else {
            return path.ancestors().collect();
        };
        let mut around = Vec::new();
        for at in path.ancestors() {
            let pops = tree.find(&at).is_some_and(|node| popper(node.widget()).is_some());
            let within = contains(&at, path);
            let bounded = pops && within;
            if bounded {
                break;
            }
            around.push(at);
        }

        around
    }
}
