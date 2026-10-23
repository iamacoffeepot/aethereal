//! Streams: a request held open, with parts sent to the asker before its answer (ADR-0017).
//!
//! Nothing in the engine is a stream. The asker asks; the receiver holds the
//! request, sends the asker parts as plain mail, and answers once to end it.
//! One sender's mail arrives in the order it was sent (ADR-0009), so every
//! part comes before the answer, and the held request ends every stream once.
//! What the engine does not give a stream, which open stream a part belongs to,
//! how fast parts come, and stopping early, is written here, once: the asker
//! opens streams through [`Streams`], and the receiver serves them through
//! [`Serving`].

mod kinds;
mod serving;
mod streams;

pub use kinds::{Enough, More, Number, Open, Part, StreamKey, Unreachable};
pub use serving::{Serving, Streaming, Unsent};
pub use streams::{Streams, Taken};

use crate::{Actors, Carries, Protocol};
use aethereal_data::{Decode, Encode, Kind, KindName, Schema};
use core::marker::PhantomData;

/// What a stream is opened with, what each of its parts carries, and what ends it, with the names its request and its
/// parts go by (ADR-0017).
///
/// A kind's id is its name and its layout with every name inside it removed (ADR-0006), so one name for every
/// stream's parts would give the parts of two streams of one layout one id. Each stream names its own:
///
/// ```
/// use aethereal_actor::Stream;
/// use aethereal_data::{KindName, Plain};
///
/// #[derive(Plain)]
/// #[kind(name = "aether.doc.read")]
/// struct Read {
///     path: String,
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.doc.chunk")]
/// struct Chunk {
///     bytes: Vec<u8>,
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.doc.read_done")]
/// enum ReadDone {
///     Ok,
///     Stopped,
/// }
///
/// struct Files;
///
/// impl Stream for Files {
///     type Open = Read;
///     type Part = Chunk;
///     type Done = ReadDone;
///
///     const OPEN: KindName = KindName::new("aether.doc.files.open");
///     const PART: KindName = KindName::new("aether.doc.files.part");
/// }
/// ```
pub trait Stream: 'static {
    /// What the asker asks to open a stream with.
    type Open: Schema + Encode<Actors> + Decode<Actors> + 'static;
    /// What each part carries.
    type Part: Schema + Encode<Actors> + Decode<Actors> + 'static;
    /// The answer that ends a stream.
    type Done: Kind + Encode<Actors> + Decode<Actors> + 'static;

    /// The name of this stream's request, [`Open<Self>`].
    const OPEN: KindName;
    /// The name of this stream's parts, [`Part<Self>`].
    const PART: KindName;
}

/// The protocol a stream of `S` is opened through: its request, answered with `S::Done`, and the asker's [`More`] and
/// [`Enough`] (ADR-0016). An actor that serves the stream handles all three, and is asked through a
/// `ProtocolRef<Streamed<S>>`.
pub struct Streamed<S>(PhantomData<fn() -> S>);

impl<S: Stream> Protocol for Streamed<S> {
    type Kinds = (Open<S>, More, Enough);
}

impl<S: Stream> Carries<Open<S>> for Streamed<S> {
    type Answers = (S::Done,);
}

impl<S: Stream> Carries<More> for Streamed<S> {
    type Answers = ();
}

impl<S: Stream> Carries<Enough> for Streamed<S> {
    type Answers = ();
}
