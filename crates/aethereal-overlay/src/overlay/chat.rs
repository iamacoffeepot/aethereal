//! How the overlay's actor reaches the chat (ADR-0057: The console is the chat): it finds the engine's link once, as it
//! is first shown, and listens to it; what the link shows it is taken into the console's chat, and what the owner does
//! is told to the link, which writes it up to the hub. An engine with no link, or an untrusted one, which the hub shows
//! no chat, keeps the console as it was.

use super::Overlay;
use super::console::Offering;
use crate::chat::{Typed, typed};
use crate::console::Command;
use aethereal_actor::{ActorRef, Allows, Asks, Fault, Handles, Level, Path, ToType, Turn, path};
use aethereal_data::{Bound, Plain};
use aethereal_link::{Link, Listen};
use aethereal_sessions::{Chat, ShowAsk, ShowEvents, ShowTabs};
use aethereal_tools::{Offer, Tools};

/// The most bytes of what a `/` command printed that are noted in the session: what a tool prints can be large, and the
/// line folded into the owner's next is read by Claude.
const MOST_RAN_BYTES: usize = 16 * 1024;

/// Whether the overlay has looked for the engine's link, and what it found.
#[derive(Debug, Bound)]
pub enum Linking {
    /// It has not looked: it looks once, as it is first shown, when every root of the program has begun.
    Unlooked,
    /// The engine's link, which it listens to.
    Linked(ActorRef<Link>),
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
    pub(super) fn listen<S, I>(&mut self, turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<ToType<Link, Listen>, I>,
    {
        let Linking::Unlooked = self.linking else {
            return Ok(());
        };
        self.linking = match turn.found(&path::<Link>(())) {
            Some(link) => {
                turn.to(link).send::<_, I>(&Listen)?;
                Linking::Linked(link)
            }
            None => Linking::Unlinked,
        };

        Ok(())
    }

    /// Tells the link each of `chat`: nothing without one.
    pub(super) fn tell<S, I>(&self, turn: &mut Turn<'_, Self, S>, chat: Vec<Chat>) -> Result<(), Fault>
    where
        S: Allows<ToType<Link, Chat>, I>,
    {
        let Linking::Linked(link) = self.linking else {
            return Ok(());
        };
        for chat in chat {
            turn.to(link).send::<_, I>(&chat)?;
        }

        Ok(())
    }

    /// Notes in the session `reports` names what a command run here printed: nothing when it reports here alone.
    pub(super) fn report<S, I>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        reports: Reports,
        printed: &str,
    ) -> Result<(), Fault>
    where
        S: Allows<ToType<Link, Chat>, I>,
    {
        let Reports::Session { session, line } = reports else {
            return Ok(());
        };
        let ran = self.chats.ran(session, line, String::from(cut(printed)));

        self.tell::<_, I>(turn, vec![ran])
    }

    /// Runs the line `line` typed with a session's tab open: said to it, run here and noted in it, or run here alone,
    /// one of the chat's own words. Answers the command a line run here comes to, for the overlay to call.
    pub(super) fn chatted<S, I>(
        &mut self,
        turn: &mut Turn<'_, Self, S>,
        session: u32,
        line: &str,
    ) -> Result<(Command, Reports), Fault>
    where
        S: Allows<ToType<Link, Chat>, I>,
    {
        let typed = typed(line);
        let reports = match &typed {
            Typed::Run(command) => Reports::Session { session, line: command.clone() },
            Typed::Blank | Typed::Say(_) | Typed::Own(_) => Reports::Here,
        };
        let (chat, command) = match typed {
            Typed::Blank => (Vec::new(), Command::Blank),
            Typed::Say(text) => (self.chats.say(text), Command::Blank),
            Typed::Run(command) | Typed::Own(command) => (Vec::new(), self.console.ran(&command)),
        };
        self.console.kept(line);
        self.tell::<_, I>(turn, chat)?;

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
    type Sends = (ToType<Link, Chat>, Asks<Path<Tools>, Offer, Offering>);

    fn handle(&mut self, shown: ShowTabs, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        let first = !self.chats.is_listed();
        let chat = self.chats.listed(shown.tabs);
        self.tell(turn, chat)?;
        if first {
            Self::offer_words(turn)?;
        }

        Ok(())
    }
}

/// Events of a tab open, taken into its transcript.
impl Handles<ShowEvents> for Overlay {
    type Sends = ();

    fn handle(&mut self, shown: ShowEvents, _: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        self.chats.shown(shown.session, &shown.events);

        Ok(())
    }
}

/// An ask that waits on the owner, kept until answered.
impl Handles<ShowAsk> for Overlay {
    type Sends = ();

    fn handle(&mut self, ask: ShowAsk, turn: &mut Turn<'_, Self, ()>) -> Result<(), Fault> {
        turn.log(Level::Info, format_args!("session {} asks: {}", ask.session, ask.what));
        self.chats.asked(ask);

        Ok(())
    }
}
