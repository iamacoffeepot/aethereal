//! How the overlay's actor reaches the chat (ADR-0057: The console is the chat): it finds the engine's link once, as it
//! is first shown, and listens to it; what the link shows it is taken into the console's chat, and what the owner does
//! is told to the link, which writes it up to the hub. An engine with no link, or an untrusted one, which the hub shows
//! no chat, keeps the console as it was.

use super::Overlay;
use crate::chat::{Typed, typed};
use crate::console::Command;
use aethereal_actor::{Anyone, Fault, Handles, Level, ProtocolRef, Turn, path};
use aethereal_data::{Bound, Plain};
use aethereal_link::{Link, Listen};
use aethereal_sessions::{Chat, ShowAsk, ShowEvents, ShowTabs};

/// The most bytes of what a command run in `!` mode printed that are sent to the session: what a tool prints can be
/// large, and the line it is sent as is read by Claude.
const MOST_RAN_BYTES: usize = 16 * 1024;

/// Whether the overlay has looked for the engine's link, and what it found.
#[derive(Debug, Bound)]
pub enum Linking {
    /// It has not looked: it looks once, as it is first shown, when every root of the program has begun.
    Unlooked,
    /// The engine's link, which it listens to and tells the chat.
    Linked(ProtocolRef<(Listen, Chat)>),
    /// The engine has no link: the console never shows the chat.
    Unlinked,
}

/// Where a command typed at the console reports what it printed.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub enum Reports {
    /// Nowhere but the console's scrollback.
    Here,
    /// To the session `session`, whose tab was open as it was typed, as `line`.
    Session { session: u32, line: String },
}

impl Overlay {
    /// Finds the engine's link, once, and listens to it.
    pub(super) fn listen(&mut self, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let Linking::Unlooked = self.linking else {
            return Ok(());
        };
        // An engine may have no link, so the overlay depends on none: it looks the link up at its path, as one that
        // may not be there, and reaches it through what it sends it (ADR-0059).
        let link = turn.resolve(path::<Link>(()).names()).and_then(|found| turn.cast::<(Listen, Chat)>(found));
        self.linking = match link {
            Some(link) => {
                turn.to(link).send(&Listen)?;
                Linking::Linked(link)
            }
            None => Linking::Unlinked,
        };

        Ok(())
    }

    /// Tells the link each of `chat`: nothing without one.
    pub(super) fn tell(&self, turn: &mut Turn<'_, Self>, chat: Vec<Chat>) -> Result<(), Fault> {
        let Linking::Linked(link) = self.linking else {
            return Ok(());
        };
        for chat in chat {
            turn.to(link).send(&chat)?;
        }

        Ok(())
    }

    /// Sends the session `reports` names a command run here and what it printed, as a line, which starts a turn:
    /// nothing when it reports here alone.
    pub(super) fn report(&mut self, turn: &mut Turn<'_, Self>, reports: Reports, printed: &str) -> Result<(), Fault> {
        let Reports::Session { session, line } = reports else {
            return Ok(());
        };
        let ran = self.chats.ran(session, line, String::from(cut(printed)));

        self.tell(turn, vec![ran])
    }

    /// Runs the line `line` typed with a session's tab open, by the tab's mode: said to it in chat mode, or in `!` mode
    /// run here and sent to it with what it printed. Answers the command a line run here comes to, for the overlay to
    /// call.
    pub(super) fn chatted(
        &mut self,
        turn: &mut Turn<'_, Self>,
        session: u32,
        line: &str,
    ) -> Result<(Command, Reports), Fault> {
        let (command, reports) = match typed(line, self.chats.mode()) {
            Typed::Blank => {
                self.console.kept(line);
                (Command::Blank, Reports::Here)
            }
            Typed::Say(text) => {
                self.console.kept(line);
                let chat = self.chats.say(text);
                self.tell(turn, chat)?;
                (Command::Blank, Reports::Here)
            }
            Typed::Run(command) => (self.console.submitted(&command), Reports::Session { session, line: command }),
        };

        Ok((command, reports))
    }
}

/// `printed`, cut to the most bytes a session is told of it, at a character's boundary.
fn cut(printed: &str) -> &str {
    let mut end = printed.len().min(MOST_RAN_BYTES);
    while !printed.is_char_boundary(end) {
        end -= 1;
    }

    &printed[..end]
}

/// The sessions the hub lists, as tabs: the first listing opens the newest, which the link is told, and offers the
/// chat's words, as only an engine the hub trusts is shown the sessions.
impl Handles<ShowTabs> for Overlay {
    type From = Anyone;

    fn handle(&mut self, shown: ShowTabs, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let first = !self.chats.is_listed();
        let chat = self.chats.listed(shown.tabs);
        self.tell(turn, chat)?;
        if first {
            self.offer_words(turn)?;
        }

        Ok(())
    }
}

/// Events of a tab open, taken into its transcript.
impl Handles<ShowEvents> for Overlay {
    type From = Anyone;

    fn handle(&mut self, shown: ShowEvents, _: &mut Turn<'_, Self>) -> Result<(), Fault> {
        self.chats.shown(shown.session, &shown.events);

        Ok(())
    }
}

/// An ask that waits on the owner, kept until answered.
impl Handles<ShowAsk> for Overlay {
    type From = Anyone;

    fn handle(&mut self, ask: ShowAsk, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.log(Level::Info, format_args!("session {} asks: {}", ask.session, ask.what));
        self.chats.asked(ask);

        Ok(())
    }
}
