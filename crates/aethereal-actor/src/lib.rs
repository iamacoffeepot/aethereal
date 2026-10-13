//! What an actor's code is written against (ADR-0009).
//!
//! An actor asks the engine for things as effects. This crate holds what an
//! author names and nothing of the engine, so a guest, which is compiled apart
//! from the engine, depends on it alone. The engine depends on it too, since
//! it runs turns and applies their effects.

#![no_std]
#![forbid(unsafe_code)]

mod effect;

pub use effect::{Code, Effect, What};
