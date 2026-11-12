//! A screen's runtime (ADR-0056: A screen is an actor): what the library keeps in the screen's actor, and what it does
//! in each of the screen's turns that is the library's.
//!
//! In every `Prepare` (ADR-0020) the screen's view builds its tree whole, which is cheap, and hands it to
//! [`Screen::prepare`], which compares it with the last:
//!
//! - **Identical, and nothing outside the tree changed since:** nothing more. No layout, no mail, and the renderer keeps
//!   the last list (ADR-0031).
//! - **Different:** the tree is laid out, each widget's draws are compared with the last by key, and only what changed
//!   is sent; the draw list is sent when it changed.
//!
//! Nothing marks what changed, so nothing can be forgotten. What changes outside the tree goes through the library, so
//! the library knows to lay out at the next `Prepare` though the tree is the same: a font's metrics coming, the
//! screen's size, frame, or UI scale changing ([`Screen::resize`]), and input changing a widget's state or the screen's
//! focus ([`Screen::pointed`], [`Screen::keyed`]).
//!
//! What a layout lists, in tree order, is each widget's draws, then a hit over its box when the pointer reaches it,
//! after the keys the screen declares; and the library keeps what each entry of the list it sent is, so input through
//! an entry reaches the widget that listed it (ADR-0056: Widget state is kept by key; Focus and keys). A scroll slides
//! what is within it by the offset its state keeps, clamped to what it can scroll as it is laid out, and cuts it to its
//! content box; a list in a scroll builds only the items its window shows, as [`Screen::rows`] says. The content of each
//! open popup is listed after everything else, in its own layer of the screen's top layer, in tree order: placed against
//! its anchor and cut by nothing around it, behind a hit that blocks the rest of the screen when it is modal.
//!
//! Each turn returns [`Orders`], the mail the library sends, which the screen's handler sends through its turn
//! ([`Orders::send`]); a turn of input returns [`Heard`], which the screen's handler takes in as
//! [`take_in`](crate::take_in) says. The handlers themselves are written by [`screen!`](crate::screen!).

mod input;
mod keys;
mod layers;
mod orders;
mod popups;
mod program;
mod scrolls;
#[cfg(test)]
mod tests;

pub use input::{Heard, ImeAsked};
pub use layers::{DrawKey, Instancing, Placing, TextPlacing, Texting};
pub use orders::{Orders, Sends};
pub use popups::Tipping;
pub use program::{Meshing, MetricsAsked, Programming, Shaping, Uniforming, Whitening};
pub use scrolls::Rows;

use crate::draw::{Content, layered};
use crate::fonts::Fonts;
use crate::fonts::Measuring;
use crate::layout::{Boxed, Laidout, Pitch, lay_out};
use crate::style::Style;
use crate::table::{Focus, Table};
use crate::tree::{Flat, KeyPath, Tree};
use crate::unkept::Unkept;
use crate::widget::{Arrange, Axis, Built, Draws, Focused, Laid, Quad, Sample, anchor};
use aethereal_actor::{Allows, Asks, Beginning, Fault, Handles, Level, On, Path, Prepare, path};
use aethereal_data::{Bound, Plain};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::Renderer;
use aethereal_render::draw_list::{Entry, Pointer};
use aethereal_render::font::Metrics;
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::pass::{Pass, PassName};
use aethereal_render::text::Font;
use aethereal_window::Cursor;
use keys::{Declared, Ime, Modifiers, Role};
use layers::Layer;
use popups::{Hover, Layers, Pending, Popper, Stands, Touch, lift, past_anchor, popper, spans};
use program::Program;

/// Where a screen draws: the pass of the frame each of its draws joins, a window's interface pass for a screen over
/// what the program drew.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub struct Target {
    pub frame: Frame,
    pub pass: PassName,
}

impl Target {
    /// The pass `P` of `frame`.
    #[must_use]
    pub const fn of<P: Pass>(frame: Frame) -> Self {
        Self { frame, pass: PassName::of::<P>() }
    }
}

/// How big a screen is and what it draws into: its own size, which its root lays out to, in pixels; the size of the
/// frame it draws in, in pixels; and its UI scale, the pixels of one logical pixel.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Viewport {
    pub size: Vec2,
    pub frame_width: u32,
    pub frame_height: u32,
    pub scale: f32,
}

impl Viewport {
    /// A screen that fills a window `width` by `height` pixels at a UI scale of `scale`.
    #[must_use]
    pub fn window(width: u32, height: u32, scale: f32) -> Self {
        // A window's sides are far fewer pixels than an f32 counts exactly.
        #[allow(clippy::cast_precision_loss)]
        let size = Vec2::new(width as f32, height as f32);

        Self { size, frame_width: width, frame_height: height, scale }
    }

    /// Its own box, in its own pixels.
    fn rect(self) -> Rect2 {
        Rect2 { min: Vec2::ZERO, max: self.size }
    }
}

/// What the library keeps of a screen: where it draws, how big it is, the program it draws quads with, its widget
/// table, what it has drawn by key and what it lists in what order, the list it last sent and what each entry of it
/// is, whether what it last laid out is current, the modifiers held, what it last asked of its window's input method,
/// and what its tooltips wait on: the hits the pointer reaches, the number of the last hover, and whether the pointer
/// moved since its tooltips were looked at.
#[derive(Debug, Bound)]
pub struct Screen {
    target: Target,
    viewport: Viewport,
    program: Program,
    table: Table,
    /// What each draw is, and what the renderer made of it, by key.
    layers: Vec<Layer>,
    /// What the last layout lists of its widgets, in tree order.
    order: Vec<Slot>,
    /// The keys the last layout declares, listed before the widgets.
    keys: Vec<Declared>,
    /// The entries of the list last sent.
    listed: Vec<Entry>,
    /// What each entry of the list last sent is, which input through it reaches.
    listing: Vec<Listed>,
    /// The number of the next draw made: an answer for an older one of the same key is let go.
    serial: u64,
    fresh: Fresh,
    modifiers: Modifiers,
    ime: Ime,
    /// The paths of the widgets whose hits the pointer reaches, as the window said it came onto and left each.
    reached: Vec<KeyPath>,
    /// The number of the last hover of a tooltip's anchor: a tooltip's sleep for any other opens nothing.
    hovers: u64,
    hover: Hover,
    unkept: Unkept<Worked>,
}

/// One place in what a screen lists of its widgets, in tree order.
#[derive(Debug, Clone, PartialEq, Bound)]
enum Slot {
    /// A layer of a widget's draws.
    Draw(DrawKey),
    /// A hit over the box of the widget at `path`, `rect` in the screen's pixels, reached as `pointer` says.
    Hit { path: KeyPath, rect: Rect2, pointer: Pointer },
    /// A hit over `rect`, the whole screen, behind the modal popup at `path`, blocking the pointer from the rest.
    Backdrop { path: KeyPath, rect: Rect2 },
}

/// What an entry of the list a screen sent is.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
enum Listed {
    /// Keys the screen declares, as `Role` says.
    Keys(Role),
    /// A layer of a widget's draws, which the pointer does not reach.
    Draw(DrawKey),
    /// A hit over the widget at this path.
    Hit(KeyPath),
    /// The hit behind the modal popup at this path.
    Backdrop(KeyPath),
}

/// Whether the last tree's layout holds until the tree changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Fresh {
    /// It was laid out and drawn, or refused, as things stand.
    Current,
    /// It is waiting for something outside it, the program or a font's metrics, whose coming makes it stale.
    Waiting,
    /// Something outside it changed since: a font's metrics came, the screen was resized, or the program was made.
    Stale,
}

/// What a screen works out again rather than keep: the tree it last built, the fonts it knows, where its last layout
/// put each widget, by key path, in tree order, and the pitch of each scroll's lines, by key path.
#[derive(Debug, Default)]
struct Worked {
    last: Last,
    fonts: Fonts,
    placed: Vec<(KeyPath, Laid)>,
    pitches: Vec<(KeyPath, Pitch)>,
}

/// The tree a screen last built.
#[derive(Debug, Default)]
enum Last {
    /// None since the screen began, or since it moved.
    #[default]
    Unbuilt,
    Built(Box<Tree>),
}

/// What a `Prepare` did, beside the mail it sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The tree was the last one, and nothing outside it changed: nothing was laid out or drawn, and only a tooltip's
    /// sleep may be sent.
    Unchanged,
    /// The tree was laid out, and what changed of its draws was sent.
    Laid,
    /// The tree was not laid out yet: the program is still being made, or a font's metrics are asked for and the
    /// next `Prepare` after they come lays it out.
    Waiting,
    /// The tree was refused, and logged: it is too deep, or two siblings share a key. The last list stands.
    Refused,
}

/// What [`Screen::prepare`] did, and the mail it sends.
#[derive(Debug)]
pub struct Prepared {
    pub outcome: Outcome,
    pub orders: Orders,
}

impl Screen {
    /// A screen drawing into `target`, `viewport` big, that has drawn nothing: it subscribes to `Prepare` and asks for
    /// the shape of its pass, to make its program against, in its actor's `begin`.
    ///
    /// # Errors
    ///
    /// A fault when the ask was not written.
    pub fn begin<A, S, I>(target: Target, viewport: Viewport, turn: &mut Beginning<'_, A, S>) -> Result<Self, Fault>
    where
        A: Handles<On<Prepare>>,
        S: Allows<Asks<Path<Renderer>, ShapeOf, Shaping>, I>,
    {
        turn.subscribe::<Prepare>();
        let shape = ShapeOf { frame: target.frame, pass: target.pass };
        turn.to(path::<Renderer>(())).ask(&shape, &Shaping)?;

        Ok(Self::new(target, viewport))
    }

    /// A screen drawing into `target`, `viewport` big, whose program is being made.
    #[must_use]
    pub(crate) fn new(target: Target, viewport: Viewport) -> Self {
        Self {
            target,
            viewport,
            program: Program::Making,
            table: Table::default(),
            layers: Vec::new(),
            order: Vec::new(),
            keys: Vec::new(),
            listed: Vec::new(),
            listing: Vec::new(),
            serial: 0,
            fresh: Fresh::Stale,
            modifiers: Modifiers::default(),
            ime: Ime::Stopped,
            reached: Vec::new(),
            hovers: 0,
            hover: Hover::Settled,
            unkept: Unkept::default(),
        }
    }

    /// Its widget table.
    #[must_use]
    pub const fn table(&self) -> &Table {
        &self.table
    }

    /// Its viewport, as last set.
    #[must_use]
    pub const fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// Sets how big the screen is and what it draws into: a change lays it out at the next `Prepare` though its tree
    /// is the same, and a frame of another size is written to the program's uniform.
    pub fn resize(&mut self, viewport: Viewport) -> Orders {
        let mut orders = Orders::default();
        if viewport == self.viewport {
            return orders;
        }
        let framed =
            (viewport.frame_width, viewport.frame_height) != (self.viewport.frame_width, self.viewport.frame_height);
        self.viewport = viewport;
        self.fresh = Fresh::Stale;
        if framed {
            self.program.write_frame(viewport, &mut orders);
        }

        orders
    }

    /// Prepares what the screen draws from `tree`, the tree its view built this `Prepare`: nothing when it is the
    /// last tree and nothing outside it changed; otherwise it is laid out and what changed of its draws is sent, with
    /// the keys it declares: `hotkeys`, the screen's own, by their places on the keyboard, and the focus keys. When the
    /// tree changed or the pointer moved onto or off a hit, its tooltips are looked at first: one newly hovered sleeps
    /// for its delay, and one left shuts.
    pub fn prepare(&mut self, tree: Tree, hotkeys: &[&str]) -> Prepared {
        let mut orders = Orders::default();
        let same = matches!(&self.unkept.last, Last::Built(last) if **last == tree);
        if !same {
            self.unkept.last = Last::Built(Box::new(tree));
        }
        let moved = self.hover == Hover::Moved;
        let looks = moved || !same;
        if looks {
            self.hover(&mut orders);
        }
        let stale = self.fresh == Fresh::Stale;
        let unchanged = same && !stale;
        if unchanged {
            return Prepared { outcome: Outcome::Unchanged, orders };
        }
        let outcome = self.lay_out(&mut orders, hotkeys);
        self.fresh = match outcome {
            Outcome::Laid | Outcome::Refused | Outcome::Unchanged => Fresh::Current,
            Outcome::Waiting => Fresh::Waiting,
        };

        Prepared { outcome, orders }
    }

    /// Lays out the last tree built, unless the program is still being made, and sends what changed of its draws,
    /// with a hit over each widget the pointer reaches and the keys it declares, `hotkeys` its own; the fonts it met and
    /// has not asked for are asked for, and a layout that met a font whose metrics have not come draws nothing. Focus
    /// on a widget that has gone, or is hidden in a shut popup, is cleared, and the window's input method stopped with
    /// it. Each scroll's offset is clamped to what it can scroll, what is within it slid by it and cut to its content
    /// box; an item whose key a sibling before it has is left out, and logged. A popup whose anchor has gone shuts; the
    /// content of each open one is listed in its own layer over the rest, placed against its anchor and cut by nothing
    /// around it, and a popup's own draws and hit are listed over its anchor's.
    fn lay_out(&mut self, orders: &mut Orders, hotkeys: &[&str]) -> Outcome {
        let unmade = self.program.made().is_none();
        if unmade {
            return Outcome::Waiting;
        }
        let Worked { last: Last::Built(tree), fonts, placed, pitches } = &mut *self.unkept else {
            return Outcome::Waiting;
        };
        let flattened = match tree.flat() {
            Ok(flattened) => flattened,
            Err(refused) => {
                orders.log(Level::Error, format!("the screen's tree was refused: {refused:?}"));
                return Outcome::Refused;
            }
        };
        for twice in &flattened.twice {
            orders.log(Level::Error, format!("an item's key is another's before it, so it is left out: {twice:?}"));
        }
        let flat = flattened.nodes;
        let surfaces = popups::stood(&flat, &mut self.table);
        let focus = self.table.focus().clone();
        let tops = popups::tops(&flat, &surfaces);
        let viewport = self.viewport;
        let mut measuring = fonts.measuring(viewport.scale);
        let Laidout { boxes, pitches: pitched } = lay_out(&flat, &tops, &mut measuring, viewport.size, viewport.scale);
        *pitches = pitched.into_iter().map(|(at, pitch)| (flat[at].path.clone(), pitch)).collect();

        let ends = spans(&flat);
        let mut drawn: Vec<(DrawKey, Content)> = Vec::new();
        let mut layers = Layers::default();
        let mut pending: Vec<Pending> = Vec::new();
        let mut covered = vec![false; flat.len()];
        let mut shifts = vec![Vec2::ZERO; flat.len()];
        let mut clips = vec![viewport.rect(); flat.len()];
        let mut laid_out = Vec::with_capacity(flat.len());
        for (at, (node, boxed)) in flat.iter().zip(&boxes).enumerate() {
            layers.settle(&mut pending, at);
            let surface = surfaces[at];
            let (laid, slide, cut) = stand(&mut self.table, node, boxed, shifts[at], clips[at], viewport.scale);
            for child in &node.children {
                shifts[*child] = shifts[at] + slide;
                clips[*child] = cut;
            }
            if surface.stands == Stands::Hidden {
                laid_out.push((node.path.clone(), laid));
                continue;
            }
            let opens = popper(node.node.widget()).filter(|popper| popper.open(&self.table, &node.path));
            if let Some(popper) = opens {
                for (content, shift) in lift(&mut layers, &flat, &boxes, at, popper, laid.rect, viewport.rect()) {
                    shifts[content] = shift;
                    clips[content] = viewport.rect();
                }
            }

            let focused = match &focus {
                Focus::On(path) if *path == node.path => Focused::Yes,
                Focus::On(_) | Focus::Nowhere => Focused::No,
            };
            let state = self.table.bytes(&node.path).unwrap_or_default();
            let live = surface.touch == Touch::Live;
            let listing = listed(node, &laid, state, focused, covered[at], live, &mut measuring);
            drawn.extend(listing.drawn);
            layers.list(surface.stands, listing.now);
            let deferred = !listing.later.is_empty();
            if deferred {
                let until = past_anchor(node, &flat, &ends, at);
                pending.push(Pending { until, stands: surface.stands, slots: listing.later });
            }
            let covers = covered[at] || listing.blocks;
            let first = anchor(&node.path);
            for child in &node.children {
                let held = flat[*child].path == first;
                let lifted = opens.is_some() && !held;
                covered[*child] = covers && !lifted;
            }
            laid_out.push((node.path.clone(), laid));
        }
        layers.settle(&mut pending, usize::MAX);
        *placed = laid_out;
        let open = popups::open(&flat, &surfaces, &self.table);
        self.keys = keys::declared(&flat, &focus, hotkeys, open);
        let typing = keys::typing(&flat, &focus);

        let waited = measuring.waited();
        for font in measuring.into_unmet() {
            fonts.asked(font);
            orders.ask_metrics(font);
        }
        for allow in self.typing(typing) {
            orders.allow_ime(allow);
        }
        if waited {
            return Outcome::Waiting;
        }
        self.draw(drawn, layers.order(), orders);

        Outcome::Laid
    }

    /// Keeps `font`'s metrics, its own and its fallbacks', as the renderer answered them: the next `Prepare` lays the
    /// screen out with them, though its tree is the same.
    pub(crate) fn metrics_read(&mut self, font: Font, own: Metrics, fallbacks: Vec<Metrics>) {
        self.unkept.fonts.read(font, own, fallbacks);
        self.fresh = Fresh::Stale;
    }

    /// Notes that `font`'s metrics will not come: its text measures nothing and draws nothing from the next
    /// `Prepare`.
    pub(crate) fn metrics_refused(&mut self, font: Font) {
        self.unkept.fonts.refused(font);
        self.fresh = Fresh::Stale;
    }

    /// The list the screen draws by, once nothing it draws is still being made: the keys it declares, then each draw
    /// the renderer made and each hit, in tree order, sent when it is not the list last sent.
    fn settle(&mut self, orders: &mut Orders) {
        let making = self.layers.iter().any(Layer::is_making);
        if making {
            return;
        }
        let Target { frame, pass } = self.target;
        let mut entries = Vec::new();
        let mut listing = Vec::new();
        for declared in &self.keys {
            entries.push(Entry::Keys { frame, keys: declared.keys.clone(), blocks: declared.blocks });
            listing.push(Listed::Keys(declared.role));
        }
        for slot in &self.order {
            match slot {
                Slot::Draw(key) => {
                    if let Some(entry) = self.layer(key).and_then(Layer::entry) {
                        entries.push(entry);
                        listing.push(Listed::Draw(key.clone()));
                    }
                }
                Slot::Hit { path, rect, pointer } => {
                    entries.push(Entry::Hit { frame, pass, rect: *rect, pointer: pointer.clone() });
                    listing.push(Listed::Hit(path.clone()));
                }
                Slot::Backdrop { path, rect } => {
                    entries.push(Entry::Hit { frame, pass, rect: *rect, pointer: Pointer::Blocks(Cursor::Default) });
                    listing.push(Listed::Backdrop(path.clone()));
                }
            }
        }
        if entries == self.listed {
            return;
        }
        orders.list(entries.clone());
        self.listed = entries;
        self.listing = listing;
        self.reached.retain(|path| self.listing.contains(&Listed::Hit(path.clone())));
    }

    /// The layer kept under `key`, if any.
    fn layer(&self, key: &DrawKey) -> Option<&Layer> {
        self.layers.binary_search_by(|layer| layer.key().cmp(key)).ok().map(|at| &self.layers[at])
    }
}

/// Where `flat`, laid out as `boxed` at `scale`, stands once the scrolls around it have slid it by `shift` and cut it to
/// `clip`; then how far it slides what is within it, and what it cuts it to: a scroll's offset, clamped in `table`, and
/// its content box.
fn stand(
    table: &mut Table,
    flat: &Flat<'_>,
    boxed: &Boxed,
    shift: Vec2,
    clip: Rect2,
    scale: f32,
) -> (Laid, Vec2, Rect2) {
    let moved = |rect: Rect2| Rect2 { min: rect.min - shift, max: rect.max - shift };
    let laid = Laid { rect: moved(boxed.rect), content: moved(boxed.content), clip, extent: boxed.extent, scale };
    let widget = flat.node.widget();
    let (slide, cut) = match widget.arrange() {
        Arrange::Scroll(axis) => (fit(table, &flat.path, widget, &laid, axis), clip.intersection(laid.content)),
        Arrange::Flex(_) | Arrange::Grid | Arrange::Stack | Arrange::Leaf | Arrange::Popup(_) => (Vec2::ZERO, clip),
    };

    (laid, slide, cut)
}

/// What a widget lists as it is laid out: its draws, by key, and their places and its hit's in the list, those listed
/// where it stands and those listed over its anchor, a popup's own; and whether it blocks the pointer from what is
/// within it.
struct Listing {
    drawn: Vec<(DrawKey, Content)>,
    now: Vec<Slot>,
    later: Vec<Slot>,
    blocks: bool,
}

/// What the widget of `node`, laid at `laid`, its state `state`, lists, focused or not as `focused` says, within a
/// widget that blocks the pointer or not, `covered`: the background and border its style fills beneath it, then its
/// own drawing, measured with `measuring`; and a hit over its box when the pointer reaches it there and its surface
/// is `live`. A popup's own drawing and hit are listed over its anchor, as it is the anchor's button.
fn listed(
    node: &Flat<'_>,
    laid: &Laid,
    state: &[u8],
    focused: Focused,
    covered: bool,
    live: bool,
    measuring: &mut Measuring<'_>,
) -> Listing {
    let widget = node.node.widget();
    let style = node.node.style();
    let over = popper(widget).is_some_and(Popper::over);
    let mut beneath = Vec::new();
    let backed = fills(style);
    if backed {
        Draws::new(&mut beneath, measuring, focused).quad(Quad {
            rect: laid.rect,
            clip: laid.clip,
            colour: style.background,
            border_width: style.border.width * laid.scale,
            border_colour: style.border.colour,
            radius: style.radius * laid.scale,
            sample: Sample::Solid,
        });
    }
    let mut above = Vec::new();
    let own = if over {
        &mut above
    } else {
        &mut beneath
    };
    widget.draw(state, style, laid, &mut Draws::new(own, measuring, focused));

    let mut drawn = Vec::new();
    let mut now = Vec::new();
    let mut later = Vec::new();
    let mut number = 0;
    for (pieces, into) in [(beneath, &mut now), (above, &mut later)] {
        for content in layered(pieces) {
            let key = DrawKey { path: node.path.clone(), layer: number };
            number += 1;
            drawn.push((key.clone(), content));
            into.push(Slot::Draw(key));
        }
    }
    let pointer = reach(widget, state, style, covered);
    let blocks = matches!(pointer, Pointer::Blocks(_));
    let reached = pointer != Pointer::Ignores;
    let rect = laid.rect.intersection(laid.clip);
    let shown = !rect.is_empty();
    let hits = reached && shown && live;
    if hits {
        let into = if over {
            &mut later
        } else {
            &mut now
        };
        into.push(Slot::Hit { path: node.path.clone(), rect, pointer });
    }

    Listing { drawn, now, later, blocks }
}

/// Clamps the offset of `widget`, a scroll along `axis` at `path` laid at `laid`, to what it can scroll, keeping the
/// clamped offset in `table`, and answers how far it slides what is within it.
fn fit(table: &mut Table, path: &KeyPath, widget: &dyn Built, laid: &Laid, axis: Axis) -> Vec2 {
    let mut state = table.bytes(path).map_or_else(|| widget.first_state(), <[u8]>::to_vec);
    let offset = widget.offset(&state);
    let fitted = offset.clamp(0.0, laid.travel(axis));
    let clamped = fitted.to_bits() != offset.to_bits();
    if clamped {
        widget.scroll_to(&mut state, fitted);
        table.write(path, widget.state_kind(), state);
    }

    axis.vector(fitted)
}

/// Whether `style` fills its box: a background or a border, drawn beneath its widget.
fn fills(style: &Style) -> bool {
    style.background.alpha > 0.0 || style.border.width > 0.0
}

/// How the pointer reaches `widget`, its state `state`, styled `style`: as it says; or, when it says nothing, blocked
/// with the default cursor where its style fills its box, so a panel is solid to what is beneath it, unless a widget
/// it is within blocks the pointer already, `covered`, as a button's label is part of the button.
fn reach(widget: &dyn Built, state: &[u8], style: &Style, covered: bool) -> Pointer {
    let pointer = widget.pointer(state);
    let solid = fills(style) && !covered;
    match pointer {
        Pointer::Ignores if solid => Pointer::Blocks(Cursor::Default),
        pointer => pointer,
    }
}
