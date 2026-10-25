#!/usr/bin/env bash
# Builds the static web version into ./dist:
#   index.html, awhbs-<backend>.js / _bg.wasm (+ .gz) for each backend, assets-<hash>/
# The page picks WebGPU when the browser offers it and falls back to WebGL2.
#
#   ./build-web.sh [profile] [backends]     defaults: wasm-release "webgpu webgl2"
#
# Needs: rustup target wasm32-unknown-unknown, wasm-bindgen-cli matching Cargo.lock,
# optionally wasm-opt (binaryen) for a smaller wasm.
set -euo pipefail
cd "$(dirname "$0")"

PROFILE="${1:-wasm-release}"
BACKENDS="${2:-webgpu webgl2}"
case "$PROFILE" in wasm-release) OPTIMIZE=1 ;; *) OPTIMIZE=0 ;; esac
PY="$(command -v python3 || command -v python)"

# Content hash of the assets: the directory name changes whenever any asset does, so nginx can
# mark everything in it immutable.
ASSET_HASH="$("$PY" - <<'PYEOF'
import hashlib, os
h = hashlib.sha1()
for d, _, files in sorted(os.walk("assets")):
    for f in sorted(files):
        p = os.path.join(d, f)
        h.update(os.path.relpath(p, "assets").replace(os.sep, "/").encode())
        h.update(b"\0")
        with open(p, "rb") as fh:
            h.update(fh.read())
print(h.hexdigest()[:10])
PYEOF
)"
export AWHBS_ASSET_DIR="assets-$ASSET_HASH"

WASM_OPT="${WASM_OPT:-$(command -v wasm-opt || true)}"
[ "$OPTIMIZE" = 1 ] && [ -z "$WASM_OPT" ] && echo "wasm-opt not found; skipping (the wasm will be larger)"

STAGE="target/web-stage"
rm -rf "$STAGE"
mkdir -p "$STAGE" dist

for backend in $BACKENDS; do
  echo "== building awhbs-$backend ($PROFILE, $AWHBS_ASSET_DIR)"
  cargo build -p awhbs --target wasm32-unknown-unknown --profile "$PROFILE" --features "$backend"
  wasm-bindgen --target web --no-typescript --out-dir "$STAGE" --out-name "awhbs-$backend" \
    "target/wasm32-unknown-unknown/$PROFILE/awhbs.wasm"
  if [ "$OPTIMIZE" = 1 ] && [ -n "$WASM_OPT" ]; then
    echo "running wasm-opt"
    "$WASM_OPT" -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
      --enable-mutable-globals --enable-reference-types --enable-multivalue \
      -o "$STAGE/awhbs-${backend}_bg.wasm" "$STAGE/awhbs-${backend}_bg.wasm"
  fi
  if [ "$OPTIMIZE" = 1 ]; then
    gzip -9 -k -f "$STAGE/awhbs-${backend}_bg.wasm"
    gzip -9 -k -f "$STAGE/awhbs-$backend.js"
  fi
done

bytes() { local f="$STAGE/awhbs-$1_bg.wasm"; [ -f "$f" ] && wc -c < "$f" | tr -d " " || echo 0; }
CACHE_BUST="$(git rev-parse --short HEAD 2>/dev/null || echo dev)-$(date +%s)"
sed -e "s|__ASSET_DIR__|$AWHBS_ASSET_DIR|g" \
    -e "s|__CACHE_BUST__|$CACHE_BUST|g" \
    -e "s|__WASM_BYTES_WEBGPU__|$(bytes webgpu)|" \
    -e "s|__WASM_BYTES_WEBGL2__|$(bytes webgl2)|" \
    web/index.html > "$STAGE/index.html"

# assets: copy once per hash, drop older hashed copies
if [ ! -d "dist/$AWHBS_ASSET_DIR" ]; then
  cp -r assets "dist/$AWHBS_ASSET_DIR.tmp" && mv "dist/$AWHBS_ASSET_DIR.tmp" "dist/$AWHBS_ASSET_DIR"
fi
for d in dist/assets-*; do
  [ "$d" != "dist/$AWHBS_ASSET_DIR" ] && rm -rf "$d"
done
for backend in $BACKENDS; do rm -f dist/awhbs-"$backend".js* dist/awhbs-"$backend"_bg.wasm*; done
mv -f "$STAGE"/awhbs-* dist/
mv -f "$STAGE/index.html" dist/index.html
rm -rf "$STAGE"
echo "web build ready in ./dist ($AWHBS_ASSET_DIR; webgpu $(wc -c < dist/awhbs-webgpu_bg.wasm 2>/dev/null || echo -) bytes, webgl2 $(wc -c < dist/awhbs-webgl2_bg.wasm 2>/dev/null || echo -) bytes)"
