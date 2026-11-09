//! Placing a text (ADR-0052): checked as it is made, as any placement is, so drawing it cannot fail. A join names a
//! pass and no program: the glyph program draws it, in a pipeline made for the pass's shape, and reads no targets, so
//! a pass that reads them is refused, as is one whose colour is whole numbers, which nothing blends into.

use super::glyph_program::Placing;
use super::{Clip, Layer, PlaceText, TextPlacementRefused};
use crate::frame::Frame;
use crate::pass::PassName;
use crate::placement::{Fits, JoinRefused};

/// Checks a text's placement before it is made: that `has_text`, its text is kept; that it is drawn as a placement
/// may be; and that it joins some pass, each that `pass` finds drawing, once, reading no targets, into colours that
/// blend. Answers what each join's pass is made against, in order.
///
/// # Errors
///
/// Why it would not be made as asked.
pub fn check(
    place: &PlaceText,
    has_text: bool,
    pass: impl Fn(Frame, PassName) -> Result<Fits, JoinRefused>,
) -> Result<Vec<Fits>, TextPlacementRefused> {
    if !has_text {
        return Err(TextPlacementRefused::Text);
    }
    check_placing(Placing { at: place.at, layer: place.layer, opacity: place.opacity, clip: place.clip })?;
    if place.joins.is_empty() {
        return Err(TextPlacementRefused::Unjoined);
    }
    let mut fitted = Vec::with_capacity(place.joins.len());
    for (join, at) in place.joins.iter().zip(0..) {
        let refused = |why| TextPlacementRefused::Join { join: at, why };
        let twice =
            place.joins[..at as usize].iter().any(|before| before.frame == join.frame && before.pass == join.pass);
        if twice {
            return Err(refused(JoinRefused::Twice));
        }
        let fits = pass(join.frame, join.pass).map_err(refused)?;
        if !fits.reads.is_empty() {
            return Err(refused(JoinRefused::Reads));
        }
        if fits.shape.colour.is_whole() {
            return Err(refused(JoinRefused::Shape));
        }
        fitted.push(fits);
    }

    Ok(fitted)
}

/// Checks how a text is placed or moved: its point finite, its opacity and its layer's depth each from 0 to 1, and its
/// clip a box of finite corners.
///
/// # Errors
///
/// Why it would not be drawn so.
pub fn check_placing(placing: Placing) -> Result<(), TextPlacementRefused> {
    let opaque = (0.0..=1.0).contains(&placing.opacity);
    if !opaque {
        return Err(TextPlacementRefused::Opacity);
    }
    if let Layer::At { depth } = placing.layer {
        let deep = (0.0..=1.0).contains(&depth);
        if !deep {
            return Err(TextPlacementRefused::Depth);
        }
    }
    let finite = placing.at.x.is_finite() && placing.at.y.is_finite();
    let clipped = match placing.clip {
        Clip::None => true,
        Clip::To(rect) => [rect.min.x, rect.min.y, rect.max.x, rect.max.y].iter().all(|corner| corner.is_finite()),
    };
    if !finite {
        return Err(TextPlacementRefused::At);
    }
    if !clipped {
        return Err(TextPlacementRefused::Clip);
    }

    Ok(())
}
