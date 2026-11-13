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
//! its anchor and cut by nothing around it, behind a hit that blocks the rest of the screen when it is modal. A child
//! actor the screen places in a slot is called where the slot's content box is, at the slot's place in the list, and is
//! mailed its size whenever it changed; a hit that hears presses alone is listed over it, so a press into the child
//! clears the screen's focus and shuts its popups as a press elsewhere in the screen does. A widget that drags, or that
//! takes drops, is listed with a hit though it says nothing of the pointer, so a press reaches the one and a release is
//! over the other; once its drag begins, a source's hit declares the drag's cursor, and the target a drag of the
//! screen's own is over draws lit.
//!
//! Each turn returns [`Orders`], the mail the library sends, which the screen's handler sends through its turn
//! ([`Orders::send`]); a turn of input returns [`Heard`], which the screen's handler takes in as
//! [`take_in`](crate::take_in) says. The handlers themselves are written by [`screen!`](crate::screen!).

mod children;
mod drags;
mod input;
mod keys;
mod layers;
mod orders;
mod popups;
mod program;
mod scrolls;
#[cfg(test)]
mod tests;

pub use drags::Landing;
pub use input::{Heard, ImeAsked};
pub use layers::{DrawKey, Instancing, Placing, TextPlacing, Texting};
pub use orders::{Orders, Sends};
pub use popups::Tipping;
pub use program::{Meshing, MetricsAsked, Programming, Shaping, Uniforming, Whitening};
pub use scrolls::Rows;

use crate::child::{self, calling};
use crate::drag::DragSource;
use crate::draw::{Content, layered};
use crate::fonts::{Fonts, Measuring, Texts};
use crate::layout::{Boxed, Laidout, Layouts, Pitch};
use crate::style::Style;
use crate::table::{Focus, Table};
use crate::tree::{Difference, Flat, KeyPath, Node, Tree};
use crate::unkept::Unkept;
use crate::widget::{Arrange, Axis, Built, Draws, Focused, Laid, Lit, Quad, Sample, anchor};
use aethereal_actor::{Allows, Asks, Beginning, Fault, Handles, Level, On, Path, Prepare, path};
use aethereal_data::{Bound, Plain};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::Renderer;
use aethereal_render::draw_list::{Area, Callee, Entry, Pointer};
use aethereal_render::font::Metrics;
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::pass::{Pass, PassName};
use aethereal_render::text::Font;
use aethereal_window::Cursor;
use children::Told;
use drags::Grip;
use input::{Hold, Typed};
use keys::{Declared, Ime, Modifiers, Role, Taking};
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
/// table, what it has drawn by key, what it drew that the list it last sent may still name, and what it lists in what
/// order, whether it takes every key, the list it last sent and what each entry of it is, what each entry a press reached was when it reached it,
/// while a button is down on it, whether what it last laid out is current, the modifiers held, where the text the last
/// key typed goes, what it last asked of its window's input method, what its tooltips wait on: the hits the pointer
/// reaches, the number of the last hover, and whether the pointer moved since its tooltips were looked at; the size it
/// last told each child it places; and where it stands with a drag.
#[derive(Debug, Bound)]
pub struct Screen {
    target: Target,
    viewport: Viewport,
    program: Program,
    table: Table,
    /// What each draw is, and what the renderer made of it, by key.
    layers: Vec<Layer>,
    /// The draws no longer drawn, or made anew, that the list last sent may still name: released as the next list is
    /// sent.
    retired: Vec<Layer>,
    /// What the last layout lists of its widgets, in tree order.
    order: Vec<Slot>,
    /// The keys the last layout declares, listed before the widgets.
    keys: Vec<Declared>,
    /// Whether it takes every key whatever has its focus, as its `Controls` last said.
    taking: Taking,
    /// The entries of the list last sent.
    listed: Vec<Entry>,
    /// What each entry of the list last sent is, which input through it reaches.
    listing: Vec<Listed>,
    /// The entries of its list a press reached that the window's capture still sends input through, each as it was
    /// when the press reached it: none while no button is down on any.
    held: Vec<Hold>,
    /// The number of the next draw made: an answer for an older one of the same key is let go.
    serial: u64,
    fresh: Fresh,
    modifiers: Modifiers,
    /// Where the text the last key typed goes, as it follows its key.
    typed: Typed,
    ime: Ime,
    /// The paths of the widgets whose hits the pointer reaches, as the window said it came onto and left each.
    reached: Vec<KeyPath>,
    /// The number of the last hover of a tooltip's anchor: a tooltip's sleep for any other opens nothing.
    hovers: u64,
    hover: Hover,
    /// The size the last layout told each child it places, or found it was told before.
    told: Vec<Told>,
    grip: Grip,
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
    /// A call of `callee`, the child placed in the slot at `path`, in `area`.
    Call { path: KeyPath, callee: Callee, area: Area },
    /// A hit over `rect`, what the child placed in the slot at `path` is cut to, which hears presses into the child
    /// alone.
    Child { path: KeyPath, rect: Rect2 },
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
    /// A call of the child placed in the slot at this path, which the pointer reaches through the child's own list.
    Call(KeyPath),
    /// The hit over the child placed in the slot at this path, which a press into the child reaches, and nothing else.
    Child(KeyPath),
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

/// What a screen works out again rather than keep: the tree it last built, the fonts it knows and what it measured of
/// its texts, what its layouts keep for the next, where its last layout put each widget, by key path, in tree order,
/// and the pitch of each scroll's lines, by key path.
#[derive(Debug, Default)]
struct Worked {
    last: Last,
    fonts: Fonts,
    texts: Texts,
    layouts: Layouts,
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
            retired: Vec::new(),
            order: Vec::new(),
            keys: Vec::new(),
            taking: Taking::Declared,
            listed: Vec::new(),
            listing: Vec::new(),
            held: Vec::new(),
            serial: 0,
            fresh: Fresh::Stale,
            modifiers: Modifiers::default(),
            typed: Typed::Focus,
            ime: Ime::Stopped,
            reached: Vec::new(),
            hovers: 0,
            hover: Hover::Settled,
            told: Vec::new(),
            grip: Grip::Loose,
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
        let Worked { last, layouts, .. } = &mut *self.unkept;
        let difference = match last {
            Last::Built(last) => last.compare(&tree, layouts),
            Last::Unbuilt => Difference::Layout,
        };
        let same = difference == Difference::Same;
        if !same {
            *last = Last::Built(Box::new(tree));
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
    /// around it, and a popup's own draws and hit are listed over its anchor's. A child placed in a slot shown is called
    /// there, and told its size when it changed. The source of a drag under way lists its hit with the drag's cursor,
    /// and the target the drag is over draws lit.
    fn lay_out(&mut self, orders: &mut Orders, hotkeys: &[&str]) -> Outcome {
        let unmade = self.program.made().is_none();
        if unmade {
            return Outcome::Waiting;
        }
        let lit = self.lit();
        let Worked { last: Last::Built(tree), fonts, texts, layouts, placed, pitches } = &mut *self.unkept else {
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
        let mut measuring = fonts.measuring(texts, viewport.scale);
        let Laidout { boxes, pitches: pitched } =
            layouts.lay_out(&flat, &tops, &mut measuring, viewport.size, viewport.scale);
        *pitches = pitched.iter().map(|(at, pitch)| (flat[*at].path.clone(), *pitch)).collect();

        let ends = spans(&flat);
        let mut drawn: Vec<(DrawKey, Content)> = Vec::new();
        let mut layers = Layers::default();
        let mut pending: Vec<Pending> = Vec::new();
        let mut covered = vec![false; flat.len()];
        let mut shifts = vec![Vec2::ZERO; flat.len()];
        let mut clips = vec![viewport.rect(); flat.len()];
        let mut laid_out = Vec::with_capacity(flat.len());
        let mut told = Vec::new();
        for (at, (node, boxed)) in flat.iter().zip(boxes).enumerate() {
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
                for (content, shift) in lift(&mut layers, &flat, boxes, at, popper, laid.rect, viewport.rect()) {
                    shifts[content] = shift;
                    clips[content] = viewport.rect();
                }
            }

            let standing = Standing::of(node, &focus, lit.as_ref(), &self.grip, covered[at]);
            let state = self.table.bytes(&node.path).unwrap_or_default();
            let live = surface.touch == Touch::Live;
            if let Some(slot) = calling(node.node.widget()) {
                told.push(Told::of(slot, &laid, viewport));
            }
            let listing = listed(node, &laid, state, standing, live, &mut measuring);
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
        self.keys = keys::declared(&flat, &focus, hotkeys, open, self.taking);
        let typing = keys::typing(&flat, &focus);

        measuring.sweep();
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
        self.tell(told, orders);

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
    /// the renderer made and each hit, in tree order, sent when it is not the list last sent; and then what was retired
    /// is released, as the list the renderer draws by no longer names it.
    fn settle(&mut self, orders: &mut Orders) {
        let making = self.layers.iter().any(Layer::is_making);
        if making {
            return;
        }
        self.listed_now(orders);
        for retired in self.retired.drain(..) {
            retired.release(orders);
        }
    }

    /// Sends the list the screen draws by when it is not the list last sent: the keys it declares, then each draw the
    /// renderer made and each hit, in tree order.
    fn listed_now(&mut self, orders: &mut Orders) {
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
                Slot::Call { path, callee, area } => {
                    entries.push(Entry::Call { child: callee.clone(), area: *area });
                    listing.push(Listed::Call(path.clone()));
                }
                Slot::Child { path, rect } => {
                    entries.push(Entry::Hit { frame, pass, rect: *rect, pointer: Pointer::Presses });
                    listing.push(Listed::Child(path.clone()));
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

/// How a widget being listed stands with its screen's input: whether it has focus; whether a drag of the screen's that
/// it takes is over it; whether it is the source of the drag under way; and whether a widget it is within blocks the
/// pointer.
#[derive(Clone, Copy)]
struct Standing<'tree> {
    focused: Focused,
    lit: Lit,
    dragged: Dragged<'tree>,
    covered: bool,
}

impl<'tree> Standing<'tree> {
    /// How the widget of `node` stands, its screen's focus `focus`, the target a drag of its own is over `lit`, where
    /// it stands with a drag `grip`, and within a widget that blocks the pointer or not, `covered`.
    fn of(node: &Flat<'tree>, focus: &Focus, lit: Option<&KeyPath>, grip: &Grip, covered: bool) -> Self {
        let focused = match focus {
            Focus::On(path) if *path == node.path => Focused::Yes,
            Focus::On(_) | Focus::Nowhere => Focused::No,
        };
        let lit = match lit {
            Some(path) if *path == node.path => Lit::Yes,
            Some(_) | None => Lit::No,
        };
        let dragged = match grip {
            Grip::Dragging { path, .. } if *path == node.path => node.node.drag().map_or(Dragged::Not, Dragged::From),
            Grip::Dragging { .. } | Grip::Pressed { .. } | Grip::Loose => Dragged::Not,
        };

        Self { focused, lit, dragged, covered }
    }
}

/// Whether a widget being listed is the source of a drag under way.
#[derive(Clone, Copy)]
enum Dragged<'tree> {
    Not,
    /// It is, and drags as this source says: its hit declares the drag's cursor.
    From(&'tree DragSource),
}

/// What the widget of `node`, laid at `laid`, its state `state`, lists, standing with its screen's input as `standing`
/// says: the background and border its style fills beneath it, then its own drawing, measured with `measuring`; and a
/// hit over its box when the pointer reaches it there and its surface is `live`, then the call of the child it places,
/// when it is a child's slot, so the child draws over the slot's background and is reached before it, and over the
/// call, where the child shows, a hit that hears presses alone. A popup's own drawing and hit are listed over its
/// anchor, as it is the anchor's button.
fn listed<'flat>(
    node: &'flat Flat<'flat>,
    laid: &Laid,
    state: &[u8],
    standing: Standing<'_>,
    live: bool,
    measuring: &mut Measuring<'flat>,
) -> Listing {
    let Standing { focused, lit, .. } = standing;
    measuring.at(&node.path);
    let widget = node.node.widget();
    let style = node.node.style();
    let over = popper(widget).is_some_and(Popper::over);
    let mut beneath = Vec::new();
    let backed = fills(style);
    if backed {
        Draws::new(&mut beneath, measuring, focused, lit).quad(Quad {
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
    widget.draw(state, style, laid, &mut Draws::new(own, measuring, focused, lit));

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
    let pointer = reach(node.node, state, standing);
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
    if let Some(slot) = calling(widget) {
        now.push(Slot::Call { path: node.path.clone(), callee: slot.callee().clone(), area: slot.area(laid) });
        let cut = child::cut(laid);
        let visible = !cut.is_empty();
        if visible {
            now.push(Slot::Child { path: node.path.clone(), rect: cut });
        }
    }

    Listing { drawn, now, later, blocks }
}

/// Settles `widget`, a scroll along `axis` at `path` laid at `laid`, at what it can now scroll, and clamps its offset to
/// it, keeping what changed of its state in `table`, and answers how far it slides what is within it.
fn fit(table: &mut Table, path: &KeyPath, widget: &dyn Built, laid: &Laid, axis: Axis) -> Vec2 {
    let kept = table.bytes(path).map_or_else(|| widget.first_state(), <[u8]>::to_vec);
    let mut state = kept.clone();
    let travel = laid.travel(axis);
    widget.settle(&mut state, travel);
    let offset = widget.offset(&state);
    let fitted = offset.clamp(0.0, travel);
    let clamped = fitted.to_bits() != offset.to_bits();
    if clamped {
        widget.scroll_to(&mut state, fitted);
    }
    let changed = state != kept;
    if changed {
        table.write(path, widget.state_kind(), state);
    }

    axis.vector(fitted)
}

/// Whether `style` fills its box: a background or a border, drawn beneath its widget.
fn fills(style: &Style) -> bool {
    style.background.alpha > 0.0 || style.border.width > 0.0
}

/// How the pointer reaches the widget of `node`, its state `state`, standing as `standing` says: as it says; or, when it
/// says nothing, blocked with the open hand when it drags, so a press reaches it, blocked with the default cursor where
/// its style fills its box, so a panel is solid to what is beneath it, unless a widget it is within blocks the pointer
/// already, as a button's label is part of the button, and passed on with the default cursor when it takes drops, so a
/// release is over it. The source of a drag under way declares the drag's cursor in place of its own.
fn reach(node: &Node, state: &[u8], standing: Standing<'_>) -> Pointer {
    let pointer = node.widget().pointer(state);
    let drags = node.drag().is_some();
    let solid = fills(node.style()) && !standing.covered;
    let takes = node.drop_target().is_some();
    let reached = match pointer {
        Pointer::Ignores if drags => Pointer::Blocks(Cursor::Grab),
        Pointer::Ignores if solid => Pointer::Blocks(Cursor::Default),
        Pointer::Ignores if takes => Pointer::Passes(Cursor::Default),
        pointer => pointer,
    };
    let Dragged::From(source) = standing.dragged else {
        return reached;
    };
    match reached {
        Pointer::Blocks(_) => Pointer::Blocks(source.cursor().clone()),
        Pointer::Passes(_) => Pointer::Passes(source.cursor().clone()),
        Pointer::Presses | Pointer::Ignores => reached,
    }
}
