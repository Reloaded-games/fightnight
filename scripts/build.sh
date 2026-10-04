#!/usr/bin/env bash
# Build the wasm game and assemble a static site in ./dist
#   scripts/build.sh            release build (optimised)
#   scripts/build.sh dev        fast debug build
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE="${1:-release}"
if [ "$PROFILE" = "release" ]; then
  cargo build -p fn-web --target wasm32-unknown-unknown --release
  OUT=release
else
  cargo build -p fn-web --target wasm32-unknown-unknown
  OUT=debug
fi

rm -rf dist
mkdir -p dist/pkg
wasm-bindgen --target web --no-typescript --out-dir dist/pkg --out-name fightnight \
  "target/wasm32-unknown-unknown/${OUT}/fn_web.wasm"
cp -r web/. dist/

if [ "$PROFILE" = "release" ] && command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
    --enable-mutable-globals -o dist/pkg/fightnight_bg.wasm dist/pkg/fightnight_bg.wasm || true
fi
echo "built dist/ ($(du -h dist/pkg/fightnight_bg.wasm | cut -f1) wasm)"
