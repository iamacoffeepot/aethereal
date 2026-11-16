//! How the overlay's actor serves its own tools: `set_theme` (ADR-0053: Colours), offered as it begins, in every engine,
//! and called by anyone, as it changes only how this engine's overlay looks; and the chat's words (ADR-0057: The
//! console is the chat): the first time the hub lists the sessions, which it does only to an engine it trusts, the
//! overlay offers `new_session`, `stop_session`, and `archive_session` to the engine's tools root with itself as their
//! worker, as the chat they tell the link is its own. Each call of the words is the owner's: one a session makes is
//! refused.

use super::Overlay;
use super::console::Offering;
use crate::chat::Which;
use crate::tools::{
    ARCHIVE_SESSION, ArchiveSession, ChooseTheme, NEW_SESSION, NewSession, SET_THEME, STOP_SESSION, StopSession,
    chat_sheets, chosen, theme_sheet,
};
use aethereal_actor::{Allows, Asks, Beginning, Fault, Handles, Path, ToSender, ToType, Turn, path};
use aethereal_data::{Budget, Plain, from_bytes, to_bytes};
use aethereal_link::Link;
use aethereal_sessions::Chat;
use aethereal_tools::{Call, Called, Caller, Done, Offer, Tools};

/// The most bytes reading a tool's input may take: a session's name is typed by hand.
const INPUT_BYTES: usize = 8 * 1024;

impl Overlay {
    /// Offers `set_theme` to the engine's tools root, with the overlay as its worker: as it begins, in every engine.
    pub(super) fn offer_theme<S, I>(turn: &mut Beginning<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Asks<Path<Tools>, Offer, Offering>, I>,
    {
        let offer = Offer { tools: vec![theme_sheet()], worker: turn.me().protocol() };
        turn.to(path::<Tools>(())).ask::<_, _, I>(&offer, &Offering)?;

        Ok(())
    }

    /// Offers the chat's words to the engine's tools root, with the overlay as their worker: as the hub first lists
    /// the sessions, so an engine it never trusts never offers them.
    pub(super) fn offer_words<S, I>(turn: &mut Turn<'_, Self, S>) -> Result<(), Fault>
    where
        S: Allows<Asks<Path<Tools>, Offer, Offering>, I>,
    {
        let offer = Offer { tools: chat_sheets(), worker: turn.me().protocol() };
        turn.to(path::<Tools>(())).ask::<_, _, I>(&offer, &Offering)?;

        Ok(())
    }

    /// What the word `call` names comes to: the chat it tells the link, or why it tells nothing.
    fn word(&mut self, call: &Call, input: &[u8]) -> Result<Chat, String> {
        match call.tool.as_str() {
            NEW_SESSION => read::<NewSession>(input).map(|new| self.chats.make(new.name)),
            STOP_SESSION => read::<StopSession>(input).and_then(|stop| self.chats.stop(&which(stop.session))),
            ARCHIVE_SESSION => {
                read::<ArchiveSession>(input).and_then(|archive| self.chats.archive(&which(archive.session)))
            }
            other => Err(format!("the overlay has no tool named `{other}`")),
        }
    }
}

/// A call of `set_theme`, whoever makes it: the palette changed to the theme it names, for the next frame to draw in,
/// and answered the themes. A call of one of the chat's words: the owner's told to the link and answered done, a
/// session's refused, as the sessions are the owner's.
impl Handles<Call> for Overlay {
    type Sends = (ToSender<Called>, ToType<Link, Chat>);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
        if call.tool == SET_THEME {
            let called = match read::<ChooseTheme>(turn.read(&call.input)) {
                Ok(choose) => {
                    let (palette, shown) = chosen(&self.palette, &choose);
                    self.palette = palette;
                    Called::output(turn, to_bytes(&shown)?)
                }
                Err(why) => Called::Refused(why),
            };
            turn.reply(&called)?;

            return Ok(());
        }
        let worded = match call.caller {
            Caller::Owner => self.word(&call, turn.read(&call.input)),
            Caller::Session { session } => Err(format!("the chat's words are the owner's, not session {session}'s")),
        };
        let called = match worded {
            Ok(chat) => {
                self.tell(turn, vec![chat])?;
                Called::output(turn, to_bytes(&Done {})?)
            }
            Err(why) => Called::Refused(why),
        };
        turn.reply(&called)?;

        Ok(())
    }
}

/// The input `bytes` as a `K`, or why they are not one.
fn read<K: Plain>(bytes: &[u8]) -> Result<K, String> {
    from_bytes::<K>(bytes, &mut Budget::new(INPUT_BYTES)).map_err(|fault| format!("the input: {fault}"))
}

/// The session a word's input names: the one so named, or with none named the one whose tab is open.
fn which(session: Option<String>) -> Which {
    session.map_or(Which::Open, Which::Named)
}
