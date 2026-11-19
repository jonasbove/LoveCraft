#!/usr/bin/env bash
# Every icon file the packages use is there and has its size (packaging/icons/render.sh writes
# them). Run by the packaging lint (craft-common-actions runs packaging/check-icons.sh when it
# exists), so a missing or wrongly sized icon fails before a release build does.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID=io.github.jonasbove.prosecraft_rs
status=0

# A PNG's width and height, from its IHDR chunk (bytes 16-23, big-endian).
png_size() {
  local w h
  w=$(od -An -tu1 -j16 -N4 "$1" | awk '{ print $1 * 16777216 + $2 * 65536 + $3 * 256 + $4 }')
  h=$(od -An -tu1 -j20 -N4 "$1" | awk '{ print $1 * 16777216 + $2 * 65536 + $3 * 256 + $4 }')
  echo "${w}x${h}"
  return
}

check_png() { # file size
  local file="$1" want="$2" got
  if [[ ! -f "$file" ]]; then
    echo "error: $file is missing (run packaging/icons/render.sh)" >&2
    status=1
    return
  fi
  if [[ "$(head -c 8 "$file" | od -An -tx1 | tr -d ' \n')" != 89504e470d0a1a0a ]]; then
    echo "error: $file is not a PNG" >&2
    status=1
    return
  fi
  got="$(png_size "$file")"
  if [[ "$got" != "${want}x${want}" ]]; then
    echo "error: $file is $got, expected ${want}x${want}" >&2
    status=1
    return
  fi
  echo "ok   $file ($got)"
  return
}

for n in 16 24 32 48 64 128 256 512; do
  check_png "$ROOT/packaging/linux/icons/hicolor/${n}x${n}/apps/$APP_ID.png" "$n"
done
check_png "$ROOT/packaging/macos/prosecraft-rs-1024.png" 1024
check_png "$ROOT/packaging/icons/prosecraft-rs-icon-2048.png" 2048
check_png "$ROOT/apps/prosecraft-rs/assets/icon-256.png" 256
check_png "$ROOT/apps/prosecraft-rs-web/assets/favicon.png" 64
# An .ico starts with reserved 0, type 1 (icon), then the number of images.
ico="$ROOT/packaging/windows/prosecraft-rs.ico"
if [[ -f "$ico" && "$(head -c 4 "$ico" | od -An -tx1 | tr -d ' \n')" = 00000100 ]]; then
  echo "ok   $ico ($(od -An -tu2 -j4 -N2 "$ico" | tr -d ' ') images)"
else
  echo "error: $ico is missing or not an .ico" >&2
  status=1
fi
exit "$status"
