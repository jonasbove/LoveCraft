# ProseCraft-rs

A novel-writing app: outline, write and publish. These files hold the app (`prosecraft-rs`) and
the MCP server for ProseCraft open sync folders (`prosecraft-mcp`). The builds are **not
code-signed** by Apple or Microsoft yet, so your system asks before the first start:

- **macOS** (`prosecraft-rs-<ver>-macos-universal.dmg`, Apple silicon and Intel): open the DMG,
  drag `ProseCraft-rs.app` to Applications, then **right-click → Open → Open** the first time
  (Gatekeeper). Or run `xattr -dr com.apple.quarantine /Applications/ProseCraft-rs.app`. The MCP
  server is `/Applications/ProseCraft-rs.app/Contents/MacOS/prosecraft-mcp`.
- **Windows** (`prosecraft-rs-<ver>-windows-{x64,arm64}.msi`): run the installer (no administrator
  rights needed). If SmartScreen says "Windows protected your PC", click **More info → Run anyway**.
  It puts `prosecraft-mcp` on your PATH for MCP clients started afterwards. The `-portable.zip`
  holds the same two programs to run without installing.
- **Linux:** the `.deb`, `.rpm`, `.pkg.tar.zst` (Arch), `.flatpak` or `.AppImage`. From the
  `.tar.gz`: unpack it and run `./install.sh` in the folder (to `~/.local`, no root;
  `./install.sh --remove` undoes it). The AppImage runs the MCP server with `--mcp` as its first
  argument. On NixOS, use the flake instead (see the project README).

Your library stays where ProseCraft-rs keeps it (your user data folder), and installing,
updating or removing the app never touches it. Keep backups of your books all the same.

Every release lists the files' SHA-256 in `SHA256SUMS.txt`, signed with the ProseCraft-rs release
key; the project README says how to check them: https://github.com/jonasbove/ProseCraft-rs#downloads
