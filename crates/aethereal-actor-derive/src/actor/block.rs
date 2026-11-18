//! An actor block, read: its `begin` and `end`, its handlers by their role words, and its other items, checked as a
//! whole (ADR-0059).

use super::handler::{Handed, Handler};
use super::role::{self, Marked, Role};
use quote::ToTokens;
use syn::{Attribute, FnArg, Generics, ImplItem, ImplItemFn, ItemImpl, Type, parse_quote};

/// An actor block, read and checked.
pub struct Block {
    /// The attributes on the block, which every impl it writes carries.
    pub attrs: Vec<Attribute>,
    pub generics: Generics,
    pub self_ty: Type,
    pub begin: Begin,
    pub end: End,
    /// Its handlers but its refusals, in the order written, which is the order its `Handles` lists them.
    pub handlers: Vec<Handler>,
    /// Its `#[refused]` methods, each paired with a handler of its kind.
    pub refusals: Vec<Handler>,
    /// What it holds that carries no role word, but `begin` and `end`: ordinary methods, and any other item.
    pub ordinary: Vec<ImplItem>,
}

/// The actor's `begin`, with a `Beginning` it takes when it was written with none.
pub struct Begin {
    pub method: ImplItemFn,
    /// The actor's config: the type of `begin`'s first parameter.
    pub config: Type,
}

/// The actor's `end`.
pub enum End {
    /// Written in the block, with an `Ending` it takes when it was written with none.
    Written(Box<ImplItemFn>),
    /// Left out: it ends with `Ok(())`.
    Default,
}

impl Block {
    /// The block `item`, read, refusing what an actor block cannot hold.
    pub fn read(item: ItemImpl) -> syn::Result<Self> {
        if let Some((_, path, _)) = &item.trait_ {
            let message = "`#[actor]` goes on the actor's own block, `impl Name { … }`, not on a trait's impl";
            return Err(syn::Error::new_spanned(path, message));
        }
        let ItemImpl { attrs, generics, self_ty, items, .. } = item;
        let mut begin = Vec::new();
        let mut end = Vec::new();
        let mut handlers = Vec::new();
        let mut refusals = Vec::new();
        let mut ordinary = Vec::new();
        for item in items {
            let ImplItem::Fn(mut method) = item else {
                ordinary.push(item);
                continue;
            };
            let marked = role::take(&mut method.attrs)?;
            let name = method.sig.ident.to_string();
            match (marked, name.as_str()) {
                (Marked::Role(_), "begin" | "end") => {
                    let message = format!("`{name}` is the actor's own, and carries no role word");
                    return Err(syn::Error::new_spanned(&method.sig.ident, message));
                }
                (Marked::Role(Role::Refused), _) => refusals.push(Handler::read(Role::Refused, method)?),
                (Marked::Role(role), _) => handlers.push(Handler::read(role, method)?),
                (Marked::Plain, "begin") => begin.push(method),
                (Marked::Plain, "end") => end.push(method),
                (Marked::Plain, _) => ordinary.push(ImplItem::Fn(method)),
            }
        }

        let begin = Begin::read(&self_ty, begin)?;
        let end = End::read(end)?;
        once_each(&handlers)?;
        paired(&handlers, &refusals)?;

        Ok(Self { attrs, generics, self_ty: *self_ty, begin, end, handlers, refusals, ordinary })
    }
}

impl Begin {
    /// The one `begin` of the block whose type is `self_ty`, of those written, `written`.
    fn read(self_ty: &Type, written: Vec<ImplItemFn>) -> syn::Result<Self> {
        let mut written = written.into_iter();
        let Some(mut method) = written.next() else {
            let message = "an actor block has a `begin`, which makes the actor from its config";
            return Err(syn::Error::new_spanned(self_ty, message));
        };
        if let Some(second) = written.next() {
            return Err(syn::Error::new_spanned(&second.sig.ident, "an actor block has one `begin`"));
        }
        let config = match method.sig.inputs.first() {
            Some(FnArg::Typed(config)) => (*config.ty).clone(),
            Some(FnArg::Receiver(receiver)) => {
                let message = "`begin` makes the actor, so it takes no `self`";
                return Err(syn::Error::new_spanned(receiver, message));
            }
            None => {
                let message = "`begin` takes the actor's config first, whose type is its `Config`";
                return Err(syn::Error::new_spanned(&method.sig.ident, message));
            }
        };
        if method.sig.inputs.len() == 1 {
            method.sig.inputs.push(parse_quote!(_: &mut ::aethereal_actor::Beginning<'_, Self>));
        }

        Ok(Self { method, config })
    }
}

impl End {
    /// The one `end` of the block, of those written, `written`.
    fn read(written: Vec<ImplItemFn>) -> syn::Result<Self> {
        let mut written = written.into_iter();
        let Some(mut method) = written.next() else {
            return Ok(Self::Default);
        };
        if let Some(second) = written.next() {
            return Err(syn::Error::new_spanned(&second.sig.ident, "an actor block has one `end`"));
        }
        let takes_self =
            matches!(method.sig.inputs.first(), Some(FnArg::Receiver(receiver)) if receiver.reference.is_none());
        if !takes_self {
            let message = "`end` takes the actor, `self`, first: it is the actor's closing turn";
            return Err(syn::Error::new_spanned(&method.sig.ident, message));
        }
        if method.sig.inputs.len() == 1 {
            method.sig.inputs.push(parse_quote!(_: &mut ::aethereal_actor::Ending<'_, Self>));
        }

        Ok(Self::Written(Box::new(method)))
    }
}

/// Refuses a second handler of what one handles already: a kind handled by two `#[request]`, `#[tell]`, or `#[on]`, an
/// answer to asks of one kind by two `#[answer]`, and a second `#[ended]` or `#[others]`.
fn once_each(handlers: &[Handler]) -> syn::Result<()> {
    let mut seen: Vec<String> = Vec::new();
    for handler in handlers {
        let (key, at) = handled_once(handler);
        if seen.contains(&key) {
            let message = format!("this block handles `{key}` already: a kind has one handler");
            return Err(syn::Error::new_spanned(at, message));
        }
        seen.push(key);
    }

    Ok(())
}

/// What `handler` handles that no other handler of the block may, and where to point when one does: the kind, or for an
/// `#[answer]` the kind of the answer, as dispatch hands an answer by its kind alone.
fn handled_once(handler: &Handler) -> (String, proc_macro2::TokenStream) {
    let at = handler.method.sig.inputs.iter().nth(1).map(ToTokens::to_token_stream).unwrap_or_default();
    let key = match &handler.handed {
        Handed::Kind(kind) => kind.to_token_stream().to_string(),
        Handed::Answer { answer, .. } => format!("Answered<{}>", answer.to_token_stream()),
        Handed::Item => handler.listed().to_string(),
    };

    (key, at)
}

/// Refuses a `#[refused]` with no `#[request]` or `#[tell]` of its kind in the block, and a second of one kind.
fn paired(handlers: &[Handler], refusals: &[Handler]) -> syn::Result<()> {
    let mut paired: Vec<String> = Vec::new();
    for refusal in refusals {
        let Some(kind) = refusal.kind() else {
            continue;
        };
        let key = kind.to_token_stream().to_string();
        let has_pair = handlers.iter().any(|handler| handler.handles_mail(&key));
        if !has_pair {
            let message =
                format!("a `#[refused]` of `{key}` pairs with a `#[request]` or `#[tell]` of it in this block");
            return Err(syn::Error::new_spanned(kind, message));
        }
        if paired.contains(&key) {
            let message = format!("`{key}` has a `#[refused]` already in this block: a kind has one");
            return Err(syn::Error::new_spanned(kind, message));
        }
        paired.push(key);
    }

    Ok(())
}
