#!/usr/bin/env bash
# Test updating the macOS app from earlier releases' DMGs, as a person does it: drag the app from
# each DMG into Applications, oldest first, then this release's.
#
#   1. each previous DMG's app is copied into a scratch Applications folder (a ProseCraft-rs.app
#      from 0.1, a LoveCraft.app from 0.2 on) and answers --version with its own name and version
#   2. this release's LoveCraft.app is copied in, replacing a LoveCraft.app already there (as
#      Finder's Replace does); a ProseCraft-rs.app stays beside it, as dragging leaves it
#   3. the new app: lovecraft and its 0.1 name prosecraft-rs (a link) answer --version with this
#      version; lovecraft-mcp and prosecraft-mcp (a link) answer an MCP initialize and tools/list;
#      the bundle id is the previous apps' one (macOS keeps an app's settings and permissions by it)
#   4. every app, old and new, uses the same library: with a home folder of the test's own, each
#      reports the same ~/Library/Application Support/prosecraft-rs/library.prosecraft when started
#      (a library it can't read, seeded there, so it says where it looked), and none changes it
#
# Usage: packaging/macos/test-update.sh --previous OLD.dmg [--previous OLD.dmg]... [NEW.dmg]
#
#   OLD.dmg is a release's DMG, prosecraft-rs-<ver>-macos-<arch>.dmg (0.1) or
#   lovecraft-<ver>-macos-<arch>.dmg; NEW.dmg defaults to $DIST/lovecraft-<version>-macos-universal.dmg.
#   Nothing outside a scratch folder is touched: the apps are run with HOME set inside it.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"

PREVIOUS=()
NEW=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --previous) PREVIOUS+=("$2"); shift 2 ;;
    -h | --help) sed -n '2,21p' "$0"; exit 0 ;;
    -*) echo "unknown option: $1" >&2; exit 2 ;;
    *) NEW="$1"; shift ;;
  esac
done
NEW="${NEW:-$DIST/lovecraft-$VERSION-macos-universal.dmg}"
[[ "${#PREVIOUS[@]}" -gt 0 ]] || { echo "error: give at least one --previous DMG" >&2; exit 2; }

fail() {
  if [[ -n "${GITHUB_ACTIONS:-}" ]]; then echo "::error::$*"; else echo "FAIL $*" >&2; fi
  exit 1
}
ok() { echo "ok   $*"; return; }

WORK="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/lovecraft-update.XXXXXX")"
APPS="$WORK/Applications"
TEST_HOME="$WORK/home"
DATA="$TEST_HOME/Library/Application Support/prosecraft-rs"
LIBRARY="$DATA/library.prosecraft"
MOUNTS=()
PIDS=()
cleanup() {
  local m p
  for p in ${PIDS[@]+"${PIDS[@]}"}; do kill "$p" 2>/dev/null || true; done
  for m in ${MOUNTS[@]+"${MOUNTS[@]}"}; do hdiutil detach "$m" -quiet >/dev/null 2>&1 || true; done
  rm -rf "$WORK"
  return
}
trap cleanup EXIT
mkdir -p "$APPS" "$DATA"

# The package (lovecraft or prosecraft-rs) and version from a DMG's name, and what it holds.
dmg_info() { # dmg → sets PKG_NAME, PKG_VERSION, APP_NAME, DISPLAY_NAME, EXE, MCP_EXE
  local name
  name="$(basename "$1")"
  [[ "$name" =~ ^(lovecraft|prosecraft-rs)-(.+)-macos-[a-z0-9_]+\.dmg$ ]] ||
    fail "$name is not <package>-<version>-macos-<arch>.dmg"
  PKG_NAME="${BASH_REMATCH[1]}"
  PKG_VERSION="${BASH_REMATCH[2]}"
  case "$PKG_NAME" in
    lovecraft) APP_NAME=LoveCraft EXE=lovecraft MCP_EXE=lovecraft-mcp ;;
    prosecraft-rs) APP_NAME=ProseCraft-rs EXE=prosecraft-rs MCP_EXE=prosecraft-mcp ;;
  esac
  DISPLAY_NAME="$APP_NAME"
  return
}

attach() { # dmg → sets MNT, where it is mounted
  MNT="$(mktemp -d "$WORK/mnt.XXXXXX")"
  hdiutil attach "$1" -readonly -nobrowse -noautoopen -mountpoint "$MNT" -quiet
  MOUNTS+=("$MNT")
  return
}

check_version() { # want, exe
  local want="$1" exe="$2" got
  got="$(HOME="$TEST_HOME" "$exe" --version)" || fail "$exe --version didn't run"
  [[ "$got" = "$want" ]] || fail "$exe --version said '$got', expected '$want'"
  ok "${exe#"$APPS"/} --version: $got"
  return
}

bundle_id() { # app
  /usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$1/Contents/Info.plist"
  return
}

# The MCP server answers initialize and tools/list for an empty folder (perl's alarm: macOS has
# no timeout command).
check_mcp() { # exe
  local exe="$1" out folder="$WORK/mcp-folder"
  mkdir -p "$folder"
  out="$(printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test-update","version":"1"}}}' \
    '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' |
    HOME="$TEST_HOME" PROSECRAFT_OPEN_DIR="$folder" perl -e 'alarm 30; exec @ARGV' "$exe" 2>&1)" ||
    { echo "$out"; fail "$exe failed"; }
  printf '%s\n' "$out" | grep '"id":1[,}]' | grep -q '"serverInfo":{' || { echo "$out"; fail "$exe didn't answer initialize"; }
  printf '%s\n' "$out" | grep '"id":2[,}]' | grep -q '"tools":\[{' || { echo "$out"; fail "$exe listed no tools"; }
  ok "$(basename "$exe") answers initialize and tools/list"
  return
}

# Started with the test's home, the app reads the library in its data folder before it opens its
# window. The seeded file isn't a library, so the app says where it looked, leaves the file alone
# and opens without one; it is stopped once it has said so.
SEED="$WORK/seed.prosecraft"
printf 'not a library: left here by packaging/macos/test-update.sh\n' >"$SEED"
check_data_folder() { # exe
  local exe="$1" log pid i said=""
  log="$(mktemp "$WORK/run.XXXXXX")"
  cp "$SEED" "$LIBRARY"
  HOME="$TEST_HOME" perl -e 'alarm 60; exec @ARGV' env -u PROSECRAFT_RS_DATA_DIR "$exe" >"$log" 2>&1 &
  pid=$!
  PIDS+=("$pid")
  for ((i = 0; i < 60; i++)); do
    said="$(grep -m1 "couldn't be read" "$log" || true)"
    [[ -z "$said" ]] || break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.5
  done
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  [[ -n "$said" ]] || { cat "$log"; fail "$exe didn't report reading $LIBRARY"; }
  [[ "$said" = "The library at $LIBRARY couldn't be read"* ]] ||
    fail "$exe read its library elsewhere: $said"
  cmp -s "$SEED" "$LIBRARY" || fail "$exe changed $LIBRARY"
  ok "$(basename "$exe") reads ~/Library/Application Support/prosecraft-rs/library.prosecraft and leaves it alone"
  return
}

# ---- 1. the previous releases ---------------------------------------------------------------------
IDS=()
for dmg in "${PREVIOUS[@]}"; do
  [[ -f "$dmg" ]] || fail "$dmg not found"
  dmg_info "$dmg"
  echo "--- install $(basename "$dmg")"
  attach "$dmg"
  [[ -d "$MNT/$APP_NAME.app" ]] || fail "$(basename "$dmg") has no $APP_NAME.app"
  rm -rf "${APPS:?}/$APP_NAME.app"
  ditto "$MNT/$APP_NAME.app" "$APPS/$APP_NAME.app"
  check_version "$DISPLAY_NAME $PKG_VERSION" "$APPS/$APP_NAME.app/Contents/MacOS/$EXE"
  check_mcp "$APPS/$APP_NAME.app/Contents/MacOS/$MCP_EXE"
  check_data_folder "$APPS/$APP_NAME.app/Contents/MacOS/$EXE"
  IDS+=("$(bundle_id "$APPS/$APP_NAME.app")")
  ok "$APP_NAME.app $PKG_VERSION installed, bundle id ${IDS[${#IDS[@]} - 1]}"
done

# ---- 2-4. this release, over them -----------------------------------------------------------------
[[ -f "$NEW" ]] || fail "$NEW not found"
dmg_info "$NEW"
[[ "$PKG_NAME" = lovecraft ]] || fail "$NEW is not a LoveCraft DMG"
echo "--- install $(basename "$NEW")"
attach "$NEW"
[[ -d "$MNT/LoveCraft.app" ]] || fail "$(basename "$NEW") has no LoveCraft.app"
rm -rf "$APPS/LoveCraft.app"
ditto "$MNT/LoveCraft.app" "$APPS/LoveCraft.app"
APP="$APPS/LoveCraft.app"
check_version "LoveCraft $PKG_VERSION" "$APP/Contents/MacOS/lovecraft"
for alias in prosecraft-rs:lovecraft prosecraft-mcp:lovecraft-mcp; do
  link="$APP/Contents/MacOS/${alias%%:*}"
  [[ -L "$link" && "$(readlink "$link")" = "${alias#*:}" ]] || fail "LoveCraft.app has no link ${alias%%:*} -> ${alias#*:}"
done
ok "LoveCraft.app links prosecraft-rs -> lovecraft, prosecraft-mcp -> lovecraft-mcp"
check_version "LoveCraft $PKG_VERSION" "$APP/Contents/MacOS/prosecraft-rs"
check_mcp "$APP/Contents/MacOS/lovecraft-mcp"
check_mcp "$APP/Contents/MacOS/prosecraft-mcp"
id="$(bundle_id "$APP")"
for old in "${IDS[@]}"; do
  [[ "$old" = "$id" ]] || fail "LoveCraft.app's bundle id is $id, a previous app's was $old"
done
ok "bundle id $id, as before"
check_data_folder "$APP/Contents/MacOS/lovecraft"
check_data_folder "$APP/Contents/MacOS/prosecraft-rs"
apps="$(cd "$APPS" && ls -d -- *.app | tr '\n' ' ')"
echo
echo "==> all checks passed: LoveCraft $PKG_VERSION over $(for d in "${PREVIOUS[@]}"; do basename "$d"; done | tr '\n' ' ')(Applications: $apps)"
