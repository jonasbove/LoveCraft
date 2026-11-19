#!/usr/bin/env bash
# Install-test one Linux release file in a clean distro container, as a person would install it:
#
#   1. install it with the system's package manager, so its dependencies resolve
#      (apt-get for .deb, dnf for .rpm, pacman for .pkg.tar.zst; install.sh for the .tar.gz;
#      the AppImage runs extract-and-run, as there is no FUSE in a container)
#   2. `prosecraft-rs --version` names the expected version (the app has no --help: any other
#      argument is a backup to open)
#   3. the files are there: both binaries on PATH, the desktop entry (desktop-file-validate), the
#      icons, the AppStream file, the .prosecraft file type (shared-mime-info) and the entry
#      registered for it in mimeinfo.cache
#   4. the MCP server answers an `initialize` and a `tools/list` over stdio for an empty folder
#   5. the app opens its window on a virtual X display (Xvfb, Mesa's software OpenGL) and is still
#      running after a few seconds: the libraries it loads at run time are there
#   6. with --previous: the previous release's package installed first, then upgraded to this one
#      (skipped, with a notice, when that release is not older)
#   7. removal: the files under /usr (or ~/.local for the .tar.gz) are gone again
#
# Usage, on a machine with Docker (the release workflow's package-test job runs exactly this):
#
#   packaging/linux/test-package.sh --image debian:12 [--previous OLD-FILE] [--no-gui] FILE
#
#   FILE is a release file, prosecraft-rs-<version>-linux-<arch>.<deb|rpm|pkg.tar.zst|AppImage|tar.gz>;
#   the version to expect is taken from its name. --no-gui skips step 5.
#
# Adapted from CraftCloud's packaging/linux/test-package.sh (MIT OR Apache-2.0; see NOTICE),
# without its sync round trip (ProseCraft-rs has no server) and with the MCP and window checks.
set -euo pipefail

APP_ID=io.github.jonasbove.prosecraft_rs
MIME=application/x-prosecraft
SELF="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

IMAGE=""
PREVIOUS=""
GUI=1
INSIDE=0
FILE=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --image) IMAGE="$2"; shift 2 ;;
    --previous) PREVIOUS="$2"; shift 2 ;;
    --no-gui) GUI=0; shift ;;
    --inside) INSIDE=1; shift ;;
    -h | --help) sed -n '2,27p' "$SELF"; exit 0 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) FILE="$1"; shift ;;
  esac
done
[[ -n "$FILE" ]] || { echo "usage: $0 --image IMAGE [--previous OLD-FILE] [--no-gui] FILE" >&2; exit 2; }

# ---- on the host: start the container and run this script in it ---------------------------------
if [[ "$INSIDE" = 0 ]]; then
  [[ -n "$IMAGE" ]] || { echo "error: --image is required (the test installs and removes system packages)" >&2; exit 2; }
  [[ -f "$FILE" ]] || { echo "error: $FILE not found" >&2; exit 1; }
  mounts=(-v "$SELF:/test-package.sh:ro" -v "$(cd "$(dirname "$FILE")" && pwd):/pkg/new:ro")
  args=(--inside)
  [[ "$GUI" = 1 ]] || args+=(--no-gui)
  if [[ -n "$PREVIOUS" ]]; then
    [[ -f "$PREVIOUS" ]] || { echo "error: $PREVIOUS not found" >&2; exit 1; }
    mounts+=(-v "$(cd "$(dirname "$PREVIOUS")" && pwd):/pkg/old:ro")
    args+=(--previous "/pkg/old/$(basename "$PREVIOUS")")
  fi
  echo "==> $(basename "$FILE") in $IMAGE"
  exec docker run --rm --tmpfs /tmp:exec -e GITHUB_ACTIONS \
    "${mounts[@]}" "$IMAGE" bash /test-package.sh "${args[@]}" "/pkg/new/$(basename "$FILE")"
fi

# ---- in the container ----------------------------------------------------------------------------
# This installs and removes packages system-wide, so it refuses to run outside a container.
if [[ ! -f /.dockerenv && ! -f /run/.containerenv ]]; then
  echo "error: --inside runs only in a container; use --image" >&2
  exit 2
fi

notice() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::notice::$*"; else echo "notice: $*"; fi
  return
}
step() { echo; echo "--- $*"; return; }
ok() { echo "ok   $*"; return; }
fail() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::error::$*"; else echo "FAIL $*" >&2; fi
  exit 1
}

# The format and version from a release file's name.
format_of() {
  case "$1" in
    *.deb) echo deb ;;
    *.rpm) echo rpm ;;
    *.pkg.tar.zst) echo arch ;;
    *.AppImage) echo appimage ;;
    *.tar.gz) echo tar ;;
    *) return 1 ;;
  esac
}
version_of() { basename "$1" | sed -n 's/^prosecraft-rs-\(.*\)-linux-[a-z0-9_]*\..*$/\1/p'; return; }

FMT="$(format_of "$FILE")" || fail "$FILE: not a .deb, .rpm, .pkg.tar.zst, .AppImage or .tar.gz"
VERSION="$(version_of "$FILE")"
[[ -n "$VERSION" ]] || fail "$FILE: the name is not prosecraft-rs-<version>-linux-<arch>.<ext>"
WORK="$(mktemp -d)"
# Copies: apt reads the file as its own user, the AppImage must be executable, the mount is read-only.
cp "$FILE" "$WORK/"
PKG="$WORK/$(basename "$FILE")"
export HOME="$WORK/home"
mkdir -p "$HOME"
DATA="$HOME/.local/share"

# In a subshell: os-release sets VERSION too.
# shellcheck source=/dev/null
OS="$(. /etc/os-release && echo "${PRETTY_NAME:-$ID}")"
echo "==> ProseCraft-rs $VERSION ($FMT) on $OS $(uname -m)"

# What to look at when something fails: the system, the package's state, and why the binary
# doesn't start (missing libraries).
debug_info() {
  echo
  echo "--- debug: $OS, $(uname -m), $(ldd --version 2>&1 | head -1)"
  case "$FMT" in
    deb) dpkg -l 'prosecraft*' 2>&1 | tail -n +6 || true ;;
    rpm) rpm -qi prosecraft-rs 2>&1 | head -5 || true ;;
    arch) pacman -Qi prosecraft-rs 2>&1 | head -5 || true ;;
  esac
  local bin=""
  case "$FMT" in
    deb | rpm | arch) bin=/usr/bin/prosecraft-rs ;;
    tar) bin="$HOME/.local/bin/prosecraft-rs" ;;
    appimage) (cd "$WORK" && "$PKG" --appimage-extract usr/bin/prosecraft-rs >/dev/null 2>&1) && bin="$WORK/squashfs-root/usr/bin/prosecraft-rs" ;;
  esac
  if [[ -n "$bin" && -f "$bin" ]]; then
    echo "--- ldd $bin"
    ldd "$bin" 2>&1 || true
  fi
  if [[ -s "$WORK/gui.log" ]]; then
    echo "--- the app's output on the virtual display"
    tail -n 50 "$WORK/gui.log"
  fi
  return
}
trap 'status=$?; if [[ $status -ne 0 ]]; then debug_info; fi' EXIT

# ---- 1. tools the checks need, from the distro (before the package, so its scripts see them) -----
step "test tools"
case "$FMT" in
  deb | appimage | tar)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update -qq
    apt-get install -y -qq --no-install-recommends desktop-file-utils shared-mime-info >/dev/null
    ;;
  rpm)
    dnf install -y -q --setopt=install_weak_deps=False desktop-file-utils shared-mime-info
    ;;
  arch)
    # A full upgrade first: Arch has no partial upgrades, and the package's dependencies come
    # from today's repositories.
    pacman -Syu --noconfirm --needed desktop-file-utils shared-mime-info >/dev/null
    ;;
esac
ok "desktop-file-utils, shared-mime-info"

# ---- install, upgrade and remove -----------------------------------------------------------------
# The commands to run, per format.
APP=()
MCP=()
case "$FMT" in
  deb | rpm | arch) APP=(prosecraft-rs) MCP=(prosecraft-mcp) ;;
  tar) APP=("$HOME/.local/bin/prosecraft-rs") MCP=("$HOME/.local/bin/prosecraft-mcp") ;;
  appimage)
    chmod +x "$PKG"
    APP=(env APPIMAGE_EXTRACT_AND_RUN=1 PROSECRAFT_NO_DESKTOP_INTEGRATION=1 "$PKG")
    MCP=(env APPIMAGE_EXTRACT_AND_RUN=1 "$PKG" --mcp)
    ;;
esac

install_file() { # file
  local file="$1"
  case "$FMT" in
    # --no-install-recommends / no weak deps: the hard dependencies are what has to resolve.
    deb) apt-get install -y -q --no-install-recommends "$file" ;;
    rpm) dnf install -y -q --setopt=install_weak_deps=False "$file" ;;
    arch) pacman -U --noconfirm "$file" ;;
    tar)
      rm -rf "$WORK/unpacked"
      mkdir -p "$WORK/unpacked"
      tar -xzf "$file" -C "$WORK/unpacked"
      "$WORK/unpacked/$(basename "$file" .tar.gz)/install.sh"
      ;;
    appimage) ;;
  esac
  return
}

remove_package() {
  case "$FMT" in
    deb) apt-get purge -y -q prosecraft-rs ;;
    rpm) dnf remove -y -q prosecraft-rs ;;
    arch) pacman -R --noconfirm prosecraft-rs ;;
    tar) "$WORK/unpacked/$(basename "$PKG" .tar.gz)/install.sh" --remove ;;
    appimage) ;;
  esac
  return
}

check_version() { # expected version
  local want="$1" out
  if ! out="$("${APP[@]}" --version 2>&1)"; then
    echo "$out"
    fail "prosecraft-rs --version didn't run"
  fi
  [[ "$out" = "ProseCraft-rs $want" ]] || fail "prosecraft-rs --version said '$out', expected 'ProseCraft-rs $want'"
  ok "--version: $out"
  return
}

# Where the files go: /usr for packages, ~/.local for install.sh.
if [[ "$FMT" = tar ]]; then PREFIX_BIN="$HOME/.local/bin" PREFIX_SHARE="$DATA"; else PREFIX_BIN=/usr/bin PREFIX_SHARE=/usr/share; fi

check_files() {
  local b
  for b in prosecraft-rs prosecraft-mcp; do
    [[ -x "$PREFIX_BIN/$b" ]] || fail "$PREFIX_BIN/$b missing"
    if [[ "$FMT" != tar ]]; then
      # Resolved: on Fedora 42 /usr/sbin is a link to /usr/bin and comes first on PATH.
      [[ "$(readlink -f "$(command -v "$b")")" = "$(readlink -f "/usr/bin/$b")" ]] ||
        fail "$b on PATH is '$(command -v "$b")'"
    fi
  done
  ok "binaries $PREFIX_BIN/prosecraft-rs, $PREFIX_BIN/prosecraft-mcp"
  local desktop="$PREFIX_SHARE/applications/$APP_ID.desktop"
  [[ -f "$desktop" ]] || fail "$desktop missing"
  desktop-file-validate "$desktop" || fail "$desktop doesn't validate"
  ok "desktop entry validates"
  local icons=("$PREFIX_SHARE"/icons/hicolor/*/apps/"$APP_ID".png)
  [[ -f "$PREFIX_SHARE/icons/hicolor/256x256/apps/$APP_ID.png" && "${#icons[@]}" -ge 8 ]] ||
    fail "icons: found ${icons[*]}"
  ok "${#icons[@]} icons"
  local meta="$PREFIX_SHARE/metainfo/$APP_ID.metainfo.xml"
  grep -q "<release version=\"$CURRENT_VERSION\"" "$meta" 2>/dev/null || fail "$meta missing or not for $CURRENT_VERSION"
  ok "AppStream file"
  # update-mime-database (the package's script, the distro's trigger or hook, or install.sh) turns
  # the shipped definition into the database's own entry for the type.
  [[ -f "$PREFIX_SHARE/mime/packages/$APP_ID.xml" ]] || fail "$PREFIX_SHARE/mime/packages/$APP_ID.xml missing"
  [[ -f "$PREFIX_SHARE/mime/$MIME.xml" ]] || fail "$MIME is not in the MIME database ($PREFIX_SHARE/mime/$MIME.xml)"
  ok "the .prosecraft file type ($MIME)"
  # update-desktop-database writes the entry's MimeType into mimeinfo.cache, which is how file
  # managers find it for .prosecraft files.
  grep -qx "$MIME=.*$APP_ID.desktop.*" "$PREFIX_SHARE/applications/mimeinfo.cache" 2>/dev/null ||
    fail "$MIME is not in $PREFIX_SHARE/applications/mimeinfo.cache"
  ok "the app is registered for $MIME"
  return
}

check_removed() {
  local left=() f
  for f in "$PREFIX_BIN/prosecraft-rs" "$PREFIX_BIN/prosecraft-mcp" "$PREFIX_SHARE/applications/$APP_ID.desktop" \
    "$PREFIX_SHARE/metainfo/$APP_ID.metainfo.xml" "$PREFIX_SHARE/mime/packages/$APP_ID.xml" \
    "$PREFIX_SHARE/mime/$MIME.xml" \
    "$PREFIX_SHARE"/icons/hicolor/*/apps/"$APP_ID".png; do
    [[ ! -e "$f" ]] || left+=("$f")
  done
  [[ "${#left[@]}" -eq 0 ]] || fail "left behind after removal: ${left[*]}"
  if grep -q "$APP_ID.desktop" "$PREFIX_SHARE/applications/mimeinfo.cache" 2>/dev/null; then
    fail "mimeinfo.cache still names $APP_ID.desktop after removal"
  fi
  ok "removed: binaries, desktop entry, icons, AppStream file, file type"
  return
}

# The AppImage installs nothing, but its AppRun adds the menu entry and icons to ~/.local/share on
# its first run (packaging/linux/AppRun), so a Wayland compositor finds the window's icon.
check_appimage_integration() {
  APPIMAGE_EXTRACT_AND_RUN=1 "$PKG" --version >/dev/null || fail "the AppImage didn't start"
  local desktop="$DATA/applications/$APP_ID.desktop"
  [[ -f "$desktop" ]] || fail "the AppImage's first run didn't add $desktop"
  desktop-file-validate "$desktop" || fail "$desktop doesn't validate"
  grep -q "^Exec=\"$PKG\" %F" "$desktop" || fail "$desktop doesn't start the AppImage: $(grep '^Exec=' "$desktop")"
  [[ -f "$DATA/icons/hicolor/256x256/apps/$APP_ID.png" ]] || fail "the AppImage's first run didn't add its icons"
  ok "first run added the menu entry and icons to ~/.local/share"
  return
}

# ---- 4. the MCP server over stdio ----------------------------------------------------------------
check_mcp() {
  local out dir="$WORK/mcp-folder"
  mkdir -p "$dir"
  # Without a folder it says which variable to set, and exits instead of waiting for input.
  if out="$(env -u PROSECRAFT_OPEN_DIR "${MCP[@]}" </dev/null 2>&1)"; then
    fail "prosecraft-mcp without PROSECRAFT_OPEN_DIR exited 0: $out"
  fi
  [[ "$out" = *PROSECRAFT_OPEN_DIR* ]] || fail "prosecraft-mcp without PROSECRAFT_OPEN_DIR said: $out"
  ok "prosecraft-mcp without a folder: $out"
  out="$(printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test-package","version":"1"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' |
    PROSECRAFT_OPEN_DIR="$dir" timeout 30 "${MCP[@]}" 2>&1)" || { echo "$out"; fail "prosecraft-mcp failed"; }
  # One JSON object per line; the keys' order is the server's, so each is matched on its own.
  printf '%s\n' "$out" | grep '"id":1[,}]' | grep -q '"serverInfo":{' ||
    { echo "$out"; fail "prosecraft-mcp didn't answer initialize"; }
  printf '%s\n' "$out" | grep '"id":2[,}]' | grep -q '"tools":\[{' ||
    { echo "$out"; fail "prosecraft-mcp listed no tools"; }
  ok "prosecraft-mcp answers initialize and tools/list"
  return
}

# ---- 5. the window, on a virtual display ---------------------------------------------------------
# Xvfb and Mesa's software OpenGL come from the distro. They pull in some of the libraries the
# package depends on too, so this proves the app starts with what it loads at run time, while the
# package manager's own install above proves its dependencies resolve.
gui_tools() {
  case "$FMT" in
    # The tarball and the AppImage bring no dependencies: they use the desktop's libraries, which
    # a container lacks. These are the .deb's dependencies (nfpm.yaml), so the window also shows
    # that list is enough.
    appimage | tar)
      apt-get install -y -qq --no-install-recommends libxkbcommon0 libxkbcommon-x11-0 libwayland-client0 \
        libwayland-egl1 libx11-6 libx11-xcb1 libxcb1 libxcursor1 libxi6 libxrandr2 libegl1 libgl1 >/dev/null
      ;;
  esac
  case "$FMT" in
    deb | appimage | tar) apt-get install -y -qq --no-install-recommends xvfb x11-utils libgl1-mesa-dri libglx-mesa0 libegl-mesa0 >/dev/null ;;
    rpm) dnf install -y -q --setopt=install_weak_deps=False xorg-x11-server-Xvfb xwininfo mesa-dri-drivers mesa-libGL mesa-libEGL ;;
    arch) pacman -S --noconfirm --needed xorg-server-xvfb xorg-xwininfo mesa >/dev/null ;;
  esac
  return
}

check_window() {
  local status=0 i shown=0
  # A display of its own, and no Wayland: winit then takes X11. The app keeps its library in
  # ~/.local/share, which is the test's own home folder.
  Xvfb :94 -screen 0 1280x1024x24 -nolisten tcp >"$WORK/xvfb.log" 2>&1 &
  local xvfb=$!
  sleep 2
  kill -0 "$xvfb" 2>/dev/null || { cat "$WORK/xvfb.log"; fail "Xvfb didn't start"; }
  DISPLAY=:94 timeout 30 env -u WAYLAND_DISPLAY "${APP[@]}" >"$WORK/gui.log" 2>&1 &
  local app=$!
  # The window, by its title, among the display's top-level windows.
  for ((i = 0; i < 40; i++)); do
    if DISPLAY=:94 xwininfo -root -tree 2>/dev/null | grep -q '"ProseCraft-rs"'; then shown=1; break; fi
    kill -0 "$app" 2>/dev/null || break
    sleep 0.5
  done
  # Then a few seconds of drawing: still running is what a window waiting for the person does.
  if [[ "$shown" = 1 ]]; then sleep 5; fi
  if kill -0 "$app" 2>/dev/null; then
    status=124
    kill "$app" 2>/dev/null || true
    wait "$app" 2>/dev/null || true
  else
    wait "$app" 2>/dev/null || status=$?
  fi
  kill "$xvfb" 2>/dev/null || true
  wait "$xvfb" 2>/dev/null || true
  [[ "$shown" = 1 ]] || { cat "$WORK/gui.log"; fail "no ProseCraft-rs window appeared on the virtual display within 20 s"; }
  [[ "$status" = 124 ]] || { cat "$WORK/gui.log"; fail "prosecraft-rs exited (status $status) right after opening its window"; }
  if grep -q "panicked" "$WORK/gui.log"; then
    cat "$WORK/gui.log"
    fail "prosecraft-rs panicked on a virtual display"
  fi
  ok "the window opened on a virtual X display and was still running 5 s later"
  return
}

CURRENT_VERSION="$VERSION"

step "2-3. install $(basename "$PKG")"
install_file "$PKG"
check_version "$VERSION"
case "$FMT" in
  appimage) check_appimage_integration ;;
  *) check_files ;;
esac

step "4. the MCP server"
check_mcp

if [[ "$GUI" = 1 ]]; then
  step "5. the window"
  gui_tools
  check_window
else
  notice "--no-gui: the window wasn't opened"
fi

if [[ "$FMT" != appimage ]]; then
  step "7. remove"
  remove_package
  check_removed
fi

# ---- 6. upgrade from the previous release --------------------------------------------------------
# GNU sort -V sorts `~` first: 0.2.0-rc.1 written as 0.2.0~rc.1 sorts before 0.2.0, as in semver.
older() { # a b → a sorts before b
  local a="${1//-/\~}" b="${2//-/\~}"
  [[ "$a" != "$b" && "$(printf '%s\n%s\n' "$a" "$b" | sort -V | head -1)" = "$a" ]]
}
if [[ -n "$PREVIOUS" ]]; then
  if [[ "$FMT" = appimage || "$FMT" = tar ]]; then
    notice "$FMT: nothing is installed, so there is no upgrade to test"
  else
    OLD_VERSION="$(version_of "$PREVIOUS")"
    if ! older "$OLD_VERSION" "$VERSION"; then
      notice "the previous release ($OLD_VERSION) is not older than $VERSION: upgrade not tested"
    else
      step "6. upgrade from $OLD_VERSION"
      cp "$PREVIOUS" "$WORK/"
      install_file "$WORK/$(basename "$PREVIOUS")"
      check_version "$OLD_VERSION"
      install_file "$PKG"
      check_version "$VERSION"
      check_files
      step "7. remove after the upgrade"
      remove_package
      check_removed
    fi
  fi
else
  notice "no --previous: the upgrade was not tested"
fi

echo
echo "==> all checks passed: ProseCraft-rs $VERSION ($FMT) on $OS $(uname -m)"
