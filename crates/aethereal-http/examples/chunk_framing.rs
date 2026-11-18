//! What framing a chunk of a streamed response costs a call: the line before each chunk, and what ends the body, as the
//! stack writes them (`new`: its own `chunk` module, built in here from its source file) beside the `format!` and
//! `concat` they replaced (`old`, kept here only to compare). Each lands in a buffer kept from call to call, as a blob's
//! does: the turn hands out a buffer the actor keeps (ADR-0042), so an old call's own allocation is all it adds.
//!
//! Run with `cargo run --release -p aethereal-http --example chunk_framing`. Each line is the best of five runs of a
//! million calls: nanoseconds a call, and heap allocations a call.

#![allow(clippy::print_stdout)] // A bench prints its table.
#![allow(clippy::cast_precision_loss)] // A rate printed to two places loses nothing it shows.

#[path = "../src/stack/chunk.rs"]
mod chunk;

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Calls in a run.
const CALLS: u32 = 1_000_000;

/// The sizes of chunk framed: a part of 4 KiB, 64 KiB, and 1 MiB.
const LENGTHS: [usize; 3] = [4 << 10, 64 << 10, 1 << 20];

/// Allocations made since the program began. An atomic, since the allocator is called from any of the process's
/// threads, which no mail orders: this is a native program, not an actor.
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

/// The system's allocator, counting each allocation.
struct Counting;

// SAFETY: every call is passed to the system's allocator unchanged; the count beside it allocates nothing.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract for `alloc` is passed on as it came.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract for `dealloc` is passed on as it came.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The framing the exchange wrote before, kept here only to compare.
mod old {
    /// What ends the last chunk of a chunked body, and its empty trailer.
    const LAST_CHUNK: &[u8] = b"0\r\n\r\n";

    /// The line before a chunk of `length` bytes, after the line ending of the chunk before it when one `follows`.
    pub fn chunk_line(length: usize, follows: bool) -> Vec<u8> {
        let ending = if follows {
            "\r\n"
        } else {
            ""
        };

        format!("{ending}{length:x}\r\n").into_bytes()
    }

    /// What ends a chunked body, after the line ending of the chunk before it when one `follows`.
    pub fn last_chunk(follows: bool) -> Vec<u8> {
        let ending: &[u8] = if follows {
            b"\r\n"
        } else {
            b""
        };

        [ending, LAST_CHUNK].concat()
    }
}

/// The best of five runs of `call` made `CALLS` times on a buffer kept from call to call, of the room the exchange
/// gives a chunk's line: nanoseconds a call, and allocations a call.
fn best(mut call: impl FnMut(&mut Vec<u8>)) -> (f64, f64) {
    let mut buffer = Vec::with_capacity(chunk::LINE_BYTES);
    let mut fastest = u128::MAX;
    let mut allocations = 0;
    for _ in 0..5 {
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        #[allow(clippy::disallowed_methods)] // A bench times itself on the native host it runs on.
        let started = Instant::now();
        for _ in 0..CALLS {
            buffer.clear();
            call(&mut buffer);
            black_box(&buffer);
        }
        fastest = fastest.min(started.elapsed().as_nanos());
        allocations = ALLOCATIONS.load(Ordering::Relaxed) - before;
    }

    (fastest as f64 / f64::from(CALLS), allocations as f64 / f64::from(CALLS))
}

fn main() {
    println!("framing          length   old ns/call  old allocs   new ns/call  new allocs");
    for length in LENGTHS {
        let (old_nanos, old_allocations) =
            best(|buffer| buffer.extend_from_slice(&old::chunk_line(black_box(length), black_box(true))));
        let (new_nanos, new_allocations) = best(|buffer| chunk::line(black_box(length), black_box(true), buffer));
        println!(
            "chunk line   {length:10}   {old_nanos:11.2}  {old_allocations:10.2}   {new_nanos:11.2}  {new_allocations:10.2}"
        );
    }
    let (old_nanos, old_allocations) = best(|buffer| buffer.extend_from_slice(&old::last_chunk(black_box(true))));
    let (new_nanos, new_allocations) = best(|buffer| buffer.extend_from_slice(chunk::last(black_box(true))));
    println!(
        "last chunk            -   {old_nanos:11.2}  {old_allocations:10.2}   {new_nanos:11.2}  {new_allocations:10.2}"
    );
}
