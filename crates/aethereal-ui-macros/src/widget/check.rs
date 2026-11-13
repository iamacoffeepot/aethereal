//! What a composite may name (ADR-0056: Expanded by a macro): it sees its own declarations, so it refuses a name it
//! does not declare. Its references read its params and its locals, and Rust items; its events pass on its `raises`;
//! and it has no `self` and no methods. It has no screen to ask which rows a list shows either, which its type, not its
//! name, says it needs, so that refusal is the type's, as the composite is built (`aethereal_ui::whole`).

use super::Composite;
use crate::markup::{self, Call, Head, Names, Scope};
use syn::{Fields, Ident};

/// What a composite's markup names: its params, its locals, and its `raises`.
struct Declared<'composite> {
    raises: &'composite [Ident],
}

/// The params of `composite`, once its declarations and its markup are checked.
///
/// # Errors
///
/// Generics, unnamed fields, a raise declared twice or named as a param, and each refusal of its markup.
pub fn composite(composite: &Composite) -> syn::Result<Vec<Ident>> {
    let item = &composite.item;
    let generic = !item.generics.params.is_empty();
    if generic {
        let message = "a composite takes no generics: the screen's event type is added by the macro";
        return Err(syn::Error::new_spanned(&item.generics, message));
    }
    let params: Vec<Ident> = match &item.fields {
        Fields::Named(named) => named.named.iter().filter_map(|field| field.ident.clone()).collect(),
        Fields::Unit => Vec::new(),
        Fields::Unnamed(unnamed) => {
            return Err(syn::Error::new_spanned(
                unnamed,
                "a composite's params are named: `struct Slot { item: Item }`",
            ));
        }
    };
    for (at, raise) in composite.raises.iter().enumerate() {
        let twice = composite.raises[..at].contains(raise);
        let param = params.contains(raise);
        if twice || param {
            return Err(syn::Error::new(raise.span(), format!("`{raise}` is declared twice")));
        }
    }
    markup::check(&composite.root, &Declared { raises: &composite.raises }, &Scope::of(&params))?;

    Ok(params)
}

impl Names for Declared<'_> {
    fn head(&self, head: &Head, scope: &Scope) -> syn::Result<()> {
        match head {
            Head::This(this) => {
                let message = "a composite has no `self`: its references are its params and the locals its `as` makes";
                Err(syn::Error::new(this.span, message))
            }
            Head::Name(name) => {
                let declared = scope.holds(name);
                let item = name.to_string().starts_with(char::is_uppercase);
                if declared || item {
                    return Ok(());
                }
                let message =
                    format!("the composite has no param `{name}`: its references are its params and its locals");
                Err(syn::Error::new(name.span(), message))
            }
            Head::Path(_) => Ok(()),
        }
    }

    fn event(&self, call: &Call) -> syn::Result<()> {
        match call {
            Call::Raise(name) if self.raises.contains(name) => Ok(()),
            Call::Raise(name) => {
                let message = format!("the composite raises no `{name}`: declare it, `raises {name};`");
                Err(syn::Error::new(name.span(), message))
            }
            Call::Method { this, .. } => {
                let message =
                    "a composite has no methods: it passes an event on as one of its `raises`, `click => click`";
                Err(syn::Error::new(this.span, message))
            }
        }
    }

    fn drop(&self, call: &Call) -> syn::Result<()> {
        let message = "a composite takes no drops: a drop calls a screen's method, which names the payload's kind; give \
                       `drop` where the composite is placed";
        Err(syn::Error::new(call.span(), message))
    }
}
