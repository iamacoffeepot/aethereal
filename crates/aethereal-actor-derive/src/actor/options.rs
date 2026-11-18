//! What `#[actor(…)]` takes: where the actor stands, its key, the types it depends on and spawns, and the library whose
//! handlers it takes in (ADR-0059).

use core::mem;
use proc_macro2::TokenStream;
use syn::meta::{self, ParseNestedMeta};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{Path, Token, Type, parenthesized, parse_quote};

/// The options of an actor block, each its default when left out.
pub struct Options {
    /// Its key: `()` for a singleton.
    pub key: Type,
    /// The type it stands beneath: `Top` for a root.
    pub parent: Type,
    /// The types it reaches by typed path.
    pub depends: Vec<Type>,
    /// The types it spawns.
    pub spawns: Vec<Type>,
    /// Whose handlers it takes in beside its own.
    pub library: Library,
}

/// Whose handlers an actor takes in beside its own.
pub enum Library {
    /// Only its own.
    Own,
    /// The library module at this path, whose `handles!` and `depends!` merge its lists with the actor's.
    With(Path),
}

/// One option as it is written.
enum Written {
    Key(Type),
    Parent(Type),
    Depends(Vec<Type>),
    Spawns(Vec<Type>),
    With(Path),
}

impl Written {
    /// The option `meta` names, read.
    fn read(meta: &ParseNestedMeta<'_>) -> syn::Result<Self> {
        if meta.path.is_ident("key") {
            return Ok(Self::Key(meta.value()?.parse()?));
        }
        if meta.path.is_ident("parent") {
            return Ok(Self::Parent(meta.value()?.parse()?));
        }
        if meta.path.is_ident("depends") {
            return Ok(Self::Depends(types(meta)?));
        }
        if meta.path.is_ident("spawns") {
            return Ok(Self::Spawns(types(meta)?));
        }
        if meta.path.is_ident("with") {
            return Ok(Self::With(library(meta)?));
        }

        Err(meta.error("`#[actor]` takes `key = K`, `parent = P`, `depends(…)`, `spawns(…)`, and `with(…)`"))
    }

    /// Whether it is the same option as `other`, whatever each holds.
    fn same_option(&self, other: &Self) -> bool {
        mem::discriminant(self) == mem::discriminant(other)
    }
}

impl Options {
    /// The options `attribute` writes, refusing one written twice or one it does not know.
    pub fn parse(attribute: TokenStream) -> syn::Result<Self> {
        let mut written: Vec<Written> = Vec::new();
        meta::parser(|meta| {
            let option = Written::read(&meta)?;
            if written.iter().any(|earlier| earlier.same_option(&option)) {
                return Err(meta.error("an option of `#[actor]` is written once"));
            }
            written.push(option);

            Ok(())
        })
        .parse2(attribute)?;

        let mut options = Self {
            key: parse_quote!(()),
            parent: parse_quote!(::aethereal_actor::Top),
            depends: Vec::new(),
            spawns: Vec::new(),
            library: Library::Own,
        };
        for option in written {
            match option {
                Written::Key(key) => options.key = key,
                Written::Parent(parent) => options.parent = parent,
                Written::Depends(depends) => options.depends = depends,
                Written::Spawns(spawns) => options.spawns = spawns,
                Written::With(library) => options.library = Library::With(library),
            }
        }

        Ok(options)
    }
}

/// The types in the parentheses after `meta`'s name.
fn types(meta: &ParseNestedMeta<'_>) -> syn::Result<Vec<Type>> {
    let content;
    parenthesized!(content in meta.input);
    let types = Punctuated::<Type, Token![,]>::parse_terminated(&content)?;

    Ok(types.into_iter().collect())
}

/// The one library module in the parentheses after `with`.
fn library(meta: &ParseNestedMeta<'_>) -> syn::Result<Path> {
    let content;
    parenthesized!(content in meta.input);
    let paths = Punctuated::<Path, Token![,]>::parse_terminated(&content)?;
    let mut paths = paths.into_iter();
    let Some(library) = paths.next() else {
        return Err(meta.error("`with(…)` names the library module whose handlers the actor takes in"));
    };
    if let Some(second) = paths.next() {
        return Err(syn::Error::new_spanned(second, "an actor takes in one library's handlers, `with(library)`"));
    }

    Ok(library)
}
