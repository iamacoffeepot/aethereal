//! Indirect draws, on a page: items held in storage, culled at every `Render` by a dispatch in the renderer's own main
//! frame's compute pass, which writes the index of each item inside a disc into a second storage and counts them into
//! the instance count of a draw's arguments, held by a third; a placement in its pass that draws draws as many as
//! the arguments hold, each the item its index names. The root reads the arguments back and shows the count the GPU
//! wrote beside the count it expects, with how long each of the frame's passes took. `build.sh` beside it builds the
//! page; `index.html` runs it.

#[cfg(target_arch = "wasm32")]
mod page;
