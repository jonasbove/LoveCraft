# Release notes

What changed in each version, and first, anything that needs care when updating. Each GitHub
release links to its section here. The newest version is at the top.

## Unreleased

## 0.2.0

ProseCraft-rs is now LoveCraft, and it syncs through CraftCloud.

### When updating

- **ProseCraft-rs is now called LoveCraft.** Only the name changes; your library, settings and window are kept
  (the data folder, the web app's browser storage, the app id and CraftCloud's id for the app keep their old
  names), and backups are still `.prosecraft` files.
  - **New command names:** the app is `lovecraft` and the MCP server `lovecraft-mcp`. The old names,
    `prosecraft-rs` and `prosecraft-mcp`, are still installed as aliases, so desktop shortcuts, scripts and MCP
    client configurations keep working, but they are **deprecated**: switch to the new ones.
  - **The packages replace the old ones:** the release files are named `lovecraft-<ver>-…`. The `.deb`, `.rpm`
    and Arch packages are named `lovecraft` and replace `prosecraft-rs` when installed (pacman asks to remove
    `prosecraft-rs`: answer yes); the AUR recipes are `lovecraft` and `lovecraft-bin`. The Windows installer
    updates a ProseCraft-rs install in its folder (the Start menu entry becomes LoveCraft). The Flatpak has the
    same app id as the old one, so it replaces it and keeps its data, and the AppImage takes over the menu entry
    the old one added. A 0.1.2 AppImage's own updater doesn't find 0.2.0 (it looks for files named
    `prosecraft-rs-…`): download the LoveCraft AppImage once by hand. On macOS, drag
    `LoveCraft.app` to Applications and delete `ProseCraft-rs.app`; an MCP client configured with a path inside
    `ProseCraft-rs.app` needs the new path, `LoveCraft.app/Contents/MacOS/lovecraft-mcp`. The Nix flake's package
    is `lovecraft` (`nix run github:jonasbove/LoveCraft`). The web download is `lovecraft-web-<ver>.zip`.
  - **The repository moved** to https://github.com/jonasbove/LoveCraft; GitHub redirects the old address. The
    release signing key is the same, still named "ProseCraft-rs releases".
- **"Use the CraftCloud folder" is now "In CraftCloud"**, and it keeps the sync method you chose instead of always
  switching to the encrypted file. Its folder is now `LoveCraft/<method>` inside CraftCloud's sync folder. A
  library already syncing through `ProseCraft` there (0.1's folder) keeps that folder: nothing moves, and a second
  computer choosing In CraftCloud with the encrypted file finds it.

### New

- **CraftCloud on the desktop.** With CraftCloud's `craftcloud` program installed, File has **Open from
  CraftCloud…** (the `.prosecraft` backups CraftCloud has, newest first, with their sync state; files not on this
  computer greyed out) and **Save to CraftCloud…** (the open project's backup into CraftCloud's `LoveCraft`
  folder, in the background, asking before it replaces a file and saying clearly when CraftCloud isn't signed in
  or its server can't be reached; a save can carry on in the background). "Replace?" says when the file already
  there last changed, and a save that CraftCloud kept beside a change made in the cloud meanwhile is reported as
  saved, as a conflict copy, never forced over it. CraftCloud is never waited on: every command runs in the
  background and is stopped if it doesn't answer. Without CraftCloud nothing changes. It works with today's CraftCloud too, which
  doesn't know the `prosecraft-rs` app yet: the listing is then filtered here.

- **The web app runs inside CraftCloud.** Opened from CraftCloud's file browser (framed with `?host=parent`), the
  web app edits the `.prosecraft` backup CraftCloud sends and saves it back there under the same name, after a
  pause in writing (at most every 15 seconds), at once with Ctrl+S, and when the page is hidden or closed, through
  the Craft apps' shared host messages (`craft-web-host`). It reports unsaved changes, typing included, so
  CraftCloud can warn before you leave, and says so when a save goes unconfirmed (the next waits for it). Projects
  you don't edit go back exactly as they came. A file from a newer ProseCraft and an encrypted backup open
  read-only and are never written back; a file that isn't a backup is refused. Outside a host, nothing changes,
  and the browser's library is never touched by a hosted file.
- **A web download.** Each release now has `lovecraft-web-<ver>.zip`, the web app to put on any static host,
  under any path (the zip CraftCloud pins). It is checksummed and signed like the other files.

### Fixes

- **Device sync leaves other sync programs' files alone.** In the open folder, a conflict copy that CraftCloud (or
  Syncthing, Nextcloud, Dropbox) made of a scene, chapter, storyboard or project file stopped the sync ("Two
  document files claim the id …"), and a copy that didn't would have been deleted by the next sync. Conflict
  copies, CraftCloud's `.craftcloud` folder and its files in progress are now never read as records and never
  removed, not even when the project they are in is deleted (its own files go, the copies and their folder
  stay); the sync names the conflict copies it found. A failure to list them no longer fails a sync.
- **The encrypted single file syncs on the desktop again.** Looking for conflict copies beside it failed with
  "Invalid sync path." after every sync, so the sync reported a failure (the file itself was written).
- **Nothing a newer release added to a project is lost when it is edited.** Saving a project keeps the fields
  and record kinds this version doesn't know, instead of rebuilding it from the ones it does (as ProseCraft
  does).
- **A backup with a project from a newer ProseCraft, opened into the library, leaves the app read-only**, so no
  edit is made that could never be saved.

## 0.1.2

The first release with downloads for every system.

- **Downloads for every system.** Each release now comes with ready-made packages, built and
  install-tested by the release workflow: a per-user installer (`.msi`, no administrator rights)
  and a portable `.zip` for Windows x64 and arm64; a universal `.dmg` for macOS (Apple silicon and
  Intel; not notarized yet, so open it with right-click › Open the first time); `.deb`, `.rpm`,
  Arch, Flatpak, AppImage and `.tar.gz` packages for Linux x86_64 and aarch64; and, when they
  build, FreeBSD and Linux riscv64 archives. Every package holds the MCP server
  (`prosecraft-mcp`) next to the app, registers the app for `.prosecraft` backups on Linux and
  Windows, and the files are checksummed in `SHA256SUMS.txt`, GPG-signed with the release key
  (`release-signing-key.asc`) and carry build-provenance attestations. There is also a Nix flake
  (`nix run github:jonasbove/ProseCraft-rs`). See the README's Downloads section.
- **An app icon**: an engraved octopus, in the window, the menus, the installers and the web app's tab.
- **Windows: no console window.** The app no longer opens a console window beside its own.
- **Windows: deeply nested text no longer risks a crash.** The app's main thread now gets 8 MB of stack on
  Windows, as on Linux and macOS (Windows' default is 1 MB), and the Markdown reader stops nesting quotes,
  lists and emphasis at 64 levels instead of 200 and reads the rest as plain text.
- **The MCP server reports its real version** (it said 0.1.0, as ProseCraft's does).
- **Chapters can be folded** in the binder (the arrow beside each chapter, or Left/Right); every chapter starts
  open. Long titles in the binder end in "…" and show in full on hover, with the actions button kept on the row.
- **Status messages fade.** Notes go after 8 seconds or with the next change of view; errors stay until then.
- **Network view:** page names no longer run into each other, relation labels keep clear of names, and Fit and
  Re-layout frame the whole graph with its names. The character card is a full-height panel.

## 0.1.1

The app is now called **ProseCraft-rs** wherever its name shows: the window title, About, the
macOS app menu, the welcome page, the diagnostics report, the producer field of exported PDF and
DOCX files, and the MCP server's title.

Nothing moves: the library's data folder, the browser database, the desktop app id and the saved
window state keep their old names, so 0.1.0 libraries open as before. The crate and command names
stay lowercase (`cargo run -p prosecraft-rs`).

Otherwise the same as 0.1.0, whose notes below list what the app does and the known limits.

## 0.1.0

The first public release of prosecraft-rs: a native Rust/egui rewrite of
[ProseCraft](https://github.com/jennraeross/ProseCraft) for Linux, macOS, Windows, BSD and the
browser. It is independent of the ProseCraft project; report problems here.

**What it does.** ProseCraft's feature set up to upstream `8cdc694` (v0.6.1):

- chapters and scenes, labels, binder drag and drop, split view
- the wiki, with files, web links, character cards and a relationship network
- outline, storyboard and timeline
- a rich-text editor with footnotes, citations, comments, snapshots, equations, diagrams,
  calculations, proofreading and focus modes
- publishing to PDF, EPUB, DOCX, HTML, Markdown, text and screenplay formats
- a local MCP server for ProseCraft open-folder sync directories

ProseCraft backups and sync folders open without losing anything; the formats are tested against
fixtures made by ProseCraft's own code. Cloud sync is planned through CraftCloud.

**Where it differs from ProseCraft, on purpose:**

- level 4–6 headings stay separate paragraphs
- imported backups are sanitized
- the MCP server keeps comments and snapshots it doesn't recognise

**Known limits:**

- the macOS native menu has only been compile-checked, not run on a Mac
- the desktop clipboard handling has only been tested headlessly
- on the web, pasting a copied image file does nothing

This is an early release: keep backups, and keep your ProseCraft copy until you trust it.

Source only; build with `cargo run -p prosecraft-rs` (see the README).
