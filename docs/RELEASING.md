# Releasing LoveCraft

How a version goes out. Everything happens on https://github.com/jonasbove/LoveCraft (called
jonasbove/ProseCraft-rs until 0.2.0; GitHub redirects the old address): the code, its pull
requests, the tags and the releases. Adapted from CraftCloud's `docs/RELEASING.md`.

## What a release is

A `v<version>` tag on `main`, whose version matches `[workspace.package] version` in `Cargo.toml`. Pushed to
GitHub, the tag starts the **Release** workflow (`.github/workflows/release.yml`). It builds the packages for
Linux, Windows and macOS (`packaging/`, see [packaging/README-desktop.md](../packaging/README-desktop.md)), each
with the app (`lovecraft`) and the MCP server (`lovecraft-mcp`), and their 0.1 names (`prosecraft-rs`,
`prosecraft-mcp`) as aliases, and the fonts from
[storytold/craft-fonts](https://github.com/storytold/craft-fonts) at `CRAFT_FONTS_REF`. Then it makes a **draft**
GitHub release titled `LoveCraft v<version>`, with the files, `SHA256SUMS.txt`, GPG signatures (`.asc`),
`release-signing-key.asc` and build-provenance attestations. Its text has the Craft apps' layout: `## What's
Changed` (GitHub's generated notes: each pull request merged since the previous release, with its author) and
`**Full Changelog**`, plus a link to [RELEASE-NOTES.md](RELEASE-NOTES.md) when it has a section for the version.
FreeBSD and riscv64 are added to the draft later by the `late` job, when (and if) they build.

Each AppImage's `.zsync` is published twice: as `lovecraft-<version>-linux-<arch>.AppImage.zsync`, and as
`prosecraft-rs-<version>-linux-<arch>.AppImage.zsync`, the name a ProseCraft-rs 0.1 AppImage's updater looks for
(its update information names `jonasbove/ProseCraft-rs`, which GitHub redirects here, and
`prosecraft-rs-*-linux-<arch>.AppImage.zsync`). Its URL is relative, so it fetches the LoveCraft AppImage beside
it. Like every file, it is signed, checksummed, attested and required (`files`).

The web app is built too (`web`, `packaging/web/package.sh`): Trunk in release mode with the same fonts, zipped as
`lovecraft-web-<version>.zip` with one top-level folder `lovecraft-web-<version>/` holding Trunk's dist
(`index.html`, the JS and wasm, relative URLs) and the licence files, the layout of the Craft apps' web zips. The
job checks that every file `index.html` loads is in the zip. CraftCloud's `apps.toml` pins it as
`[apps.prosecraft-rs]` (the app's id keeps its former name) with `asset = "lovecraft-web-<version>.zip"` (it was
`prosecraft-rs-web-<version>.zip` before 0.2.0), `bridge = "native"` and the sha256
GitHub shows for the published asset (`cargo xtask bump-apps` there reads it), so a published release is all it
needs.

The draft is made only when:

- **every required file** was built (`files`), the web zip included;
- the **package tests** pass (`package-test`, `packaging/linux/test-package.sh`): every Linux package is installed
  in a clean container with its distro's package manager (`.deb` on Debian 12, Ubuntu 22.04 and 24.04; `.rpm` on
  Fedora 41 and 42; both on x86_64 and aarch64; the Arch package on Arch Linux; the AppImage and the tarball on
  Ubuntu 22.04), checked (`--version`, the binaries and their 0.1 names, the desktop entry, icons, AppStream file
  and the `.prosecraft` file type), the MCP server answers `initialize` and `tools/list` over stdio, the app opens
  its window, titled LoveCraft, on a virtual X display (Xvfb with Mesa's software OpenGL) and keeps running, the
  package is upgraded from the previous published release, and from ProseCraft-rs 0.1.2 (`LAST_PROSECRAFT_RS`,
  the last release before the rename, whose `prosecraft-rs` package `lovecraft` replaces, its commands still
  working), and removed again. The AppImage takes over the previous one's menu entry, and the tarball's
  `install.sh` installs over the previous one's. A previous release without the file is an error;
- the **workspace tests on Windows** (`windows-test`: `cargo test --workspace` on Windows x64), which `ci.yml`
  runs on Linux only;
- the **MSI tests** in the Windows jobs (`packaging/windows/test-msi.ps1`, x64 and arm64): the previous published
  release's MSI is installed, this one updates it in place (one product left, in the same folder, the 0.1
  program names still working), the MCP server answers, and uninstalling removes it all; then the same from
  ProseCraft-rs 0.1.2's MSI;
- the **macOS update test** (`packaging/macos/test-update.sh`): ProseCraft-rs 0.1.2's and the previous
  release's apps are copied from their DMGs into an Applications folder, then this one's `LoveCraft.app` over
  them; each answers `--version`, the MCP servers answer, the 0.1 command names are links, the bundle id is
  unchanged, and every app, old and new, reads the same library in
  `~/Library/Application Support/prosecraft-rs` and leaves it alone.

The macOS job checks the DMG as shipped (`packaging/macos/verify.sh`: both architectures in both binaries, the
icon, `--version`, the signature's integrity); the Windows jobs check each exe's architecture and PE subsystem
(the app a window program, the MCP server a console one). Nothing is released by itself: a person publishes the
draft.

## Checklist

Steps 1 and 2 are the release pull request ("Release <version>"), merged like any other: by a maintainer's
explicit confirmation.

1. **Release notes:** turn `## Unreleased` in [RELEASE-NOTES.md](RELEASE-NOTES.md) into `## <version>`, with what
   changed and, first, anything that needs care when updating. Start a new empty `## Unreleased` above it.
2. **Version:** set `version` in the root `Cargo.toml` and run `cargo update -w`. Update the README where it
   names the version (Status, and the `<ver>` example under Downloads). The AUR recipes in `packaging/arch/`
   stay on the published version: their sums are of the release's files, so they move after publishing
   (step 7).
3. **Gate:** `cargo xtask ci` must pass. To try the whole release without publishing anything, start the Release
   workflow by hand (Actions › Release › Run workflow, on `main` or a branch, or
   `gh workflow run release.yml --ref <branch>`): it builds every file and runs the package tests, and makes no
   draft (that needs a tag) and never touches the signing key.
4. **Before a bigger release, on a desktop:** install the new packages on a Fedora VM (`.rpm`) and a Debian VM
   (`.deb`), each with a desktop session, and check what containers can't: the Open and Save dialogs (through
   the desktop portal), double-clicking a `.prosecraft` backup in the file manager, the icon in the dock, and a
   real GPU. On a Mac: the DMG opens with right-click › Open, and the native menu works. On Windows: the
   installer, the Start menu shortcut, and `lovecraft-mcp` on PATH in a new terminal.
5. **Tag** `main` after the release pull request is merged: `git pull`, then
   `git tag -a v<version> -m "LoveCraft <version>"` on the merge commit and `git push origin v<version>`.
   Tags can't be moved or deleted afterwards, so check the commit first.
6. **Watch** the Release run. A failed package test means no draft: its log shows the distro, the step and the
   app's own output; fix it and tag a new version. When the draft has every file (and FreeBSD/riscv64, or their
   jobs have given up), read it over and **publish** it. Published releases are immutable: their files can't be
   changed afterwards.
7. **After publishing:** move the AUR recipes in `packaging/arch/` to the new version in a pull request:
   `pkgver=<version>` and `pkgrel=1` in both `PKGBUILD`s, `updpkgsums` (from `pacman-contrib`; it downloads
   the published files and the tag's source archive and writes their sums), `makepkg --verifysource`, and
   `makepkg --printsrcinfo > .SRCINFO`. [packaging/arch/README.md](../packaging/arch/README.md) has the exact
   commands. Then push `PKGBUILD` and `.SRCINFO` to the AUR (a maintainer's decision).

## The signing key

- Releases are signed with the key `29E57E45001D294F72E2EC80C42E3F3BDD6A7FF4`, named "ProseCraft-rs releases"
  (LoveCraft's former name: a user ID can only be added with the offline main key, so the key keeps it); its
  public part is [release-signing-key.asc](../release-signing-key.asc), attached to every release too.
- The main key is certify-only and kept offline. GitHub holds only its signing subkey
  `F518B82ABC37347A5ADF0CB322B89C26692F6185` (ed25519), without a passphrase, as the `RELEASE_SIGNING_KEY` secret
  of the `release` environment, which only `v*` tags may use. A run by hand never reaches it.
- The release job checks that `release-signing-key.asc` is in the repository and holds the subkey the secret
  signs with, and verifies every signature against that file alone, as a person downloading would. It fails, and
  makes no draft, when either is missing or they disagree.
- **The subkey expires on 2028-11-18.** Before then, make a new signing subkey with the main key, put it in the
  secret, and publish the updated public key (in the repository and on keys.openpgp.org). If the subkey leaks,
  revoke it with the main key, do the same, and say so in the next release's notes.

## GitHub settings the release relies on

- Environment `release`: deployments only from tags matching `v*`; holds `RELEASE_SIGNING_KEY`.
- Optional, unset for now: Apple (`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `KEYCHAIN_PASSWORD`,
  `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`) and Windows (`WINDOWS_CERTIFICATE`,
  `WINDOWS_CERTIFICATE_PASSWORD`, or Azure Trusted Signing's `AZURE_*`) code-signing secrets. Without them the
  builds are signed ad hoc on macOS and not at all on Windows, with a warning
  (`packaging/macos/import-cert.sh`, `packaging/windows/sign.ps1`).
- Build-provenance attestations need a public repository (or GitHub Enterprise Cloud).
