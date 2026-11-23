#!/bin/sh
# Adds LoveCraft to your app menu (no root needed): the app and the MCP server to ~/.local/bin,
# the menu entry, the .prosecraft file type, the AppStream file and icons to ~/.local/share. Run
# from the unpacked release tarball (lovecraft-<ver>-linux-<arch>/): ./install.sh
# Undo: ./install.sh --remove (your library and projects are not touched).
#
# LoveCraft was ProseCraft-rs until 0.2.0. Its 0.1 commands, prosecraft-rs and prosecraft-mcp,
# become links to lovecraft and lovecraft-mcp, replacing a ProseCraft-rs install.sh put there, so
# scripts and MCP client configurations made for them keep working; --remove takes them off too.
# The menu entry, file type and icons keep their file names (the app id), so they are replaced.
# Adapted from CraftCloud's packaging/linux/install.sh (MIT OR Apache-2.0; see NOTICE).
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
bin="${HOME}/.local/bin"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
id=io.github.jonasbove.prosecraft_rs

refresh() {
    if command -v update-mime-database >/dev/null 2>&1 && [ -d "$data/mime/packages" ]; then
        update-mime-database "$data/mime" 2>/dev/null || true
    fi
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$data/applications" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
    fi
}

if [ "${1:-}" = --remove ]; then
    rm -f "$bin/lovecraft" "$bin/lovecraft-mcp" "$bin/prosecraft-rs" "$bin/prosecraft-mcp" \
        "$data/applications/$id.desktop" "$data/metainfo/$id.metainfo.xml" "$data/mime/packages/$id.xml"
    for icon in "$data"/icons/hicolor/*/apps/"$id".png; do rm -f "$icon"; done
    refresh
    echo "Removed LoveCraft from the app menu. Your library and projects are untouched."
    exit 0
fi

[ -x "$here/bin/lovecraft" ] || { echo "error: run this from the unpacked lovecraft-<ver>-linux-<arch> folder" >&2; exit 1; }
install -Dm755 "$here/bin/lovecraft" "$bin/lovecraft"
install -Dm755 "$here/bin/lovecraft-mcp" "$bin/lovecraft-mcp"
# The 0.1 names, as links (-f replaces a ProseCraft-rs binary there).
ln -sf lovecraft "$bin/prosecraft-rs"
ln -sf lovecraft-mcp "$bin/prosecraft-mcp"
# The menu entry starts the installed copy by its full path, so ~/.local/bin needn't be on PATH.
tmp="$data/applications/.$id.desktop.tmp"
mkdir -p "$data/applications"
sed -e "s|^Exec=lovecraft|Exec=\"$bin/lovecraft\"|" -e "s|^TryExec=.*|TryExec=$bin/lovecraft|" \
    "$here/share/applications/$id.desktop" > "$tmp"
mv -f "$tmp" "$data/applications/$id.desktop"
install -Dm644 "$here/share/metainfo/$id.metainfo.xml" "$data/metainfo/$id.metainfo.xml"
install -Dm644 "$here/share/mime/packages/$id.xml" "$data/mime/packages/$id.xml"
(cd "$here/share/icons/hicolor" && find . -type f) | while IFS= read -r rel; do
    install -Dm644 "$here/share/icons/hicolor/$rel" "$data/icons/hicolor/$rel"
done
refresh
echo "Installed: LoveCraft is in your app menu (binaries: $bin/lovecraft, $bin/lovecraft-mcp; $bin/prosecraft-rs and $bin/prosecraft-mcp link to them)."
