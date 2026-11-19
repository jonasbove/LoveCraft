#!/usr/bin/env bash
# Render the app icon (packaging/icons/prosecraft-rs-icon-2048.png) into every size the app and
# the packages use:
#
#   packaging/linux/icons/hicolor/<n>x<n>/apps/io.github.jonasbove.prosecraft_rs.png   n = 16 … 512
#   packaging/macos/prosecraft-rs-1024.png    with the 16-512 PNGs, macos/package.sh makes the .icns
#   packaging/windows/prosecraft-rs.ico       16, 24, 32, 48, 64, 128 and 256 (the MSI's icon)
#   apps/prosecraft-rs/assets/icon-256.png    the desktop window's icon (main.rs embeds it)
#   apps/prosecraft-rs-web/assets/favicon.png the web app's favicon (64 px)
#
# Usage: packaging/icons/render.sh        (needs ImageMagick 7's `magick`; optipng if installed)
#
# The master is a raster engraving, so every size is a Lanczos downscale of it; 32 px and smaller
# get a light unsharp mask so the tentacles stay apart. To change the icon, replace the master,
# run this and commit what it wrote (packaging/icons/README.md). packaging/check-icons.sh, which
# this runs at the end, checks that every file is there with its size.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
APP_ID=io.github.jonasbove.prosecraft_rs
MASTER="$HERE/prosecraft-rs-icon-2048.png"
command -v magick >/dev/null || { echo "error: magick (ImageMagick 7) not found" >&2; exit 1; }

# One size from the master, without timestamps or other metadata, so a re-run changes nothing.
render() { # size out
  local n="$1" out="$2" sharpen=()
  if [[ "$n" -le 32 ]]; then sharpen=(-unsharp 0x0.5); fi
  mkdir -p "$(dirname "$out")"
  magick "$MASTER" -filter Lanczos -resize "${n}x${n}" "${sharpen[@]}" -strip \
    -define png:exclude-chunks=date,time "PNG32:$out"
  if command -v optipng >/dev/null; then optipng -quiet -o2 -strip all "$out"; fi
  return
}

HICOLOR="$ROOT/packaging/linux/icons/hicolor"
for n in 16 24 32 48 64 128 256 512; do
  render "$n" "$HICOLOR/${n}x${n}/apps/$APP_ID.png"
done
render 1024 "$ROOT/packaging/macos/prosecraft-rs-1024.png"
render 256 "$ROOT/apps/prosecraft-rs/assets/icon-256.png"
render 64 "$ROOT/apps/prosecraft-rs-web/assets/favicon.png"

ico=()
for n in 16 24 32 48 64 128 256; do ico+=("$HICOLOR/${n}x${n}/apps/$APP_ID.png"); done
magick "${ico[@]}" "$ROOT/packaging/windows/prosecraft-rs.ico"

"$ROOT/packaging/check-icons.sh"
