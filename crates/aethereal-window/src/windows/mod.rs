//! The window root: the capability that holds the one source its host brings in what every window hears through, and
//! the controller it asks the host for windows through; it makes each window's actor, sends each event on to the
//! actor of the window it was heard in, and answers each actor that asked it to open or close a window, to set the
//! mode one is shown in, or to let its input method compose, once its host has (ADR-0024, ADR-0027, ADR-0051). It
//! also asks its host to show the cursor each window's actor asks for, which nothing answers (ADR-0055).

mod asked;
#[cfg(test)]
mod tests;

pub use asked::{
    AllowIme, CloseWindow, Closed, ImeAllowed, ListWindows, Listed, Logical, ModeSet, MoveWindow, Moved, OpenWindow,
    Opened, Place, ResizeWindow, Resized, SetMode, Unclosed, Unopened, Unplaced, Unset, WindowList,
};

use crate::cursor::{Cursor, ShowCursor};
use crate::event::{Event, Mode, Surface};
use crate::window::{Window, WindowConfig, WindowId};
use aethereal_actor::{
    Actor, Beginning, Carries, ChildOf, Controller, Ending, Fault, Given, Handle, Handles, Held, Level, Source,
    ToSender, ToType, Top, Turn, child,
};
use aethereal_data::{Bound, Plain};
use core::mem;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, VecDeque};

/// What the window root's one source brings in: what the host heard in one of its windows, or what it did with a
/// window the root asked it for, each beside the window, in the order the host pushed them. A window's opening comes
/// through the source its events do, so it reaches the root before anything the window heard.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.windows.heard")]
pub enum Heard {
    /// The window heard `event`.
    Event { window: WindowId, event: Event },
    /// The host opened the window the root asked for, shown windowed, its surface and focus standing so.
    Opened { window: WindowId, surface: Surface, focused: bool },
    /// The host did not open the window the root asked for, for this reason.
    Unopened { window: WindowId, why: Unopened },
    /// The host closed the window, as the root asked or of its own accord.
    Closed { window: WindowId },
    /// The host did not close the window the root asked it to, for this reason: it stays open.
    Unclosed { window: WindowId, why: Unclosed },
    /// The host shows the window in the mode the root asked, its surface as it draws it now. Its `Moded`, when the
    /// mode changed, is pushed as an event before this.
    Set { window: WindowId, surface: Surface },
    /// The host did not show the window in the mode the root asked, for this reason.
    Unset { window: WindowId, why: Unset },
    /// The host asked its windowing system for the size the root asked, the window's surface as it draws it now.
    Resized { window: WindowId, surface: Surface },
    /// The host did not resize the window as the root asked, for this reason.
    Unresized { window: WindowId, why: Unplaced },
    /// The host moved the window as the root asked.
    Moved { window: WindowId },
    /// The host did not move the window as the root asked, for this reason.
    Unmoved { window: WindowId, why: Unplaced },
    /// The host let the window's input method compose, or stopped it, as the root asked.
    ImeAllowed { window: WindowId },
    /// The host did not let the window's input method compose or stop it as the root asked, for this reason.
    ImeUnallowed { window: WindowId, why: Unset },
}

/// What the window root asks its host through its controller, in the order its turns asked: to open a window under
/// the id the root gave it, `title`d, at `size` in the host's logical pixels, shown windowed; to close a window; to
/// show a window in a mode (ADR-0027); to resize or move a window shown windowed (ADR-0047); to let a window's input
/// method compose, or stop it (ADR-0051); or to show a cursor at the pointer over a window, as its actor found it in
/// the draw lists, which the host does without answering, and which the next replaces (ADR-0055).
#[derive(Debug, Clone, PartialEq)]
pub enum Windowing {
    Open { window: WindowId, title: String, size: Logical },
    Close { window: WindowId },
    SetMode { window: WindowId, mode: Mode },
    Resize { window: WindowId, size: Logical },
    Move { window: WindowId, at: Place },
    AllowIme { window: WindowId, allowed: bool },
    SetCursor { window: WindowId, cursor: Cursor },
}

/// What the close button of a window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
pub enum Closing {
    /// It closes the window, as though [`CloseWindow`] had been asked, once the window's actor has sent its
    /// subscribers `CloseRequested`.
    #[default]
    ByButton,
    /// It only asks: the window's subscribers are sent `CloseRequested`, and the program closes the window, or does
    /// not.
    ByProgram,
}

/// A window the host opened as the program was set up, its title, and how it stood, its id among it.
#[derive(Debug, Clone, PartialEq, Plain)]
pub struct Initial {
    pub(crate) title: String,
    pub(crate) config: WindowConfig,
}

/// What the window root is made from: the source its host feeds, the controller it asks its host through, the windows
/// open as it begins, which it makes an actor for each of, and what a window's close button does, which closes it
/// unless the program says otherwise. Made by [`Opening::config`](crate::Opening::config).
#[derive(Bound)]
#[kind(name = "aether.windows.config")]
pub struct WindowsConfig {
    pub(crate) source: Given<Source<Heard>>,
    pub(crate) control: Given<Controller<Windowing>>,
    pub(crate) windows: Vec<Initial>,
    pub closing: Closing,
}

/// Why the window root did not begin: the actor of this window, opened as the program was set up, was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.windows.unmade")]
pub struct Unmade {
    pub window: WindowId,
}

/// The window root: it holds the source every window's events come in through and the controller it asks its host
/// through, and sends each event to the actor of its window, [`Window`], beneath it, keyed by the window's id. It
/// gives each window asked for its id, from a count that starts after the windows opened as the program was set up,
/// so ids never repeat and are in the order the windows were asked for, and keeps how each window stands, with the
/// requests it holds for it.
#[derive(Bound)]
#[kind(name = "aether.windows")]
pub struct Windows {
    /// Held so what the host brings comes here; nothing opens it.
    source: Handle<Source<Heard>>,
    control: Handle<Controller<Windowing>>,
    closing: Closing,
    /// The number of the next window asked for.
    next: u64,
    standing: BTreeMap<WindowId, Standing>,
}

/// How a window stands, as the window root keeps it, with the requests it holds for it.
#[derive(Bound)]
enum Standing {
    /// Asked of the host, which has not said whether it opened it: the request to open it, held, and the title asked.
    Opening { held: Held<Opened>, title: String },
    /// Open, shown as this says.
    Open(Shown),
}

/// How an open window is shown, as the root last heard from its host: its title; its surface and mode, kept so a
/// request for the mode it has is answered at once; whether it has the keyboard; the requests to set its mode, resize
/// it, move it, and allow or stop its input method the host has not answered, each held in the order it was asked
/// them, as it answers them in that order; and whether it has been asked to close.
#[derive(Bound)]
struct Shown {
    title: String,
    surface: Surface,
    mode: Mode,
    focused: bool,
    setting: VecDeque<Held<ModeSet>>,
    resizing: VecDeque<Held<Resized>>,
    moving: VecDeque<Held<Moved>>,
    allowing: VecDeque<Held<ImeAllowed>>,
    close: Close,
}

/// Whether the host has been asked to close an open window.
#[derive(Bound)]
enum Close {
    /// It has not.
    Unasked,
    /// It has, and has not said whether it closed it: the requests to close it, held, none when its close button or
    /// its actor's refusal asked.
    Asked(Vec<Held<Closed>>),
}

impl Shown {
    /// A window `title`d, standing as `config` says, asked nothing.
    const fn new(title: String, config: WindowConfig) -> Self {
        Self {
            title,
            surface: config.surface,
            mode: config.mode,
            focused: config.focused,
            setting: VecDeque::new(),
            resizing: VecDeque::new(),
            moving: VecDeque::new(),
            allowing: VecDeque::new(),
            close: Close::Unasked,
        }
    }

    /// Takes in how `event`, heard in the window, says it is shown.
    fn heard(&mut self, event: &Event) {
        match event {
            Event::Resized(surface) => self.surface = *surface,
            Event::Moded(mode) => self.mode = *mode,
            Event::FocusGained => self.focused = true,
            Event::FocusLost => self.focused = false,
            _ => {}
        }
    }

    /// The window as a list names it, as `window`.
    fn listed(&self, window: WindowId) -> Listed {
        Listed { window, title: self.title.clone(), surface: self.surface, mode: self.mode, focused: self.focused }
    }
}

/// What the window root's turns that hear its source may send: an event, to a window's actor.
type Sends = (ToType<Window, Event>,);

/// The window root is a root, made by the chassis that opens the windows.
impl ChildOf for Windows {
    type Parent = Top;
}

/// What asking the window root at its typed path, `path::<Windows>(())`, gets.
impl Carries<OpenWindow> for Windows {
    type Answers = (Opened,);
}

impl Carries<CloseWindow> for Windows {
    type Answers = (Closed,);
}

impl Carries<SetMode> for Windows {
    type Answers = (ModeSet,);
}

impl Carries<ListWindows> for Windows {
    type Answers = (WindowList,);
}

impl Carries<ResizeWindow> for Windows {
    type Answers = (Resized,);
}

impl Carries<MoveWindow> for Windows {
    type Answers = (Moved,);
}

impl Carries<AllowIme> for Windows {
    type Answers = (ImeAllowed,);
}

impl Actor for Windows {
    type Config = WindowsConfig;
    type Key = ();
    type Handles =
        (Heard, OpenWindow, CloseWindow, SetMode, ListWindows, ResizeWindow, MoveWindow, AllowIme, ShowCursor);
    type BeginSends = ();
    type EndSends = ();

    /// Takes the source and the controller, and makes the actor of each window open, so a root made after it reaches
    /// each as it begins.
    fn begin(config: WindowsConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let source = turn.take(config.source)?;
        let control = turn.take(config.control)?;
        // A host opens some windows, never more than a `u64` counts.
        let next = config.windows.len() as u64;
        let mut standing = BTreeMap::new();
        for Initial { title, config } in config.windows {
            let window = config.window;
            if let Err(refused) = turn.spawn::<Window>(&window, config) {
                turn.log(Level::Error, format_args!("the actor of window {window:?} was refused: {refused}"));
                return Err(Fault::new(&Unmade { window })?);
            }
            standing.insert(window, Standing::Open(Shown::new(title, config)));
        }

        Ok(Self { source, control, closing: config.closing, next, standing })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// Gives the window the next id, holds the request, and asks the host to open it: answered once the host says whether
/// it did.
impl Handles<OpenWindow> for Windows {
    type Sends = (ToSender<Opened>,);

    fn handle(&mut self, open: OpenWindow, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let held = turn.hold()?;
        let window = WindowId::nth(self.next);
        self.next += 1;
        self.standing.insert(window, Standing::Opening { held, title: open.title.clone() });
        // Pushed last, so a turn that fails before it asks the host for nothing (ADR-0019).
        turn.open(&mut self.control)?.push(Windowing::Open { window, title: open.title, size: open.size });

        Ok(())
    }
}

/// Holds the request, and asks the host to close the window, once however many ask: each is answered once the host
/// says whether it did. A window that is not open, as one still opening, is refused.
impl Handles<CloseWindow> for Windows {
    type Sends = (ToSender<Closed>,);

    fn handle(&mut self, close: CloseWindow, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let window = close.window;
        match self.standing.get_mut(&window) {
            Some(Standing::Open(Shown { close: close @ Close::Unasked, .. })) => {
                *close = Close::Asked(vec![turn.hold()?]);
                turn.open(&mut self.control)?.push(Windowing::Close { window });
            }
            Some(Standing::Open(Shown { close: Close::Asked(waiting), .. })) => waiting.push(turn.hold()?),
            Some(Standing::Opening { .. }) | None => turn.reply(&Closed::Refused(Unclosed::NotOpen))?,
        }

        Ok(())
    }
}

/// Holds the request, and asks the host to show the window in the mode asked: answered once the host says whether it
/// did. A window shown in that mode, with no other request to set its mode unanswered, is answered at once with its
/// surface, and the host is asked nothing; while another is unanswered, the host is asked, as only it knows the mode
/// that one leaves. A window that is not open, as one still opening or closing, is refused.
impl Handles<SetMode> for Windows {
    type Sends = (ToSender<ModeSet>,);

    fn handle(&mut self, set: SetMode, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let SetMode { window, mode } = set;
        let Some(Standing::Open(shown @ Shown { close: Close::Unasked, .. })) = self.standing.get_mut(&window) else {
            turn.reply(&ModeSet::Refused(Unset::NotOpen))?;
            return Ok(());
        };
        let is_shown = shown.mode == mode;
        let is_settled = shown.setting.is_empty();
        if is_shown && is_settled {
            turn.reply(&ModeSet::Set { surface: shown.surface })?;
            return Ok(());
        }
        shown.setting.push_back(turn.hold()?);
        turn.open(&mut self.control)?.push(Windowing::SetMode { window, mode });

        Ok(())
    }
}

/// Answers every open window, as the root last heard from its host, with nothing asked of the host.
impl Handles<ListWindows> for Windows {
    type Sends = (ToSender<WindowList>,);

    fn handle(&mut self, _: ListWindows, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let windows = self
            .standing
            .iter()
            .filter_map(|(window, standing)| match standing {
                Standing::Open(shown) => Some(shown.listed(*window)),
                Standing::Opening { .. } => None,
            })
            .collect();
        turn.reply(&WindowList { windows })?;

        Ok(())
    }
}

/// Holds the request, and asks the host to resize the window: answered once the host says whether it did. A window
/// that is not open, as one still opening or closing, is refused, and so is one not shown windowed, whose size its mode
/// sets (ADR-0047).
impl Handles<ResizeWindow> for Windows {
    type Sends = (ToSender<Resized>,);

    fn handle(&mut self, resize: ResizeWindow, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let ResizeWindow { window, size } = resize;
        let shown = match self.placeable(window) {
            Ok(shown) => shown,
            Err(why) => {
                turn.reply(&Resized::Refused(why))?;
                return Ok(());
            }
        };
        shown.resizing.push_back(turn.hold()?);
        turn.open(&mut self.control)?.push(Windowing::Resize { window, size });

        Ok(())
    }
}

/// Holds the request, and asks the host to move the window: answered once the host says whether it did. Refused as a
/// resize is (ADR-0047).
impl Handles<MoveWindow> for Windows {
    type Sends = (ToSender<Moved>,);

    fn handle(&mut self, moving: MoveWindow, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let MoveWindow { window, at } = moving;
        let shown = match self.placeable(window) {
            Ok(shown) => shown,
            Err(why) => {
                turn.reply(&Moved::Refused(why))?;
                return Ok(());
            }
        };
        shown.moving.push_back(turn.hold()?);
        turn.open(&mut self.control)?.push(Windowing::Move { window, at });

        Ok(())
    }
}

/// Holds the request, and asks the host to let the window's input method compose, or to stop it: answered once the host
/// says whether it did. The host is asked each time, as the root keeps no record of whether it is allowed (ADR-0051). A
/// window that is not open, as one still opening or closing, is refused.
impl Handles<AllowIme> for Windows {
    type Sends = (ToSender<ImeAllowed>,);

    fn handle(&mut self, allow: AllowIme, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let AllowIme { window, allowed } = allow;
        let Some(Standing::Open(shown @ Shown { close: Close::Unasked, .. })) = self.standing.get_mut(&window) else {
            turn.reply(&ImeAllowed::Refused(Unset::NotOpen))?;
            return Ok(());
        };
        shown.allowing.push_back(turn.hold()?);
        turn.open(&mut self.control)?.push(Windowing::AllowIme { window, allowed });

        Ok(())
    }
}

/// Asks the host to show the cursor at the pointer over the window, as the window's own actor asked: it alone follows
/// what the pointer is over. Nothing answers it. What another actor asks is not acted on, as the cursor is declared in
/// the draw lists, never set by whoever asks last; and a window that is not open shows none.
impl Handles<ShowCursor> for Windows {
    type Sends = ();

    fn handle(&mut self, show: ShowCursor, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        let ShowCursor { window, cursor } = show;
        let from_window = turn.child::<Window>(turn.me(), &window).is_some_and(|actor| turn.sender() == actor.into());
        if !from_window {
            turn.log(Level::Warn, "a cursor another actor than its window's asked for was not shown");
            return Ok(());
        }
        let is_open = matches!(self.standing.get(&window), Some(Standing::Open(_)));
        if !is_open {
            return Ok(());
        }
        turn.open(&mut self.control)?.push(Windowing::SetCursor { window, cursor });

        Ok(())
    }
}

/// What the source brought, from this actor to itself (ADR-0010): an event, sent on to the actor of the window it was
/// heard in, or what the host did with a window, which answers what the root holds for it. What another actor sends
/// is not the host's, and is not acted on; what was heard in a window with no actor reaches nothing.
impl Handles<Heard> for Windows {
    type Sends = Sends;

    fn handle(&mut self, heard: Heard, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let me = turn.me();
        let is_brought = turn.sender() == me.into();
        if !is_brought {
            turn.log(Level::Warn, "what another actor said of a window was not acted on: only the host's is");
            return Ok(());
        }

        match heard {
            Heard::Event { window, event } => self.heard(window, &event, turn),
            Heard::Opened { window, surface, focused } => {
                self.opened(window, WindowConfig { window, surface, mode: Mode::Windowed, focused }, turn)
            }
            Heard::Unopened { window, why } => self.unopened(window, why, turn),
            Heard::Closed { window } => self.closed(window, turn),
            Heard::Unclosed { window, why } => self.unclosed(window, why, turn),
            Heard::Set { window, surface } => self.set(window, ModeSet::Set { surface }, turn),
            Heard::Unset { window, why } => self.set(window, ModeSet::Refused(why), turn),
            Heard::Resized { window, surface } => self.resized(window, Resized::Resized { surface }, turn),
            Heard::Unresized { window, why } => self.resized(window, Resized::Refused(why), turn),
            Heard::Moved { window } => self.moved(window, Moved::Moved, turn),
            Heard::Unmoved { window, why } => self.moved(window, Moved::Refused(why), turn),
            Heard::ImeAllowed { window } => self.allowed(window, ImeAllowed::Allowed, turn),
            Heard::ImeUnallowed { window, why } => self.allowed(window, ImeAllowed::Refused(why), turn),
        }
    }
}

impl Windows {
    /// The request held to open `window`, taken, with the title it asked: none when the window is not opening.
    fn opening(&mut self, window: WindowId) -> Option<(Held<Opened>, String)> {
        let Entry::Occupied(standing) = self.standing.entry(window) else {
            return None;
        };
        let is_opening = matches!(standing.get(), Standing::Opening { .. });
        if !is_opening {
            return None;
        }
        let Standing::Opening { held, title } = standing.remove() else {
            return None;
        };

        Some((held, title))
    }

    /// How `window` is shown, when a request to resize or move it may be asked of the host: it is open, not asked to
    /// close, and shown windowed.
    fn placeable(&mut self, window: WindowId) -> Result<&mut Shown, Unplaced> {
        let Some(Standing::Open(shown @ Shown { close: Close::Unasked, .. })) = self.standing.get_mut(&window) else {
            return Err(Unplaced::NotOpen);
        };
        if shown.mode != Mode::Windowed {
            return Err(Unplaced::NotWindowed);
        }

        Ok(shown)
    }

    /// Sends `event`, heard in `window`, to the window's actor, and closes the window on its close button when the
    /// button closes it. What is heard in a window once it has closed is not sent on, so `Closed` is the last its
    /// subscribers hear; and that a window closed is the host's to say, with [`Heard::Closed`], so an event that says
    /// it is not sent on either.
    fn heard(&mut self, window: WindowId, event: &Event, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        if *event == Event::Closed {
            turn.log(Level::Warn, format_args!("the host said window {window:?} closed as an event: not sent on"));
            return Ok(());
        }
        let Some(standing) = self.standing.get_mut(&window) else {
            // Heard after the window closed, or of a window the root does not know: it reaches no actor.
            return Ok(());
        };
        turn.to(child::<Window>(window)).send(event)?;
        let Standing::Open(shown) = standing else {
            return Ok(());
        };
        shown.heard(event);
        let by_button = *event == Event::CloseRequested && self.closing == Closing::ByButton;
        let unasked = matches!(shown.close, Close::Unasked);
        if by_button && unasked {
            shown.close = Close::Asked(Vec::new());
            turn.open(&mut self.control)?.push(Windowing::Close { window });
        }

        Ok(())
    }

    /// Makes the actor of `window`, which the host opened standing as `config` says, then answers the request held for
    /// it. A window whose actor is refused is answered so, and the host is asked to close it.
    fn opened(
        &mut self,
        window: WindowId,
        config: WindowConfig,
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some((held, title)) = self.opening(window) else {
            turn.log(Level::Warn, format_args!("the host opened window {window:?}, which was not asked for"));
            return Ok(());
        };
        if let Err(refused) = turn.spawn::<Window>(&window, config) {
            turn.log(Level::Error, format_args!("the actor of window {window:?} was refused: {refused}"));
            let closing = Shown { close: Close::Asked(Vec::new()), ..Shown::new(title, config) };
            self.standing.insert(window, Standing::Open(closing));
            turn.answer(held, &Opened::Refused(Unopened::Refused))?;
            turn.open(&mut self.control)?.push(Windowing::Close { window });
            return Ok(());
        }
        self.standing.insert(window, Standing::Open(Shown::new(title, config)));
        turn.answer(held, &Opened::Window { window, surface: config.surface })?;

        Ok(())
    }

    /// Answers the request held to open `window`, which the host did not open, with why.
    fn unopened(&mut self, window: WindowId, why: Unopened, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Some((held, _)) = self.opening(window) else {
            turn.log(Level::Warn, format_args!("the host did not open window {window:?}, which was not asked for"));
            return Ok(());
        };
        turn.answer(held, &Opened::Refused(why))?;

        Ok(())
    }

    /// Tells the actor of `window`, which the host closed, so it sends its subscribers `Closed` and ends, then answers
    /// each request held to set its mode, resize it, move it, or allow or stop its input method, which the host closed
    /// it before answering, as not open, and each held to close it.
    fn closed(&mut self, window: WindowId, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Shown { setting, resizing, moving, allowing, close, .. } = match self.standing.remove(&window) {
            Some(Standing::Open(shown)) => shown,
            Some(Standing::Opening { held, .. }) => {
                turn.log(Level::Warn, format_args!("the host closed window {window:?} before it said it opened"));
                turn.answer(held, &Opened::Refused(Unopened::Failed(String::from("closed as it opened"))))?;
                return Ok(());
            }
            None => {
                turn.log(Level::Warn, format_args!("the host closed window {window:?}, which was not open"));
                return Ok(());
            }
        };
        turn.to(child::<Window>(window)).send(&Event::Closed)?;
        for held in setting {
            turn.answer(held, &ModeSet::Refused(Unset::NotOpen))?;
        }
        for held in resizing {
            turn.answer(held, &Resized::Refused(Unplaced::NotOpen))?;
        }
        for held in moving {
            turn.answer(held, &Moved::Refused(Unplaced::NotOpen))?;
        }
        for held in allowing {
            turn.answer(held, &ImeAllowed::Refused(Unset::NotOpen))?;
        }
        if let Close::Asked(waiting) = close {
            for held in waiting {
                turn.answer(held, &Closed::Closed)?;
            }
        }

        Ok(())
    }

    /// Answers each request held to close `window`, which the host did not close, with why: it stays open.
    fn unclosed(&mut self, window: WindowId, why: Unclosed, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Some(Standing::Open(Shown { close: close @ Close::Asked(_), .. })) = self.standing.get_mut(&window) else {
            turn.log(Level::Warn, format_args!("the host did not close window {window:?}, which was not closing"));
            return Ok(());
        };
        let Close::Asked(waiting) = mem::replace(close, Close::Unasked) else {
            return Ok(());
        };
        for held in waiting {
            turn.answer(held, &Closed::Refused(why))?;
        }

        Ok(())
    }

    /// Answers the first request held to set the mode of `window` with `set`, what the host did with it: the host
    /// answers them in the order it was asked.
    fn set(&mut self, window: WindowId, set: ModeSet, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Some(Standing::Open(shown)) = self.standing.get_mut(&window) else {
            turn.log(Level::Warn, format_args!("the host set the mode of window {window:?}, which was not open"));
            return Ok(());
        };
        let Some(held) = shown.setting.pop_front() else {
            turn.log(Level::Warn, format_args!("the host set the mode of window {window:?}, which was not asked"));
            return Ok(());
        };
        turn.answer(held, &set)?;

        Ok(())
    }

    /// Answers the first request held to resize `window` with `resized`, what the host did with it: the host answers
    /// them in the order it was asked.
    fn resized(&mut self, window: WindowId, resized: Resized, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Some(Standing::Open(shown)) = self.standing.get_mut(&window) else {
            turn.log(Level::Warn, format_args!("the host resized window {window:?}, which was not open"));
            return Ok(());
        };
        let Some(held) = shown.resizing.pop_front() else {
            turn.log(Level::Warn, format_args!("the host resized window {window:?}, which was not asked"));
            return Ok(());
        };
        turn.answer(held, &resized)?;

        Ok(())
    }

    /// Answers the first request held to move `window` with `moved`, what the host did with it: the host answers them
    /// in the order it was asked.
    fn moved(&mut self, window: WindowId, moved: Moved, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Some(Standing::Open(shown)) = self.standing.get_mut(&window) else {
            turn.log(Level::Warn, format_args!("the host moved window {window:?}, which was not open"));
            return Ok(());
        };
        let Some(held) = shown.moving.pop_front() else {
            turn.log(Level::Warn, format_args!("the host moved window {window:?}, which was not asked"));
            return Ok(());
        };
        turn.answer(held, &moved)?;

        Ok(())
    }

    /// Answers the first request held to allow or stop the input method of `window` with `allowed`, what the host did
    /// with it: the host answers them in the order it was asked.
    fn allowed(
        &mut self,
        window: WindowId,
        allowed: ImeAllowed,
        turn: &mut Turn<'_, Self, Sends>,
    ) -> Result<(), Fault> {
        let Some(Standing::Open(shown)) = self.standing.get_mut(&window) else {
            turn.log(Level::Warn, format_args!("the host answered for window {window:?}'s IME, which was not open"));
            return Ok(());
        };
        let Some(held) = shown.allowing.pop_front() else {
            turn.log(Level::Warn, format_args!("the host answered for window {window:?}'s IME, which was not asked"));
            return Ok(());
        };
        turn.answer(held, &allowed)?;

        Ok(())
    }
}
