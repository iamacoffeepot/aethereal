//! Styles written out: each a function that builds the runtime's `Style` over the one it starts from, each property's
//! value read by its shape.
//!
//! ```text
//! padding: 8               ──> padding: Edges::all(Space::Pixels(8.0))
//! columns: 6               ──> columns: vec![Track::Fraction(1.0); 6]
//! colour: rgb(232, 217, 160) ──> colour: aethereal_ui::rgb(232, 217, 160)
//! background: "#1c1814"    ──> background: aethereal_ui::rgba(28, 24, 20, 255)
//! align_items: Alignment::Center ──> align_items: Into::into(Alignment::Center)
//! ```

use super::check::{Shape, shape};
use super::{Base, Definition, Property};
use crate::spans;
use proc_macro2::{Literal, Span, TokenStream};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Expr, ExprCall, ExprLit, ExprUnary, Lit, LitStr, UnOp};

/// Each of `definitions`, a function.
pub fn definitions(definitions: &[Definition]) -> TokenStream {
    definitions.iter().map(definition).collect()
}

/// `definition`, a function that builds its style.
fn definition(definition: &Definition) -> TokenStream {
    let Definition { attributes, visibility, name, base, properties } = definition;
    let base = match base {
        Base::Default => quote!(<::aethereal_ui::Style as ::core::default::Default>::default()),
        Base::Style(path) => quote_spanned!(spans::at(path.span())=> #path()),
    };
    let body = extended(&base, properties);

    quote_spanned!(spans::at(name.span())=>
        #(#attributes)*
        #[must_use]
        #visibility fn #name() -> ::aethereal_ui::Style {
            #body
        }
    )
}

/// The style `base` builds, with `properties` set over it.
pub fn extended(base: &TokenStream, properties: &[Property]) -> TokenStream {
    if properties.is_empty() {
        return base.clone();
    }
    let set = properties.iter().map(|property| {
        let name = &property.name;
        let value = value(property);
        quote!(#name: #value)
    });

    quote!(::aethereal_ui::Style { #(#set,)* ..#base })
}

/// `property`'s value, as its field takes it.
fn value(property: &Property) -> TokenStream {
    let expression = &property.value;
    let span = spans::at(expression.span());
    let Some(shape) = shape(&property.name.to_string()) else {
        return quote_spanned!(span=> ::core::compile_error!("a style has no such property"));
    };
    if let Some(colour) = colour(expression, shape) {
        return colour;
    }
    let Some(number) = number(expression) else {
        return quote_spanned!(span=> ::core::convert::Into::into(#expression));
    };
    let pixels = number.float(span);
    match shape {
        Shape::Length => quote_spanned!(span=> ::aethereal_ui::Length::Pixels(#pixels)),
        Shape::Margin => quote_spanned!(span=> ::aethereal_ui::Edges::all(::aethereal_ui::Length::Pixels(#pixels))),
        Shape::Padding => quote_spanned!(span=> ::aethereal_ui::Edges::all(::aethereal_ui::Space::Pixels(#pixels))),
        Shape::Gap => quote_spanned!(span=> ::aethereal_ui::Gap {
            rows: ::aethereal_ui::Space::Pixels(#pixels),
            columns: ::aethereal_ui::Space::Pixels(#pixels),
        }),
        Shape::Number => pixels,
        Shape::Tracks => {
            let count = number.whole(span);
            quote_spanned!(span=> ::std::vec![::aethereal_ui::Track::Fraction(1.0); #count])
        }
        Shape::Placed => {
            let line = number.whole(span);
            quote_spanned!(span=> ::aethereal_ui::Placed::At { line: #line, span: 1 })
        }
        Shape::Colour | Shape::Own => quote_spanned!(span=> ::core::convert::Into::into(#expression)),
    }
}

/// A number written in a style: its digits, and whether it is negative.
struct Number {
    digits: String,
    negative: bool,
    whole: bool,
}

impl Number {
    /// It as an `f32` literal.
    fn float(&self, span: Span) -> TokenStream {
        let digits = if self.whole {
            format!("{}.0", self.digits)
        } else {
            self.digits.clone()
        };
        let literal: TokenStream = digits.parse().unwrap_or_else(|_| quote!(0.0));
        let literal = respanned(literal, span);
        let sign = self.sign(span);

        quote!(#sign #literal)
    }

    /// It as an unsuffixed integer literal, or an error at `span` for one that is not whole.
    fn whole(&self, span: Span) -> TokenStream {
        if !self.whole {
            return quote_spanned!(span=> ::core::compile_error!("a count of tracks or a grid line is a whole number"));
        }
        let literal: TokenStream = self.digits.parse().unwrap_or_else(|_| quote!(0));
        let literal = respanned(literal, span);
        let sign = self.sign(span);

        quote!(#sign #literal)
    }

    /// Its sign, if it is negative.
    fn sign(&self, span: Span) -> TokenStream {
        if self.negative {
            quote_spanned!(span=> -)
        } else {
            quote!()
        }
    }
}

/// `tokens`, each placed at `span`.
fn respanned(tokens: TokenStream, span: Span) -> TokenStream {
    tokens
        .into_iter()
        .map(|mut tree| {
            tree.set_span(span);
            tree
        })
        .collect()
}

/// The number `expression` writes, if it is a number literal, signed or not.
fn number(expression: &Expr) -> Option<Number> {
    match expression {
        Expr::Lit(ExprLit { lit: Lit::Int(int), .. }) => {
            Some(Number { digits: String::from(int.base10_digits()), negative: false, whole: true })
        }
        Expr::Lit(ExprLit { lit: Lit::Float(float), .. }) => {
            Some(Number { digits: String::from(float.base10_digits()), negative: false, whole: false })
        }
        Expr::Unary(ExprUnary { op: UnOp::Neg(_), expr, .. }) => {
            number(expr).map(|number| Number { negative: !number.negative, ..number })
        }
        _ => None,
    }
}

/// The colour `expression` writes for a property of `shape`, if it writes one the macro reads: a call of `rgb` or
/// `rgba`, named as the library's, or a string of hex digits.
fn colour(expression: &Expr, shape: Shape) -> Option<TokenStream> {
    let span = spans::at(expression.span());
    match expression {
        Expr::Call(ExprCall { func, args, .. }) => {
            let Expr::Path(path) = &**func else {
                return None;
            };
            let named = path.path.get_ident()?;
            let library = named == "rgb" || named == "rgba";
            library.then(|| quote_spanned!(span=> ::aethereal_ui::#named(#args)))
        }
        Expr::Lit(ExprLit { lit: Lit::Str(hex), .. }) if shape == Shape::Colour => Some(hexed(hex)),
        _ => None,
    }
}

/// The colour of a string of hex digits, `"#rrggbb"` or `"#rrggbbaa"`, or an error at it.
fn hexed(hex: &LitStr) -> TokenStream {
    let span = spans::at(hex.span());
    let value = hex.value();
    let digits = value.strip_prefix('#').unwrap_or(&value);
    let sized = digits.len() == 6 || digits.len() == 8;
    let channels: Option<Vec<u8>> = (0..digits.len())
        .step_by(2)
        .map(|at| digits.get(at..at + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok()))
        .collect();
    let Some(mut channels) = channels.filter(|_| sized) else {
        return quote_spanned!(span=> ::core::compile_error!("a colour string is `\"#rrggbb\"` or `\"#rrggbbaa\"`"));
    };
    if channels.len() == 3 {
        channels.push(u8::MAX);
    }
    let channels = channels.into_iter().map(Literal::u8_unsuffixed);

    quote_spanned!(span=> ::aethereal_ui::rgba(#(#channels),*))
}
