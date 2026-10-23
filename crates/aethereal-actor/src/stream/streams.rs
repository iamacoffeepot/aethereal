//! The asker's side of its streams: opening them, taking their parts, stopping them, and their answers (ADR-0017).

use super::{Enough, More, Number, Open, Part, Stream, Streamed};
use crate::{Allows, Answer, Asks, Handles, ProtocolRef, ToAny, Turn};
use aethereal_data::{ActorId, Bound, EncodeError, Plain};
use alloc::collections::BTreeMap;
use core::num::NonZeroU32;

/// The streams of `S` an actor has opened and not yet had answered, each with the `C` it keeps for it (ADR-0017).
///
/// It numbers each stream it opens, and keeps each stream's receiver at most
/// `window` parts ahead of what the actor has taken: it grants that many as
/// the stream opens, and one more for each part taken. An actor keeps one for
/// each protocol it opens streams through, as it handles each answer kind with
/// one kind of context. It is a field of its holder's state.
#[derive(Bound)]
pub struct Streams<S: Stream, C> {
    window: u32,
    next: u64,
    open: BTreeMap<u64, Opened<S, C>>,
}

/// A part an actor took: the stream it belongs to, what it carries, and what the actor keeps for the stream.
pub struct Taken<'s, S: Stream, C> {
    /// The stream's number, to stop it by.
    pub number: Number,
    /// What the part carries.
    pub value: S::Part,
    /// What the actor keeps for the stream.
    pub kept: &'s mut C,
}

/// One stream opened: whom it was asked of, whether parts are still taken, and what the asker keeps for it.
#[derive(Bound)]
struct Opened<S: Stream, C> {
    receiver: ProtocolRef<Streamed<S>>,
    flow: Flow,
    kept: C,
}

/// Whether the asker still takes a stream's parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
enum Flow {
    Flowing,
    Stopped,
}

impl<S: Stream, C> Opened<S, C> {
    /// Whether a part from `sender` is taken: it comes from the receiver, and the stream is not stopped.
    fn takes_from(&self, sender: ActorId) -> bool {
        let from_receiver = self.receiver.id() == sender;
        let flowing = self.flow == Flow::Flowing;

        from_receiver && flowing
    }
}

impl<S: Stream, C> Streams<S, C> {
    /// None open, each to be kept at most `window` parts ahead.
    #[must_use]
    pub const fn new(window: NonZeroU32) -> Self {
        Self { window: window.get(), next: 0, open: BTreeMap::new() }
    }

    /// Opens a stream of `S` asked of `receiver` with `request`, keeping `kept` for it, and grants the receiver its
    /// first `window` parts. Answers the stream's number.
    ///
    /// It builds only when the actor handles the stream's parts and its answer, and the turn's sends list the request
    /// and the credit.
    ///
    /// # Errors
    ///
    /// Why the request or the credit could not be written. Nothing is opened.
    pub fn open<A, Sends, IA, IM>(
        &mut self,
        turn: &mut Turn<'_, A, Sends>,
        receiver: ProtocolRef<Streamed<S>>,
        request: S::Open,
        kept: C,
    ) -> Result<Number, EncodeError>
    where
        A: Handles<Part<S>> + Handles<Answer<S::Done, Number>>,
        Sends: Allows<Asks<Streamed<S>, Open<S>, Number>, IA> + Allows<ToAny<Streamed<S>, More>, IM>,
    {
        let number = Number::new(self.next);
        let credit = More::new::<S>(number.get(), self.window);

        turn.to(receiver).ask::<_, _, IA>(&Open::new(number.get(), request), &number)?.send::<_, IM>(&credit)?;

        self.next += 1;
        self.open.insert(number.get(), Opened { receiver, flow: Flow::Flowing, kept });

        Ok(number)
    }

    /// What `part` carries, taken for its stream, granting the receiver one more part: none, and no credit, when the
    /// part is of no stream open, of one stopped, or from an actor that is not its receiver.
    ///
    /// # Errors
    ///
    /// Why the credit could not be written.
    pub fn part<A, Sends, IM>(
        &mut self,
        turn: &mut Turn<'_, A, Sends>,
        part: Part<S>,
    ) -> Result<Option<Taken<'_, S, C>>, EncodeError>
    where
        Sends: Allows<ToAny<Streamed<S>, More>, IM>,
    {
        let sender = turn.sender().id();
        let (number, value) = part.into_parts();
        let Some(opened) = self.open.get_mut(&number) else {
            return Ok(None);
        };
        if !opened.takes_from(sender) {
            return Ok(None);
        }

        turn.to(opened.receiver).send::<_, IM>(&More::new::<S>(number, 1))?;

        Ok(Some(Taken { number: Number::new(number), value, kept: &mut opened.kept }))
    }

    /// Stops the stream `number`: its receiver is told, and its parts that arrive after are passed over. Its answer
    /// still comes, and [`answered`](Self::answered) hands back what the actor kept. Answers false, and sends
    /// nothing, when the stream is not open or is stopped already.
    ///
    /// # Errors
    ///
    /// Why the stop could not be written. The stream is then not stopped.
    pub fn stop<A, Sends, IS>(&mut self, turn: &mut Turn<'_, A, Sends>, number: Number) -> Result<bool, EncodeError>
    where
        Sends: Allows<ToAny<Streamed<S>, Enough>, IS>,
    {
        let Some(opened) = self.open.get_mut(&number.get()) else {
            return Ok(false);
        };
        if opened.flow == Flow::Stopped {
            return Ok(false);
        }

        turn.to(opened.receiver).send::<_, IS>(&Enough::new::<S>(number.get()))?;
        opened.flow = Flow::Stopped;

        Ok(true)
    }

    /// What the actor kept for the stream `number`, which its answer has ended: none when the stream is not open here.
    pub fn answered(&mut self, number: Number) -> Option<C> {
        self.open.remove(&number.get()).map(|opened| opened.kept)
    }

    /// How many streams are open.
    #[must_use]
    pub fn len(&self) -> usize {
        self.open.len()
    }

    /// Whether no stream is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }
}
