//! A handler, read from its signature: what it is handed, the guard of its turn, and what it returns (ADR-0058,
//! ADR-0059).

use super::role::Role;
use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use syn::{FnArg, GenericArgument, ImplItemFn, PathArguments, ReturnType, Signature, Type, TypePath};

/// A method that carries a role word.
pub struct Handler {
    pub role: Role,
    /// The method, its role word taken out.
    pub method: ImplItemFn,
    /// What it is handed, its second parameter.
    pub handed: Handed,
    /// Its turn, its third parameter.
    pub turn: Turn,
    /// What it returns on `Ok`.
    pub answer: Type,
}

/// What a handler is handed, as its role reads its second parameter.
pub enum Handed {
    /// A kind of mail, of a `#[request]`, a `#[tell]`, or a `#[refused]`, or the item of a stage, of an `#[on]`.
    Kind(Box<Type>),
    /// The answer to an ask of `answer`, with the context the ask kept, of an `#[answer]`.
    Answer { answer: Box<Type>, context: Box<Type> },
    /// What the engine hands as it is, of an `#[ended]` or an `#[others]`.
    Item,
}

/// A handler's turn, by which it reads its guard.
pub enum Turn {
    /// It takes none, so it takes its mail from anyone.
    Absent,
    /// `Turn<'_, Self>`: from anyone.
    Unguarded,
    /// `Turn<'_, Self, G>`: from whoever the guard `G` admits.
    Guarded(Box<Type>),
}

impl Handler {
    /// The handler `method` is, of the role `role`, read from its signature.
    pub fn read(role: Role, method: ImplItemFn) -> syn::Result<Self> {
        let named = role.named();
        let sig = &method.sig;
        let mut inputs = sig.inputs.iter();

        let takes_self = matches!(inputs.next(), Some(FnArg::Receiver(receiver)) if is_mut_reference(receiver));
        if !takes_self {
            let message = format!("{named} takes `&mut self` first");
            return Err(syn::Error::new_spanned(&sig.ident, message));
        }
        let Some(FnArg::Typed(handed)) = inputs.next() else {
            let message = format!("{named} takes what it handles as its second parameter, after `&mut self`");
            return Err(syn::Error::new(sig.paren_token.span.join(), message));
        };
        let handed = Handed::read(role, &handed.ty)?;
        let turn = match inputs.next() {
            None => Turn::Absent,
            Some(FnArg::Typed(turn)) => Turn::read(&turn.ty),
            Some(receiver @ FnArg::Receiver(_)) => {
                return Err(syn::Error::new_spanned(receiver, "a handler takes `self` once, first"));
            }
        };
        if let Some(extra) = inputs.next() {
            let message = format!("{named} takes `&mut self`, what it handles, and its turn, and nothing more");
            return Err(syn::Error::new_spanned(extra, message));
        }
        let answer = answer(role, sig)?;

        Ok(Self { role, method, handed, turn, answer })
    }

    /// The kind it handles, for a role handed one.
    pub fn kind(&self) -> Option<&Type> {
        match &self.handed {
            Handed::Kind(kind) => Some(kind),
            Handed::Answer { .. } | Handed::Item => None,
        }
    }

    /// What its actor's `Handles` lists for it: the kind of a `#[request]` or a `#[tell]`, and what a `Receives` is of
    /// otherwise.
    pub fn listed(&self) -> TokenStream {
        match (&self.handed, self.role) {
            (Handed::Kind(kind), Role::Request | Role::Tell | Role::Refused) => kind.to_token_stream(),
            (Handed::Kind(stage), _) => quote!(::aethereal_actor::On<#stage>),
            (Handed::Answer { answer, context }, _) => quote!(::aethereal_actor::Answer<#answer, #context>),
            (Handed::Item, Role::Ended) => quote!(::aethereal_actor::Ended),
            (Handed::Item, _) => quote!(::aethereal_actor::Others),
        }
    }

    /// The guard of its turn.
    pub fn guard(&self) -> TokenStream {
        match &self.turn {
            Turn::Absent | Turn::Unguarded => quote!(::aethereal_actor::Anyone),
            Turn::Guarded(guard) => guard.to_token_stream(),
        }
    }

    /// Whether it is the `#[request]` or `#[tell]` of the kind written `kind`, which a `#[refused]` of it pairs with.
    pub fn handles_mail(&self, kind: &str) -> bool {
        let is_mail = matches!(self.role, Role::Request | Role::Tell);
        let is_kind = self.kind().is_some_and(|handled| handled.to_token_stream().to_string() == kind);

        is_mail && is_kind
    }

    /// Whether `refusal`, a `#[refused]` of the block, is of the kind this handles as mail.
    pub fn pairs(&self, refusal: &Self) -> bool {
        let kind = refusal.kind().map(|kind| kind.to_token_stream().to_string()).unwrap_or_default();

        self.handles_mail(&kind)
    }

    /// Whether it takes a turn.
    pub const fn takes_turn(&self) -> bool {
        !matches!(self.turn, Turn::Absent)
    }
}

impl Handed {
    /// What a handler of `role` is handed, read from its second parameter's type `ty`.
    fn read(role: Role, ty: &Type) -> syn::Result<Self> {
        let named = role.named();
        match role {
            Role::Request | Role::Tell | Role::Refused | Role::On => {
                let Type::Path(_) = ty else {
                    let message = format!("{named} handles the type of its second parameter, which names a kind");
                    return Err(syn::Error::new_spanned(ty, message));
                };

                Ok(Self::Kind(Box::new(ty.clone())))
            }
            Role::Answer => {
                let Type::Tuple(tuple) = ty else {
                    return Err(unanswered(ty));
                };
                let mut elements = tuple.elems.iter();
                let (Some(answered), Some(context), None) = (elements.next(), elements.next(), elements.next()) else {
                    return Err(unanswered(ty));
                };
                let Some(answer) = only_argument(answered, "Answered") else {
                    return Err(unanswered(answered));
                };

                Ok(Self::Answer { answer: Box::new(answer.clone()), context: Box::new(context.clone()) })
            }
            Role::Ended | Role::Others => Ok(Self::Item),
        }
    }
}

impl Turn {
    /// The turn whose type is `ty`, `&mut Turn<'_, Self>` or `&mut Turn<'_, Self, G>`: its guard is its second type
    /// argument, and `Anyone` when it names none.
    fn read(ty: &Type) -> Self {
        let Type::Reference(reference) = ty else {
            return Self::Unguarded;
        };
        let Type::Path(TypePath { path, .. }) = &*reference.elem else {
            return Self::Unguarded;
        };
        let Some(PathArguments::AngleBracketed(arguments)) = path.segments.last().map(|segment| &segment.arguments)
        else {
            return Self::Unguarded;
        };
        let guard = arguments
            .args
            .iter()
            .filter_map(|argument| match argument {
                GenericArgument::Type(ty) => Some(ty),
                _ => None,
            })
            .nth(1);

        guard.map_or(Self::Unguarded, |guard| Self::Guarded(Box::new(guard.clone())))
    }
}

/// What a handler of `role` with the signature `sig` returns on `Ok`, refusing a return type its role cannot take: a
/// `#[request]` answers, so returns no `()`, and a handler of any other role but `#[refused]` answers nothing.
fn answer(role: Role, sig: &Signature) -> syn::Result<Type> {
    let named = role.named();
    let ReturnType::Type(_, returned) = &sig.output else {
        let message = format!("{named} returns {}", role.returns());
        return Err(syn::Error::new_spanned(&sig.ident, message));
    };
    let Some(answer) = only_argument(returned, "Result") else {
        let message = format!("{named} returns {}", role.returns());
        return Err(syn::Error::new_spanned(returned, message));
    };
    let is_unit = matches!(answer, Type::Tuple(tuple) if tuple.elems.is_empty());

    match role {
        Role::Request if is_unit => {
            let message = "a `#[request]` answers its asker, and returns `Result<A, Fault>`, `Result<Later<A>, \
                           Fault>`, or `Result<Reply<A>, Fault>`: a handler that answers nothing is a `#[tell]`";
            Err(syn::Error::new_spanned(returned, message))
        }
        Role::Tell if !is_unit => {
            let message = "a `#[tell]` answers nothing, and returns `Result<(), Fault>`: a handler that answers is a \
                           `#[request]`";
            Err(syn::Error::new_spanned(returned, message))
        }
        Role::Answer | Role::On | Role::Ended | Role::Others if !is_unit => {
            let message = format!("{named} returns `Result<(), Fault>`: what the engine brings is not answered");
            Err(syn::Error::new_spanned(returned, message))
        }
        _ => Ok(answer.clone()),
    }
}

/// The first type argument of `ty` when it is a path whose last segment is `name`.
fn only_argument<'a>(ty: &'a Type, name: &str) -> Option<&'a Type> {
    let Type::Path(TypePath { path, .. }) = ty else {
        return None;
    };
    let last = path.segments.last()?;
    let PathArguments::AngleBracketed(arguments) = &last.arguments else {
        return None;
    };
    let is_named = last.ident == name;
    let first = arguments.args.iter().find_map(|argument| match argument {
        GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });

    first.filter(|_| is_named)
}

/// Whether `receiver` is `&mut self`.
const fn is_mut_reference(receiver: &syn::Receiver) -> bool {
    receiver.reference.is_some() && receiver.mutability.is_some()
}

/// The refusal of a second parameter of an `#[answer]` that is not `(Answered<R>, C)`.
fn unanswered(ty: impl ToTokens) -> syn::Error {
    let message = "an `#[answer]` is handed `(Answered<R>, C)`: the answer to an ask of `R`, and the context the ask \
                   kept";

    syn::Error::new_spanned(ty, message)
}
