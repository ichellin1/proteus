#!/usr/bin/env bash
# Builds the wasm size probes and the browser and Node timings, from
# checks/text-engines. Needs wasm-pack, which brings wasm-opt.
#
#   scripts/wasm.sh sizes    # out/wasm/*.opt.wasm, and their sizes
#   scripts/wasm.sh web      # out/web, for a browser: serve it and open index.html
#   scripts/wasm.sh node     # runs the timings in Node

set -euo pipefail
cd "$(dirname "$0")/.."

case "${1:-}" in
  sizes)
    opt=$(find ~/Library/Caches/.wasm-pack ~/.cache/.wasm-pack -name wasm-opt -type f 2>/dev/null | head -1)
    mkdir -p out/wasm
    for engine in none fontdue cosmic parley; do
      features=()
      [ "$engine" != none ] && features=(--features "$engine")
      cargo build -q --release --lib --target wasm32-unknown-unknown --no-default-features "${features[@]}"
      cp target/wasm32-unknown-unknown/release/text_engines.wasm "out/wasm/$engine.wasm"
      # wasm-pack's default optimization, as the SDK is built with.
      "$opt" -O "out/wasm/$engine.wasm" -o "out/wasm/$engine.opt.wasm"
      echo "$engine: $(stat -f %z "out/wasm/$engine.opt.wasm" 2>/dev/null || stat -c %s "out/wasm/$engine.opt.wasm") bytes, $(gzip -9 -c "out/wasm/$engine.opt.wasm" | wc -c | tr -d ' ') gzipped"
    done
    ;;
  web)
    wasm-pack build --release --target web --out-dir out/web/pkg --no-typescript -- --features bench
    cp web/index.html out/web/
    echo "Serve out/web, for example: python3 -m http.server -d out/web 8000"
    ;;
  node)
    wasm-pack build --release --target nodejs --out-dir out/node/pkg --no-typescript -- --features bench
    node -e "const t = require('./out/node/pkg/text_engines.js'); console.log('Node ' + process.version + '\n'); console.log(t.run_timings(30));"
    ;;
  *)
    echo "usage: scripts/wasm.sh sizes|web|node" >&2
    exit 2
    ;;
esac
