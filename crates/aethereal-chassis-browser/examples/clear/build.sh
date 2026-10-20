#!/usr/bin/env bash
# Builds the page into pkg/ beside this script: the example for wasm32, then the bindings the page imports. Serve the
# directory over http on localhost, where WebGPU is let run, and open index.html.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"
cargo build --release --target wasm32-unknown-unknown --example clear
target="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"
wasm-bindgen --target web --no-typescript --out-dir pkg "$target/wasm32-unknown-unknown/release/examples/clear.wasm"
echo "built bytes=$(wc -c < pkg/clear_bg.wasm)"
