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

pub use kinds::{Enough, More, Number, Open, Part, StreamKey};
pub use serving::{Serving, Streaming, Unsent};
pub use streams::{Streams, Taken};

use crate::{Actors, protocol};
use aethereal_data::{Decode, Encode, Kind};
use core::marker::PhantomData;

/// What a stream is opened with, what each of its parts carries, and what ends it (ADR-0017).
///
/// Its request, [`Open<Self>`], goes by the name of `Open`, and its parts, [`Part<Self>`], by the name of `Part`, each
/// with the stream's number beside the value. A kind's id is its name and its layout (ADR-0006), so a kind is the
/// request or the part of one stream: two streams that share one would share a kind id, and a type that handles both
/// does not build.
///
/// ```
/// use aethereal_actor::{Anyone, Bytes, Stream};
/// use aethereal_data::{Bound, Plain};
///
/// #[derive(Plain)]
/// #[kind(name = "aether.doc.read")]
/// struct Read {
///     path: String,
/// }
///
/// #[derive(Bound)]
/// #[kind(name = "aether.doc.chunk")]
/// struct Chunk {
///     bytes: Bytes,
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
/// }
/// ```
///
/// Two streams whose parts are one kind, taken by one actor, do not build:
///
/// ```compile_fail
/// # use aethereal_actor::{Declared, Fault, Part, Stream, actor};
/// # use aethereal_data::Plain;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.read")]
/// # struct Read;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.chunk")]
/// # struct Chunk;
/// #
/// # #[derive(Plain)]
/// # #[kind(name = "aether.doc.done")]
/// # struct Done;
/// #
/// struct Files;
///
/// impl Stream for Files {
///     type Open = Read;
///     type Part = Chunk;
///     type Done = Done;
/// }
///
/// struct Logs;
///
/// impl Stream for Logs {
///     type Open = Read;
///     type Part = Chunk;
///     type Done = Done;
/// }
///
/// #[derive(Plain)]
/// #[kind(name = "aether.doc.reader")]
/// struct Reader;
///
/// #[actor]
/// impl Reader {
///     fn begin(reader: Reader) -> Result<Self, Fault> {
///         Ok(reader)
///     }
///
///     #[tell]
///     fn files(&mut self, _: Part<Files>) -> Result<(), Fault> {
///         Ok(())
///     }
///
///     #[tell]
///     fn logs(&mut self, _: Part<Logs>) -> Result<(), Fault> {
///         Ok(())
///     }
/// }
///
/// const DECLARED: &Declared = Declared::of::<Reader>();
/// ```
pub trait Stream: 'static {
    /// What the asker asks to open a stream with.
    type Open: Kind + Encode<Actors> + Decode<Actors> + 'static;
    /// What each part carries.
    type Part: Kind + Encode<Actors> + Decode<Actors> + 'static;
    /// The answer that ends a stream.
    type Done: Kind + Encode<Actors> + Decode<Actors> + 'static;
}

/// The protocol a stream of `S` is opened through: its request, answered with `S::Done`, and the asker's [`More`] and
/// [`Enough`] (ADR-0016). An actor that serves the stream handles all three, and is asked through a
/// `ProtocolRef<Streamed<S>>`.
#[protocol(Open<S>(answers = S::Done), More, Enough)]
pub struct Streamed<S: Stream>(PhantomData<fn() -> S>);
