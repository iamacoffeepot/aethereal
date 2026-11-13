//! What the markup refuses before anything is written (ADR-0056: Expanded by a macro): the shapes no element may take,
//! here, and what a reference or an event may name, which is each macro's own ([`Names`]): a screen's view reads
//! `self` and calls its methods, and a composite reads its params and passes on its `raises`.
//!
//! Each refusal is an error at the token that caused it; every one found is reported, not only the first.

use super::reference::{Head, Reference};
use super::{Call, Dropping, Element, Kind, Look, Outside, Repeat, Rest, Shown};
use crate::style;
use syn::Ident;

/// What one macro allows its markup to name.
pub trait Names {
    /// Refuses a reference whose head is not one this macro reads, with the locals in `scope` around it.
    fn head(&self, head: &Head, scope: &Scope) -> syn::Result<()>;

    /// Refuses an event that does not call what this macro's events call.
    fn event(&self, call: &Call) -> syn::Result<()>;

    /// Refuses a drop this macro cannot take.
    fn drop(&self, call: &Call) -> syn::Result<()>;

    /// Refuses a list in a `Scroll`, which builds only the rows its screen says it shows, where the macro cannot ask.
    fn rows(&self, scroll: &Element) -> syn::Result<()>;
}

/// The names in scope where a reference is read: the locals each `as` around it made, and a composite's params.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    names: Vec<Ident>,
}

impl Scope {
    /// The scope of a composite's params, `params`.
    pub fn of(params: &[Ident]) -> Self {
        Self { names: params.to_vec() }
    }

    /// Whether `name` is in it.
    pub fn holds(&self, name: &Ident) -> bool {
        self.names.contains(name)
    }

    /// It, and `local` within it.
    pub fn with(&self, local: &Ident) -> Self {
        let mut names = self.names.clone();
        names.push(local.clone());

        Self { names }
    }
}

/// The errors a check found, every one of them.
#[derive(Default)]
struct Found {
    errors: Vec<syn::Error>,
}

impl Found {
    /// Keeps the error of `checked`, if it is one.
    fn keep(&mut self, checked: syn::Result<()>) {
        if let Err(error) = checked {
            self.errors.push(error);
        }
    }

    /// Every error found, as one.
    fn finish(self) -> syn::Result<()> {
        let mut errors = self.errors.into_iter();
        let Some(mut first) = errors.next() else {
            return Ok(());
        };
        for error in errors {
            first.combine(error);
        }

        Err(first)
    }
}

/// Checks the markup whose root is `root`, its references and events as `names` allows them, from `scope`.
///
/// # Errors
///
/// Every refusal found, each at its token.
pub fn check(root: &Element, names: &impl Names, scope: &Scope) -> syn::Result<()> {
    let mut found = Found::default();
    let hidden = !matches!(root.shown, Shown::Always);
    if hidden {
        found.keep(Err(syn::Error::new(root.span, "the root is always shown: `show` belongs to an element within it")));
    }
    if let Kind::Child(keyword) = &root.kind {
        found.keep(Err(syn::Error::new(keyword.span(), "the root is a widget: a `child` slot stands within one")));
    }
    element(root, names, scope, &mut found);

    found.finish()
}

/// Checks `element` and what it holds, read in `scope`.
fn element(element: &Element, names: &impl Names, scope: &Scope, found: &mut Found) {
    let own = match &element.shown {
        Shown::Always => scope.clone(),
        Shown::While(condition) => {
            reference(condition, names, scope, found);
            scope.clone()
        }
        Shown::Bound(option, local) => {
            reference(option, names, scope, found);
            scope.with(local)
        }
    };
    match &element.kind {
        Kind::Child(_) => slot(element, names, &own, found),
        Kind::Widget(_) => widget(element, names, &own, found),
    }
}

/// Checks a widget's element, read in `scope`.
fn widget(element: &Element, names: &impl Names, scope: &Scope, found: &mut Found) {
    look(&element.look, found);
    for field in &element.fields {
        value(&field.value, names, scope, found);
    }
    for event in &element.events {
        call(&event.call, names, scope, found);
    }
    if let super::Drag::Carries(payload) = &element.drag {
        reference(payload, names, scope, found);
    }
    if let Dropping::Takes(taken) = &element.drop {
        found.keep(names.drop(taken));
        arguments(taken, names, scope, found);
    }
    if let Outside::Raises(outside) = &element.outside {
        let fixed = matches!(element.drag, super::Drag::Fixed);
        if fixed {
            found.keep(Err(syn::Error::new(outside.span(), "`drop_outside` belongs to an element with `drag`")));
        }
        call(outside, names, scope, found);
    }
    for hole in element.text.iter().flat_map(super::text::Text::holes) {
        reference(hole, names, scope, found);
    }
    let within = match &element.repeat {
        Repeat::Once => scope.clone(),
        Repeat::Items { list, local, key } => {
            reference(list, names, scope, found);
            let within = scope.with(local);
            reference(key, names, &within, found);
            let one = element.children.len() == 1 && element.text.is_none();
            if !one {
                let message = "`items` repeats one child, its template, once for each item: a header above the list \
                               is a sibling in an outer element";
                found.keep(Err(syn::Error::new(element.span, message)));
            }
            if element.is("Scroll") {
                found.keep(names.rows(element));
            }
            within
        }
    };
    if element.is("Tabs") {
        tabs(element, found);
    }
    for child in &element.children {
        self::element(child, names, &within, found);
    }
}

/// Checks a `Tabs`: it takes its open tab as `selected`, and holds a `Tab` for each, written rather than repeated,
/// whose `title` its headers show.
fn tabs(element: &Element, found: &mut Found) {
    let open = element.field("selected").is_some();
    if !open {
        found.keep(Err(syn::Error::new(element.span, "`Tabs` takes the open tab, `selected: …`")));
    }
    let repeated = matches!(element.repeat, Repeat::Items { .. });
    let empty = element.children.is_empty();
    if repeated || empty {
        let message = "`Tabs` holds a `Tab` for each tab, each written, its panel the `Tab`'s children";
        found.keep(Err(syn::Error::new(element.span, message)));
    }
    for tab in &element.children {
        let named = tab.is("Tab") && tab.field("title").is_some();
        if !named {
            found.keep(Err(syn::Error::new(tab.span, "each child of `Tabs` is a `Tab { title: …, … }`")));
        }
        let hidden = !matches!(tab.shown, Shown::Always);
        if hidden {
            found.keep(Err(syn::Error::new(
                tab.span,
                "a `Tab` is shown as its `Tabs`' `selected` says, never by `show`",
            )));
        }
    }
}

/// Checks a child actor's slot: it takes its `actor` and its `area`, and a `style` and a `show`, and holds nothing.
fn slot(element: &Element, names: &impl Names, scope: &Scope, found: &mut Found) {
    look(&element.look, found);
    for field in &element.fields {
        let known = field.name == "actor" || field.name == "area";
        if !known {
            let message = "a `child` slot takes `actor`, `area`, `style`, and `show`";
            found.keep(Err(syn::Error::new(field.name.span(), message)));
        }
        value(&field.value, names, scope, found);
    }
    for needed in ["actor", "area"] {
        let given = element.field(needed).is_some();
        if !given {
            let message = format!("a `child` slot takes `{needed}: …`");
            found.keep(Err(syn::Error::new(element.span, message)));
        }
    }
    if let Some(event) = element.events.first() {
        let message = "a `child` slot raises nothing: its actor talks to the screen by mail";
        found.keep(Err(syn::Error::new(event.name.span(), message)));
    }
    let repeated = matches!(element.repeat, Repeat::Items { .. });
    let dragged = matches!(element.drag, super::Drag::Carries(_));
    let dropped = matches!(element.drop, Dropping::Takes(_));
    let defaulted = matches!(element.rest, Rest::Defaulted(_));
    let holds = !element.children.is_empty() || element.text.is_some();
    let more = repeated || dragged || dropped || defaulted || holds;
    if more {
        let message = "a `child` slot holds nothing, and takes only `actor`, `area`, `style`, and `show`";
        found.keep(Err(syn::Error::new(element.span, message)));
    }
}

/// Checks the properties of a style an element extends for itself.
fn look(look: &Look, found: &mut Found) {
    if let Look::Extended(_, properties) = look {
        found.keep(style::properties(properties));
    }
}

/// Checks a setting's value.
fn value(value: &super::Value, names: &impl Names, scope: &Scope, found: &mut Found) {
    if let super::Value::Reference(read) = value {
        reference(read, names, scope, found);
    }
}

/// Checks an event's call, and its arguments.
fn call(called: &Call, names: &impl Names, scope: &Scope, found: &mut Found) {
    found.keep(names.event(called));
    arguments(called, names, scope, found);
}

/// Checks the arguments a call copies.
fn arguments(called: &Call, names: &impl Names, scope: &Scope, found: &mut Found) {
    if let Call::Method { arguments, .. } = called {
        for argument in arguments {
            reference(argument, names, scope, found);
        }
    }
}

/// Checks a reference, and each argument of its calls.
fn reference(read: &Reference, names: &impl Names, scope: &Scope, found: &mut Found) {
    found.keep(names.head(&read.head, scope));
    for argument in read.arguments() {
        reference(argument, names, scope, found);
    }
}
