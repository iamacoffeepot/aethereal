//! The receiver's side of the streams it serves: the requests that open them, the credit and stops that reach them,
//! their parts, and their answers (ADR-0017).

use super::{Enough, More, Open, Part, Stream, StreamKey};
use crate::{Ended, Guard, Held, Later, ProtocolRef, Receives, Speaks, Turn};
use aethereal_data::{Bound, EncodeError};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// The streams of `S` an actor serves, by asker and then by the number the asker gave each (ADR-0017).
///
/// Each asker is watched as its first stream opens, so the streams of one
/// that ends are handed back to the holder's handler of endings, through
/// [`ended`](Self::ended). It is a field of its holder's state.
#[derive(Bound)]
pub struct Serving<S: Stream> {
    streams: BTreeMap<u64, BTreeMap<u64, Streaming<S>>>,
}

/// One stream an actor serves: the asker it sends parts to, the request it holds, the stream's number, and how many
/// more parts the asker lets it send.
///
/// It is answered once: [`finish`](Self::finish) takes it, so no part is sent
/// after its answer. One dropped unfinished drops the request it holds, which
/// the engine answers as a request let go of (ADR-0017).
#[derive(Bound)]
pub struct Streaming<S: Stream> {
    asker: ProtocolRef<(Part<S>,)>,
    held: Held<S::Done>,
    number: u64,
    credit: u32,
}

/// Why a part was not sent.
#[derive(Debug, PartialEq, Eq)]
pub enum Unsent<P> {
    /// The asker has let the stream send no more. The part is handed back, to send when credit comes.
    NoCredit(P),
    /// The part could not be written.
    Unwritable(EncodeError),
}

/// What opening a stream answers: its key, what it was asked to open with, and the [`Later`] the handler of `Open<S>`
/// returns.
type Opened<S> = (StreamKey, <S as Stream>::Open, Later<<S as Stream>::Done>);

impl<S: Stream> Serving<S> {
    /// None served.
    #[must_use]
    pub const fn new() -> Self {
        Self { streams: BTreeMap::new() }
    }

    /// Opens the stream `open` asks for, holding its request, and answers its key, what it was asked to open with,
    /// and the [`Later`] the handler of `Open<S>` returns. The stream may send nothing until the asker's credit
    /// comes, which follows the request.
    ///
    /// It builds only when the actor handles endings, and in the turn of a handler that takes `Open<S>` from
    /// [`Speaks<(Part<S>,)>`](Speaks): an asker whose type takes the stream's parts, as it sent the request (ADR-0058).
    /// That is checked from what the request carries, so an asker that ended after asking is served as it asked, and
    /// its ending, which its watch hands on at once, lets the stream go. A request from an actor that takes no parts
    /// is refused before the handler runs.
    pub fn open<A>(&mut self, turn: &mut Turn<'_, A, Speaks<(Part<S>,)>>, open: Open<S>) -> Opened<S>
    where
        A: Receives<Ended>,
    {
        let asker = *turn.from();
        let (held, later) = turn.hold::<S::Done>();
        let (number, value) = open.into_parts();
        let key = StreamKey::new(asker.id().bits(), number);

        let is_new_asker = !self.streams.contains_key(&key.asker());
        if is_new_asker {
            turn.watch(asker);
        }
        self.streams.entry(key.asker()).or_default().insert(number, Streaming { asker, held, number, credit: 0 });

        (key, value, later)
    }

    /// Adds the credit `more` grants to the stream it names, and answers that stream's key: none when it names no
    /// stream of `S` served here, such as one already finished.
    pub fn more<A>(&mut self, turn: &Turn<'_, A>, more: &More) -> Option<StreamKey> {
        let number = more.number::<S>()?;
        let streaming = self.streams.get_mut(&turn.sender().id().bits())?.get_mut(&number)?;
        streaming.credit = streaming.credit.saturating_add(more.credit());

        Some(streaming.key())
    }

    /// The stream `stop` stops, taken out to be finished: none when it names no stream of `S` served here.
    pub fn stop<A>(&mut self, turn: &Turn<'_, A>, stop: &Enough) -> Option<Streaming<S>> {
        let number = stop.number::<S>()?;

        self.take(StreamKey::new(turn.sender().id().bits(), number))
    }

    /// The streams of the asker `ended` says has ended, taken out: their parts would reach no one, and dropping them
    /// lets go of the requests they hold.
    pub fn ended(&mut self, ended: &Ended) -> Vec<Streaming<S>> {
        let streams = self.streams.remove(&ended.actor().id().bits());

        streams.map(|streams| streams.into_values().collect()).unwrap_or_default()
    }

    /// The stream `key` names, to send to: none when it is not served here.
    pub fn get_mut(&mut self, key: StreamKey) -> Option<&mut Streaming<S>> {
        self.streams.get_mut(&key.asker())?.get_mut(&key.number())
    }

    /// Whether the stream `key` names is served here and may be sent another part.
    #[must_use]
    pub fn has_credit(&self, key: StreamKey) -> bool {
        let streaming = self.streams.get(&key.asker()).and_then(|streams| streams.get(&key.number()));

        streaming.is_some_and(|streaming| streaming.credit > 0)
    }

    /// The stream `key` names, taken out to be finished: none when it is not served here.
    pub fn take(&mut self, key: StreamKey) -> Option<Streaming<S>> {
        let streams = self.streams.get_mut(&key.asker())?;
        let streaming = streams.remove(&key.number());
        if streams.is_empty() {
            self.streams.remove(&key.asker());
        }

        streaming
    }

    /// How many streams are served.
    #[must_use]
    pub fn len(&self) -> usize {
        self.streams.values().map(BTreeMap::len).sum()
    }

    /// Whether no stream is served.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.streams.is_empty()
    }
}

impl<S: Stream> Default for Serving<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: Stream> Streaming<S> {
    /// Which stream it is.
    #[must_use]
    pub fn key(&self) -> StreamKey {
        StreamKey::new(self.asker.id().bits(), self.number)
    }

    /// How many more parts the asker lets it send.
    #[must_use]
    pub const fn credit(&self) -> u32 {
        self.credit
    }

    /// Sends the asker a part carrying `value`, spending one credit.
    ///
    /// Its send is checked at its call, as any is: the asker's reference takes the stream's parts.
    ///
    /// # Errors
    ///
    /// [`Unsent::NoCredit`], with `value` handed back, when the asker lets it send no more; [`Unsent::Unwritable`]
    /// when the part could not be written.
    pub fn send<A, F: Guard<A>>(&mut self, turn: &mut Turn<'_, A, F>, value: S::Part) -> Result<(), Unsent<S::Part>> {
        if self.credit == 0 {
            return Err(Unsent::NoCredit(value));
        }

        turn.to(self.asker).send(&Part::new(self.number, value)).map_err(Unsent::Unwritable)?;
        self.credit -= 1;

        Ok(())
    }

    /// Ends the stream with `done`, its answer, in this turn.
    ///
    /// # Errors
    ///
    /// Why `done` could not be written. The request is then let go of, and the engine answers it as one let go of.
    pub fn finish<A, F: Guard<A>>(self, turn: &mut Turn<'_, A, F>, done: &S::Done) -> Result<(), EncodeError> {
        turn.answer(self.held, done)
    }
}
