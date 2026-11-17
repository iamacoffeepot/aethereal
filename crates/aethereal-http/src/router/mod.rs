//! A router: the last layer of a stack with routes, picking the handler by method and path (ADR-0035).
//!
//! A route is a method and a pattern, `/` and then segments, each a literal or a `{name}` capture covering the whole
//! segment. The routes are compiled to a [`Trie`] as the stack begins, refusing a route whose pattern is malformed,
//! whose captures are not the names its [`Captures`] type reads, or that matches a path another route does for the
//! same method. A request is then one walk down it, against the path as sent: it [finds](Trie::find) the one route
//! for its method, with the slices of the path its captures matched; or the methods the path's routes take, for
//! `405`; or nothing, for `404`.
//!
//! The trie holds each route's value, a type of its user's: the stack's typed router keeps what it needs to ask the
//! route's handler there.

#![cfg_attr(
    not(test),
    expect(dead_code, unused_imports, reason = "the stack's typed router is the trie's one user, and comes with it")
)]

mod captures;
mod pattern;
mod trie;
mod unroutable;
mod walk;

pub use captures::Captures;
pub use trie::{Found, Routed, Trie};
pub use unroutable::Unroutable;

#[cfg(test)]
mod tests;
