#!/usr/bin/env bash
# Repackage a LoveCraft Linux tarball as a single-file Flatpak bundle:
#
#   $DIST/lovecraft-<version>-linux-<arch>.flatpak
#
# Usage: packaging/linux/flatpak-bundle.sh [--no-test] [TARBALL]
#
# TARBALL defaults to $DIST/lovecraft-<version>-linux-<arch>.tar.gz from package.sh. The bundle is
# built for the host architecture (x86_64 or aarch64), which must match the tarball's. Needs
# flatpak and flatpak-builder; the freedesktop runtime and SDK named in the manifest are installed
# per-user from Flathub. Unless --no-test, the bundle is then installed per-user, and the binary
# answers --version inside the sandbox as a smoke test.
# Manifest: packaging/linux/flatpak/io.github.jonasbove.prosecraft_rs.yml. No OCI image is pushed.
# The app id keeps the app's former name (ProseCraft-rs), so this bundle updates a 0.1 one.
#
# Adapted from CraftCloud's packaging/linux/flatpak-bundle.sh, itself adapted from craft-launcher's
# (MIT OR Apache-2.0; see NOTICE).
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
APP_ID=io.github.jonasbove.prosecraft_rs
PKG=lovecraft
FLATHUB=https://dl.flathub.org/repo/flathub.flatpakrepo

TEST=1
TARBALL=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-test) TEST=0; shift ;;
    -h | --help) sed -n '2,16p' "$0"; exit 0 ;;
    -*) echo "unknown argument: $1" >&2; exit 2 ;;
    *) TARBALL="$1"; shift ;;
  esac
done

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
BASENAME="$PKG-$VERSION-linux-$ARCH"
TARBALL="${TARBALL:-$DIST/$BASENAME.tar.gz}"
[[ -f "$TARBALL" ]] || { echo "error: $TARBALL not found (run packaging/linux/package.sh --formats tar first)" >&2; exit 1; }
for tool in flatpak flatpak-builder; do
  command -v "$tool" >/dev/null || { echo "error: $tool not found (apt install flatpak flatpak-builder)" >&2; exit 1; }
done

echo "==> LoveCraft $VERSION Flatpak bundle for $ARCH from $(basename "$TARBALL")"

WORK="$CARGO_TARGET_DIR/flatpak-bundle"
rm -rf "$WORK"
mkdir -p "$WORK/stage"
tar -xzf "$TARBALL" -C "$WORK/stage" --strip-components=1
[[ -x "$WORK/stage/bin/$PKG" ]] || { echo "error: $TARBALL has no bin/$PKG" >&2; exit 1; }
cp "$HERE/flatpak/$APP_ID.yml" "$WORK/$APP_ID.yml"

flatpak remote-add --user --if-not-exists flathub "$FLATHUB"
# --disable-rofiles-fuse: no FUSE in containers and CI runners.
flatpak-builder --user --install-deps-from=flathub --disable-rofiles-fuse --force-clean \
  --default-branch=stable --repo="$WORK/repo" --state-dir="$WORK/state" "$WORK/build" "$WORK/$APP_ID.yml"

OUT="$DIST/$BASENAME.flatpak"
# --runtime-repo lets `flatpak install` fetch the freedesktop runtime from Flathub if missing.
flatpak build-bundle --runtime-repo="$FLATHUB" "$WORK/repo" "$OUT" "$APP_ID" stable
echo "wrote $OUT"

if [[ "$TEST" = 1 ]]; then
  flatpak install --user -y --noninteractive --reinstall "$OUT"
  flatpak info --user "$APP_ID"
  flatpak run --command=sh "$APP_ID" -c 'ls /app/share/applications /app/share/metainfo /app/share/mime/packages /app/share/icons/hicolor/256x256/apps && /app/bin/lovecraft --version && test -x /app/bin/lovecraft-mcp && /app/bin/prosecraft-rs --version && test -x /app/bin/prosecraft-mcp'
fi
echo "==> done"
ls -lh "$OUT"
