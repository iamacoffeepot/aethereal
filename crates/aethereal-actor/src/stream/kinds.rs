//! The mail a stream is written in: its request and its parts, named by each stream, and the asker's credit and stop,
//! which every stream shares (ADR-0017).

use super::Stream;
use aethereal_data::{Bound, Kind, KindId, KindName, Plain};

/// The request that opens a stream of `S`, with the number the asker gave the stream. Only [`Streams`](super::Streams)
/// makes one.
#[derive(Bound)]
pub struct Open<S: Stream> {
    number: u64,
    value: S::Open,
}

impl<S: Stream> Open<S> {
    pub(super) const fn new(number: u64, value: S::Open) -> Self {
        Self { number, value }
    }

    /// The stream's number, and what it was asked to open with.
    pub(super) fn into_parts(self) -> (u64, S::Open) {
        (self.number, self.value)
    }
}

/// Named as what it opens with is.
impl<S: Stream> Kind for Open<S> {
    const NAME: KindName = <S::Open as Kind>::NAME;
}

/// One part of a stream of `S`, with the number of the stream it belongs to. Only a
/// [`Streaming`](super::Streaming) makes one.
#[derive(Bound)]
pub struct Part<S: Stream> {
    number: u64,
    value: S::Part,
}

impl<S: Stream> Part<S> {
    pub(super) const fn new(number: u64, value: S::Part) -> Self {
        Self { number, value }
    }

    /// The stream's number, and what the part carries.
    pub(super) fn into_parts(self) -> (u64, S::Part) {
        (self.number, self.value)
    }
}

/// Named as what it carries is.
impl<S: Stream> Kind for Part<S> {
    const NAME: KindName = <S::Part as Kind>::NAME;
}

/// The asker lets the receiver send `credit` more parts of one stream.
///
/// It names the stream by the id of its request's kind beside its number: an asker numbers each protocol's streams
/// apart, and handles each answer kind with one kind of context (ADR-0017), so it keeps one
/// [`Streams`](super::Streams) for each protocol, and no two of its streams share both.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.stream.more")]
pub struct More {
    open: KindId,
    number: u64,
    credit: u32,
}

impl More {
    pub(super) const fn new<S: Stream>(number: u64, credit: u32) -> Self {
        Self { open: KindId::of::<Open<S>>(), number, credit }
    }

    /// The number of the stream it grants credit to, when it is a stream of `S`.
    pub(super) fn number<S: Stream>(&self) -> Option<u64> {
        let is_streamed = self.open == KindId::of::<Open<S>>();

        is_streamed.then_some(self.number)
    }

    /// How many more parts it lets the receiver send.
    pub(super) const fn credit(&self) -> u32 {
        self.credit
    }
}

/// The asker takes no more parts of one stream, named as [`More`] names it.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.stream.enough")]
pub struct Enough {
    open: KindId,
    number: u64,
}

impl Enough {
    pub(super) const fn new<S: Stream>(number: u64) -> Self {
        Self { open: KindId::of::<Open<S>>(), number }
    }

    /// The number of the stream it stops, when it is a stream of `S`.
    pub(super) fn number<S: Stream>(&self) -> Option<u64> {
        let is_streamed = self.open == KindId::of::<Open<S>>();

        is_streamed.then_some(self.number)
    }
}

/// The number the asker's [`Streams`](super::Streams) gave a stream it opened: what it keeps for the stream's answer,
/// and what it names the stream by to stop it. It names a stream among one asker's and proves nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Plain)]
#[kind(name = "aether.stream.number")]
pub struct Number(u64);

impl Number {
    pub(super) const fn new(number: u64) -> Self {
        Self(number)
    }

    pub(super) const fn get(self) -> u64 {
        self.0
    }
}

/// Which stream a receiver serves: the asker's id and the number it gave the stream. It is what a receiver keeps beside
/// work it does for one stream, to find the stream again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Plain)]
#[kind(name = "aether.stream.key")]
pub struct StreamKey {
    asker: u64,
    number: u64,
}

impl StreamKey {
    pub(super) const fn new(asker: u64, number: u64) -> Self {
        Self { asker, number }
    }

    pub(super) const fn asker(self) -> u64 {
        self.asker
    }

    pub(super) const fn number(self) -> u64 {
        self.number
    }
}

/// The reason a receiver's turn fails when the actor that asked to open a stream does not take its parts. Only a
/// request [`Streams`](super::Streams) did not make can be one, since it opens streams only for an actor that handles
/// them.
#[derive(Debug, PartialEq, Eq, Plain)]
#[kind(name = "aether.stream.unreachable")]
pub struct Unreachable;
