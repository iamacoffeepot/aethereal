//! A screen's draws, kept by key (ADR-0056: Draws are kept by widget key): each layer of each widget's draws, what it
//! should be, and what the renderer made of it, from the asks under way to its placement. A layout compares what it
//! drew with what is kept: a layer that is gone is released, a new one is made, a changed one is written or moved, and
//! one changed past what a write can change is made anew. An answer for a layer that has since gone, or been made
//! anew, is let go, and what it made released.

use super::orders::Orders;
use super::program::Made;
use super::{Screen, Sent, Slot, Target};
use crate::draw::{Binding, Content, Placing as Placed};
use crate::tree::KeyPath;
use aethereal_actor::{Answered, Level};
use aethereal_data::Plain;
use aethereal_render::Release;
use aethereal_render::draw_list::{Entry, Pointer};
use aethereal_render::instances::{Instanced, InstancesId};
use aethereal_render::placement::{
    Bind, Drawn, Filter, Join, Place, Placed as PlaceAnswered, PlacementId, Resource, Sampling, Wrap,
};
use aethereal_render::text::{
    Clip, CreateText, Layer as TextLayer, MoveText, PlaceText, Run, TextId, TextJoin, TextMade, TextPlaced, WriteText,
};
use core::fmt::Debug;
use core::mem;

/// Where a draw is kept: its widget's key path, and its place among the widget's layers.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Plain)]
pub struct DrawKey {
    pub path: KeyPath,
    pub layer: u32,
}

/// The instances of a layer of quads were asked for: the layer, and the number of the making.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.instancing")]
pub struct Instancing {
    key: DrawKey,
    serial: u64,
}

/// The placement of a layer of quads was asked for, over its instances.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.placing")]
pub struct Placing {
    key: DrawKey,
    serial: u64,
    instances: InstancesId,
}

/// A layer's text was asked for.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.texting")]
pub struct Texting {
    key: DrawKey,
    serial: u64,
}

/// A layer's text was made, and its placement asked for.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.ui.text_placing")]
pub struct TextPlacing {
    key: DrawKey,
    serial: u64,
}

/// One layer of a widget's draws: where it is kept, what it should be, and what the renderer made of it.
#[derive(Debug, Clone, PartialEq, Plain)]
pub(super) struct Layer {
    key: DrawKey,
    content: Content,
    stage: Stage,
}

/// What the renderer made of a layer, and what it holds of it: a layer of quads, its instances and its placement,
/// with the records written to them; a text, the text and its placement, with the runs written and where it was
/// placed.
#[derive(Debug, Clone, PartialEq, Plain)]
enum Stage {
    /// Its instances are asked for, holding `written`.
    Instancing { serial: u64, capacity: u32, written: Vec<u8> },
    /// Its placement is asked for.
    Placing { serial: u64, instances: InstancesId, capacity: u32, written: Vec<u8> },
    /// It is drawn by `placement`.
    Quads { instances: InstancesId, placement: PlacementId, capacity: u32, written: Vec<u8> },
    /// Its text is asked for, laid out of `written`.
    Texting { serial: u64, written: Vec<Run> },
    /// Its text is made, and its placement asked for at `placed`.
    TextPlacing { serial: u64, text: TextId, written: Vec<Run>, placed: Placed },
    /// It is drawn by `placement`.
    Text { text: TextId, placement: PlacementId, written: Vec<Run>, placed: Placed },
}

impl Layer {
    /// Where it is kept.
    pub(super) const fn key(&self) -> &DrawKey {
        &self.key
    }

    /// What it should be.
    #[cfg(test)]
    pub(super) const fn content(&self) -> &Content {
        &self.content
    }

    /// Whether something of it is still being made, so no list may name it yet.
    pub(super) const fn is_making(&self) -> bool {
        matches!(
            self.stage,
            Stage::Instancing { .. } | Stage::Placing { .. } | Stage::Texting { .. } | Stage::TextPlacing { .. }
        )
    }

    /// Its entry in the screen's list, once it is placed: drawn, and not there to the pointer.
    pub(super) const fn entry(&self) -> Option<Entry> {
        match self.stage {
            Stage::Quads { placement, .. } | Stage::Text { placement, .. } => {
                Some(Entry::Draw { placement, pointer: Pointer::Ignores })
            }
            Stage::Instancing { .. } | Stage::Placing { .. } | Stage::Texting { .. } | Stage::TextPlacing { .. } => {
                None
            }
        }
    }

    /// Releases what the renderer made of it and holds: what an answer still to come makes is released as it comes.
    pub(super) fn release(&self, orders: &mut Orders) {
        match self.stage {
            Stage::Quads { instances, placement, .. } => {
                orders.release(Release::Placement(placement));
                orders.release(Release::Instances(instances));
            }
            Stage::TextPlacing { text, .. } | Stage::Text { text, .. } => orders.release(Release::Text(text)),
            Stage::Instancing { .. } | Stage::Placing { .. } | Stage::Texting { .. } => {}
        }
    }

    /// Whether `content` can be drawn by what is kept: a text of the same shape, or quads of the same binding that fit
    /// the instances.
    fn holds(&self, content: &Content) -> bool {
        match (&self.content, content) {
            (Content::Quads { binding, .. }, Content::Quads { binding: wanted, records }) => {
                let capacity = match self.stage {
                    Stage::Instancing { capacity, .. }
                    | Stage::Placing { capacity, .. }
                    | Stage::Quads { capacity, .. } => capacity,
                    Stage::Texting { .. } | Stage::TextPlacing { .. } | Stage::Text { .. } => 0,
                };
                let fits = records_in(*wanted, records) <= capacity;
                binding == wanted && fits
            }
            (Content::Text { shape, .. }, Content::Text { shape: wanted, .. }) => shape == wanted,
            (Content::Quads { .. }, Content::Text { .. }) | (Content::Text { .. }, Content::Quads { .. }) => false,
        }
    }

    /// Writes what the renderer holds of it to what it should be, once it is placed: its records, or its text's runs
    /// and where it is placed.
    fn settle(&mut self, orders: &mut Orders) {
        match (&self.content, &mut self.stage) {
            (Content::Quads { binding, records }, Stage::Quads { instances, written, .. }) => {
                if records != written {
                    let shorter = records.len() < written.len();
                    orders.write_instances(*instances, 0, records.clone());
                    if shorter {
                        orders.write_instances(*instances, records_in(*binding, records), Vec::new());
                    }
                    written.clone_from(records);
                }
            }
            (Content::Text { runs, placing, .. }, Stage::Text { text, placement, written, placed }) => {
                if runs != written {
                    orders.write_text(WriteText { text: *text, runs: runs.clone() });
                    written.clone_from(runs);
                }
                if placing != placed {
                    orders.move_text(moved(*placement, *placing));
                    *placed = *placing;
                }
            }
            _ => {}
        }
    }
}

/// How many records `records`, of `binding`'s, holds.
fn records_in(binding: Binding, records: &[u8]) -> u32 {
    // A record is at most 128 bytes, and a widget draws at most thousands.
    u32::try_from(records.len() / binding.record_bytes() as usize).unwrap_or(u32::MAX)
}

/// Room for `records` records, and for some more besides, so a layer whose records grow a little is written, not made
/// anew.
fn capacity_for(records: u32) -> u32 {
    records.max(1).next_power_of_two()
}

/// The move that places `placement` as `placing` says, over what was drawn, opaque.
const fn moved(placement: PlacementId, placing: Placed) -> MoveText {
    MoveText { placement, at: placing.at, layer: TextLayer::Over, opacity: 1.0, clip: Clip::To(placing.clip) }
}

impl Screen {
    /// Draws `drawn`, each layer of each widget drawn anew in tree order, beside its key, listed as `order` says: each
    /// kept layer `order` no longer lists is retired, each new one made, each changed one written or made anew, the one
    /// it replaces retired, and each a widget listed again as it was stands as it is; and the list is sent once nothing in it is still being made, and only then is what was retired
    /// released, so the list the renderer draws by until then never names what it has let go.
    pub(super) fn draw(&mut self, drawn: Vec<(DrawKey, Content)>, order: Vec<Slot>, orders: &mut Orders) {
        let reordered = order != self.order;
        if reordered {
            self.order = order;
            self.sent = Sent::Stale;
            let mut wanted: Vec<&DrawKey> = self.order.iter().filter_map(Slot::draws).collect();
            wanted.sort_unstable();
            let mut wanted = wanted.into_iter().peekable();
            let (kept, retired): (Vec<Layer>, Vec<Layer>) = self.layers.drain(..).partition(|layer| {
                while wanted.next_if(|key| **key < layer.key).is_some() {}
                wanted.next_if(|key| **key == layer.key).is_some()
            });
            self.layers = kept;
            self.retired.extend(retired);
        }
        for (key, content) in drawn {
            match self.layers.binary_search_by(|layer| layer.key.cmp(&key)) {
                Ok(at) if self.layers[at].holds(&content) => {
                    let layer = &mut self.layers[at];
                    layer.content = content;
                    layer.settle(orders);
                }
                Ok(at) => {
                    let layer = self.make(key, content, orders);
                    let replaced = mem::replace(&mut self.layers[at], layer);
                    self.retired.push(replaced);
                    self.sent = Sent::Stale;
                }
                Err(at) => {
                    let layer = self.make(key, content, orders);
                    self.layers.insert(at, layer);
                    self.sent = Sent::Stale;
                }
            }
        }
        self.settle(orders);
    }

    /// Asks for what draws `content`, kept under `key`.
    fn make(&mut self, key: DrawKey, content: Content, orders: &mut Orders) -> Layer {
        let serial = self.serial;
        self.serial += 1;
        let stage = match &content {
            Content::Quads { binding, records } => {
                let capacity = capacity_for(records_in(*binding, records));
                let asked = Instancing { key: key.clone(), serial };
                orders.ask_instances(asked, binding.layout(), capacity, records.clone());
                Stage::Instancing { serial, capacity, written: records.clone() }
            }
            Content::Text { shape, runs, .. } => {
                let text = CreateText {
                    font: shape.font,
                    size: shape.size,
                    runs: runs.clone(),
                    wrap: shape.wrap,
                    align: shape.align,
                    outline: shape.outline,
                };
                orders.ask_text(text, Texting { key: key.clone(), serial });
                Stage::Texting { serial, written: runs.clone() }
            }
        };

        Layer { key, content, stage }
    }

    /// The layer kept under `key`, made by the making numbered `serial`, to write.
    fn making(&mut self, key: &DrawKey, serial: u64) -> Option<&mut Layer> {
        let at = self.layers.binary_search_by(|layer| layer.key.cmp(key)).ok()?;
        let layer = &mut self.layers[at];
        let ours = match layer.stage {
            Stage::Instancing { serial: making, .. }
            | Stage::Placing { serial: making, .. }
            | Stage::Texting { serial: making, .. }
            | Stage::TextPlacing { serial: making, .. } => making == serial,
            Stage::Quads { .. } | Stage::Text { .. } => false,
        };

        ours.then_some(layer)
    }

    /// Drops the layer kept under `key`, which was not made, and logs `why`: what its widget listed is let go, so the
    /// next layout draws it anew and asks for it again.
    fn unmade(&mut self, key: &DrawKey, why: &dyn Debug, orders: &mut Orders) {
        if let Ok(at) = self.layers.binary_search_by(|layer| layer.key.cmp(key)) {
            self.layers.remove(at);
        }
        self.sent = Sent::Stale;
        if let Some(at) = self.unkept.shape.position(&key.path) {
            self.unkept.listings.forget(at);
        }
        orders.log(Level::Error, format!("the screen's draw at {key:?} was not made: {why:?}"));
        self.settle(orders);
    }

    /// A layer's instances were made or refused: made, its placement is asked for; one that has gone is released.
    pub fn instanced(&mut self, answered: Answered<Instanced>, context: Instancing) -> Orders {
        let mut orders = Orders::default();
        let Instancing { key, serial } = context;
        let target = self.target;
        let made = self.program.made();
        let instances = match answered {
            Answered::Yes(Instanced::Made(instances)) => instances,
            refused => {
                if self.making(&key, serial).is_some() {
                    self.unmade(&key, &refused, &mut orders);
                }
                return orders;
            }
        };
        let (Some(layer), Some(made)) = (self.making(&key, serial), made) else {
            orders.release(Release::Instances(instances));
            return orders;
        };
        let Stage::Instancing { capacity, written, .. } = &layer.stage else {
            return orders;
        };
        let Content::Quads { binding, .. } = layer.content else {
            return orders;
        };
        let stage = Stage::Placing { serial, instances, capacity: *capacity, written: written.clone() };
        layer.stage = stage;
        let place =
            Place { mesh: made.mesh, instances: Drawn::Records(instances), joins: vec![joined(target, made, binding)] };
        orders.ask_place(place, Placing { key, serial, instances });

        orders
    }

    /// A layer's placement was made or refused: made, the layer is drawn, written to what it should be now, and the
    /// list sent once nothing is still being made; one that has gone is released, with its instances.
    pub fn placed(&mut self, answered: Answered<PlaceAnswered>, context: Placing) -> Orders {
        let mut orders = Orders::default();
        let Placing { key, serial, instances } = context;
        let placement = match answered {
            Answered::Yes(PlaceAnswered::Made(placement)) => placement,
            refused => {
                orders.release(Release::Instances(instances));
                if self.making(&key, serial).is_some() {
                    self.unmade(&key, &refused, &mut orders);
                }
                return orders;
            }
        };
        let Some(layer) = self.making(&key, serial) else {
            orders.release(Release::Placement(placement));
            orders.release(Release::Instances(instances));
            return orders;
        };
        let Stage::Placing { capacity, written, .. } = &layer.stage else {
            return orders;
        };
        layer.stage = Stage::Quads { instances, placement, capacity: *capacity, written: written.clone() };
        layer.settle(&mut orders);
        self.settle(&mut orders);

        orders
    }

    /// A layer's text was made or refused: made, its placement is asked for where it should be now; one that has gone
    /// is released.
    pub fn text_made(&mut self, answered: Answered<TextMade>, context: Texting) -> Orders {
        let mut orders = Orders::default();
        let Texting { key, serial } = context;
        let target = self.target;
        let text = match answered {
            Answered::Yes(TextMade::Made(text)) => text,
            refused => {
                if self.making(&key, serial).is_some() {
                    self.unmade(&key, &refused, &mut orders);
                }
                return orders;
            }
        };
        let Some(layer) = self.making(&key, serial) else {
            orders.release(Release::Text(text));
            return orders;
        };
        let (Content::Text { placing, .. }, Stage::Texting { written, .. }) = (&layer.content, &layer.stage) else {
            return orders;
        };
        let placed = *placing;
        layer.stage = Stage::TextPlacing { serial, text, written: written.clone(), placed };
        let place = PlaceText {
            text,
            at: placed.at,
            layer: TextLayer::Over,
            opacity: 1.0,
            clip: Clip::To(placed.clip),
            joins: vec![TextJoin { frame: target.frame, pass: target.pass }],
        };
        orders.ask_place_text(place, TextPlacing { key, serial });

        orders
    }

    /// A layer's text was placed or refused: placed, the layer is drawn, written and moved to what it should be now,
    /// and the list sent once nothing is still being made. A text that has gone was released with its placements.
    pub fn text_placed(&mut self, answered: Answered<TextPlaced>, context: TextPlacing) -> Orders {
        let mut orders = Orders::default();
        let TextPlacing { key, serial } = context;
        let Some(layer) = self.making(&key, serial) else {
            return orders;
        };
        let Stage::TextPlacing { text, written, placed, .. } = &layer.stage else {
            return orders;
        };
        let text = *text;
        let placement = match answered {
            Answered::Yes(TextPlaced::Made(placement)) => placement,
            refused => {
                orders.release(Release::Text(text));
                self.unmade(&key, &refused, &mut orders);
                return orders;
            }
        };
        layer.stage = Stage::Text { text, placement, written: written.clone(), placed: *placed };
        layer.settle(&mut orders);
        self.settle(&mut orders);

        orders
    }
}

/// The one pass a layer of quads joins: the screen's, drawn with the fill program, binding the frame's size; or with
/// the library's program, binding the frame's size, the texture its quads sample, and a sampler that blends texels
/// and reads the edge past it.
fn joined(target: Target, made: Made, binding: Binding) -> Join {
    let frame = Bind { group: 0, binding: 0, to: Resource::Uniforms(made.uniforms) };
    let sampling = Sampling { filter: Filter::Linear, wrap: Wrap::Clamp };
    let sampled = |texture| {
        let bindings = vec![
            frame,
            Bind { group: 0, binding: 1, to: Resource::Texture(texture) },
            Bind { group: 0, binding: 2, to: Resource::Sampler(sampling) },
        ];
        (made.program, bindings)
    };
    let (program, bindings) = match binding {
        Binding::Fill => (made.fills, vec![frame]),
        Binding::White => sampled(made.white),
        Binding::Texture(texture) => sampled(texture),
    };

    Join { frame: target.frame, pass: target.pass, program, bindings }
}
