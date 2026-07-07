#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build the StormSewer WebAssembly bundle into wasm/pkg/.
#
# Requires: rustup target add wasm32-unknown-unknown
#           cargo install wasm-bindgen-cli   (version must match wasm-bindgen)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> compiling engine to wasm32"
cargo build -p stormsewer-wasm --target wasm32-unknown-unknown --release

echo "==> generating JS bindings (wasm/pkg)"
wasm-bindgen \
  target/wasm32-unknown-unknown/release/stormsewer_wasm.wasm \
  --target web --out-dir wasm/pkg

echo "==> done. Serve it:"
echo "    cd wasm && python3 -m http.server   # open http://localhost:8000"
