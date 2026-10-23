//! Indirect draws, on a page: items held in storage, culled at every `Render` by an ordered compaction in three compute
//! passes of the window's main frame, which the root makes. The cull, `cull.wesl`, imports the shared `scan` and
//! `compact` pieces and is linked by the build script: it writes the index of each item inside a disc into a second
//! storage, in the items' order, and how many into the instance count of a draw's arguments, held by a third; a
//! placement in the frame's pass that draws draws as many as the arguments hold, each the item its index names. The
//! root reads the arguments and the list back and shows the count the GPU wrote beside the count it expects, whether
//! the list holds the kept items in order, and how long each of the frame's passes took.
//! `cargo xtask example indirect --serve` builds the page and serves it.

#[cfg(target_arch = "wasm32")]
mod page;
#[cfg(target_arch = "wasm32")]
#[path = "../shared/said.rs"]
mod said;
