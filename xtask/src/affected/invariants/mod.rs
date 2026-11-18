//! What cargo's package graph cannot see, checked on every run of xtask's suite, which every narrowed selection
//! includes ([`super::select::WORKSPACE_SCANNERS`]).
//!
//! `cargo xtask affected` selects by the graph, and narrows a change under a package's `tests/` to that package. That
//! is sound only while every coupling between packages is an edge of the graph, or a path the rules screen. When one
//! stops holding it under-selects silently: a package that should have run its tests does not, and nothing says so
//! until `main` goes red with no changed path to blame. A missed package is the one failure this tool must never have:
//! a slow suite costs minutes, a miss costs a bisect. So the properties are recomputed from the tree here, and fail the
//! pull request that breaks one, rather than recorded in a comment.
//!
//! - [`references`]: a change to a file a source reads by path, at compile time or at run time, selects at least what a
//!   change to that source does; and every compile-time read names its file in a form that check follows.
//! - [`nesting`]: a member inside another member's directory, as the headless chassis's guests are, is built by it;
//!   a change that rebuilds it, to its files or to a member it is built from, selects what a change to its builder does.
//! - [`test_isolation`]: every target compiles from inside its own package, only test targets from `tests/`, and no
//!   symlink aliases one tree into another.

mod nesting;
mod references;
mod source;
mod test_isolation;
mod workspace;
