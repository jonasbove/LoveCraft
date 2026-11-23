#!/usr/bin/env bash
# Build the web app and package it for hosting:
#
#   $DIST/lovecraft-web-<version>.zip   lovecraft-web-<version>/{index.html,*.js,*.wasm,...}
#                                       plus README.md, LICENSE-MIT, LICENSE-APACHE, NOTICE and
#                                       ATTRIBUTION.md
#
# The layout of the Craft apps' web zips (wordcraft-web-<version>.zip): one top-level folder with
# Trunk's dist inside, which CraftCloud's `cargo xtask fetch-apps` unpacks to /apps/<app>/. Asset
# URLs are relative (`--public-url ./`), so the app works under any sub-path. The zip is named for
# the app (LoveCraft) since 0.2.0 (prosecraft-rs-web-<version>.zip before); CraftCloud still knows
# the app by its id, prosecraft-rs (apps/prosecraft-rs-web/src/hosted.rs).
#
# Usage: packaging/web/package.sh [--skip-build]
#
# Needs: trunk, wasm-bindgen and wasm-opt at the versions apps/prosecraft-rs-web/Trunk.toml pins
# (trunk downloads them when they aren't installed), the wasm32-unknown-unknown target, zip, and
# a craft-fonts checkout (CRAFT_FONTS_DIR, see ../env.sh). The build uses Cargo.lock as it is
# (`--locked`).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

SKIP_BUILD=0
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    -h | --help) sed -n '2,18p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

NAME="lovecraft-web-$VERSION"
WEB_DIST="$CARGO_TARGET_DIR/web-dist"
STAGE="$CARGO_TARGET_DIR/web-stage"

if [[ "$SKIP_BUILD" == 0 ]]; then
  rm -rf "$WEB_DIST"
  (cd "$ROOT/apps/prosecraft-rs-web" && trunk build --release --locked --public-url ./ --dist "$WEB_DIST")
fi
[[ -f "$WEB_DIST/index.html" ]] || { echo "error: no $WEB_DIST/index.html (build first)" >&2; exit 1; }

# Every URL index.html loads must be relative: an absolute one ("/x.js") would miss the app's
# folder when it is served under /apps/<app>/.
if grep -oE '(src|href|module_or_path)[=:] *["'"'"'][^"'"'"']*' "$WEB_DIST/index.html" | grep -E "[\"'](/|https?:)"; then
  echo "error: index.html loads an absolute URL (above); build with --public-url ./" >&2
  exit 1
fi

rm -rf "$STAGE"
mkdir -p "$STAGE/$NAME"
cp -R "$WEB_DIST/." "$STAGE/$NAME/"
# Trunk's lock file for its own builds; nothing serves it.
rm -f "$STAGE/$NAME/.trunk-lock"
copy_docs "$STAGE/$NAME"
cp "$ROOT/ATTRIBUTION.md" "$STAGE/$NAME/"

zip_file="$DIST/$NAME.zip"
rm -f "$zip_file"
(cd "$STAGE" && find "$NAME" -print | LC_ALL=C sort | zip -X -q "$zip_file" -@)
unzip -l "$zip_file"
echo "wrote $zip_file"
