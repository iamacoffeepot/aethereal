//! The window root: the capability that holds the one source its host brings in what every window hears through,
//! makes each window's actor, and sends each event on to the actor of the window it was heard in.

#[cfg(test)]
mod tests;

use crate::event::Event;
use crate::window::{Window, WindowConfig, WindowId};
use aethereal_actor::{
    Actor, Beginning, ChildOf, Ending, Fault, Given, Handle, Handles, Level, Source, ToType, Top, Turn, child,
};
use aethereal_data::{Bound, Plain};

/// What the host heard in one of its windows: what its source brings in.
#[derive(Debug, Clone, PartialEq, Plain)]
#[kind(name = "aether.windows.heard")]
pub struct Heard {
    pub(crate) window: WindowId,
    pub(crate) event: Event,
}

/// A window the host opened as the program was set up, and how it stood.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Opened {
    pub(crate) window: WindowId,
    pub(crate) config: WindowConfig,
}

/// What the window root is made from: the source its host feeds, and the windows open as it begins, which it makes an
/// actor for each of. Made by [`Opening::config`](crate::Opening::config).
#[derive(Bound)]
#[kind(name = "aether.windows.config")]
pub struct WindowsConfig {
    pub(crate) source: Given<Source<Heard>>,
    pub(crate) windows: Vec<Opened>,
}

/// Why the window root did not begin: the actor of this window was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.windows.unopened")]
pub struct Unopened {
    pub window: WindowId,
}

/// The window root: it holds the source every window's events come in through, and sends each to the actor of its
/// window, [`Window`], beneath it, keyed by the window's id.
#[derive(Bound)]
#[kind(name = "aether.windows")]
pub struct Windows {
    source: Handle<Source<Heard>>,
}

/// The window root is a root, made by the chassis that opens the windows.
impl ChildOf for Windows {
    type Parent = Top;
}

impl Actor for Windows {
    type Config = WindowsConfig;
    type Key = ();
    type Handles = (Heard,);
    type BeginSends = ();
    type EndSends = ();

    /// Takes the source, and makes the actor of each window open, so a root made after it reaches each as it begins.
    fn begin(config: WindowsConfig, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
        let source = turn.take(config.source)?;
        for Opened { window, config } in config.windows {
            if let Err(refused) = turn.spawn::<Window>(&window, config) {
                turn.log(Level::Error, format_args!("the actor of window {window:?} was refused: {refused}"));
                return Err(Fault::new(&Unopened { window })?);
            }
        }

        Ok(Self { source })
    }

    fn end(self, _: &mut Ending<'_, Self, Self::EndSends>) -> Result<(), Fault> {
        Ok(())
    }
}

/// What the source brought, from this actor to itself (ADR-0010), sent on to the actor of the window it was heard in.
/// What another actor sends is not the host's, and is not sent on; what was heard in a window with no actor reaches
/// nothing.
impl Handles<Heard> for Windows {
    type Sends = (ToType<Window, Event>,);

    fn handle(&mut self, heard: Heard, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let me = turn.me();
        let is_brought = turn.sender() == me.into();
        if !is_brought {
            turn.log(Level::Warn, "what another actor said a window heard was not sent on: only the host's is");
            return Ok(());
        }
        turn.to(child::<Window>(heard.window)).send(&heard.event)?;

        Ok(())
    }
}
