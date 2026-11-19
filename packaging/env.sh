# shellcheck shell=bash
# Shared setup for the packaging scripts. Source it: `. "$(dirname "$0")/../env.sh"`.
# Adapted from CraftCloud's packaging/env.sh, itself adapted from craft-launcher's
# packaging/env.sh (MIT OR Apache-2.0; see NOTICE).
#
# Exports:
#   ROOT                     workspace root
#   VERSION                  [workspace.package] version from Cargo.toml (override: PROSECRAFT_VERSION)
#   DIST                     output directory for release files (default: $ROOT/dist/release)
#   PROSECRAFT_BUILD_SHA     git commit of the build
#   PROSECRAFT_BUILD_DATE    UTC build date, YYYY-MM-DD (the AppStream release date)
#   PROSECRAFT_MAINTAINER    the deb/rpm Maintainer field (override for your own builds)
#   CARGO_TARGET_DIR         cargo's target dir (default: $ROOT/target)
#   CRAFT_FONTS_DIR          the craft-fonts checkout the build embeds Inter and Source Serif 4 from
#                            (default: $ROOT/craft-fonts or $ROOT/../craft-fonts, when there)
#   CRAFT_FONTS_REQUIRED     1 (default): a release build without the fonts is an error. Set it to 0
#                            to package a build that falls back to egui's and the system's fonts.
#
# And the helpers warn, copy_docs and sha256.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in exactly one place: `[workspace.package] version` in the root Cargo.toml.
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]") ; next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
  return
}

VERSION="${PROSECRAFT_VERSION:-$(workspace_version)}"
if [[ -z "$VERSION" ]]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$DIST"
export DIST

if [[ -z "${PROSECRAFT_BUILD_SHA:-}" ]]; then
  PROSECRAFT_BUILD_SHA="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || true)"
fi
export PROSECRAFT_BUILD_SHA
export PROSECRAFT_BUILD_DATE="${PROSECRAFT_BUILD_DATE:-$(date -u +%Y-%m-%d)}"
export PROSECRAFT_MAINTAINER="${PROSECRAFT_MAINTAINER:-jonasbove <31095576+jonasbove@users.noreply.github.com>}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

# The interface and manuscript fonts (crates/ui-egui/build.rs). Release builds must have them.
if [[ -z "${CRAFT_FONTS_DIR:-}" ]]; then
  for dir in "$ROOT/craft-fonts" "$ROOT/../craft-fonts"; do
    if [[ -d "$dir/fonts" ]]; then CRAFT_FONTS_DIR="$(cd "$dir" && pwd)"; break; fi
  done
fi
if [[ -n "${CRAFT_FONTS_DIR:-}" ]]; then export CRAFT_FONTS_DIR; fi
export CRAFT_FONTS_REQUIRED="${CRAFT_FONTS_REQUIRED:-1}"

# Emit a GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
  return
}

# Copy the licence, notice and readme files into a package directory.
copy_docs() {
  local dest="$1" f
  for f in README.md LICENSE-MIT LICENSE-APACHE NOTICE; do
    if [[ -f "$ROOT/$f" ]]; then cp "$ROOT/$f" "$dest/"; fi
  done
  return
}

# Portable SHA-256 of a file (prints just the hash).
sha256() {
  local file="$1"
  if command -v sha256sum >/dev/null; then sha256sum "$file" | cut -d' ' -f1; else shasum -a 256 "$file" | cut -d' ' -f1; fi
  return
}
