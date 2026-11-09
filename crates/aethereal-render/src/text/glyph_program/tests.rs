//! The glyph program's WGSL, as the renderer's checks read a program's, and the uniforms a placement writes.

use super::*;
use crate::program::parsed;

#[test]
fn the_glyph_program_is_valid_wgsl_with_its_two_stages() {
    // Catches WGSL that only a device that compiles it would refuse, as the device the renderer's tests draw on does
    // not: the glyph program is made at the first placed text, and an invalid one fails every `Render` after.
    let parsed = parsed(GLYPH);
    let Ok((module, _)) = parsed else {
        panic!("the glyph program is valid: {:?}", parsed.err());
    };
    let entries: Vec<&str> = module.entry_points.iter().map(|entry| entry.name.as_str()).collect();
    assert_eq!(entries, [VERTEX, FRAGMENT]);
}

#[test]
fn a_placement_s_uniforms_lie_where_the_glyph_program_reads_them() {
    // Catches the bytes a placement writes laid out otherwise than WGSL lays out `Placed`, which it does by its own
    // rules: a field moved in one and not the other draws text at another place, clipped wrong, or not at all.
    let Ok((module, _)) = parsed(GLYPH) else {
        panic!("the glyph program is valid");
    };
    let members = module.types.iter().find_map(|(_, ty)| match &ty.inner {
        naga::TypeInner::Struct { members, span } if ty.name.as_deref() == Some("Placed") => Some((members, *span)),
        _ => None,
    });
    let Some((members, span)) = members else {
        panic!("the glyph program reads a `Placed`");
    };
    assert_eq!(span as usize, PLACED_BYTES);

    let placing = Placing {
        at: Vec2::new(3.0, 4.0),
        layer: Layer::At { depth: 0.25 },
        opacity: 0.5,
        clip: Clip::To(aethereal_math::Rect2 { min: Vec2::new(5.0, 6.0), max: Vec2::new(7.0, 8.0) }),
    };
    let outline = Outline::Of { width: 1.5, colour: Colour { red: 0.1, green: 0.2, blue: 0.3, alpha: 0.4 } };
    let bytes = placed(Size { width: 640, height: 480 }, placing, outline);
    let read = |offset: u32, count: usize| -> Vec<f32> {
        (0..count)
            .map(|at| {
                let start = offset as usize + at * 4;
                f32::from_ne_bytes([bytes[start], bytes[start + 1], bytes[start + 2], bytes[start + 3]])
            })
            .collect()
    };
    let expected: [(&str, Vec<f32>); 8] = [
        ("frame", vec![640.0, 480.0]),
        ("at", vec![3.0, 4.0]),
        ("clip", vec![5.0, 6.0, 7.0, 8.0]),
        ("rim", vec![0.1, 0.2, 0.3, 0.4]),
        ("opacity", vec![0.5]),
        ("depth", vec![0.25]),
        ("rim_width", vec![1.5]),
        ("span", vec![8.0]),
    ];
    for (name, values) in expected {
        let Some(member) = members.iter().find(|member| member.name.as_deref() == Some(name)) else {
            panic!("`Placed` has `{name}`");
        };
        assert_eq!(read(member.offset, values.len()), values, "`{name}` at {}", member.offset);
    }
}
