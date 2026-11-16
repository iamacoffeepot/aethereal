//! How the overlay's actor serves the chat's words (ADR-0057: The console is the chat): the first time the hub lists
//! the sessions, which it does only to an engine it trusts, the overlay offers `new`, `stop`, and `archive` to the
//! engine's tools root with itself as their worker, as the chat they tell the link is its own. Each call of them is the
//! owner's: one a session makes is refused.

use super::Overlay;
use super::console::Offering;
use crate::chat::Which;
use crate::tools::{ARCHIVE, ArchiveSession, NEW, NewSession, STOP, StopSession, chat_sheets};
use aethereal_actor::{Allows, Asks, Fault, Handles, Path, ToSender, ToType, Turn, path};
use aethereal_data::{Budget, Plain, from_bytes, to_bytes};
use aethereal_link::Link;
use aethereal_sessions::Chat;
use aethereal_tools::{Call, Called, Caller, Done, Offer, Tools};

/// The most bytes reading a word's input may take: a session's name is typed by hand.
const INPUT_BYTES: usize = 8 * 1024;

impl Overlay {
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
            NEW => read::<NewSession>(input).map(|new| self.chats.make(new.name)),
            STOP => read::<StopSession>(input).and_then(|stop| self.chats.stop(&which(stop.session))),
            ARCHIVE => read::<ArchiveSession>(input).and_then(|archive| self.chats.archive(&which(archive.session))),
            other => Err(format!("the overlay has no tool named `{other}`")),
        }
    }
}

/// A call of one of the chat's words: the owner's told to the link and answered done, a session's refused, as the
/// sessions are the owner's.
impl Handles<Call> for Overlay {
    type Sends = (ToSender<Called>, ToType<Link, Chat>);

    fn handle(&mut self, call: Call, turn: &mut Turn<'_, Self, Self::Sends>) -> Result<(), Fault> {
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
