//! A handler's role word: what the item is to the actor, which says which trait its impl is of (ADR-0059).

use syn::{Attribute, Meta};

/// The role a method of the block plays, as its word says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// `#[request]`: mail of a kind that is answered, `Handles<K>`.
    Request,
    /// `#[tell]`: mail of a kind that is not answered, `Handles<K>`.
    Tell,
    /// `#[answer]`: the answer to an ask, `Receives<Answer<R, C>>`.
    Answer,
    /// `#[on]`: a stage, `Receives<On<S>>`.
    On,
    /// `#[ended]`: the end of an actor it watches, `Receives<Ended>`.
    Ended,
    /// `#[others]`: mail of a kind it lists no handler for, `Receives<Others>`.
    Others,
    /// `#[refused]`: mail of a kind its handler's guard refused, `refused` in that kind's `Handles<K>`.
    Refused,
}

/// Every role.
const ROLES: [Role; 7] = [Role::Request, Role::Tell, Role::Answer, Role::On, Role::Ended, Role::Others, Role::Refused];

/// The attributes of one word that the compiler gives a meaning, which a method may carry beside its role word. Any
/// other attribute of one word is read as a role word, so one misspelt is refused here, not passed on.
const KEPT: [&str; 13] = [
    "allow",
    "cfg",
    "cfg_attr",
    "cold",
    "deny",
    "deprecated",
    "doc",
    "expect",
    "forbid",
    "inline",
    "must_use",
    "track_caller",
    "warn",
];

/// What a method of the block carries.
pub enum Marked {
    /// A role word, so it is a handler.
    Role(Role),
    /// None: `begin`, `end`, or an ordinary method.
    Plain,
}

impl Role {
    /// Its word, as written in its attribute.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Tell => "tell",
            Self::Answer => "answer",
            Self::On => "on",
            Self::Ended => "ended",
            Self::Others => "others",
            Self::Refused => "refused",
        }
    }

    /// A handler of this role, as its diagnostics name it: "a `#[tell]`", "an `#[on]`".
    pub fn named(self) -> String {
        let word = self.word();
        let article = if word.starts_with(['a', 'e', 'o']) {
            "an"
        } else {
            "a"
        };

        format!("{article} `#[{word}]`")
    }

    /// What a handler of this role returns, as its diagnostics name it.
    pub const fn returns(self) -> &'static str {
        match self {
            Self::Request => "`Result<A, Fault>`, `Result<Later<A>, Fault>`, or `Result<Reply<A>, Fault>`",
            Self::Refused => "what its handler returns",
            Self::Tell | Self::Answer | Self::On | Self::Ended | Self::Others => "`Result<(), Fault>`",
        }
    }

    /// The role whose word `word` is, when it is one.
    fn of_word(word: &str) -> Option<Self> {
        ROLES.into_iter().find(|role| role.word() == word)
    }
}

/// The role word among `attrs`, taken out of them, leaving the rest. Two role words, a role word with arguments, and
/// an attribute of one word that is neither a role word nor one the compiler knows are refused at the attribute.
pub fn take(attrs: &mut Vec<Attribute>) -> syn::Result<Marked> {
    let mut marked = Marked::Plain;
    let mut kept = Vec::with_capacity(attrs.len());
    for attr in attrs.drain(..) {
        let Some(word) = attr.path().get_ident().map(ToString::to_string) else {
            kept.push(attr);
            continue;
        };
        if KEPT.contains(&word.as_str()) {
            kept.push(attr);
            continue;
        }
        let Some(role) = Role::of_word(&word) else {
            let message = format!(
                "`#[{word}]` is no role word: a handler is a `#[request]`, `#[tell]`, `#[answer]`, `#[on]`, \
                 `#[ended]`, `#[others]`, or `#[refused]`"
            );
            return Err(syn::Error::new_spanned(attr, message));
        };
        if let Marked::Role(first) = marked {
            let message = format!("a method carries one role word, and this one carries `#[{}]` already", first.word());
            return Err(syn::Error::new_spanned(attr, message));
        }
        if !matches!(attr.meta, Meta::Path(_)) {
            return Err(syn::Error::new_spanned(attr, format!("`#[{word}]` takes no arguments")));
        }
        marked = Marked::Role(role);
    }
    *attrs = kept;

    Ok(marked)
}
