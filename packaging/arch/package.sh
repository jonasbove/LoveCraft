#!/usr/bin/env bash
# Build the release's Arch Linux package from the Linux tarball, with makepkg:
#
#   $DIST/prosecraft-rs-<version>-linux-<arch>.pkg.tar.zst     (pacman -U it)
#
# Usage: packaging/arch/package.sh [TARBALL] [ARCH]
#
# TARBALL defaults to $DIST/prosecraft-rs-<version>-linux-<ARCH>.tar.gz from packaging/linux/package.sh,
# ARCH to the host's (x86_64 or aarch64). It only repackages the tarball's bin/ and share/, so an
# x86_64 host can build the aarch64 package. Needs makepkg (Arch, or the archlinux:base-devel
# container as an unprivileged user: makepkg refuses to run as root).
#
# Adapted from CraftCloud's packaging/arch/package.sh, which follows craft-common-actions'
# arch-package (MIT OR Apache-2.0; see NOTICE), with ProseCraft-rs's own dependencies. The AUR
# recipes are in prosecraft-rs-bin/ and prosecraft-rs/.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
PKG=prosecraft-rs
MCP=prosecraft-mcp

ARCH="${2:-$(uname -m)}"
case "$ARCH" in
  x86_64) ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  riscv64) ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
TARBALL="${1:-$DIST/$PKG-$VERSION-linux-$ARCH.tar.gz}"
[[ -f "$TARBALL" ]] || { echo "error: $TARBALL not found (run packaging/linux/package.sh --formats tar first)" >&2; exit 1; }
for tool in makepkg bsdtar zstd; do
  command -v "$tool" >/dev/null || { echo "error: $tool not found" >&2; exit 1; }
done
TARBALL="$(cd "$(dirname "$TARBALL")" && pwd)/$(basename "$TARBALL")"

# pacman's pkgver has no hyphens: 0.2.0-rc.1 becomes 0.2.0rc.1, which sorts before 0.2.0. A
# numeric pre-release needs a letter in front (0.2.0-1 as 0.2.01 would sort after 0.2.0).
PKGVER="$VERSION"
if [[ "$PKGVER" =~ ^[0-9]+\.[0-9]+\.[0-9]+-[0-9] ]]; then PKGVER="${PKGVER/-/pre}"; fi
PKGVER="${PKGVER//-/}"
[[ "$PKGVER" =~ ^[A-Za-z0-9._+]+$ ]] || { echo "error: can't make a pkgver from $VERSION" >&2; exit 1; }

WORK="$CARGO_TARGET_DIR/arch-package"
rm -rf "$WORK"
mkdir -p "$WORK/stage"
# Absolute: makepkg runs in $WORK, and the PKGBUILD names files in it.
WORK="$(cd "$WORK" && pwd)"
tar -xzf "$TARBALL" -C "$WORK/stage" --strip-components=1
# -f, not -x: the work dir may be on a noexec mount (a container tmpfs).
for b in "$PKG" "$MCP"; do
  [[ -f "$WORK/stage/bin/$b" ]] || { echo "error: $TARBALL has no bin/$b" >&2; exit 1; }
done

cat >"$WORK/makepkg.conf" <<EOF
if [[ -f /etc/makepkg.conf ]]; then source /etc/makepkg.conf; fi
CARCH=$ARCH
CHOST=$ARCH-unknown-linux-gnu
PKGEXT='.pkg.tar.zst'
# Who built it (pacman -Qi shows it); PACKAGER in the environment wins.
PACKAGER='${PACKAGER:-jonasbove <31095576+jonasbove@users.noreply.github.com>}'
COMPRESSZST=(zstd -c -T0 -19 -)
EOF

# The windowing and OpenGL libraries are loaded at runtime (dlopen), so makepkg can't find them.
# The file dialogs talk to the XDG desktop portal in pure Rust (zbus): no libdbus or GTK.
# pacman's own hooks (shared-mime-info, desktop-file-utils, gtk-update-icon-cache) refresh the
# MIME, menu and icon caches.
cat >"$WORK/PKGBUILD" <<EOF
# Maintainer: ProseCraft-rs contributors <https://github.com/jonasbove/ProseCraft-rs>
pkgname=$PKG
pkgver=$PKGVER
pkgrel=1
pkgdesc='Novel-writing app: outline, write and publish (a Rust rewrite of ProseCraft)'
arch=($ARCH)
url='https://github.com/jonasbove/ProseCraft-rs'
license=('MIT OR Apache-2.0')
depends=('glibc' 'gcc-libs' 'hicolor-icon-theme' 'libglvnd' 'libxkbcommon' 'libxkbcommon-x11' 'wayland'
  'libx11' 'libxcursor' 'libxi' 'libxrandr')
optdepends=(
  'xdg-desktop-portal: the Open and Save dialogs'
  'xdg-utils: opening web links and wiki files'
  'mesa: OpenGL drivers'
  'shared-mime-info: the .prosecraft file type'
)
provides=('$PKG')
conflicts=('$PKG-bin' '$PKG-git')
options=('!strip' '!debug')

package() {
  install -Dm755 "$WORK/stage/bin/$PKG" "\$pkgdir/usr/bin/$PKG"
  install -Dm755 "$WORK/stage/bin/$MCP" "\$pkgdir/usr/bin/$MCP"
  cp -dr --no-preserve=ownership "$WORK/stage/share" "\$pkgdir/usr/"
  install -Dm644 "$WORK/stage/share/doc/$PKG/LICENSE-MIT" "\$pkgdir/usr/share/licenses/$PKG/LICENSE-MIT"
  install -Dm644 "$WORK/stage/share/doc/$PKG/LICENSE-APACHE" "\$pkgdir/usr/share/licenses/$PKG/LICENSE-APACHE"
}
EOF

echo "==> Arch package $PKG $PKGVER-1 for $ARCH"
(cd "$WORK" && SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(git -C "$ROOT" log -1 --format=%ct 2>/dev/null || date +%s)}" \
  makepkg --config "$WORK/makepkg.conf" --nodeps --clean --force)
shopt -s nullglob
built=("$WORK"/*.pkg.tar.zst)
shopt -u nullglob
[[ "${#built[@]}" -eq 1 ]] || { echo "error: expected one package, found ${#built[@]}" >&2; exit 1; }
OUT="$DIST/$PKG-$VERSION-linux-$ARCH.pkg.tar.zst"
cp -f "${built[0]}" "$OUT"
echo "wrote $OUT"
