//! The window root: the capability that holds the one source its host brings in what every window hears through, and
//! the controller it asks the host for windows through; it makes each window's actor, sends each event on to the
//! actor of the window it was heard in, and answers each actor that asked it to open or close a window, or to set the
//! mode one is shown in, once its host has (ADR-0024, ADR-0027).

mod asked;
#[cfg(test)]
mod tests;

pub use asked::{CloseWindow, Closed, Logical, ModeSet, OpenWindow, Opened, SetMode, Unclosed, Unopened, Unset};

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
}

/// What the window root asks its host through its controller, in the order its turns asked: to open a window under
/// the id the root gave it, `title`d, at `size` in the host's logical pixels, shown windowed; to close a window; or to
/// show a window in a mode (ADR-0027).
#[derive(Debug, Clone, PartialEq)]
pub enum Windowing {
    Open { window: WindowId, title: String, size: Logical },
    Close { window: WindowId },
    SetMode { window: WindowId, mode: Mode },
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

/// A window the host opened as the program was set up, and how it stood.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Initial {
    pub(crate) window: WindowId,
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
    /// Asked of the host, which has not said whether it opened it: the request to open it, held.
    Opening(Held<Opened>),
    /// Open, shown as this says.
    Open(Shown),
}

/// How an open window is shown, as the root last heard from its host: its surface and mode, kept so a request for the
/// mode it has is answered at once; the requests to set its mode the host has not answered, held in the order it was
/// asked them, as it answers them in that order; and whether it has been asked to close.
#[derive(Bound)]
struct Shown {
    surface: Surface,
    mode: Mode,
    setting: VecDeque<Held<ModeSet>>,
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
    /// A window standing as `config` says, asked nothing.
    const fn new(config: WindowConfig) -> Self {
        Self { surface: config.surface, mode: config.mode, setting: VecDeque::new(), close: Close::Unasked }
    }

    /// Takes in how `event`, heard in the window, says it is shown.
    fn heard(&mut self, event: &Event) {
        match event {
            Event::Resized(surface) => self.surface = *surface,
            Event::Moded(mode) => self.mode = *mode,
            _ => {}
        }
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

impl Actor for Windows {
    type Config = WindowsConfig;
    type Key = ();
    type Handles = (Heard, OpenWindow, CloseWindow, SetMode);
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
        for Initial { window, config } in config.windows {
            if let Err(refused) = turn.spawn::<Window>(&window, config) {
                turn.log(Level::Error, format_args!("the actor of window {window:?} was refused: {refused}"));
                return Err(Fault::new(&Unmade { window })?);
            }
            standing.insert(window, Standing::Open(Shown::new(config)));
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
        self.standing.insert(window, Standing::Opening(held));
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
            Some(Standing::Opening(_)) | None => turn.reply(&Closed::Refused(Unclosed::NotOpen))?,
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
                self.opened(window, WindowConfig { surface, mode: Mode::Windowed, focused }, turn)
            }
            Heard::Unopened { window, why } => self.unopened(window, why, turn),
            Heard::Closed { window } => self.closed(window, turn),
            Heard::Unclosed { window, why } => self.unclosed(window, why, turn),
            Heard::Set { window, surface } => self.set(window, ModeSet::Set { surface }, turn),
            Heard::Unset { window, why } => self.set(window, ModeSet::Refused(why), turn),
        }
    }
}

impl Windows {
    /// The request held to open `window`, taken, with the window: none when the window is not opening.
    fn opening(&mut self, window: WindowId) -> Option<Held<Opened>> {
        let Entry::Occupied(standing) = self.standing.entry(window) else {
            return None;
        };
        let is_opening = matches!(standing.get(), Standing::Opening(_));
        if !is_opening {
            return None;
        }
        let Standing::Opening(held) = standing.remove() else {
            return None;
        };

        Some(held)
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
        let Some(held) = self.opening(window) else {
            turn.log(Level::Warn, format_args!("the host opened window {window:?}, which was not asked for"));
            return Ok(());
        };
        if let Err(refused) = turn.spawn::<Window>(&window, config) {
            turn.log(Level::Error, format_args!("the actor of window {window:?} was refused: {refused}"));
            let closing = Shown { close: Close::Asked(Vec::new()), ..Shown::new(config) };
            self.standing.insert(window, Standing::Open(closing));
            turn.answer(held, &Opened::Refused(Unopened::Refused))?;
            turn.open(&mut self.control)?.push(Windowing::Close { window });
            return Ok(());
        }
        self.standing.insert(window, Standing::Open(Shown::new(config)));
        turn.answer(held, &Opened::Window { window, surface: config.surface })?;

        Ok(())
    }

    /// Answers the request held to open `window`, which the host did not open, with why.
    fn unopened(&mut self, window: WindowId, why: Unopened, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Some(held) = self.opening(window) else {
            turn.log(Level::Warn, format_args!("the host did not open window {window:?}, which was not asked for"));
            return Ok(());
        };
        turn.answer(held, &Opened::Refused(why))?;

        Ok(())
    }

    /// Tells the actor of `window`, which the host closed, so it sends its subscribers `Closed` and ends, then answers
    /// each request held to set its mode, which the host closed it before answering, as not open, and each held to
    /// close it.
    fn closed(&mut self, window: WindowId, turn: &mut Turn<'_, Self, Sends>) -> Result<(), Fault> {
        let Shown { setting, close, .. } = match self.standing.remove(&window) {
            Some(Standing::Open(shown)) => shown,
            Some(Standing::Opening(held)) => {
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
}
