//! The properties a style may set: each field of the runtime's `Style`, by the shape its value takes. A number is
//! read by that shape, so `padding: 8` is eight pixels on every side; any other value is the field's own type, or
//! turns into it.

use super::{Definition, Property};

/// How a property's value is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A `Length`: a number is pixels.
    Length,
    /// The `Edges<Length>` of a margin: a number is pixels on every side.
    Margin,
    /// The `Edges<Space>` of a padding: a number is pixels on every side.
    Padding,
    /// A `Gap`: a number is pixels between rows and between columns.
    Gap,
    /// An `f32`.
    Number,
    /// A grid's `Vec<Track>`: a number is that many equal columns or rows.
    Tracks,
    /// A grid's `Placed`: a number is the line its child starts at, one track across.
    Placed,
    /// A `Colour`: `rgb(…)`, `rgba(…)`, or `"#rrggbb"` or `"#rrggbbaa"`.
    Colour,
    /// The field's own type: a `bool`, an alignment, a border, a font, an outline.
    Own,
}

/// The shape of the property `name`, if `Style` has a field of that name.
pub fn shape(name: &str) -> Option<Shape> {
    let shape = match name {
        "width" | "height" | "min_width" | "min_height" | "max_width" | "max_height" | "basis" => Shape::Length,
        "margin" => Shape::Margin,
        "padding" => Shape::Padding,
        "gap" => Shape::Gap,
        "grow" | "shrink" | "radius" | "font_size" => Shape::Number,
        "columns" | "rows" => Shape::Tracks,
        "column" | "row" => Shape::Placed,
        "background" | "colour" => Shape::Colour,
        "wrap" | "justify_content" | "align_content" | "align_items" | "justify_items" | "align_self"
        | "justify_self" | "border" | "font" | "text_align" | "outline" => Shape::Own,
        _ => return None,
    };

    Some(shape)
}

/// Refuses each property of each of `definitions` that names no field of `Style`.
pub fn definitions(definitions: &[Definition]) -> syn::Result<()> {
    properties(definitions.iter().flat_map(|definition| &definition.properties))
}

/// Refuses each of `properties` that names no field of `Style`, every one found.
pub fn properties<'style>(properties: impl IntoIterator<Item = &'style Property>) -> syn::Result<()> {
    let mut refused =
        properties.into_iter().filter(|property| shape(&property.name.to_string()).is_none()).map(|property| {
            let message = format!("a style has no property `{}`", property.name);
            syn::Error::new(property.name.span(), message)
        });
    let Some(mut first) = refused.next() else {
        return Ok(());
    };
    for error in refused {
        first.combine(error);
    }

    Err(first)
}
