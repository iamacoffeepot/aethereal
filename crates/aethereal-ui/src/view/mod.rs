//! What a screen is written as (ADR-0056: A screen is an actor; Events call the screen's methods; Focus and keys): the
//! [`View`] its markup compiles into, and the [`Controls`] written by hand beside the methods its events call. The
//! handlers [`screen!`](crate::screen!) writes read both, and take in what input leaves with [`take_in`].
//!
//! ```text
//! inventory.rs: view! ── macro ───> impl View for Inventory { type Event = InventoryEvent; fn view; fn on }
//! inventory.rs  ── by hand ───────> impl Controls for Inventory { type Sends; const HOTKEYS; fn key }
//!                                   fn equip(&mut self, turn, item: ItemId) -> Result<(), Fault>   (what `on` calls)
//! ```

mod method;
#[cfg(test)]
mod tests;

pub use method::{Handed, Method};

use crate::drag::{Dropped, Drops};
use crate::input::{Event, Key, Raised};
use crate::screen::{Heard, ImeAsked};
use crate::tree::Tree;
use aethereal_actor::{Actor, Allows, Asks, DeclaredBy, Fault, Level, Path, Sends, ToAny, Turn, path};
use aethereal_window::{AllowIme, Windows};

/// What a screen writes by hand beside the methods its events call: what those methods and its `key` may send, the
/// keys it hears of its own, and what it does with each.
pub trait Controls: Actor {
    /// What the screen's event methods and its `key` may send: [`sends!`](crate::sends!) of its own sends, which adds
    /// the library's ask of the window's input method, its drop mailed to another actor, and its refusal of a drop it
    /// takes nothing of. A method an event calls takes `&mut Turn<'_, Self, Self::Sends>`.
    type Sends: Sends + DeclaredBy<Self>;

    /// The keys `key` handles, by their places on the keyboard (`"KeyI"`, `"Escape"`): the screen's list declares them,
    /// blocking, while no widget that types has its focus. Written beside `key`, whose `match` reads them.
    const HOTKEYS: &'static [&'static str] = &[];

    /// The keys `key` takes before the focused widget hears them, by their places on the keyboard: a key that must
    /// reach the screen whatever has its focus, as a console's toggle does while its line has focus, or its Tab, which
    /// completes the line rather than moving focus. The text such a key types is dropped. Listing a key here declares
    /// nothing: it reaches the screen as its list declares it, among [`Self::HOTKEYS`], or as every key while a widget
    /// that types has focus.
    const FIRST: &'static [&'static str] = &[];

    /// Takes in a key pressed that neither the focused widget nor the focus keys used: one of [`Self::HOTKEYS`], or,
    /// while a widget that types has focus, any key it passed; and each of [`Self::FIRST`], before the focus hears it.
    ///
    /// # Errors
    ///
    /// A fault when a send was not written.
    fn key(&mut self, turn: &mut Turn<'_, Self, Self::Sends>, key: Key) -> Result<(), Fault> {
        let _ = (turn, key);

        Ok(())
    }
}

/// What a screen's markup compiles into: the enum of its events, its view, and the call of the method each event
/// names. [`view!`](crate::view!) writes it, and nothing else in the same impl:
///
/// ```text
/// // what view! { Inventory, ui, … } in inventory.rs expands to
/// pub enum InventoryEvent {
///     Select(<Inventory as __InventoryEventArgs>::select_0),   // ItemSlot { click => self.select(item.id) }
///     Rename(<Inventory as __InventoryEventArgs>::rename_0),   // TextInput { change => self.rename(item.id) }
/// }                                                            //   each argument's type, as #[events] declares it
///
/// impl aethereal_ui::View for Inventory {
///     type Event = InventoryEvent;
///
///     fn view(&self) -> Tree { /* the tree, each event built from the arguments copied now */ }
///
///     fn on(&mut self, turn: &mut Turn<'_, Self, Self::Sends>, raised: Raised<InventoryEvent>) -> Result<(), Fault> {
///         match raised.event() {
///             InventoryEvent::Select(item) => Method::call(Self::select, self, turn, (item,), raised),
///             InventoryEvent::Rename(item) => Method::call(Self::rename, self, turn, (item,), raised),
///         }   // select(turn, item) on a click; rename(turn, item, text) on a change, the text its hand
///     }
/// }
/// ```
pub trait View: Controls {
    /// One variant for each event of the markup, holding the arguments the view copied as it was built.
    type Event: Event;

    /// Builds the screen's tree from the screen as it is.
    fn view(&self) -> Tree;

    /// Calls the method `raised` names, with its arguments, then the value its widget handed with it.
    ///
    /// # Errors
    ///
    /// A fault when a send of the method's was not written.
    fn on(&mut self, turn: &mut Turn<'_, Self, Self::Sends>, raised: Raised<Self::Event>) -> Result<(), Fault>;
}

/// Logs an event that came with a value no widget hands with it, which only a tree built by hand can raise: what a
/// generated [`View::on`] answers past its own arms.
///
/// # Errors
///
/// None: it is a result as the arms beside it are.
pub fn unmatched<A, S, E: Event>(turn: &mut Turn<'_, A, S>, raised: &Raised<E>) -> Result<(), Fault> {
    turn.log(Level::Error, format!("an event came with a value its method does not take: {raised:?}"));

    Ok(())
}

/// Whether taking in a turn of input called any of the screen's methods.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Took {
    /// None: no event was raised, and no key was left for the screen.
    Nothing,
    /// One or more: what its view builds may have changed.
    Called,
}

/// Takes in `heard`, what a turn of input left, in `screen`'s handler: asks the window's input method what it said,
/// logs its lines, and mails each drag dropped on another actor that takes drops, its payload put as a blob; then calls
/// the method each raised event names, in the order raised, a drag's `drop_outside` last when the actor it was dropped
/// on takes no drops or its payload was not put, and then the screen's `key` with each key left for it. An event of
/// another screen's type, which no markup raises, is logged.
///
/// # Errors
///
/// A fault when a send was not written.
pub fn take_in<A, I, J>(screen: &mut A, heard: Heard, turn: &mut Turn<'_, A, A::Sends>) -> Result<Took, Fault>
where
    A: View,
    A::Sends: Allows<Asks<Path<Windows>, AllowIme, ImeAsked>, I> + Allows<ToAny<Drops, Dropped>, J>,
{
    let Heard { mut raised, keys, imes, away, lines } = heard;
    for (level, line) in lines {
        turn.log(level, line);
    }
    for allow in imes {
        turn.to(path::<Windows>(())).ask(&allow, &ImeAsked)?;
    }
    for away in away {
        let Some(target) = turn.cast::<Drops>(away.to) else {
            raised.extend(away.outside);
            continue;
        };
        let Ok(blob) = turn.put(away.payload.bytes()) else {
            turn.log(Level::Error, "a drop was not mailed: its payload was not put");
            raised.extend(away.outside);
            continue;
        };
        let dropped =
            Dropped { payload: away.payload.carried(blob), entry: away.entry, at: away.at, source: away.source };
        turn.to(target).send(&dropped)?;
    }
    let quiet = raised.is_empty() && keys.is_empty();
    let took = if quiet {
        Took::Nothing
    } else {
        Took::Called
    };
    for raised in raised {
        match raised.typed::<A::Event>() {
            Ok(raised) => screen.on(turn, raised)?,
            Err(other) => turn.log(Level::Error, format!("a widget raised another screen's event: {other:?}")),
        }
    }
    for key in keys {
        screen.key(turn, key)?;
    }

    Ok(took)
}
