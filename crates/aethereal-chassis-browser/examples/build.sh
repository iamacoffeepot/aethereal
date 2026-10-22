#!/usr/bin/env bash
# Builds one example's page into pkg/ in its directory: the example for wasm32, the bindings the page imports, and the
# page, made from shared/index.html. The page's layout is the example's own layout.css and layout.html when it has them,
# and otherwise shared/'s, one canvas filling the page. Serve that pkg/ over http on localhost, where WebGPU is let run,
# and open index.html.
#
#   build.sh <example>
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here"

if [[ $# -ne 1 || ! -f "$1/lib.rs" ]]; then
  echo "usage: build.sh <example>, one of:" $(ls -d */lib.rs | sed 's|/lib.rs||') >&2
  exit 2
fi
name="$1"
out="$name/pkg"

cargo build --release --target wasm32-unknown-unknown --example "$name"
target="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"
wasm-bindgen --target web --no-typescript --out-dir "$out" "$target/wasm32-unknown-unknown/release/examples/$name.wasm"

python3 - "$name" "$out/index.html" <<'PAGE'
import pathlib, sys

name, out = sys.argv[1], pathlib.Path(sys.argv[2])
page = pathlib.Path("shared/index.html").read_text()
for part in ("layout.css", "layout.html"):
    own = pathlib.Path(name, part)
    chosen = own if own.exists() else pathlib.Path("shared", part)
    page = page.replace("{{" + part + "}}", chosen.read_text().rstrip("\n"))
out.write_text(page.replace("{{name}}", name))
PAGE

echo "built example=$name bytes=$(wc -c < "$out/${name}_bg.wasm") page=$out/index.html"
