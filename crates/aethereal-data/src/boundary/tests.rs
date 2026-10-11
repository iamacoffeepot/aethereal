//! A boundary that makes an offer, and a bound value that needs it (ADR-0004).

use crate::{Boundary, Budget, Decode, DecodeError, Encode, EncodeError, Reader, Writer, to_bytes};
use alloc::vec;
use alloc::vec::Vec;

/// The offer a writer makes: it keeps a payload beside the bytes and says where.
trait Attach {
    /// Keeps `payload` and returns the handle it is found under.
    fn attach(&mut self, payload: &[u8]) -> u32;
}

/// The offer a reader makes: it gives back the payload kept under a handle.
trait Detach {
    /// The payload kept under `handle`, when there is one.
    fn detach(&self, handle: u32) -> Option<Vec<u8>>;
}

/// A boundary whose writer and reader carry payloads beside the bytes.
struct Carrying;

impl Boundary for Carrying {
    type Writer<'a> = CarryingWriter;
    type Reader<'a> = CarryingReader<'a>;
}

/// The bytes written so far, and the payloads attached so far.
struct CarryingWriter {
    bytes: Vec<u8>,
    attached: Vec<Vec<u8>>,
}

impl Writer for CarryingWriter {
    fn bytes(&mut self) -> &mut Vec<u8> {
        &mut self.bytes
    }
}

impl Attach for CarryingWriter {
    fn attach(&mut self, payload: &[u8]) -> u32 {
        let handle = u32::try_from(self.attached.len()).unwrap_or(u32::MAX);
        self.attached.push(payload.to_vec());

        handle
    }
}

/// The bytes still to read, the budget, and the payloads that came with them.
struct CarryingReader<'input> {
    input: &'input [u8],
    budget: Budget,
    attached: &'input [Vec<u8>],
}

impl<'input> Reader<'input> for CarryingReader<'input> {
    fn input(&mut self) -> &mut &'input [u8] {
        &mut self.input
    }

    fn budget(&mut self) -> &mut Budget {
        &mut self.budget
    }
}

impl Detach for CarryingReader<'_> {
    fn detach(&self, handle: u32) -> Option<Vec<u8>> {
        self.attached.get(handle as usize).cloned()
    }
}

/// A bound value: written as a handle, with its payload attached to the writer.
#[derive(Debug, PartialEq, Eq)]
struct Attached(Vec<u8>);

impl<B: Boundary> Encode<B> for Attached
where
    for<'a> B::Writer<'a>: Attach,
{
    fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
        let handle = out.attach(&self.0);
        Encode::<B>::encode(&handle, out)
    }
}

impl<B: Boundary> Decode<B> for Attached
where
    for<'a> B::Reader<'a>: Detach,
{
    fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
        let handle = <u32 as Decode<B>>::decode(input)?;
        input.detach(handle).map(Self).ok_or(DecodeError::UnexpectedEnd)
    }
}

/// Two bound values beside plain data, inside containers.
fn carried() -> Vec<(u8, Attached)> {
    vec![(7, Attached(vec![0xAA, 0xBB])), (9, Attached(vec![0xCC]))]
}

#[test]
fn bound_value_in_a_vec_writes_the_bytes_of_its_shape() {
    // Catches a container or leaf codec that names one boundary, drops the boundary's writer on the way to an
    // element, or writes other bytes away from `Bare`.
    let mut out = CarryingWriter { bytes: Vec::new(), attached: Vec::new() };
    let written = Encode::<Carrying>::encode(&carried(), &mut out);
    assert_eq!(written, Ok(()));
    assert_eq!(Ok(out.bytes), to_bytes(&vec![(7u8, 0u32), (9u8, 1u32)]));
    assert_eq!(out.attached, vec![vec![0xAA, 0xBB], vec![0xCC]]);
}

#[test]
fn bound_value_in_a_vec_reads_back_through_the_reader() {
    // Catches a container decode that hands its elements a reader without what the boundary offers.
    let bytes = to_bytes(&vec![(7u8, 0u32), (9u8, 1u32)]).unwrap_or_default();
    let attached = [vec![0xAA, 0xBB], vec![0xCC]];
    let mut reader = CarryingReader { input: &bytes, budget: Budget::new(256), attached: &attached };
    let read = <Vec<(u8, Attached)> as Decode<Carrying>>::decode(&mut reader);
    assert_eq!(read, Ok(carried()));
    assert!(reader.input.is_empty());
}
