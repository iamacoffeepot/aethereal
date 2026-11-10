//! Text as the render root keeps and draws it: on the device that draws nothing, what a text shows as its glyphs come
//! and as it is written, who may write and release it, the order its placements draw in among meshes', and the joins
//! refused; and, where the machine has a GPU adapter, the pixels a glyph covers.

// The GPU test says which adapter drew, or that there was none and the run was skipped.
#![allow(clippy::print_stderr)]

use super::*;
use crate::frame::{CreateFrame, Does, Ends, FrameId, Framed, Load, PassSpec, Target, TargetKind, TargetSpec};
use crate::pass::{Main, Pass};
use crate::placement::JoinRefused;
use crate::readback::Region;
use crate::renderer::tests::lists::{OTHER, adapter_device, drawn_in_main, main_list, placed as mesh, tinted};
use crate::renderer::tests::{MAKER, kept, window};
use crate::renderer::{Reading, copy_frame, mapped_bytes};
use crate::shape::{ColourFormat, Samples};
use crate::text::generate::generate;
use crate::text::{Clip, Colour, Font, Layer, TextJoin, TextPlacementRefused, widest_outline};
use crate::texture::TextureFormat;
use crate::{Gpu, frame};
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};

/// A pass that reads the target the pass before it drew.
#[derive(Plain)]
#[kind(name = "aether.test.overlay")]
struct Overlay;

impl Pass for Overlay {}

/// A text of `text` in white, in the renderer's own proportional font at `size`, made by `maker` and kept in `kept`.
fn made(kept: &mut Kept, maker: ActorId, text: &str, size: f32) -> TextId {
    outlined(kept, maker, (text, size), Outline::None)
}

/// A text of `text` in white, in the renderer's own proportional font at `size`, outlined as `outline`, made by
/// `maker` and kept in `kept`.
fn outlined(kept: &mut Kept, maker: ActorId, (text, size): (&str, f32), outline: Outline) -> TextId {
    let create = CreateText {
        font: Font::Sans,
        size,
        runs: vec![Run::new(text, Colour::WHITE)],
        wrap: Wrap::None,
        align: Align::Left,
        outline,
    };
    let Ok((made, _)) = kept.text.text(create) else {
        panic!("the text is made");
    };
    let id = kept.ids.next();
    kept.text.made.keep(id, maker, made);

    TextId { id }
}

/// Generates the glyphs the texts in `kept` wait on at once, as their job would, and writes them into the atlas on
/// `gpu`.
fn generated(kept: &mut Kept, gpu: Opened<'_>) {
    let Some((job, Generating { batch })) = kept.text.wanted() else {
        return;
    };
    let (fields, pixels) = generate(&job.glyphs);
    let fields: Vec<Field> = fields
        .iter()
        .map(|field| Field { width: field.width, height: field.height, left: field.left, top: field.top })
        .collect();
    assert_eq!(kept.text.generated(gpu, batch, &fields, &pixels), Vec::new(), "every glyph fits");
}

/// The glyphs of the layout `text` shows, each once: none while it shows nothing.
fn shown(kept: &Kept, text: TextId) -> Option<Vec<GlyphKey>> {
    match &kept.text.made.get(text.id)?.shown {
        Shown::Ready { shown } | Shown::Rewriting { shown, .. } => Some(shown.laid.keys().collect()),
        Shown::Waiting { .. } => None,
    }
}

/// The glyphs the renderer's own proportional font draws `text` with, each once, in the order a layout keys them.
fn keys_of(kept: &Kept, text: &str) -> Vec<GlyphKey> {
    let Some(sans) = kept.text.faces.face(Font::Sans) else {
        panic!("the renderer's own font is kept");
    };
    let mut keys: Vec<GlyphKey> = text
        .chars()
        .filter_map(|character| Some(GlyphKey { font: FontKey::Sans, glyph: sans.font.glyph(character)? }))
        .collect();
    keys.sort_unstable();
    keys.dedup();

    keys
}

/// A placement by `maker` of `text`, its top left at `at`, over the window's main frame's pass [`Main`], joined as
/// `order` says, made and kept in `kept`.
fn placed_text(
    kept: &mut Kept,
    device: &wgpu::Device,
    (maker, text, order): (ActorId, TextId, placement::Order),
    at: Vec2,
) -> PlacementId {
    let frame = Frame::Main(window());
    let join = match order {
        placement::Order::Loose => TextJoin::to::<Main>(frame),
        placement::Order::Listed => TextJoin::listed::<Main>(frame),
    };
    let place = PlaceText { text, at, layer: Layer::Over, opacity: 1.0, clip: Clip::None, joins: vec![join] };
    let made = kept.place_text(device, &place);
    let Ok(made) = made else {
        panic!("the text is placed: {:?}", made.err());
    };
    let id = kept.ids.next();
    kept.text.placed.keep(id, maker, made);

    PlacementId { id }
}

/// Fits the window's main frame of `kept` to `width` by `height`, gets its texts ready, and draws it into `frame`.
fn drawn_into(kept: &mut Kept, gpu: Opened<'_>, frame: &wgpu::Texture) -> wgpu::CommandEncoder {
    let (device, _) = gpu;
    kept.fit(device, window(), Size { width: frame.width(), height: frame.height() });
    assert!(kept.ready_texts(gpu).is_none(), "every glyph is in the atlas");
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let windows = [(window(), frame.create_view(&wgpu::TextureViewDescriptor::default()))];
    kept.run_all(&mut encoder, &windows, None, &mut Tally::Off);

    encoder
}

/// A texture `side` by `side` of the window's format, drawn into and copied from as its surface would be.
fn surface(device: &wgpu::Device, side: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d { width: side, height: side, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ColourFormat::Bgra8.wgpu(),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// Draws the window's main frame of `kept` on `gpu` into a surface `side` by `side`, and answers its pixels, four
/// bytes each, row by row from the top.
fn drawn_pixels(kept: &mut Kept, gpu: Opened<'_>, side: u32) -> Vec<u8> {
    let (device, queue) = gpu;
    let frame = surface(device, side);
    let mut encoder = drawn_into(kept, gpu, &frame);
    let Ok(Reading::Pixels { buffer, copied, order, .. }) = copy_frame(device, &mut encoder, &frame) else {
        panic!("the frame is copied");
    };
    queue.submit([encoder.finish()]);
    buffer.map_async(wgpu::MapMode::Read, .., |mapped| mapped.expect("the copy maps"));
    device.poll(wgpu::PollType::wait_indefinitely()).expect("the GPU finishes");
    let Some(pixels) = mapped_bytes(&buffer, |bytes| copied.unpad(bytes, order)) else {
        panic!("the copy is read");
    };

    pixels
}

/// The one glyph `text`, of `size` pixels to the em, shows, and its quad in the frame's pixels
/// when the text's top left is at `at`: where the layout and the field say the glyph's box is, from the field's place
/// at the base size.
fn quad_of(kept: &Kept, text: TextId, size: f32, at: Vec2) -> (GlyphKey, Rect2) {
    let Some(laid) = shown(kept, text).and_then(|keys| keys.first().copied()) else {
        panic!("the text shows its glyph");
    };
    let Some(slot) = kept.text.atlas.slot(laid) else {
        panic!("the glyph is in the atlas");
    };
    let Some(sans) = kept.text.faces.face(Font::Sans) else {
        panic!("the renderer's own font is kept");
    };
    let ascender = f32::from(sans.font.line_metrics().ascender) * sans.font.pixels_per_unit(size);
    let factor = size / font::msdf::BASE_PIXELS_PER_EM;
    // A field's place and size are some tens of texels, which an f32 holds exactly.
    #[allow(clippy::cast_precision_loss, clippy::suboptimal_flops)]
    let min = Vec2::new(at.x + slot.field.left as f32 * factor, at.y + ascender - slot.field.top as f32 * factor);
    #[allow(clippy::cast_precision_loss)]
    let extent = Vec2::new(slot.field.width as f32, slot.field.height as f32) * factor;

    (laid, Rect2 { min, max: min + extent })
}

#[test]
fn a_new_text_shows_nothing_until_its_glyphs_are_in_and_a_written_one_shows_what_it_was_until_its_new_ones_are() {
    // Catches a text drawn with half its glyphs before the atlas holds them all, and a written text that blanks, or
    // shows its new glyphs where they are not yet, until its new glyphs come: a line that flickers as it changes.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = (&device, &queue);
    let mut kept = kept(&device);
    let text = made(&mut kept, MAKER, "ab", 20.0);

    kept.text.refresh(gpu);
    assert_eq!(shown(&kept, text), None, "its glyphs are not generated");
    generated(&mut kept, gpu);
    kept.text.refresh(gpu);
    assert_eq!(shown(&kept, text), Some(keys_of(&kept, "ab")));

    let write = WriteText { text, runs: vec![Run::new("xy", Colour::WHITE)] };
    assert!(matches!(kept.text.write(&write, MAKER), Owned::Mine(Ok(_))));
    kept.text.refresh(gpu);
    assert_eq!(shown(&kept, text), Some(keys_of(&kept, "ab")), "what it was, until x and y are in");
    generated(&mut kept, gpu);
    kept.text.refresh(gpu);
    assert_eq!(shown(&kept, text), Some(keys_of(&kept, "xy")));
}

#[test]
fn only_a_text_s_maker_writes_or_releases_it_and_releasing_it_takes_every_placement_of_it() {
    // Catches a write or release by another actor let through; and a released text's placements left behind, drawing
    // a text that is gone, or left in another actor's list, found missing at `Render`.
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut kept = kept(&device);
    let text = made(&mut kept, MAKER, "hi", 16.0);
    let mine = placed_text(&mut kept, &device, (MAKER, text, placement::Order::Loose), Vec2::ZERO);
    let theirs = placed_text(&mut kept, &device, (OTHER, text, placement::Order::Listed), Vec2::ZERO);
    assert_eq!(kept.listed(&main_list(&[theirs]), OTHER), Ok(()));
    assert_eq!(drawn_in_main(&kept), vec![mine.id, theirs.id]);

    let write = WriteText { text, runs: vec![Run::new("no", Colour::WHITE)] };
    assert!(matches!(kept.text.write(&write, OTHER), Owned::NotOwner));
    assert_eq!(kept.release_placement(mine.id, OTHER), Owned::NotOwner);
    assert_eq!(kept.text.release_text(text.id, OTHER, &mut kept.lists), Owned::NotOwner);
    assert_eq!(drawn_in_main(&kept), vec![mine.id, theirs.id]);

    assert_eq!(kept.text.release_text(text.id, MAKER, &mut kept.lists), Owned::Mine(()));
    assert_eq!(drawn_in_main(&kept), Vec::<u64>::new());
    assert_eq!(kept.lists.all().count(), 0, "the other's list lets go of its placement");
}

#[test]
fn placed_texts_draw_after_loose_meshes_and_in_their_list_s_order_among_the_maker_s_meshes() {
    // Catches a list that names a placed text refused as naming no placement, a listed text drawn loose or in another
    // order than its list's, and a placed text the device refuses to draw.
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let gpu = Gpu::new(device.clone(), queue.clone());
    let mut kept = kept(&device);
    let program = tinted(&gpu, &mut kept);
    let loose_mesh = mesh(&gpu, &mut kept, (MAKER, program, placement::Order::Loose), (-1.0, 1.0), [0.5; 4]);
    let listed_mesh = mesh(&gpu, &mut kept, (MAKER, program, placement::Order::Listed), (-1.0, 1.0), [0.5; 4]);
    let text = made(&mut kept, MAKER, "Ag", 24.0);
    let loose_text = placed_text(&mut kept, &device, (MAKER, text, placement::Order::Loose), Vec2::ZERO);
    let listed_text = placed_text(&mut kept, &device, (MAKER, text, placement::Order::Listed), Vec2::ZERO);
    assert_eq!(drawn_in_main(&kept), vec![loose_mesh.id, loose_text.id]);

    assert_eq!(kept.listed(&main_list(&[listed_text, listed_mesh]), MAKER), Ok(()));
    assert_eq!(drawn_in_main(&kept), vec![loose_mesh.id, loose_text.id, listed_text.id, listed_mesh.id]);
    assert_eq!(kept.listed(&main_list(&[listed_mesh, listed_text]), MAKER), Ok(()));
    assert_eq!(drawn_in_main(&kept), vec![loose_mesh.id, loose_text.id, listed_mesh.id, listed_text.id]);

    generated(&mut kept, (&device, &queue));
    let encoder = drawn_into(&mut kept, (&device, &queue), &surface(&device, 8));
    queue.submit([encoder.finish()]);
}

#[test]
fn a_placement_is_refused_a_pass_that_reads_targets_a_pass_twice_and_a_text_or_frame_not_kept() {
    // Catches a text joined to a pass that reads targets, where the glyph program, which binds the atlas where the pass
    // binds what it reads, would not draw; a pass joined twice; and a text, a frame, or a placement not drawable.
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut kept = kept(&device);
    let glow = PassSpec::of::<Main>(Does::Draw {
        into: Target::Texture(0),
        depth: DepthUse::Off,
        load: Load::Clear,
        reads: Vec::new(),
    });
    let over = PassSpec::of::<Overlay>(Does::Draw {
        into: Target::End,
        depth: DepthUse::Off,
        load: Load::Clear,
        reads: vec![Target::Texture(0)],
    });
    let create = CreateFrame {
        ends: Ends::Texture { size: Size { width: 4, height: 4 }, format: TextureFormat::Rgba8 },
        targets: vec![TargetSpec { kind: TargetKind::Float, scale: frame::Scale::Full, samples: Samples::One }],
        passes: vec![glow, over],
    };
    assert_eq!(frame::check(&create, &kept), Ok(()));
    let Ok(Framed::Texture { frame, .. }) = kept.frame(&device, create, MAKER) else {
        panic!("the frame is made");
    };
    let text = made(&mut kept, MAKER, "a", 12.0);
    let place = |joins| PlaceText { text, at: Vec2::ZERO, layer: Layer::Over, opacity: 1.0, clip: Clip::None, joins };
    let main = TextJoin::to::<Main>(Frame::Texture(frame));
    let overlay = TextJoin::to::<Overlay>(Frame::Texture(frame));
    let refused = |join, why| Some(TextPlacementRefused::Join { join, why });

    assert!(kept.place_text(&device, &place(vec![main])).is_ok());
    assert_eq!(kept.place_text(&device, &place(vec![main, overlay])).err(), refused(1, JoinRefused::Reads));
    assert_eq!(kept.place_text(&device, &place(vec![main, main])).err(), refused(1, JoinRefused::Twice));
    let gone = TextJoin::to::<Main>(Frame::Texture(FrameId { id: 999 }));
    assert_eq!(kept.place_text(&device, &place(vec![gone])).err(), refused(0, JoinRefused::Frame));
    assert_eq!(kept.place_text(&device, &place(Vec::new())).err(), Some(TextPlacementRefused::Unjoined));
    let unknown = PlaceText { text: TextId { id: 999 }, ..place(vec![main]) };
    assert_eq!(kept.place_text(&device, &unknown).err(), Some(TextPlacementRefused::Text));
    let faint = PlaceText { opacity: 1.5, ..place(vec![main]) };
    assert_eq!(kept.place_text(&device, &faint).err(), Some(TextPlacementRefused::Opacity));
}

// A machine with no adapter, as CI's, skips it and says so.
#[test]
fn on_a_gpu_a_glyph_covers_the_pixels_inside_its_edge_where_it_is_placed_and_none_elsewhere() {
    // Catches a glyph program that samples the atlas at the wrong place, reads the field's distance the wrong way
    // round, or sets the quad off its layout: nothing drawn, a box drawn, or the glyph drawn somewhere else.
    let (device, queue, adapter) = match adapter_device() {
        Ok(opened) => opened,
        Err(why) => {
            eprintln!("skipped: no GPU adapter to draw the glyph on: {why}");
            return;
        }
    };
    eprintln!("ran adapter={adapter:?}");
    let gpu = (&device, &queue);
    let mut kept = kept(&device);
    let at = Vec2::new(16.0, 8.0);
    let text = made(&mut kept, MAKER, "I", 40.0);
    placed_text(&mut kept, &device, (MAKER, text, placement::Order::Loose), at);
    generated(&mut kept, gpu);

    let side = 64;
    let pixels = drawn_pixels(&mut kept, gpu, side);
    let (_, bounds) = quad_of(&kept, text, 40.0, at);
    // A glyph's place and size are some tens of pixels, inside the frame.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let quad = Region {
        x: bounds.min.x.floor() as u32,
        y: bounds.min.y.floor() as u32,
        width: (bounds.max.x - bounds.min.x).ceil() as u32 + 1,
        height: (bounds.max.y - bounds.min.y).ceil() as u32 + 1,
    };
    let inside =
        |x: u32, y: u32| (quad.x..quad.x + quad.width).contains(&x) && (quad.y..quad.y + quad.height).contains(&y);
    let red = |x: u32, y: u32| pixels[((y * side + x) * 4) as usize];
    let lit: Vec<(u32, u32)> =
        (0..side).flat_map(|y| (0..side).map(move |x| (x, y))).filter(|(x, y)| red(*x, *y) > 0).collect();
    let solid = lit.iter().filter(|(x, y)| red(*x, *y) == u8::MAX).count();

    assert!(solid >= 60, "a 40-pixel I's stem is solid white over some 60 pixels or more, not {solid}, on {adapter}");
    let outside: Vec<&(u32, u32)> = lit.iter().filter(|(x, y)| !inside(*x, *y)).collect();
    assert!(outside.is_empty(), "nothing is drawn outside the glyph's quad {quad:?}: {outside:?} on {adapter}");
    // The field reaches some 3 pixels past the glyph's edge at this size, so the quad's corners lie outside it.
    let corners = [(quad.x + 1, quad.y + 1), (quad.x + quad.width - 2, quad.y + quad.height - 2)];
    let padding: Vec<u8> = corners.iter().map(|(x, y)| red(*x, *y)).collect();
    assert_eq!(padding, [0, 0], "the field's padding inside the quad is not drawn, on {adapter}");
    let edged = lit.iter().any(|(x, y)| red(*x, *y) < u8::MAX);
    assert!(edged, "the stem's edge is covered in part, not cut square, on {adapter}");
}

// A machine with no adapter, as CI's, skips it and says so.
#[test]
fn on_a_gpu_the_widest_outline_is_a_rim_around_the_glyph_and_its_quad_past_the_field_s_reach_is_not_drawn() {
    // Catches an outline allowed as wide as the field reaches: where the field holds nothing nearer than its reach, the
    // rim's half-pixel smoothing still covers half of every pixel, and each glyph draws a dark box over its whole quad.
    let (device, queue, adapter) = match adapter_device() {
        Ok(opened) => opened,
        Err(why) => {
            eprintln!("skipped: no GPU adapter to draw the outlined glyph on: {why}");
            return;
        }
    };
    eprintln!("ran adapter={adapter:?}");
    let gpu = (&device, &queue);
    let opened = Gpu::new(device.clone(), queue.clone());
    let at = Vec2::new(16.0, 8.0);
    let side = 192;

    for size in [8.0, 12.0, 18.0, 48.0, 128.0] {
        let mut kept = kept(&device);
        let program = tinted(&opened, &mut kept);
        let sand = [1.0, 0.85, 0.55, 1.0];
        mesh(&opened, &mut kept, (MAKER, program, placement::Order::Loose), (-1.0, 1.0), sand);
        let outline = Outline::Of { width: widest_outline(size), colour: Colour::BLACK };
        let text = outlined(&mut kept, MAKER, ("o", size), outline);
        placed_text(&mut kept, &device, (MAKER, text, placement::Order::Loose), at);
        generated(&mut kept, gpu);

        let pixels = drawn_pixels(&mut kept, gpu, side);
        let (laid, quad) = quad_of(&kept, text, size, at);
        let Some(field) =
            kept.text.faces.face(Font::Sans).and_then(|sans| {
                sans.font.msdf(laid.glyph, font::msdf::BASE_PIXELS_PER_EM, font::msdf::DEFAULT_PADDING)
            })
        else {
            panic!("the glyph has a field");
        };
        let pixel = |x: u32, y: u32| {
            let at = ((y * side + x) * 4) as usize;
            [pixels[at], pixels[at + 1], pixels[at + 2]]
        };
        let background = pixel(0, 0);
        let factor = size / font::msdf::BASE_PIXELS_PER_EM;
        // A pixel's centre is some hundreds of pixels in, which an f32 holds exactly.
        #[allow(clippy::cast_precision_loss)]
        let centre = |x: u32, y: u32| Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
        let in_quad = |at: Vec2| quad.contains_point(at);
        // Where the field holds nothing nearer than its reach: well outside any rim it allows.
        let beyond = |at: Vec2| {
            let texel = (at - quad.min) * (1.0 / factor);
            field.median(texel.x, texel.y) == 0.0
        };
        let probes: Vec<(u32, u32)> = (0..side)
            .flat_map(|y| (0..side).map(move |x| (x, y)))
            .filter(|&(x, y)| in_quad(centre(x, y)) && beyond(centre(x, y)))
            .collect();
        // The glyph is white and the background below it in every channel, so only the black rim darkens a pixel.
        let rimmed = (0..side)
            .flat_map(|y| (0..side).map(move |x| (x, y)))
            .any(|(x, y)| pixel(x, y).into_iter().zip(background).all(|(drawn, under)| drawn < under));

        assert!(!probes.is_empty(), "the {size} px o's quad has corners past its field's reach, on {adapter}");
        let drawn: Vec<&(u32, u32)> = probes.iter().filter(|&&(x, y)| pixel(x, y) != background).collect();
        assert!(
            drawn.is_empty(),
            "at {size} px, the quad {quad:?} past the field's reach is drawn: {drawn:?} on {adapter}"
        );
        assert!(rimmed, "at {size} px, a dark rim is drawn around the o, on {adapter}");
    }
}
