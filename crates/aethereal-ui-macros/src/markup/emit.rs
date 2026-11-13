//! Markup written out as the runtime's own calls (ADR-0056: Lists and conditions are settings; Child actors; Drag and
//! drop is one path; Stock widgets): each element a `Node`, built by `Element`, which places a primitive as itself and
//! a composite as the tree its own markup builds.
//!
//! ```text
//! Grid { style: slots, items: self.items as item key item.id, ItemSlot { item: item } }
//!     ──> Element::styled(Grid {}, Segment::Place(1), slots(), {
//!             let mut children = Vec::new();
//!             for item in &self.items {
//!                 let key = ItemKey::from(item.id);
//!                 children.push(Element::node(ItemSlot { item: item.clone() }, Segment::Item(key), Vec::new()));
//!             }
//!             children
//!         })
//! ```
//!
//! A node's segment is its place among its siblings in the markup, counted from zero, whether or not a sibling before
//! it is shown, or the key of its item; the root's is `Segment::Place(0)`. A `Tabs` builds the open tab's panel alone,
//! at that tab's place.
//!
//! An element with `items` asks its type whether it windows them (`Element::WINDOWS`, which a primitive declares as
//! `Widget::WINDOWS`), so the choice is the type's, never its name's. In a screen both ways are written, and the
//! compiler folds the one the type's constant leaves out: a type that windows asks its screen which rows to build by
//! its own key path, written here from its place in the markup and the keys of the items around it, and takes the
//! lines they stand at; any other builds every item. A composite has no screen to ask, so it builds every item, and
//! one whose type windows them is refused as the composite is built.
//!
//! ```text
//! Scroll { .., items: self.items as item key item.id, Text { "{item.name}" } }
//!     ──> let (widget, items) = {
//!             let widget = Scroll { ..Default::default() };
//!             if windows(&widget) {
//!                 let (lines, rows) = { self.ui.rows(&KeyPath::of(&[…]), len) … rows.finish() };
//!                 (Element::window(widget, lines), rows)
//!             } else {
//!                 (widget, { every item })
//!             }
//!         };
//!         Element::node(widget, Segment::Place(2), items)
//! ```
//!
//! Each event is set through `Into`, so a field the widget must raise, `E`, and one it may, `Raises<E>`, are written
//! alike, and the widget is named as raising the screen's event type, or a composite's `E`, through `raising`, which
//! tells a generic widget the type `Into` converts to.

use super::check::Scope;
use super::reference::{Head, Reference};
use super::{Call, Drag, Dropping, Element, Kind, Look, Memo, Outside, Repeat, Rest, Shown, Value};
use crate::spans;
use crate::style;
use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote, quote_spanned};
use syn::{Ident, Path};

/// Whose markup is written: what its events compile into, and whom its lists ask which rows they show.
pub enum Owner<'markup> {
    /// A screen's view: its event enum, each event a variant of it, and the field that holds its `Screen`.
    Screen { events: &'markup Ident, field: &'markup Ident },
    /// A composite's tree, built from its params: each event one of its `raises`, passed on.
    Composite,
}

/// Writes out markup as `owner`'s.
pub struct Emitter<'markup> {
    owner: Owner<'markup>,
}

/// Where a node stands: its segment among its siblings, and the segments from the root to it, the root's first.
struct Place {
    segment: TokenStream,
    path: Vec<TokenStream>,
}

/// The style a node is built with.
enum Styling {
    /// The style its markup names.
    Own,
    /// The style its caller hands it: a composite's root, styled where the composite is placed.
    Given(TokenStream),
}

/// What follows a node's own children.
enum Appended {
    Nothing,
    /// The children a composite's caller hands it, after its root's own.
    After(TokenStream),
}

impl Place {
    /// The place of a node at `segment` within this one.
    fn within(&self, segment: TokenStream) -> Self {
        let mut path = self.path.clone();
        path.push(segment.clone());

        Self { segment, path }
    }
}

impl<'markup> Emitter<'markup> {
    /// Writes out markup as `owner`'s.
    pub const fn new(owner: Owner<'markup>) -> Self {
        Self { owner }
    }

    /// The node of a screen's root, at `Segment::Place(0)`.
    pub fn root(&self, root: &Element) -> TokenStream {
        let segment = quote!(::aethereal_ui::Segment::Place(0));
        let place = Place { path: vec![segment.clone()], segment };

        self.node(root, &place, &Scope::default(), &Styling::Own, &Appended::Nothing)
    }

    /// The node of a composite's root: at `segment`, styled `style`, its own children followed by `appended`, its
    /// references read in `scope`, its params.
    pub fn composite(
        &self,
        root: &Element,
        scope: &Scope,
        segment: &Ident,
        style: &Ident,
        appended: &Ident,
    ) -> TokenStream {
        let place = Place { segment: quote!(#segment), path: Vec::new() };

        self.node(root, &place, scope, &Styling::Given(quote!(#style)), &Appended::After(quote!(#appended)))
    }

    /// The style `look` names, as an expression of the runtime's `Style`.
    pub fn look(look: &Look) -> TokenStream {
        match look {
            Look::Unstyled => quote!(<::aethereal_ui::Style as ::core::default::Default>::default()),
            Look::Named(path) => quote_spanned!(spans::at(span_of(path))=> #path()),
            Look::Extended(path, properties) => {
                let base = quote_spanned!(spans::at(span_of(path))=> #path());
                style::extended(&base, properties)
            }
        }
    }

    /// The node of `element` at `place`, read in `scope`.
    fn node(
        &self,
        element: &Element,
        place: &Place,
        scope: &Scope,
        styling: &Styling,
        appended: &Appended,
    ) -> TokenStream {
        match &element.kind {
            Kind::Child(keyword) => Self::slot(element, keyword, place, scope),
            Kind::Widget(path) => self.widget(element, path, place, scope, styling, appended),
        }
    }

    /// The slot of a child actor: `Node::call`, its actor copied from the screen.
    fn slot(element: &Element, keyword: &Ident, place: &Place, scope: &Scope) -> TokenStream {
        let segment = &place.segment;
        let actor = field(element, "actor", scope);
        let area = field(element, "area", scope);
        let style = Self::look(&element.look);

        quote_spanned!(spans::at(keyword.span())=> ::aethereal_ui::Node::call(#segment, #actor, #area, #style))
    }

    /// The node of a widget's element: its widget built from its fields and events, its children, and its drag and
    /// drop.
    fn widget(
        &self,
        element: &Element,
        path: &Path,
        place: &Place,
        scope: &Scope,
        styling: &Styling,
        appended: &Appended,
    ) -> TokenStream {
        let built = if element.is("Tabs") {
            self.tabs(element, place, scope)
        } else {
            match &element.repeat {
                Repeat::Once => self.children(element, place, scope),
                Repeat::Items { list, local, key } => self.items(element, place, scope, (list, local, key)),
            }
        };
        let Built { before, extra, children, listed } = built;
        let children = match appended {
            Appended::Nothing => children,
            Appended::After(appended) => quote!({
                let mut all: ::std::vec::Vec<::aethereal_ui::Node> = #children;
                all.extend(#appended);
                all
            }),
        };
        let skipped = if element.is("Tabs") {
            "selected"
        } else {
            ""
        };
        let mut entries: Vec<TokenStream> = element
            .fields
            .iter()
            .filter(|field| field.name != skipped)
            .map(|field| {
                let name = &field.name;
                let value = value(&field.value, scope);
                quote!(#name: #value)
            })
            .collect();
        entries.extend(element.events.iter().map(|event| {
            let name = &event.name;
            let value = self.event(&event.call);
            quote_spanned!(spans::at(event.call.span())=> #name: ::core::convert::Into::into(#value))
        }));
        entries.extend(element.text.iter().map(|text| {
            let string = text.string();
            quote_spanned!(spans::at(text.literal.span())=> text: #string)
        }));
        entries.extend(extra);
        let rest = match element.rest {
            Rest::Written => quote!(),
            Rest::Defaulted(span) => quote_spanned!(spans::at(span)=> ..::core::default::Default::default()),
        };
        let span = spans::at(span_of(path));
        let literal = quote_spanned!(span=> #path { #(#entries,)* #rest });
        let widget = if element.events.is_empty() {
            literal
        } else {
            let raised = self.raised();
            quote_spanned!(span=> ::aethereal_ui::raising::<#raised, _>(#literal))
        };
        if let (Memo::Kept(_), Owner::Screen { field, .. }) = (&element.memo, &self.owner) {
            return self.memo(element, path, place, scope, field, &widget);
        }
        let Listing { widget, children, built } = listing(widget, children, listed, span);
        let segment = &place.segment;
        let node = match (styling, &element.look) {
            (Styling::Own, Look::Unstyled) => {
                quote_spanned!(span=> ::aethereal_ui::Element::node(#widget, #segment, #children))
            }
            (Styling::Own, look) => {
                let style = Self::look(look);
                quote_spanned!(span=> ::aethereal_ui::Element::styled(#widget, #segment, #style, #children))
            }
            (Styling::Given(style), _) => {
                quote_spanned!(span=> ::aethereal_ui::Element::styled(#widget, #segment, #style, #children))
            }
        };
        let drag = self.drag(element);
        let drop = self.drop(element);

        quote!({ #before #built #node #drag #drop })
    }

    /// The node of `element`, a composite of the type at `path` marked `memo` in a screen's view, at `place`, read in
    /// `scope`, its screen's library in `field`, built as `widget`: the screen keeps it while each input its markup
    /// writes, a field read from the screen or a local, an event, and its text, is the same as the one it was last built
    /// from at its key path, and its style is too. A literal, and a field left at its default, is the same at each build
    /// of one place in the markup, which the key path names, so it is not compared.
    ///
    /// ```text
    /// Message { memo, message: message, click => self.open(message.id) }
    ///     ──> self.ui.memo(&[…path], Look::Own, || Message { message: message.clone(), click: … },
    ///             |&Message { message: ref a0, click: ref a1, .. }| same(a0, Borrow::borrow(&message)) && same(a1, &…))
    /// ```
    fn memo(
        &self,
        element: &Element,
        path: &Path,
        place: &Place,
        scope: &Scope,
        field: &Ident,
        widget: &TokenStream,
    ) -> TokenStream {
        let span = spans::at(span_of(path));
        let segments = &place.path;
        let look = match &element.look {
            Look::Unstyled => quote!(::aethereal_ui::Look::Own),
            look => {
                let style = Self::look(look);
                quote!(::aethereal_ui::Look::Given(#style))
            }
        };
        let mut bound = Vec::new();
        let mut compared = Vec::new();
        let mut bind = |name: &Ident, now: TokenStream| {
            let local = format_ident!("memo_{}", bound.len(), span = spans::hidden());
            bound.push(quote!(#name: ref #local));
            compared.push(quote_spanned!(spans::at(name.span())=> ::aethereal_ui::same(#local, #now)));
        };
        for field in &element.fields {
            if let Value::Reference(read) = &field.value {
                let now = if item(read, scope) {
                    quote!(&#read)
                } else {
                    quote_spanned!(spans::at(read.span())=> ::core::borrow::Borrow::borrow(&#read))
                };
                bind(&field.name, now);
            }
        }
        for event in &element.events {
            let value = self.event(&event.call);
            bind(&event.name, quote_spanned!(spans::at(event.call.span())=> &::core::convert::Into::into(#value)));
        }
        if let Some(text) = &element.text {
            let holes = text.holes().next().is_some();
            if holes {
                let string = text.string();
                bind(&format_ident!("text", span = text.literal.span()), quote!(&#string));
            }
        }
        let same = if compared.is_empty() {
            quote!(true)
        } else {
            quote!(#(#compared)&&*)
        };

        quote_spanned!(span=>
            self.#field.memo(&[#(#segments),*], #look, || #widget, |&#path { #(#bound,)* .. }| #same)
        )
    }

    /// The type an element's events convert to: the screen's event enum, or a composite's `E`.
    fn raised(&self) -> TokenStream {
        match &self.owner {
            Owner::Screen { events, .. } => quote!(#events),
            Owner::Composite => quote!(E),
        }
    }

    /// The `.with_drag` of a dragged element, its `drop_outside` with it.
    fn drag(&self, element: &Element) -> TokenStream {
        let Drag::Carries(payload) = &element.drag else {
            return quote!();
        };
        let outside = match &element.outside {
            Outside::Nothing => quote!(),
            Outside::Raises(call) => {
                let event = self.event(call);
                quote_spanned!(spans::at(call.span())=> .outside(#event))
            }
        };

        quote_spanned!(spans::at(payload.span())=> .with_drag(::aethereal_ui::DragSource::new(#payload) #outside))
    }

    /// The `.with_drop` of an element that takes drops: its event, and the method that names the payload's kind.
    fn drop(&self, element: &Element) -> TokenStream {
        let Dropping::Takes(call) = &element.drop else {
            return quote!();
        };
        let event = self.event(call);
        let method = match call {
            Call::Method { method, .. } => quote_spanned!(spans::at(method.span())=> Self::#method),
            Call::Raise(name) => quote_spanned!(spans::at(name.span())=>
                ::core::compile_error!("a composite takes no drops: a drop calls a screen's method")
            ),
        };

        quote_spanned!(spans::at(call.span())=> .with_drop(::aethereal_ui::DropTarget::new(#event, #method)))
    }

    /// The children of an element that builds each once, at its place in the markup.
    fn children(&self, element: &Element, place: &Place, scope: &Scope) -> Built {
        if element.children.is_empty() {
            return Built::children(quote!(::std::vec::Vec::new()));
        }
        let children = format_ident!("children", span = spans::hidden());
        let pushes = element.children.iter().zip(0_u32..).map(|(child, at)| {
            let at = Literal::u32_unsuffixed(at);
            let place = place.within(quote!(::aethereal_ui::Segment::Place(#at)));
            self.shown(child, &place, scope, |node| quote!(#children.push(#node);))
        });

        Built::children(quote!({
            let mut #children = ::std::vec::Vec::new();
            #(#pushes)*
            #children
        }))
    }

    /// The children of an element whose `items` repeat its one child: every item, each at its key, and, in a screen,
    /// the rows its screen says it shows, for a type that windows them.
    fn items(&self, element: &Element, place: &Place, scope: &Scope, (list, local, key): Items<'_>) -> Built {
        let children = format_ident!("children", span = spans::hidden());
        let keyed = key_of(place);
        let within = scope.with(local);
        let template = place.within(quote!(::aethereal_ui::Segment::Item(#keyed)));
        let push = self.template(element, &template, &within, |node| quote!(#children.push(#node);));
        let every = quote!({
            let mut #children = ::std::vec::Vec::new();
            for #local in &#list {
                let #keyed = ::aethereal_ui::ItemKey::from(#key);
                #push
            }
            #children
        });
        let listed = match &self.owner {
            Owner::Screen { field, .. } => {
                Listed::Windowed(self.rows(element, place, scope, field, (list, local, key)))
            }
            Owner::Composite => Listed::Whole,
        };

        Built { before: TokenStream::new(), extra: Vec::new(), children: every, listed }
    }

    /// The rows of a screen's list whose type windows them: only those its screen says it shows, asked by the list's
    /// own key path, and the lines they stand at, `(Lines, Vec<Node>)`.
    fn rows(
        &self,
        element: &Element,
        place: &Place,
        scope: &Scope,
        field: &Ident,
        (list, local, key): Items<'_>,
    ) -> TokenStream {
        let [path, source, rows, at] =
            ["path", "source", "rows", "at"].map(|name| format_ident!("{name}", span = spans::hidden()));
        let keyed = key_of(place);
        let within = scope.with(local);
        let template = place.within(quote!(::aethereal_ui::Segment::Item(#keyed)));
        let push = self.template(element, &template, &within, |node| quote!(#rows.push(#at, #node);));
        let segments = &place.path;

        quote!({
            let #path = ::aethereal_ui::KeyPath::of(&[#(#segments),*]);
            let #source = &#list;
            let mut #rows = self.#field.rows(&#path, #source.len());
            for (#at, #local) in ::core::iter::IntoIterator::into_iter(#source).enumerate() {
                let #keyed = ::aethereal_ui::ItemKey::from(#key);
                if #rows.builds(#at, #keyed) {
                    #push
                }
            }
            #rows.finish()
        })
    }

    /// The panel of a `Tabs`' open tab, at that tab's place, and the titles of every tab, from their `Tab`s.
    fn tabs(&self, element: &Element, place: &Place, scope: &Scope) -> Built {
        let [selected, panels] = ["selected", "panels"].map(|name| format_ident!("{name}", span = spans::hidden()));
        let open = field(element, "selected", scope);
        let titles = element.children.iter().map(|tab| field(tab, "title", scope));
        let arms = element.children.iter().zip(0_u32..).map(|(tab, at)| {
            let at = Literal::u32_unsuffixed(at);
            let place = place.within(quote!(::aethereal_ui::Segment::Place(#at)));
            let node = self.node(tab, &place, scope, &Styling::Own, &Appended::Nothing);
            quote!(#at => ::std::vec![#node],)
        });
        let before = quote!(
            let #selected = #open;
            let #panels = match #selected {
                #(#arms)*
                _ => ::std::vec::Vec::new(),
            };
        );
        let extra = vec![quote!(titles: ::std::vec![#(#titles),*]), quote!(selected: #selected)];

        Built { before, extra, children: quote!(#panels), listed: Listed::Fixed }
    }

    /// The one child of an element with `items`, its template, built at `place` and handed to `push`.
    fn template(
        &self,
        element: &Element,
        place: &Place,
        scope: &Scope,
        push: impl Fn(TokenStream) -> TokenStream,
    ) -> TokenStream {
        let [template] = element.children.as_slice() else {
            return quote!(::core::compile_error!("`items` repeats one child"););
        };

        self.shown(template, place, scope, push)
    }

    /// `child`'s node at `place`, handed to `push` while it is shown: always, while its `bool` is true, or while its
    /// `Option` is `Some`, its value named by its local within it.
    fn shown(
        &self,
        child: &Element,
        place: &Place,
        scope: &Scope,
        push: impl Fn(TokenStream) -> TokenStream,
    ) -> TokenStream {
        match &child.shown {
            Shown::Always => push(self.node(child, place, scope, &Styling::Own, &Appended::Nothing)),
            Shown::While(condition) => {
                let pushed = push(self.node(child, place, scope, &Styling::Own, &Appended::Nothing));
                quote_spanned!(spans::at(condition.span())=> if #condition { #pushed })
            }
            Shown::Bound(option, local) => {
                let within = scope.with(local);
                let pushed = push(self.node(child, place, &within, &Styling::Own, &Appended::Nothing));
                let source = if option.ends_in_call() {
                    quote!(#option)
                } else {
                    quote!(&#option)
                };
                quote_spanned!(spans::at(option.span())=>
                    if let ::core::option::Option::Some(#local) = #source { #pushed }
                )
            }
        }
    }

    /// What an event's field holds: in a screen, the variant of its event enum its method names, holding the
    /// arguments copied now; in a composite, the event it passes on.
    fn event(&self, call: &Call) -> TokenStream {
        match (&self.owner, call) {
            (Owner::Screen { events, .. }, Call::Method { method, arguments, .. }) => {
                let variant = variant(method);
                let span = spans::at(method.span());
                if arguments.is_empty() {
                    quote_spanned!(span=> #events::#variant)
                } else {
                    quote_spanned!(span=> #events::#variant(#(#arguments),*))
                }
            }
            (Owner::Composite, Call::Raise(name)) => quote!(#name),
            (Owner::Screen { .. }, Call::Raise(name)) => quote_spanned!(spans::at(name.span())=>
                ::core::compile_error!("an event of a screen calls one of its methods")
            ),
            (Owner::Composite, Call::Method { method, .. }) => quote_spanned!(spans::at(method.span())=>
                ::core::compile_error!("a composite has no methods: it passes on one of its `raises`")
            ),
        }
    }
}

/// The value of `element`'s field `name`, which the check found it sets.
fn field(element: &Element, name: &str, scope: &Scope) -> TokenStream {
    element.field(name).map_or_else(
        || quote!(::core::compile_error!("a field the markup needs is not set")),
        |field| value(&field.value, scope),
    )
}

/// A setting's value as its field takes it: a string literal turned into the field's type, another literal as it is,
/// a Rust item as it is, and a reference into the screen or a local cloned, so the widget holds its own.
fn value(value: &Value, scope: &Scope) -> TokenStream {
    match value {
        Value::Text(literal) => quote_spanned!(spans::at(literal.span())=> ::core::convert::Into::into(#literal)),
        Value::Literal(literal) => literal.clone(),
        Value::Reference(read) if item(read, scope) => quote!(#read),
        Value::Reference(read) => quote_spanned!(spans::at(read.span())=> #read.clone()),
    }
}

/// An element's list, its local, and its key, as its `items` names them.
type Items<'markup> = (&'markup Reference, &'markup Ident, &'markup Reference);

/// What a widget's node is built from, besides its own settings: statements written before it, fields of its widget
/// the macro sets, the expression of its children, and whether they are a list's items.
struct Built {
    before: TokenStream,
    extra: Vec<TokenStream>,
    children: TokenStream,
    listed: Listed,
}

impl Built {
    /// Children alone, with nothing before them and no field set.
    fn children(children: TokenStream) -> Self {
        Self { before: TokenStream::new(), extra: Vec::new(), children, listed: Listed::Fixed }
    }
}

/// Whether an element's children are a list's items, and so how its widget is finished.
enum Listed {
    /// They are not: its children are those its markup writes.
    Fixed,
    /// They are a composite's list, every item built: its widget is refused as the composite is built when its type
    /// windows its items.
    Whole,
    /// They are a screen's list: every item built, or, when its widget's type windows them, the rows and lines this
    /// expression asks the screen for.
    Windowed(TokenStream),
}

/// A widget finished as its children's listing says: the expression of the widget and of its children, after the
/// statements `built`.
struct Listing {
    widget: TokenStream,
    children: TokenStream,
    built: TokenStream,
}

/// `widget` and `children` finished as `listed` says, written at `span`: a screen's list asks the widget's type
/// whether it windows, and both ways are written, the compiler folding the one its constant leaves out.
fn listing(widget: TokenStream, children: TokenStream, listed: Listed, span: proc_macro2::Span) -> Listing {
    match listed {
        Listed::Fixed => Listing { widget, children, built: TokenStream::new() },
        Listed::Whole => {
            let widget = quote_spanned!(span=> ::aethereal_ui::whole(#widget));
            Listing { widget, children, built: TokenStream::new() }
        }
        Listed::Windowed(rows) => {
            let [windowed, items, lines, shown] =
                ["windowed", "items", "lines", "shown"].map(|name| format_ident!("{name}", span = spans::hidden()));
            let built = quote_spanned!(span=>
                let (#windowed, #items) = {
                    let #windowed = #widget;
                    if ::aethereal_ui::windows(&#windowed) {
                        let (#lines, #shown) = #rows;
                        (::aethereal_ui::Element::window(#windowed, #lines), #shown)
                    } else {
                        (#windowed, #children)
                    }
                };
            );
            Listing { widget: quote!(#windowed), children: quote!(#items), built }
        }
    }
}

/// The local of the key of an item built within the node at `place`, numbered by how deep it stands, so a list within
/// an item still names the keys of the items around it.
fn key_of(place: &Place) -> Ident {
    format_ident!("key_{}", place.path.len(), span = spans::hidden())
}

/// Whether `read` names a Rust item, which is written as it is: a path of more than one segment, or a name that is no
/// local or param in `scope`.
fn item(read: &Reference, scope: &Scope) -> bool {
    match &read.head {
        Head::This(_) => false,
        Head::Name(name) => !scope.holds(name),
        Head::Path(_) => true,
    }
}

/// The variant of a screen's event enum that calls `method`: its name in upper camel case.
pub fn variant(method: &Ident) -> Ident {
    let name: String = method
        .to_string()
        .split('_')
        .map(|word| {
            let mut chars = word.chars();
            chars.next().map(|first| first.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
        })
        .collect();

    Ident::new(&name, method.span())
}

/// Where a path is written, by its last segment, the widget's or style's own name.
fn span_of(path: &Path) -> proc_macro2::Span {
    path.segments.last().map_or_else(proc_macro2::Span::call_site, |last| last.ident.span())
}
