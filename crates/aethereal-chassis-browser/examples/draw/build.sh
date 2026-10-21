#!/usr/bin/env bash
# Builds the page into pkg/ beside this script: the example for wasm32, then the bindings the page imports. Serve the
# directory over http on localhost, where WebGPU is let run, and open index.html.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"
cargo build --release --target wasm32-unknown-unknown --example draw
target="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"
wasm-bindgen --target web --no-typescript --out-dir pkg "$target/wasm32-unknown-unknown/release/examples/draw.wasm"
echo "built bytes=$(wc -c < pkg/draw_bg.wasm)"
