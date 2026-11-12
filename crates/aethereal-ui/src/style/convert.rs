//! A widget's style as Taffy lays it out: its lengths multiplied by the screen's UI scale, its container's mode set by
//! how the widget arranges its children, the room it keeps for its own drawing inside its border; under a `Stack`, its
//! cell the stack's one, and under a scroll, never shrunk to fit.

use super::{Alignment, Distribute, Edges, Length, Placed, SelfAlign, Space, Style, Track};
use crate::widget::{Arrange, Axis};
use taffy::{
    AlignContent, AlignItems, Dimension, Display, FlexDirection, FlexWrap, GridPlacement, GridTemplateComponent,
    LengthPercentage, LengthPercentageAuto, Line, MaxTrackSizingFunction, MinMax, MinTrackSizingFunction, Overflow,
    Point, Rect, Size, TrackSizingFunction,
};

/// What a node's parent is, as its style is laid out: a `Stack` sets every child in its one cell, and a scroll never
/// shrinks its children to fit its box, which they overflow and it slides through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parent {
    Stack,
    Scroll,
    Other,
}

impl Parent {
    /// What a widget that arranges its children as `arrange` is to them.
    #[must_use]
    pub const fn of(arrange: Arrange) -> Self {
        match arrange {
            Arrange::Stack => Self::Stack,
            Arrange::Scroll(_) => Self::Scroll,
            Arrange::Flex(_) | Arrange::Grid | Arrange::Leaf => Self::Other,
        }
    }
}

/// `style`, of a widget that arranges its children as `arrange` and keeps `room` pixels on each side for its own
/// drawing, under a parent of `parent`, as Taffy lays it out at `scale` pixels to the logical pixel. The room is laid
/// out as border, inside the style's own, so it is a length whatever the padding is.
pub fn flow(style: &Style, arrange: Arrange, parent: Parent, room: Edges<f32>, scale: f32) -> taffy::Style {
    let (display, flex_direction) = match arrange {
        Arrange::Flex(Axis::Row) | Arrange::Scroll(Axis::Row) | Arrange::Leaf => (Display::Flex, FlexDirection::Row),
        Arrange::Flex(Axis::Column) | Arrange::Scroll(Axis::Column) => (Display::Flex, FlexDirection::Column),
        Arrange::Grid | Arrange::Stack => (Display::Grid, FlexDirection::Row),
    };
    let overflow = match arrange {
        Arrange::Scroll(_) => Overflow::Hidden,
        Arrange::Flex(_) | Arrange::Grid | Arrange::Stack | Arrange::Leaf => Overflow::Visible,
    };
    let flex_shrink = match parent {
        Parent::Scroll => 0.0,
        Parent::Stack | Parent::Other => style.shrink,
    };
    let (grid_row, grid_column) = match parent {
        Parent::Stack => (first_cell(), first_cell()),
        Parent::Scroll | Parent::Other => (placed(style.row), placed(style.column)),
    };
    let wrap = if style.wrap {
        FlexWrap::Wrap
    } else {
        FlexWrap::NoWrap
    };
    let border = style.border.width * scale;
    let bordered = |room: f32| LengthPercentage::length(border + room);

    taffy::Style {
        display,
        flex_direction,
        flex_wrap: wrap,
        size: Size { width: dimension(style.width, scale), height: dimension(style.height, scale) },
        min_size: Size { width: length(style.min_width, scale), height: length(style.min_height, scale) },
        max_size: Size { width: length(style.max_width, scale), height: length(style.max_height, scale) },
        margin: edges(style.margin, |side| length(side, scale)),
        padding: edges(style.padding, |side| space(side, scale)),
        border: edges(room, bordered),
        overflow: Point { x: overflow, y: overflow },
        gap: Size { width: space(style.gap.columns, scale), height: space(style.gap.rows, scale) },
        flex_grow: style.grow,
        flex_shrink,
        flex_basis: dimension(style.basis, scale),
        justify_content: distribute(style.justify_content),
        align_content: distribute(style.align_content),
        align_items: alignment(style.align_items),
        justify_items: alignment(style.justify_items),
        align_self: self_alignment(style.align_self),
        justify_self: self_alignment(style.justify_self),
        grid_template_columns: style.columns.iter().map(|track| tracked(*track, scale)).collect(),
        grid_template_rows: style.rows.iter().map(|track| tracked(*track, scale)).collect(),
        grid_row,
        grid_column,
        ..taffy::Style::default()
    }
}

/// The first cell of a grid, one track across: where a `Stack` sets each child.
fn first_cell() -> Line<GridPlacement> {
    Line { start: GridPlacement::Line(1.into()), end: GridPlacement::Span(1) }
}

/// A child's place along one axis of its grid.
fn placed(placed: Placed) -> Line<GridPlacement> {
    match placed {
        Placed::Auto => Line { start: GridPlacement::Auto, end: GridPlacement::Auto },
        Placed::At { line, span } => Line { start: GridPlacement::Line(line.into()), end: GridPlacement::Span(span) },
        Placed::Span(span) => Line { start: GridPlacement::Auto, end: GridPlacement::Span(span) },
    }
}

/// `edges`, each side made Taffy's by `side`.
fn edges<T: Copy, U>(edges: Edges<T>, side: impl Fn(T) -> U) -> Rect<U> {
    Rect { left: side(edges.left), right: side(edges.right), top: side(edges.top), bottom: side(edges.bottom) }
}

/// A size, at `scale`.
fn dimension(length: Length, scale: f32) -> Dimension {
    match length {
        Length::Auto => Dimension::auto(),
        Length::Pixels(pixels) => Dimension::length(pixels * scale),
        Length::Share(share) => Dimension::percent(share),
    }
}

/// A margin or a bound on a size, at `scale`.
fn length(length: Length, scale: f32) -> LengthPercentageAuto {
    match length {
        Length::Auto => LengthPercentageAuto::auto(),
        Length::Pixels(pixels) => LengthPercentageAuto::length(pixels * scale),
        Length::Share(share) => LengthPercentageAuto::percent(share),
    }
}

/// A padding or a gap, at `scale`.
fn space(space: Space, scale: f32) -> LengthPercentage {
    match space {
        Space::Pixels(pixels) => LengthPercentage::length(pixels * scale),
        Space::Share(share) => LengthPercentage::percent(share),
    }
}

/// One track of a grid, at `scale`.
fn tracked(track: Track, scale: f32) -> GridTemplateComponent<String> {
    let (min, max) = match track {
        Track::Pixels(pixels) => {
            (MinTrackSizingFunction::length(pixels * scale), MaxTrackSizingFunction::length(pixels * scale))
        }
        Track::Share(share) => (MinTrackSizingFunction::percent(share), MaxTrackSizingFunction::percent(share)),
        Track::Fraction(parts) => (MinTrackSizingFunction::auto(), MaxTrackSizingFunction::fr(parts)),
        Track::Auto => (MinTrackSizingFunction::auto(), MaxTrackSizingFunction::auto()),
        Track::MinContent => (MinTrackSizingFunction::min_content(), MaxTrackSizingFunction::min_content()),
        Track::MaxContent => (MinTrackSizingFunction::max_content(), MaxTrackSizingFunction::max_content()),
    };
    let sized: TrackSizingFunction = MinMax { min, max };

    GridTemplateComponent::Single(sized)
}

/// How a container shares out its free space along an axis.
const fn distribute(distribute: Distribute) -> AlignContent {
    match distribute {
        Distribute::Normal => AlignContent::NORMAL,
        Distribute::Start => AlignContent::START,
        Distribute::End => AlignContent::END,
        Distribute::Center => AlignContent::CENTER,
        Distribute::Stretch => AlignContent::STRETCH,
        Distribute::SpaceBetween => AlignContent::SPACE_BETWEEN,
        Distribute::SpaceAround => AlignContent::SPACE_AROUND,
        Distribute::SpaceEvenly => AlignContent::SPACE_EVENLY,
    }
}

/// Where children are set along an axis.
const fn alignment(alignment: Alignment) -> AlignItems {
    match alignment {
        Alignment::Normal => AlignItems::NORMAL,
        Alignment::Start => AlignItems::START,
        Alignment::End => AlignItems::END,
        Alignment::Center => AlignItems::CENTER,
        Alignment::Baseline => AlignItems::BASELINE,
        Alignment::Stretch => AlignItems::STRETCH,
    }
}

/// Where a child sets itself along an axis: none leaves it to its container.
const fn self_alignment(alignment: SelfAlign) -> Option<AlignItems> {
    match alignment {
        SelfAlign::Auto => None,
        SelfAlign::Own(own) => Some(self::alignment(own)),
    }
}
