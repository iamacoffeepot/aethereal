//! A screen written in markup builds the tree its expansion is written to build: the same tree written by hand, node
//! for node, so a wrong place among siblings, a wrong key, a wrong event variant or its arguments, a composite built
//! wrong, a hidden element counted out of its siblings' places, or a scroll's rows asked wrong fails it.

use crate::prelude::*;
use crate::widget::{Axis, Lines};
use crate::widgets::Text;
use aethereal_actor::{Actor, Asks, Beginning, ChildOf, Ending, Fault, Path, Top, Turn};
use aethereal_data::{Bound, Budget, Plain, from_bytes};
use aethereal_render::Renderer;
use aethereal_render::frame::{Frame, ShapeOf};
use aethereal_render::pass::Interface;

/// What a ware is told apart by, and what a drag of one carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.ware_id")]
pub struct WareId(u32);

impl From<WareId> for ItemKey {
    fn from(id: WareId) -> Self {
        Self::from(id.0)
    }
}

/// A ware on sale, in the slot it stands in.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.test.ware")]
pub struct Ware {
    id: WareId,
    name: String,
    slot: u32,
}

/// A shop: its wares, the one chosen, and the settings its widgets show.
#[derive(Bound)]
#[kind(name = "aether.test.shop")]
pub struct Shop {
    wares: Vec<Ware>,
    chosen: WareId,
    open: bool,
    query: String,
    muted: bool,
    volume: f32,
    tab: u32,
    ui: Screen,
}

aethereal_ui::style! {
    pub panel { padding: 8, gap: 4 }
    pub title: panel { font_size: 24, colour: rgb(232, 217, 160) }
    pub cells { background: "#1c1814" }
}

aethereal_ui::widget! {
    /// A ware's slot: a button labelled with its name.
    pub struct WareSlot { ware: Ware }
    raises click;

    Button { click => click,
        Text { "{ware.name}" }
    }
}

aethereal_ui::view! { Shop, ui,
    Column { style: panel,
        Text { style: title, "Wares: {self.wares.len()}" }
        Grid { style: cells { columns: 3 }, items: self.wares as ware key ware.id,
            WareSlot {
                ware: ware,
                click => self.select(ware.id),
                drag: ware.id,
                drop => self.place(ware.slot),
                drop_outside => self.discard(),
            }
        }
        Column { show: self.chosen() as ware,
            Text { "{ware.name}" }
        }
        Text { show: self.open, "{{open}}" }
        TextInput { text: self.query, change => self.search(), submit: None }
        Toggle { on: self.muted, change => self.mute() }
        Slider { value: self.volume, min: 0.0, max: 1.0, step: 0.25, change => self.set_volume() }
        Tabs { selected: self.tab, change => self.open_tab(),
            Tab { title: "Wares", Text { "wares" } }
            Tab { title: "Sold", Text { "sold" } }
        }
        Scroll { .., items: self.wares as ware key ware.id,
            Text { "{ware.name}" }
        }
    }
}

/// What the shop's methods send: none of their own.
type ShopSends = aethereal_ui::sends![];

impl Controls for Shop {
    type Sends = ShopSends;
}

#[aethereal_ui::events]
impl Shop {
    fn chosen(&self) -> Option<&Ware> {
        self.wares.iter().find(|ware| ware.id == self.chosen)
    }

    fn select(&mut self, _: &mut Turn<'_, Self, ShopSends>, ware: WareId) -> Result<(), Fault> {
        self.chosen = ware;
        Ok(())
    }

    fn place(&mut self, _: &mut Turn<'_, Self, ShopSends>, slot: u32, ware: WareId) -> Result<(), Fault> {
        self.wares.iter_mut().filter(|held| held.id == ware).for_each(|held| held.slot = slot);
        Ok(())
    }

    fn discard(&mut self, _: &mut Turn<'_, Self, ShopSends>, ware: WareId) -> Result<(), Fault> {
        self.wares.retain(|held| held.id != ware);
        Ok(())
    }

    fn search(&mut self, _: &mut Turn<'_, Self, ShopSends>, query: String) -> Result<(), Fault> {
        self.query = query;
        Ok(())
    }

    fn mute(&mut self, _: &mut Turn<'_, Self, ShopSends>, on: bool) -> Result<(), Fault> {
        self.muted = on;
        Ok(())
    }

    fn set_volume(&mut self, _: &mut Turn<'_, Self, ShopSends>, volume: f32) -> Result<(), Fault> {
        self.volume = volume;
        Ok(())
    }

    fn open_tab(&mut self, _: &mut Turn<'_, Self, ShopSends>, tab: u32) -> Result<(), Fault> {
        self.tab = tab;
        Ok(())
    }
}

impl ChildOf for Shop {
    type Parent = Top;
}

impl Actor for Shop {
    type Config = Frame;
    type Key = ();
    type Handles = aethereal_ui::handles![];
    type BeginSends = (Asks<Path<Renderer>, ShapeOf, Shaping>,);
    type EndSends = ();

    fn begin(frame: Frame, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let ui = Screen::begin(Target::of::<Interface>(frame), Viewport::window(1, 1, 1.0), turn)?;

        Ok(shop(ui))
    }

    fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
        Ok(())
    }
}

/// A shop of an apple and a pear, the pear chosen, its other settings each set apart from its default, over `ui`.
fn shop(ui: Screen) -> Shop {
    let apple = Ware { id: WareId(1), name: String::from("Apple"), slot: 4 };
    let pear = Ware { id: WareId(2), name: String::from("Pear"), slot: 5 };

    Shop {
        wares: vec![apple, pear],
        chosen: WareId(2),
        open: false,
        query: String::from("pe"),
        muted: true,
        volume: 0.5,
        tab: 1,
        ui,
    }
}

/// A screen never laid out, so a scroll builds its first row alone.
fn unlaid() -> Screen {
    let Ok(window) = from_bytes(&0_u64.to_le_bytes(), &mut Budget::new(8)) else {
        panic!("a window's id is its number");
    };

    Screen::new(Target::of::<Interface>(Frame::Main(window)), Viewport::window(200, 100, 1.0))
}

/// A node of `widget` at `segment`, unstyled, over `children`.
fn node<W: crate::Widget>(segment: Segment, widget: W, children: Vec<Node>) -> Node {
    Node::new(segment, widget, Style::default(), children)
}

/// A text of `text` at `segment`, unstyled.
fn text(segment: Segment, text: &str) -> Node {
    node(segment, Text::new(text), vec![])
}

/// The slot of a ware, keyed by its id: the composite's button, dragged with its id and taking drops into its slot.
fn slot(ware: WareId, name: &str, slot: u32) -> Node {
    let key = Segment::Item(ItemKey::from(ware));
    let label = text(Segment::Place(0), name);

    node(key, Button::new(ShopEvent::Select(ware)), vec![label])
        .with_drag(DragSource::new(ware).outside(ShopEvent::Discard))
        .with_drop(DropTarget::new(ShopEvent::Place(slot), Shop::place))
}

/// The shop's tree, as its markup is written to build it: each place counted in the markup, the hidden `Text` at 3
/// among them, each item at its key, the open tab's panel alone at its place, and the scroll's first row alone.
fn by_hand() -> Tree {
    let grid = Style { columns: vec![Track::Fraction(1.0); 3], ..cells() };
    let tabs =
        Tabs { titles: vec![String::from("Wares"), String::from("Sold")], selected: 1, change: ShopEvent::OpenTab };
    let sold = node(Segment::Place(1), Tab { title: String::from("Sold") }, vec![text(Segment::Place(0), "sold")]);
    let scroll = Scroll { axis: Axis::Column, lines: Lines::Window { count: 2, at: vec![0] } };
    let children = vec![
        Node::new(Segment::Place(0), Text::new("Wares: 2"), title(), vec![]),
        Node::new(Segment::Place(1), Grid, grid, vec![slot(WareId(1), "Apple", 4), slot(WareId(2), "Pear", 5)]),
        node(Segment::Place(2), Column, vec![text(Segment::Place(0), "Pear")]),
        node(
            Segment::Place(4),
            TextInput { text: String::from("pe"), change: ShopEvent::Search, submit: None },
            vec![],
        ),
        node(Segment::Place(5), Toggle::new(true, ShopEvent::Mute), vec![]),
        node(Segment::Place(6), Slider::new(0.5, 0.0, 1.0, 0.25, ShopEvent::SetVolume), vec![]),
        node(Segment::Place(7), tabs, vec![sold]),
        node(Segment::Place(8), scroll, vec![text(Segment::Item(ItemKey::from(WareId(1))), "Apple")]),
    ];

    Tree::new(Node::new(Segment::Place(0), Column, panel(), children))
}

#[test]
fn a_view_in_markup_builds_the_tree_written_by_hand() {
    let shop = shop(unlaid());

    assert_eq!(View::view(&shop), by_hand());
}
