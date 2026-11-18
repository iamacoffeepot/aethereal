//! How the overlay's actor answers its tools' worker (ADR-0053: Commands are tools): the overlay serves no tool itself.
//! As it begins it spawns [`OverlayTools`] and offers its tools, `mark`, `memory`, and `set_theme`, in every engine;
//! the first time the hub lists the sessions, which it does only to an engine it trusts, it offers the chat's words
//! (ADR-0057: The console is the chat) with the same worker. The worker mails the overlay what needs the overlay's
//! state: a mark, kept to be pinned; a theme's [`Choice`], whose palette the next frame is drawn in; and a
//! [`ChatWord`], which the chat turns into what the link is told.

use super::Overlay;
use super::console::Offering;
use crate::tools::{
    ChatWord, Choice, Marked, OverlayTools, OverlayToolsConfig, ThemesShown, Worded, chat_sheets, chosen,
};
use aethereal_actor::{ActorRef, Beginning, Fault, Spawn, Turn, path};
use aethereal_data::Kind;
use aethereal_tools::{Offer, Tool, Tools, Unlisted};

impl Overlay {
    /// Spawns the worker of the overlay's tools and offers them to the engine's tools root: as it begins, in every
    /// engine. A worker refused as it was spawned is a program built wrong, as it is for a toolbox, and the overlay
    /// does not begin.
    pub(super) fn spawn_tools(turn: &mut Beginning<'_, Self>) -> Result<ActorRef<OverlayTools>, Fault> {
        let Ok(tools) = turn.spawn::<OverlayTools>(&(), OverlayToolsConfig) else {
            return Err(Fault::new(&Unlisted::Unspawned(OverlayTools::NAME.to_string()))?);
        };
        let offer = Offer { tools: OverlayTools::sheets(), worker: OverlayTools::worker(tools) };
        turn.to(path::<Tools>(())).ask(&offer, &Offering)?;

        Ok(tools)
    }

    /// Offers the chat's words to the engine's tools root, with the overlay's tools' worker as theirs: as the hub first
    /// lists the sessions, so an engine it never trusts never offers them.
    pub(super) fn offer_words(&self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let offer = Offer { tools: chat_sheets(), worker: OverlayTools::worker(self.tools) };
        turn.to(path::<Tools>(())).ask(&offer, &Offering)?;

        Ok(())
    }
}

impl Overlay {
    /// A mark, kept to be pinned at the next `Tick`'s time: its tick is drawn on the chart from then, and it is printed
    /// in the mark's ink then.
    #[allow(clippy::unnecessary_wraps, reason = "its handler's body, which answers a fault as every handler does")]
    pub(super) fn handle_marked(&mut self, marked: Marked) -> Result<(), Fault> {
        self.marks.wait(marked.text);

        Ok(())
    }

    /// A theme chosen through `set_theme`: the palette changed to the theme it names, for the next frame to draw in,
    /// and answered the themes and the one in use.
    #[allow(clippy::unnecessary_wraps, reason = "its handler's body, which answers a fault as every handler does")]
    pub(super) fn handle_choice(&mut self, choice: Choice) -> Result<ThemesShown, Fault> {
        let (palette, shown) = chosen(&self.palette, choice);
        self.palette = palette;

        Ok(shown)
    }

    /// One of the chat's words, which its worker took only from the owner: the chat it comes to told the link, or
    /// refused for a session the chat does not list.
    pub(super) fn handle_chat_word(&mut self, word: ChatWord, turn: &mut Turn<'_, Self>) -> Result<Worded, Fault> {
        let chat = match word {
            ChatWord::New { name } => Ok(self.chats.make(name)),
            ChatWord::Stop(which) => self.chats.stop(&which),
            ChatWord::Archive(which) => self.chats.archive(&which),
        };
        let worded = match chat {
            Ok(chat) => {
                self.tell(turn, vec![chat])?;
                Worded::Told
            }
            Err(why) => Worded::Refused(why),
        };

        Ok(worded)
    }
}
