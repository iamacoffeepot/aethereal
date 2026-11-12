//! Text as the render root keeps and draws it (ADR-0052): the fonts, the texts laid out, the texts placed, and the
//! atlas they are drawn from. A text's glyphs not in the atlas are generated in a job off the turn and written in as
//! it finishes; at each `Render` a text whose glyphs are all in shows what it was last laid out as, and until then
//! its placements draw nothing if it is new, or what it was before if it was written.

// The tests draw on a device that draws nothing, which only a native build has.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;

use super::{Kept, Placement, Renderer};
use crate::draw_list::{self, EntryRefused, Listing, Lists, Placements};
use crate::font;
use crate::font::FontRefused;
use crate::frame::{DepthUse, Frame, Size};
use crate::made::{Made, Owned};
use crate::pass::PassName;
use crate::placement::{self, Finds, Fits, PlacementId};
use crate::shape::Shape;
use crate::stats::{self, AtlasFill, Tally};
use crate::text::atlas::texture::AtlasTexture;
use crate::text::atlas::{Atlas, Field, GlyphKey};
use crate::text::faces::{Face, Faces, FontKey};
use crate::text::generate::{GenerateGlyphs, Generating, GlyphOutline, GlyphsGenerated};
use crate::text::glyph_program::{self, INSTANCE_BYTES, Layouts, PLACED_BYTES, Pipelines, Placing};
use crate::text::layout::{self, Laid};
use crate::text::{
    self, Align, CreateFont, CreateText, FallbackFonts, FontId, FontMade, MoveText, Outline, PlaceText, Run, TextId,
    TextMade, TextPlaced, Wrap, WriteText,
};
use aethereal_actor::{Answer, Answered, Fault, Handles, Level, ToSender, Turn};
use aethereal_data::ActorId;
use core::ops::Range;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::Arc;

/// What the render root keeps of text: the fonts; the texts laid out, and the texts placed, each by id beside its
/// maker; the atlas and its texture; the glyph program's layouts, and its pipelines for each shape of pass a text
/// joined; the batches of glyphs whose jobs run, and the count they are numbered by; the glyphs that fitted nowhere in
/// the atlas, not asked for again until a text goes; each character no font had that was logged; and how many
/// `Render`s it has drawn, which says how lately each text was drawn.
pub(super) struct Texts {
    pub faces: Faces,
    pub made: Made<Text>,
    pub placed: Made<PlacedText>,
    atlas: Atlas,
    texture: AtlasTexture,
    layouts: Layouts,
    pipelines: HashMap<Shape, Pipelines>,
    batches: Vec<Batch>,
    batched: u64,
    unfit: HashSet<GlyphKey>,
    logged: HashSet<char>,
    renders: u64,
}

/// The GPU as a turn opens it: its device and its queue, held apart, as a turn opens one resource at a time.
pub(super) type Opened<'a> = (&'a wgpu::Device, &'a wgpu::Queue);

/// The glyphs one job generates, in the order it was asked to.
struct Batch {
    batch: u64,
    keys: Vec<GlyphKey>,
}

/// A text: the font it is laid out in and the fallbacks it was last laid out with, its size, wrap, alignment, and
/// outline, what it shows, and the last `Render` that drew it, 0 before any.
pub(super) struct Text {
    own: Face,
    fallbacks: Vec<Face>,
    size: f32,
    wrap: Wrap,
    align: Align,
    outline: Outline,
    shown: Shown,
    drawn: u64,
}

/// What a text shows.
enum Shown {
    /// Nothing: it is new, and these glyphs are not all in the atlas.
    Waiting { next: Laid },
    /// This layout.
    Ready { shown: Built },
    /// This layout, until every glyph of the one it was written as is in the atlas.
    Rewriting { shown: Built, next: Laid },
}

/// A layout laid into the atlas: its instances, how many, and the atlas's count of packings they were laid at.
struct Built {
    laid: Laid,
    instances: wgpu::Buffer,
    count: u32,
    packing: u64,
}

/// A text placed: the text, how it is drawn, and each pass it joins.
pub(super) struct PlacedText {
    text: u64,
    placing: Placing,
    joins: Vec<TextJoined>,
}

/// One pass a placed text draws in: whether loose or by its maker's list, what the pass is made against, its uniforms
/// there and the group binding them, and the glyph program's pipelines for the pass's shape.
pub(super) struct TextJoined {
    frame: Frame,
    pass: PassName,
    pub listing: Listing,
    fits: Fits,
    uniforms: wgpu::Buffer,
    group: wgpu::BindGroup,
    pipelines: Pipelines,
}

impl Texts {
    /// The renderer's own fonts, read, no text, and an empty atlas that grows to `longest`, on `device`.
    ///
    /// # Errors
    ///
    /// Why one of the renderer's own fonts was not read, which a font compiled in never is.
    pub fn new(device: &wgpu::Device, longest: u32) -> Result<Self, FontRefused> {
        let layouts = Layouts::new(device);
        let atlas = Atlas::new(longest);
        let texture = AtlasTexture::new(device, atlas.side(), layouts.atlas.clone(), layouts.sampler.clone());

        Ok(Self {
            faces: Faces::new()?,
            made: Made::default(),
            placed: Made::default(),
            atlas,
            texture,
            layouts,
            pipelines: HashMap::new(),
            batches: Vec::new(),
            batched: 0,
            unfit: HashSet::new(),
            logged: HashSet::new(),
            renders: 0,
        })
    }

    /// The text `create` asks for, laid out, with each character no font had that is not logged yet, to log.
    ///
    /// # Errors
    ///
    /// Why it would not be made as asked.
    pub fn text(&mut self, create: CreateText) -> Result<(Text, Vec<char>), text::TextRefused> {
        let own = self.faces.face(create.font);
        text::check(&create, own.is_some())?;
        let own = own.ok_or(text::TextRefused::Font)?;
        let CreateText { size, runs, wrap, align, outline, .. } = create;
        let (laid, fallbacks, unlogged) = self.lay_out(&own, size, &runs, (wrap, align));
        let made = Text { own, fallbacks, size, wrap, align, outline, shown: Shown::Waiting { next: laid }, drawn: 0 };

        Ok((made, unlogged))
    }

    /// Lays the text `write` names out again, when `by` made it, in the fallbacks kept now: it shows what it showed
    /// until every glyph of its new layout is in the atlas. Answers each character no font had that is not logged
    /// yet, to log, or why the write was refused, the text left as it was.
    pub fn write(&mut self, write: &WriteText, by: ActorId) -> Owned<Result<Vec<char>, text::TextRefused>> {
        let checked = text::check_runs(&write.runs);
        let fallbacks = self.faces.fallbacks();
        let logged = &mut self.logged;

        self.made.owned(write.text.id, by).map(|text| {
            checked?;
            let laid = layout::lay_out(&text.own, &fallbacks, text.size, &write.runs, (text.wrap, text.align));
            let unlogged = laid.missing.iter().copied().filter(|character| logged.insert(*character)).collect();
            let shown = mem::replace(&mut text.shown, Shown::Waiting { next: Laid::default() });
            text.shown = shown.rewritten(laid);
            text.fallbacks = fallbacks;
            Ok(unlogged)
        })
    }

    /// `runs` laid out in `own` at `size`, wrapped and aligned as `shaping` says, in the fallbacks kept now, and the
    /// fallbacks, with each character no font had that is not logged yet, to log.
    fn lay_out(&mut self, own: &Face, size: f32, runs: &[Run], shaping: (Wrap, Align)) -> (Laid, Vec<Face>, Vec<char>) {
        let fallbacks = self.faces.fallbacks();
        let laid = layout::lay_out(own, &fallbacks, size, runs, shaping);
        let unlogged = laid.missing.iter().copied().filter(|character| self.logged.insert(*character)).collect();

        (laid, fallbacks, unlogged)
    }

    /// The glyphs texts wait on that are not in the atlas, not being generated, and fitted somewhere, as the job that
    /// generates them, kept as a batch, and what the render root keeps for it: none when there are none.
    fn wanted(&mut self) -> Option<(GenerateGlyphs, Generating)> {
        let generating: HashSet<GlyphKey> = self.batches.iter().flat_map(|batch| batch.keys.iter().copied()).collect();
        let mut keys = Vec::new();
        let mut glyphs = Vec::new();
        for text in self.made.values() {
            let next = match &text.shown {
                Shown::Waiting { next } | Shown::Rewriting { next, .. } => next,
                Shown::Ready { .. } => continue,
            };
            for key in next.keys() {
                let held = self.atlas.slot(key).is_some();
                let coming = generating.contains(&key);
                let unfit = self.unfit.contains(&key);
                let asked = keys.contains(&key);
                let known = held || coming || unfit || asked;
                if known {
                    continue;
                }
                let Some(face) = text.face(key.font) else {
                    continue;
                };
                let Some(outline) = face.font.outline(key.glyph) else {
                    continue;
                };
                keys.push(key);
                glyphs.push(GlyphOutline::of(outline, face.font.units_per_em()));
            }
        }
        if keys.is_empty() {
            return None;
        }
        self.batched += 1;
        let batch = self.batched;
        self.batches.push(Batch { batch, keys });

        Some((GenerateGlyphs { glyphs }, Generating { batch }))
    }

    /// Writes the glyphs the job of `batch` generated, `fields` and their texels `pixels`, into the atlas, on `gpu`.
    /// Answers each glyph that fitted nowhere.
    fn generated(&mut self, gpu: Opened<'_>, batch: u64, fields: &[Field], pixels: &[u8]) -> Vec<GlyphKey> {
        let Some(at) = self.batches.iter().position(|batched| batched.batch == batch) else {
            return Vec::new();
        };
        let Batch { keys, .. } = self.batches.swap_remove(at);
        let mut texels: HashMap<GlyphKey, Range<usize>> = HashMap::with_capacity(keys.len());
        let mut start = 0;
        for (key, field) in keys.iter().zip(fields) {
            let end = start + field.width as usize * field.height as usize * 3;
            texels.insert(*key, start..end);
            start = end;
        }
        let added: Vec<(GlyphKey, Field)> = keys.into_iter().zip(fields.iter().copied()).collect();

        let drawn = self.recency();
        let added = self.atlas.add(&added, &drawn, self.renders);
        let (device, queue) = gpu;
        self.texture.arrange(device, queue, &added.arranged);
        for (key, slot) in &added.placed {
            let field = texels.get(key).and_then(|range| pixels.get(range.clone()));
            if let Some(field) = field {
                self.texture.write(queue, *slot, field);
            }
        }
        self.unfit.extend(added.unfit.iter().copied());

        added.unfit
    }

    /// Lets go of the glyphs the job of `batch` was to generate, which it did not: asked for again at the next
    /// `Render`.
    fn ungenerated(&mut self, batch: u64) {
        self.batches.retain(|batched| batched.batch != batch);
    }

    /// The last `Render` each glyph a text shows was drawn at: the latest of the texts that show it.
    fn recency(&self) -> HashMap<GlyphKey, u64> {
        let mut drawn: HashMap<GlyphKey, u64> = HashMap::new();
        for text in self.made.values() {
            let built = match &text.shown {
                Shown::Ready { shown } | Shown::Rewriting { shown, .. } => shown,
                Shown::Waiting { .. } => continue,
            };
            for key in built.laid.keys() {
                let last = drawn.entry(key).or_default();
                *last = (*last).max(text.drawn);
            }
        }

        drawn
    }

    /// Lays each text whose glyphs are all in the atlas into it, as a `Render` begins: a text waiting shows its layout,
    /// a text written shows its new one, and a text the atlas was packed again under is laid into it again, or waits
    /// once more when a glyph it shows was dropped.
    fn refresh(&mut self, gpu: Opened<'_>) {
        for text in self.made.values_mut() {
            let shown = mem::replace(&mut text.shown, Shown::Waiting { next: Laid::default() });
            text.shown = shown.refreshed(&self.atlas, gpu, text.size);
        }
    }

    /// Notes that each text with a placement joined loose, or held by a list in `lists`, is drawn at this `Render`.
    fn mark_drawn(&mut self, lists: &Lists) {
        self.renders += 1;
        let listed: HashSet<u64> = lists.all().collect();
        let drawn: Vec<u64> = self
            .placed
            .iter()
            .filter(|(id, placed)| {
                let loose = placed.joins.iter().any(|join| join.listing == Listing::Loose);
                loose || listed.contains(id)
            })
            .map(|(_, placed)| placed.text)
            .collect();
        for id in drawn {
            if let Some(text) = self.made.get_mut(id) {
                text.drawn = self.renders;
            }
        }
    }

    /// Writes each placed text's uniforms in each pass it joins, by the size of the pass's frame, which `size` finds:
    /// none is written for a frame that is gone.
    fn write_uniforms(&self, queue: &wgpu::Queue, size: impl Fn(Frame) -> Option<Size>) {
        for placed in self.placed.values() {
            let Some(text) = self.made.get(placed.text) else {
                continue;
            };
            for join in &placed.joins {
                if let Some(size) = size(join.frame) {
                    queue.write_buffer(&join.uniforms, 0, &glyph_program::placed(size, placed.placing, text.outline));
                }
            }
        }
    }

    /// Releases the placements of the text `text`, whoever made them, taking each out of its maker's lists in `lists`.
    fn release_placements_of(&mut self, text: u64, lists: &mut Lists) {
        let placed: Vec<u64> = self.placed.iter().filter(|(_, placed)| placed.text == text).map(|(id, _)| id).collect();
        for id in placed {
            if let Some(maker) = self.placed.maker(id) {
                self.release_placement(id, maker, lists);
            }
        }
    }

    /// Releases the text placement `placement`, when `by` made it, taking it out of `by`'s lists in `lists`.
    pub fn release_placement(&mut self, placement: u64, by: ActorId, lists: &mut Lists) -> Owned<()> {
        let released = self.placed.release(placement, by);
        if let Owned::Mine(_) = &released {
            lists.released(placement, by);
        }

        released.map(drop)
    }

    /// Releases the text `text`, when `by` made it, with its placements.
    pub fn release_text(&mut self, text: u64, by: ActorId, lists: &mut Lists) -> Owned<()> {
        let released = self.made.release(text, by);
        if let Owned::Mine(_) = &released {
            self.release_placements_of(text, lists);
            self.unfit.clear();
        }

        released.map(drop)
    }

    /// Releases everything `maker`, which ended, made: its fonts, its texts with their placements, and its placements.
    pub fn ended(&mut self, maker: ActorId, lists: &mut Lists) {
        self.faces.made.release_all(maker);
        for (text, _) in self.made.release_all(maker) {
            self.release_placements_of(text, lists);
            self.unfit.clear();
        }
        self.placed.release_all(maker);
    }

    /// The glyph program's pipelines for passes of `shape`, made the first time a text joins one.
    fn pipelines(&mut self, device: &wgpu::Device, shape: Shape) -> Pipelines {
        let layouts = &self.layouts;

        self.pipelines.entry(shape).or_insert_with(|| Pipelines::new(device, layouts, shape)).clone()
    }

    /// The text placement `place` asks for, whose joins' passes are made against `fitted`, in order.
    fn placement(&mut self, device: &wgpu::Device, place: &PlaceText, fitted: Vec<Fits>) -> PlacedText {
        let joins = place
            .joins
            .iter()
            .zip(fitted)
            .map(|(join, fits)| {
                let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: PLACED_BYTES as u64,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.layouts.placed,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() }],
                });
                let listing = match join.order {
                    placement::Order::Loose => Listing::Loose,
                    placement::Order::Listed => Listing::Listed { stamp: 0 },
                };
                let pipelines = self.pipelines(device, fits.shape);
                TextJoined { frame: join.frame, pass: join.pass, listing, fits, uniforms, group, pipelines }
            })
            .collect();
        let placing = Placing { at: place.at, layer: place.layer, opacity: place.opacity, clip: place.clip };

        PlacedText { text: place.text.id, placing, joins }
    }

    /// How full the atlas is, and the bytes of its texture (ADR-0053).
    pub fn atlas(&self) -> (AtlasFill, u64) {
        let fill =
            AtlasFill { used_texels: self.atlas.used_texels(), side: self.atlas.side(), most_side: self.atlas.most() };

        (fill, self.texture.bytes())
    }

    /// Records the placed text `placed`'s draw in its join `join` into `recording`, a pass that uses depth as `depth`
    /// says, counted into `tally`: nothing while its text shows nothing.
    pub fn draw(
        &self,
        recording: &mut wgpu::RenderPass<'_>,
        (placed, join): (&PlacedText, &TextJoined),
        depth: DepthUse,
        tally: &mut Tally,
    ) {
        let built = match self.made.get(placed.text).map(|text| &text.shown) {
            Some(Shown::Ready { shown } | Shown::Rewriting { shown, .. }) => shown,
            Some(Shown::Waiting { .. }) | None => return,
        };
        if built.count == 0 {
            return;
        }
        recording.set_pipeline(join.pipelines.drawing(placed.placing.layer, depth));
        recording.set_bind_group(0, self.texture.group(), &[]);
        recording.set_bind_group(1, &join.group, &[]);
        recording.set_vertex_buffer(0, built.instances.slice(..));
        recording.draw(0..6, 0..built.count);
        tally.draw(stats::triangles(6, built.count));
    }

    /// Each text placement's join drawn loose in the pass `pass` of `frame`, whose programs are made against `fits`,
    /// under its id, in the order of the ids.
    pub fn loose(&self, (frame, pass): (Frame, PassName), fits: &Fits) -> Vec<(u64, &PlacedText, &TextJoined)> {
        let mut loose: Vec<(u64, &PlacedText, &TextJoined)> = self
            .placed
            .iter()
            .flat_map(|(id, placed)| placed.joins.iter().map(move |join| (id, placed, join)))
            .filter(|(_, _, join)| {
                let draws = join.draws_in(frame, pass, fits);
                let unlisted = join.listing == Listing::Loose;
                draws && unlisted
            })
            .collect();
        loose.sort_unstable_by_key(|(id, ..)| *id);

        loose
    }
}

impl Text {
    /// The face of `font` it was laid out in: its own, or a fallback. None when it was laid out in no such font.
    fn face(&self, font: FontKey) -> Option<&Face> {
        let own = self.own.key == font;
        if own {
            return Some(&self.own);
        }

        self.fallbacks.iter().find(|face| face.key == font)
    }
}

impl Shown {
    /// What a text that showed this shows now, laid into `atlas` on `gpu` at `size`, as [`Texts::refresh`] says.
    fn refreshed(self, atlas: &Atlas, gpu: Opened<'_>, size: f32) -> Self {
        let ready = match &self {
            Self::Waiting { next } | Self::Rewriting { next, .. } => atlas.holds(next.keys()),
            Self::Ready { .. } => false,
        };
        match self {
            Self::Waiting { next } | Self::Rewriting { next, .. } if ready => {
                Self::Ready { shown: Built::new(next, atlas, gpu, size) }
            }
            Self::Waiting { next } => Self::Waiting { next },
            Self::Rewriting { shown, next } => match shown.current(atlas, gpu, size) {
                Current::Shown(shown) => Self::Rewriting { shown, next },
                Current::Lost(_) => Self::Waiting { next },
            },
            Self::Ready { shown } => match shown.current(atlas, gpu, size) {
                Current::Shown(shown) => Self::Ready { shown },
                Current::Lost(next) => Self::Waiting { next },
            },
        }
    }

    /// What a text that showed this shows once written as `next`: nothing while it waited, and what it showed until
    /// `next` is ready.
    fn rewritten(self, next: Laid) -> Self {
        match self {
            Self::Waiting { .. } => Self::Waiting { next },
            Self::Ready { shown } | Self::Rewriting { shown, .. } => Self::Rewriting { shown, next },
        }
    }
}

/// A shown layout, after the atlas changed.
enum Current {
    /// Still shown: laid into the atlas as it is now.
    Shown(Built),
    /// No longer: a glyph of it was dropped from the atlas, so it waits on it again.
    Lost(Laid),
}

impl Built {
    /// `laid` laid into `atlas`, its instances on `gpu`, at `size`.
    fn new(laid: Laid, atlas: &Atlas, (device, queue): Opened<'_>, size: f32) -> Self {
        let bytes: Vec<u8> = laid
            .glyphs
            .iter()
            .filter_map(|glyph| glyph_program::instance(glyph, atlas.slot(glyph.key)?, size))
            .flatten()
            .collect();
        let count = u32::try_from(bytes.len() / INSTANCE_BYTES).unwrap_or(u32::MAX);
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: bytes.len().max(INSTANCE_BYTES) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&instances, 0, &bytes);

        Self { laid, instances, count, packing: atlas.packings() }
    }

    /// It, as the atlas is now: as it was when the atlas was not packed again since, laid in again when it holds every
    /// glyph, and lost when it does not.
    fn current(self, atlas: &Atlas, gpu: Opened<'_>, size: f32) -> Current {
        let packed = self.packing != atlas.packings();
        if !packed {
            return Current::Shown(self);
        }
        if !atlas.holds(self.laid.keys()) {
            return Current::Lost(self.laid);
        }

        Current::Shown(Self::new(self.laid, atlas, gpu, size))
    }
}

impl PlacedText {
    /// Each pass it joins, in the order its placement named them.
    pub fn joins(&self) -> &[TextJoined] {
        &self.joins
    }
}

impl TextJoined {
    /// Whether it draws in the pass `pass` of `frame`, whose programs are made against `fits`: the pass it joined, of
    /// the shape it was made against.
    pub fn draws_in(&self, frame: Frame, pass: PassName, fits: &Fits) -> bool {
        self.joins(frame, pass) && self.fits == *fits
    }

    /// Whether it draws listed in the pass `pass` of `frame`, whose programs are made against `fits`: while its maker's
    /// list holds it, as [`draws_in`](Self::draws_in) says.
    pub fn listed_in(&self, frame: Frame, pass: PassName, fits: &Fits) -> bool {
        let listed = self.listing != Listing::Loose;

        listed && self.draws_in(frame, pass, fits)
    }

    /// Whether it is the join of the pass `pass` of `frame`.
    fn joins(&self, frame: Frame, pass: PassName) -> bool {
        self.frame == frame && self.pass == pass
    }
}

/// The placements a list's check finds: those of meshes, and those of texts, under one count of ids.
pub(super) struct Listable<'a> {
    pub meshes: &'a mut Made<Placement>,
    pub texts: &'a mut Made<PlacedText>,
}

impl Placements for Listable<'_> {
    fn stamp(&mut self, placement: u64, by: ActorId, number: u64) -> Result<(), EntryRefused> {
        if self.meshes.get(placement).is_some() {
            return self.meshes.stamp(placement, by, number);
        }
        let joins = match self.texts.owned(placement, by) {
            Owned::Mine(made) => &mut made.joins,
            Owned::NotOwner => return Err(EntryRefused::NotOwner),
            Owned::Unknown => return Err(EntryRefused::Placement),
        };

        draw_list::stamp(joins.iter_mut().map(|join| &mut join.listing), number)
    }
}

impl Kept {
    /// The text placement `place` asks for, checked, with the glyph program's pipelines made for each shape of pass
    /// it joins that no text joined before, on `device`.
    ///
    /// # Errors
    ///
    /// Why it would not be made as asked.
    pub(super) fn place_text(
        &mut self,
        device: &wgpu::Device,
        place: &PlaceText,
    ) -> Result<PlacedText, text::TextPlacementRefused> {
        let has_text = self.text.made.get(place.text.id).is_some();
        let fitted = text::placement::check(place, has_text, |frame, pass| self.pass(frame, pass))?;

        Ok(self.text.placement(device, place, fitted))
    }

    /// Gets the texts ready for this `Render`, on `gpu`: each laid into the atlas whose glyphs are all in it, each
    /// noted as drawn that will be, and each placement's uniforms written for its frames' sizes now. Answers the job
    /// that generates the glyphs texts still wait on, to run: none when none does.
    pub(super) fn ready_texts(&mut self, gpu: Opened<'_>) -> Option<(GenerateGlyphs, Generating)> {
        self.text.refresh(gpu);
        self.text.mark_drawn(&self.lists);
        self.text.write_uniforms(gpu.1, |frame| self.walk(frame).map(super::walk::Walk::size));

        self.text.wanted()
    }
}

impl aethereal_actor::Carries<CreateFont> for Renderer {
    type Answers = (FontMade,);
}

impl aethereal_actor::Carries<CreateText> for Renderer {
    type Answers = (TextMade,);
}

impl aethereal_actor::Carries<PlaceText> for Renderer {
    type Answers = (TextPlaced,);
}

/// Reads the font in this turn, and answers it, or why its bytes were refused, naming the table.
impl Handles<CreateFont> for Renderer {
    type Sends = (ToSender<FontMade>,);

    fn handle(&mut self, create: CreateFont, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let read = font::Font::read(turn.read(&create.bytes));
        let font = match read {
            Ok(font) => Arc::new(font),
            Err(why) => {
                turn.reply(&FontMade::Refused(why))?;
                return Ok(());
            }
        };
        let id = self.made(turn, |kept, id, maker| kept.text.faces.made.keep(id, maker, font))?;
        turn.reply(&FontMade::Made(FontId { id }))?;

        Ok(())
    }
}

/// Keeps the list in place of the last, for every text laid out from now.
impl Handles<FallbackFonts> for Renderer {
    type Sends = ();

    fn handle(&mut self, fallbacks: FallbackFonts, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.open(&mut self.kept)?.text.faces.fall_back(&fallbacks);

        Ok(())
    }
}

/// Lays the text out, logging each character no font has the first time, keeps it, and asks for the glyphs it waits
/// on.
impl Handles<CreateText> for Renderer {
    type Sends = (ToSender<TextMade>,);

    fn handle(&mut self, create: CreateText, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let made = turn.open(&mut self.kept)?.text.text(create);
        let (made, unlogged) = match made {
            Ok(made) => made,
            Err(why) => {
                turn.reply(&TextMade::Refused(why))?;
                return Ok(());
            }
        };
        let id = self.made(turn, |kept, id, maker| kept.text.made.keep(id, maker, made))?;
        log_missing(turn, &unlogged);
        let wanted = turn.open(&mut self.kept)?.text.wanted();
        if let Some((job, generating)) = wanted {
            turn.run(&job, &generating)?;
        }
        turn.reply(&TextMade::Made(TextId { id }))?;

        Ok(())
    }
}

/// Lays the sender's text out again, in the fallbacks kept now; its placements draw what it was until every glyph of
/// what it is now is in the atlas. A write refused is logged, and the text keeps what it had.
impl Handles<WriteText> for Renderer {
    type Sends = ();

    fn handle(&mut self, write: WriteText, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let kept = turn.open(&mut self.kept)?;
        let written = kept.text.write(&write, by);
        let wanted = kept.text.wanted();
        match written {
            Owned::Mine(Ok(unlogged)) => log_missing(turn, &unlogged),
            Owned::Mine(Err(why)) => turn.log(Level::Warn, format_args!("{:?} was not written: {why:?}", write.text)),
            Owned::NotOwner => turn.log(Level::Warn, format_args!("{:?} was not written: another made it", write.text)),
            Owned::Unknown => turn.log(Level::Warn, format_args!("{:?} was not written: no such thing", write.text)),
        }
        if let Some((job, generating)) = wanted {
            turn.run(&job, &generating)?;
        }

        Ok(())
    }
}

/// Checks the placement, makes the glyph program's pipelines for each pass's shape it joins that none has joined
/// before, and keeps it.
impl Handles<PlaceText> for Renderer {
    type Sends = (ToSender<TextPlaced>,);

    fn handle(&mut self, place: PlaceText, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let device = turn.open(&mut self.gpu)?.device().clone();
        let placed = turn.open(&mut self.kept)?.place_text(&device, &place);
        let made = match placed {
            Ok(made) => made,
            Err(why) => {
                turn.reply(&TextPlaced::Refused(why))?;
                return Ok(());
            }
        };
        let id = self.made(turn, |kept, id, maker| kept.text.placed.keep(id, maker, made))?;
        turn.reply(&TextPlaced::Made(PlacementId { id }))?;

        Ok(())
    }
}

/// Moves the sender's placed text, laying nothing out. A move refused is logged, and the placement stays where it was.
impl Handles<MoveText> for Renderer {
    type Sends = ();

    fn handle(&mut self, moved: MoveText, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let by = turn.sender().id();
        let MoveText { placement, at, layer, opacity, clip } = moved;
        let placing = Placing { at, layer, opacity, clip };
        if let Err(why) = text::placement::check_placing(placing) {
            turn.log(Level::Warn, format_args!("{placement:?} was not moved: {why:?}"));
            return Ok(());
        }
        let kept = turn.open(&mut self.kept)?;
        let owned = kept.text.placed.owned(placement.id, by).map(|placed| placed.placing = placing);
        match owned {
            Owned::Mine(()) => {}
            Owned::NotOwner => turn.log(Level::Warn, format_args!("{placement:?} was not moved: another made it")),
            Owned::Unknown => turn.log(Level::Warn, format_args!("{placement:?} was not moved: no such text placed")),
        }

        Ok(())
    }
}

/// Writes the glyphs the job generated into the atlas, logging each that fitted nowhere; a job that failed lets its
/// glyphs go, to be asked for again.
impl Handles<Answer<GlyphsGenerated, Generating>> for Renderer {
    type Sends = ();

    fn handle(
        &mut self,
        (answered, Generating { batch }): (Answered<GlyphsGenerated>, Generating),
        turn: &mut Turn<'_, Self, ()>,
    ) -> Result<(), Fault> {
        let generated = match answered {
            Answered::Yes(generated) => generated,
            Answered::No(why) => {
                turn.log(Level::Warn, format_args!("glyphs were not generated: {why:?}"));
                turn.open(&mut self.kept)?.text.ungenerated(batch);
                return Ok(());
            }
        };
        let pixels = turn.read(&generated.pixels).to_vec();
        let fields: Vec<Field> = generated
            .fields
            .iter()
            .map(|field| Field { width: field.width, height: field.height, left: field.left, top: field.top })
            .collect();
        // Each field's three channels are written as four.
        self.watch.uploaded(pixels.len() / 3 * 4);
        let gpu = turn.open(&mut self.gpu)?;
        let (device, queue) = (gpu.device().clone(), gpu.queue().clone());
        let unfit = turn.open(&mut self.kept)?.text.generated((&device, &queue), batch, &fields, &pixels);
        for key in unfit {
            turn.log(Level::Warn, format_args!("glyph {key:?} is not drawn: the atlas is full"));
        }

        Ok(())
    }
}

/// Logs each character of `unlogged`, which no font has, and which is drawn as its font's missing glyph.
fn log_missing<S>(turn: &mut Turn<'_, Renderer, S>, unlogged: &[char]) {
    for character in unlogged {
        turn.log(Level::Warn, format_args!("{character:?} is drawn as the missing glyph: no font has it"));
    }
}
