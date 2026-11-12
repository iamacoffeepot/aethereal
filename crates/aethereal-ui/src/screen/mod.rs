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
//! the library knows to lay out at the next `Prepare` though the tree is the same: a font's metrics coming, and the
//! screen's size, frame, or UI scale changing ([`Screen::resize`]).
//!
//! Each turn returns [`Orders`], the mail the library sends, which the screen's handler sends through its turn
//! ([`Orders::send`]). The handlers themselves are written by [`screen!`](crate::screen!).

mod layers;
mod orders;
mod program;
#[cfg(test)]
mod tests;

pub use layers::{DrawKey, Instancing, Placing, TextPlacing, Texting};
pub use orders::{Orders, Sends};
pub use program::{Meshing, MetricsAsked, Programming, Shaping, Uniforming, Whitening};

use crate::draw::{Content, layered};
use crate::fonts::Fonts;
use crate::layout::lay_out;
use crate::table::Table;
use crate::tree::Tree;
use crate::unkept::Unkept;
use crate::widget::{Draws, Laid, Quad, Sample};
use aethereal_actor::{Allows, Asks, Beginning, Fault, Handles, Level, On, Path, Prepare, path};
use aethereal_data::{Bound, Plain};
use aethereal_math::{Rect2, Vec2};
use aethereal_render::Renderer;
use aethereal_render::draw_list::Entry;
use aethereal_render::font::Metrics;
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::pass::{Pass, PassName};
use aethereal_render::text::Font;
use layers::Layer;
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
/// table, what it has drawn by key and in what order, the list it last sent, and whether what it last laid out is
/// current.
#[derive(Debug, Bound)]
pub struct Screen {
    target: Target,
    viewport: Viewport,
    program: Program,
    table: Table,
    /// What each draw is, and what the renderer made of it, by key.
    layers: Vec<Layer>,
    /// The draws in tree order, as the last layout put them.
    order: Vec<DrawKey>,
    /// The entries of the list last sent.
    listed: Vec<Entry>,
    /// The number of the next draw made: an answer for an older one of the same key is let go.
    serial: u64,
    fresh: Fresh,
    unkept: Unkept<Worked>,
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

/// What a screen works out again rather than keep: the tree it last built, and the fonts it knows.
#[derive(Debug, Default)]
struct Worked {
    last: Last,
    fonts: Fonts,
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
    /// The tree was the last one, and nothing outside it changed: nothing was laid out or sent.
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
            listed: Vec::new(),
            serial: 0,
            fresh: Fresh::Stale,
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
    /// last tree and nothing outside it changed; otherwise it is laid out and what changed of its draws is sent.
    pub fn prepare(&mut self, tree: Tree) -> Prepared {
        let mut orders = Orders::default();
        let same = matches!(&self.unkept.last, Last::Built(last) if **last == tree);
        let stale = self.fresh == Fresh::Stale;
        let unchanged = same && !stale;
        if unchanged {
            return Prepared { outcome: Outcome::Unchanged, orders };
        }
        self.unkept.last = Last::Built(Box::new(tree));
        let outcome = self.lay_out(&mut orders);
        self.fresh = match outcome {
            Outcome::Laid | Outcome::Refused | Outcome::Unchanged => Fresh::Current,
            Outcome::Waiting => Fresh::Waiting,
        };

        Prepared { outcome, orders }
    }

    /// Lays out the last tree built, unless the program is still being made, and sends what changed of its draws;
    /// the fonts it met and has not asked for are asked for, and a layout that met a font whose metrics have not come
    /// draws nothing.
    fn lay_out(&mut self, orders: &mut Orders) -> Outcome {
        let unmade = self.program.made().is_none();
        if unmade {
            return Outcome::Waiting;
        }
        let Worked { last: Last::Built(tree), fonts } = &mut *self.unkept else {
            return Outcome::Waiting;
        };
        let flat = match tree.flat() {
            Ok(flat) => flat,
            Err(refused) => {
                orders.log(Level::Error, format!("the screen's tree was refused: {refused:?}"));
                return Outcome::Refused;
            }
        };
        self.table.refresh(&flat);
        let viewport = self.viewport;
        let mut measuring = fonts.measuring(viewport.scale);
        let boxes = lay_out(&flat, &mut measuring, viewport.size, viewport.scale);

        let mut drawn: Vec<(DrawKey, Content)> = Vec::new();
        for (flat, boxed) in flat.iter().zip(&boxes) {
            let laid = Laid { rect: boxed.rect, content: boxed.content, clip: viewport.rect(), scale: viewport.scale };
            let mut pieces = Vec::new();
            let mut into = Draws::new(&mut pieces, &mut measuring);
            let style = flat.node.style();
            let backed = style.background.alpha > 0.0 || style.border.width > 0.0;
            if backed {
                into.quad(Quad {
                    rect: laid.rect,
                    clip: laid.clip,
                    colour: style.background,
                    border_width: style.border.width * laid.scale,
                    border_colour: style.border.colour,
                    radius: style.radius * laid.scale,
                    sample: Sample::Solid,
                });
            }
            let state = self.table.bytes(&flat.path).unwrap_or_default();
            flat.node.widget().draw(state, style, &laid, &mut into);
            for (layer, content) in (0..).zip(layered(pieces)) {
                drawn.push((DrawKey { path: flat.path.clone(), layer }, content));
            }
        }

        let waited = measuring.waited();
        for font in measuring.into_unmet() {
            fonts.asked(font);
            orders.ask_metrics(font);
        }
        if waited {
            return Outcome::Waiting;
        }
        self.draw(drawn, orders);

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

    /// The list the screen draws by, once nothing it draws is still being made: each draw the renderer made, in tree
    /// order, sent when it is not the list last sent.
    fn settle(&mut self, orders: &mut Orders) {
        let making = self.layers.iter().any(Layer::is_making);
        if making {
            return;
        }
        let entries: Vec<Entry> =
            self.order.iter().filter_map(|key| self.layer(key)).filter_map(Layer::entry).collect();
        if entries == self.listed {
            return;
        }
        orders.list(entries.clone());
        self.listed = entries;
    }

    /// The layer kept under `key`, if any.
    fn layer(&self, key: &DrawKey) -> Option<&Layer> {
        self.layers.binary_search_by(|layer| layer.key().cmp(key)).ok().map(|at| &self.layers[at])
    }
}
