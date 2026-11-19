//! How the overlay's actor reaches the chat (ADR-0057: The console is the chat): it depends on no link. An actor that
//! carries the chat to the engine's link, the [`ChatRelay`](crate::ChatRelay) a program with a link lists, tells it
//! [`Relays`], and it keeps that actor; what the relay shows it is taken into the console's chat, and what the owner
//! does is told to the relay, which tells the link, which writes it up to the hub. An engine with no link, which lists
//! no relay, or an untrusted one, which the hub shows no chat, keeps the console as it was.

use super::Overlay;
use crate::chat::{Typed, typed};
use crate::console::Command;
use crate::relay::Relays;
use aethereal_actor::{Fault, Level, ProtocolRef, Speaks, Turn};
use aethereal_data::{Bound, Plain};
use aethereal_sessions::{Chat, ShowAsk, ShowDeclined, ShowEvents, ShowSessions};

/// The most bytes of what a command run in `!` mode printed that are sent to the session: what a tool prints can be
/// large, and the line it is sent as is read by Claude.
const MOST_RAN_BYTES: usize = 16 * 1024;

/// What carries the overlay's chat to the engine's link.
#[derive(Debug, Bound)]
pub enum Linking {
    /// Nothing has told it [`Relays`]: what the owner does goes nowhere, and the console shows no chat, as no hub lists
    /// it the sessions.
    Unlinked,
    /// The actor that told it [`Relays`] last, which it tells the chat.
    Linked(ProtocolRef<(Chat,)>),
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
    /// Tells what carries the chat to the link each of `chat`: nothing without one.
    pub(super) fn tell(&self, turn: &mut Turn<'_, Self>, chat: Vec<Chat>) -> Result<(), Fault> {
        let Linking::Linked(relay) = self.linking else {
            return Ok(());
        };
        for chat in chat {
            turn.to(relay).send(&chat)?;
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

impl Overlay {
    /// An actor that carries the chat to the engine's link: what the owner does is told to it from then on, in place of
    /// any that told it before.
    #[allow(clippy::unnecessary_wraps, reason = "its handler's body, which answers a fault as every handler does")]
    pub(super) fn handle_relays(&mut self, _: Relays, turn: &mut Turn<'_, Self, Speaks<(Chat,)>>) -> Result<(), Fault> {
        self.linking = Linking::Linked(*turn.from());

        Ok(())
    }

    /// The sessions the hub lists, as tabs: the first listing opens the newest, which the link is told, and offers the
    /// chat's words, as only an engine the hub trusts is shown the sessions.
    pub(super) fn handle_show_sessions(&mut self, shown: ShowSessions, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let first = !self.chats.is_listed();
        let chat = self.chats.listed(shown.list);
        self.tell(turn, chat)?;
        if first {
            self.offer_words(turn)?;
        }

        Ok(())
    }

    /// Events of a tab open, taken into its transcript.
    #[allow(clippy::needless_pass_by_value, reason = "the mail its handler is handed, taken as it is handed")]
    #[allow(clippy::unnecessary_wraps, reason = "its handler's body, which answers a fault as every handler does")]
    pub(super) fn handle_show_events(&mut self, shown: ShowEvents) -> Result<(), Fault> {
        self.chats.shown(shown.session, &shown.events);

        Ok(())
    }

    /// An ask that waits on the owner, kept until answered.
    #[allow(clippy::unnecessary_wraps, reason = "its handler's body, which answers a fault as every handler does")]
    pub(super) fn handle_show_ask(&mut self, ask: ShowAsk, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        turn.log(Level::Info, format_args!("session {} asks: {}", ask.session, ask.what));
        self.chats.asked(ask);

        Ok(())
    }

    /// A chat frame of this engine's that the hub could not do: logged, as the console does not print it yet.
    #[allow(clippy::unnecessary_wraps, reason = "its handler's body, which answers a fault as every handler does")]
    pub(super) fn handle_show_declined(declined: &ShowDeclined, turn: &mut Turn<'_, Self>) -> Result<(), Fault> {
        let ShowDeclined { chat, why } = declined;
        turn.log(Level::Info, format_args!("the hub declined {chat:?}: {why:?}"));

        Ok(())
    }
}
