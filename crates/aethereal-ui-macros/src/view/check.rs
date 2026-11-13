//! What a screen's markup may name, and the events it compiles into: one variant of its event enum for each method its
//! events call, holding the arguments each copies.
//!
//! A method called by two events is one variant, so every call of it copies as many arguments, and is a widget's event
//! or a drop alike: a drop's method takes the payload after them, and a widget's event's the value its widget hands.

use crate::markup::{Call, Dropping, Element, Head, Names, Outside, Scope};
use syn::Ident;

/// What a screen's markup names: `self`, its locals, and Rust items, and its events call its methods.
pub struct Screen;

impl Names for Screen {
    fn head(&self, _: &Head, _: &Scope) -> syn::Result<()> {
        Ok(())
    }

    fn event(&self, call: &Call) -> syn::Result<()> {
        match call {
            Call::Method { .. } => Ok(()),
            Call::Raise(name) => {
                let message = "an event of a screen calls one of its methods: `click => self.select(item.id)`";
                Err(syn::Error::new(name.span(), message))
            }
        }
    }

    fn drop(&self, call: &Call) -> syn::Result<()> {
        self.event(call)
    }

    fn rows(&self, _: &Element) -> syn::Result<()> {
        Ok(())
    }
}

/// One variant of a screen's event enum: the method it calls, how many arguments the view copies for it, and what
/// comes with it.
pub struct Variant {
    pub method: Ident,
    pub arguments: usize,
    pub taking: Taking,
}

/// What a variant's method takes after the arguments the view copied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taking {
    /// What its widget hands with the event, if it hands anything: a text, a flag, a number, or a choice.
    Handed,
    /// The payload of a drag: a drop's, or a `drop_outside`'s.
    Payload,
}

/// The variants of the events in the markup whose root is `root`, in the order their methods are first called.
///
/// # Errors
///
/// A method called with two counts of arguments, or by a widget's event and a drop.
pub fn variants(root: &Element) -> syn::Result<Vec<Variant>> {
    let mut variants: Vec<Variant> = Vec::new();
    let mut stack = vec![root];
    while let Some(element) = stack.pop() {
        let handed = element.events.iter().map(|event| (&event.call, Taking::Handed));
        let dropped = match &element.drop {
            Dropping::Refuses => None,
            Dropping::Takes(call) => Some((call, Taking::Payload)),
        };
        let outside = match &element.outside {
            Outside::Nothing => None,
            Outside::Raises(call) => Some((call, Taking::Payload)),
        };
        for (call, taking) in handed.chain(dropped).chain(outside) {
            let Call::Method { method, arguments, .. } = call else {
                continue;
            };
            let variant = Variant { method: method.clone(), arguments: arguments.len(), taking };
            match variants.iter().find(|known| known.method == variant.method) {
                Some(known) => known.agrees(&variant)?,
                None => variants.push(variant),
            }
        }
        stack.extend(element.children.iter().rev());
    }

    Ok(variants)
}

impl Variant {
    /// Refuses `other`, a later call of the same method, if it disagrees with this one.
    fn agrees(&self, other: &Self) -> syn::Result<()> {
        let counted = self.arguments == other.arguments;
        let taken = self.taking == other.taking;
        if !counted {
            let message = format!(
                "`{}` is called with {} arguments here and {} before: each call of a method copies as many",
                other.method, other.arguments, self.arguments
            );
            return Err(syn::Error::new(other.method.span(), message));
        }
        if !taken {
            let message = format!(
                "`{}` is called by a widget's event and by a drop: a drop's method takes the payload last",
                other.method
            );
            return Err(syn::Error::new(other.method.span(), message));
        }

        Ok(())
    }
}
