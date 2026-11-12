//! Fonts as the render root keeps them: one made from the renderer's own shares its face, a font's fallbacks are its
//! own faces in order, fixed as it is made, and every face's metrics are the blob put for it once.

use super::*;
use crate::made::Owned;
use crate::text::{Colour, FontId};
use aethereal_actor::{Maker, Puts};
use core::iter;

/// Noto Sans Regular (SIL OFL), the font module's fixture.
const NOTO: &[u8] = include_bytes!("../../../fixtures/fonts/NotoSans-Regular.ttf");

/// A character the renderer's own monospace font draws, and its proportional one and Noto lack: a box-drawing line.
const ONLY_MONO: char = '\u{2500}';

/// The maker of every font here.
const MAKER: aethereal_data::ActorId = aethereal_data::ActorId::new(1);

/// The puts of a writer with room for any blob, which reads back what it put.
fn puts() -> Puts {
    Puts::new(0, 0, usize::MAX)
}

/// The renderer's own fonts, their metrics put by `puts`.
fn faces_in(puts: &mut Puts) -> Faces {
    let Ok(faces) = Faces::new(|bytes| puts.pinned(bytes, Maker::Chassis)) else {
        panic!("the renderer's own fonts are read and put");
    };

    faces
}

/// The renderer's own fonts, their metrics put as blobs no store holds: for a test that reads none back.
pub fn faces() -> Faces {
    faces_in(&mut puts())
}

/// Noto, read, its metrics put by `puts`.
fn noto(puts: &mut Puts) -> Own {
    let Ok(font) = font::Font::read(NOTO) else {
        panic!("Noto reads");
    };
    let Ok(metrics) = puts.pinned(font.metrics().encode(), Maker::Chassis) else {
        panic!("Noto's metrics are put");
    };

    Own::Read { font: Arc::new(font), metrics }
}

/// Makes the font of `own` with `fallbacks` in `faces`, kept under `id`.
fn made(faces: &mut Faces, id: u64, own: Own, fallbacks: &[FontId]) -> FontId {
    let Ok(typeface) = faces.make(own, fallbacks) else {
        panic!("the font is made");
    };
    faces.made.keep(id, MAKER, typeface);

    FontId { id }
}

/// The font kept as `font`.
fn kept(faces: &Faces, font: FontId) -> &Typeface {
    let Some(typeface) = faces.typeface(Font::Made(font)) else {
        panic!("the font is kept");
    };

    typeface
}

/// Which font draws `character`, laid out alone in `typeface`, and whether it is the missing glyph.
fn drawn_by(typeface: &Typeface, character: char) -> Option<(FontKey, bool)> {
    let laid = typeface.lay_out(16.0, &[Run::new(&character.to_string(), Colour::WHITE)], (Wrap::None, Align::Left));
    let key = laid.glyphs.first().and_then(|glyph| typeface.key(glyph))?;

    Some((key.font, key.glyph == font::GlyphId::MISSING))
}

/// The ids of the blobs `metrics` answers, own first.
fn blobs(metrics: &FontMetrics) -> Vec<aethereal_actor::BlobId> {
    match metrics {
        FontMetrics::Read { own, fallbacks } => iter::once(own).chain(fallbacks).map(Bytes::id).collect(),
        FontMetrics::Refused(_) => Vec::new(),
    }
}

#[test]
fn a_font_made_from_the_renderer_s_own_shares_its_face_its_glyphs_and_its_metrics() {
    // Catches a font made from Sans that reads Sans again, holding a second copy of its outlines; that is keyed apart
    // from Sans in the atlas, so every glyph it draws is generated and held twice; or that puts its metrics again.
    let mut faces = faces();
    let font = made(&mut faces, 1, Own::Sans, &[]);

    let (made, sans) = (&kept(&faces, font).own, &faces.typeface(Font::Sans).expect("Sans is kept").own);

    assert_eq!(made.key, FontKey::Sans);
    assert!(Arc::ptr_eq(&made.font, &sans.font), "the face is Sans's, not read again");
    assert_eq!(made.metrics, sans.metrics, "the blob is Sans's, not put again");
}

#[test]
fn a_font_s_fallbacks_are_their_own_faces_in_order_and_theirs_are_not_tried() {
    // Catches a font that tries its fallbacks' fallbacks, so whether a character draws depends on fonts it never
    // named; and fallbacks kept in another order than named.
    let mut puts = puts();
    let mut faces = faces_in(&mut puts);
    let mono = made(&mut faces, 1, Own::Mono, &[]);
    let sans_over_mono = made(&mut faces, 2, Own::Sans, &[mono]);
    let over_that = made(&mut faces, 3, Own::Sans, &[sans_over_mono]);
    let sans = made(&mut faces, 4, Own::Sans, &[]);
    let noto_own = noto(&mut puts);
    let noto_then = made(&mut faces, 5, noto_own, &[sans, mono]);

    assert_eq!(drawn_by(kept(&faces, sans_over_mono), ONLY_MONO), Some((FontKey::Mono, false)));
    assert_eq!(drawn_by(kept(&faces, over_that), ONLY_MONO), Some((FontKey::Sans, true)), "Mono is two levels down");
    let order: Vec<FontKey> = kept(&faces, noto_then).fallbacks.iter().map(|face| face.key).collect();
    assert_eq!(order, [FontKey::Sans, FontKey::Mono]);
}

#[test]
fn releasing_a_fallback_changes_nothing_about_a_font_that_named_it_and_a_font_naming_it_after_is_refused() {
    // Catches a font that looks its fallbacks up by id as it lays a text out, or answers its metrics: releasing one
    // would silently change every text and every measure in a font that named it.
    let mut faces = faces();
    let mono = made(&mut faces, 1, Own::Mono, &[]);
    let over_mono = made(&mut faces, 2, Own::Sans, &[mono]);
    let before = blobs(&kept(&faces, over_mono).metrics());

    assert!(matches!(faces.made.release(mono.id, MAKER), Owned::Mine(_)));

    assert_eq!(drawn_by(kept(&faces, over_mono), ONLY_MONO), Some((FontKey::Mono, false)));
    assert_eq!(blobs(&kept(&faces, over_mono).metrics()), before);
    let refused = [faces.make(Own::Sans, &[mono]).err(), faces.make(Own::Sans, &[over_mono, mono]).err()];
    assert_eq!(refused, [Some(FontNotMade::Fallback { fallback: 0 }), Some(FontNotMade::Fallback { fallback: 1 })]);
}

#[test]
fn a_font_s_metrics_are_the_blobs_put_once_for_its_faces_in_order_each_its_own_face_s_metrics() {
    // Catches metrics answered by encoding them again, so each ask puts tens of kilobytes; blobs answered in another
    // order than the fallbacks are tried; and a face given another face's blob, so an actor measures in the wrong font.
    let mut puts = puts();
    let mut faces = faces_in(&mut puts);
    let mono = made(&mut faces, 1, Own::Mono, &[]);
    let noto_own = noto(&mut puts);
    let font = made(&mut faces, 2, noto_own, &[mono]);
    let typeface = kept(&faces, font);

    let (first, second) = (typeface.metrics(), typeface.metrics());

    let faces_blobs: Vec<_> =
        iter::once(&typeface.own).chain(&typeface.fallbacks).map(|face| face.metrics.id()).collect();
    assert_eq!(blobs(&first), faces_blobs);
    assert_eq!(blobs(&second), faces_blobs, "the same blobs at every ask");
    for face in iter::once(&typeface.own).chain(&typeface.fallbacks) {
        let decoded = puts.read(face.metrics.id()).map(Metrics::decode);
        assert_eq!(decoded, Some(Ok(face.font.metrics().clone())), "{:?}'s blob is its own metrics", face.key);
    }
}
