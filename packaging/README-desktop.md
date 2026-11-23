# LoveCraft

A novel-writing app: outline, write and publish. These files hold the app (`lovecraft`) and the
MCP server for ProseCraft open sync folders (`lovecraft-mcp`). LoveCraft was called ProseCraft-rs
until 0.2.0; its commands then, `prosecraft-rs` and `prosecraft-mcp`, still work (they are
deprecated). The builds are **not code-signed** by Apple or Microsoft yet, so your system asks
before the first start:

- **macOS** (`lovecraft-<ver>-macos-universal.dmg`, Apple silicon and Intel): open the DMG,
  drag `LoveCraft.app` to Applications, then **right-click → Open → Open** the first time
  (Gatekeeper). Or run `xattr -dr com.apple.quarantine /Applications/LoveCraft.app`. The MCP
  server is `/Applications/LoveCraft.app/Contents/MacOS/lovecraft-mcp`. Updating from
  ProseCraft-rs, delete `ProseCraft-rs.app` afterwards.
- **Windows** (`lovecraft-<ver>-windows-{x64,arm64}.msi`): run the installer (no administrator
  rights needed). If SmartScreen says "Windows protected your PC", click **More info → Run anyway**.
  It puts `lovecraft-mcp` on your PATH for MCP clients started afterwards, and updates a
  ProseCraft-rs install in its folder. The `-portable.zip` holds the same programs to run without
  installing.
- **Linux:** the `.deb`, `.rpm`, `.pkg.tar.zst` (Arch), `.flatpak` or `.AppImage`. The packages
  replace the `prosecraft-rs` ones. From the `.tar.gz`: unpack it and run `./install.sh` in the
  folder (to `~/.local`, no root; `./install.sh --remove` undoes it). The AppImage runs the MCP
  server with `--mcp` as its first argument. On NixOS, use the flake instead (see the project
  README).

Your library stays where LoveCraft keeps it (your user data folder, still named `prosecraft-rs`),
and installing, updating or removing the app never touches it. Keep backups of your books all
the same.

Every release lists the files' SHA-256 in `SHA256SUMS.txt`, signed with the release key (named
"ProseCraft-rs releases", LoveCraft's former name); the project README says how to check them:
https://github.com/jonasbove/LoveCraft#downloads
