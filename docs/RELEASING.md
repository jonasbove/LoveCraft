# Releasing ProseCraft-rs

How a version goes out. Everything happens on https://github.com/jonasbove/ProseCraft-rs: the code, its pull
requests, the tags and the releases. Adapted from CraftCloud's `docs/RELEASING.md`.

## What a release is

A `v<version>` tag on `main`, whose version matches `[workspace.package] version` in `Cargo.toml`. Pushed to
GitHub, the tag starts the **Release** workflow (`.github/workflows/release.yml`). It builds the packages for
Linux, Windows and macOS (`packaging/`, see [packaging/README-desktop.md](../packaging/README-desktop.md)), each
with the app (`prosecraft-rs`) and the MCP server (`prosecraft-mcp`), and the fonts from
[storytold/craft-fonts](https://github.com/storytold/craft-fonts) at `CRAFT_FONTS_REF`. Then it makes a **draft**
GitHub release titled `ProseCraft-rs v<version>`, with the files, `SHA256SUMS.txt`, GPG signatures (`.asc`),
`release-signing-key.asc` and build-provenance attestations. Its text has the Craft apps' layout: `## What's
Changed` (GitHub's generated notes: each pull request merged since the previous release, with its author) and
`**Full Changelog**`, plus a link to [RELEASE-NOTES.md](RELEASE-NOTES.md) when it has a section for the version.
FreeBSD and riscv64 are added to the draft later by the `late` job, when (and if) they build.

The draft is made only when:

- **every required file** was built (`files`);
- the **package tests** pass (`package-test`, `packaging/linux/test-package.sh`): every Linux package is installed
  in a clean container with its distro's package manager (`.deb` on Debian 12, Ubuntu 22.04 and 24.04; `.rpm` on
  Fedora 41 and 42; both on x86_64 and aarch64; the Arch package on Arch Linux; the AppImage and the tarball on
  Ubuntu 22.04), checked (`--version`, the binaries, the desktop entry, icons, AppStream file and the
  `.prosecraft` file type), the MCP server answers `initialize` and `tools/list` over stdio, the app opens its
  window on a virtual X display (Xvfb with Mesa's software OpenGL) and keeps running, the package is upgraded from
  the previous published release when that release has packages (0.1.0 and 0.1.1 have none), and removed again;
- not yet: the **workspace tests on Windows** (`windows-test`). The job runs on every release, but some tests
  fail on Windows (a stack overflow in prosecraft-core's unit tests, line endings in the byte-for-byte fixtures,
  `/` in two MCP tests' paths), so it doesn't hold the draft back until they are fixed. Read its result before
  publishing.

The macOS job checks the DMG as shipped (`packaging/macos/verify.sh`: both architectures in both binaries, the
icon, `--version`, the signature's integrity); the Windows jobs check each exe's architecture and PE subsystem
(the app a window program, the MCP server a console one). Nothing is released by itself: a person publishes the
draft.

## Checklist

Steps 1 and 2 are the release pull request ("Release <version>"), merged like any other: by a maintainer's
explicit confirmation.

1. **Release notes:** turn `## Unreleased` in [RELEASE-NOTES.md](RELEASE-NOTES.md) into `## <version>`, with what
   changed and, first, anything that needs care when updating. Start a new empty `## Unreleased` above it.
2. **Version:** set `version` in the root `Cargo.toml` and run `cargo update -w`. Update the README if it names
   the version, and `pkgver` in `packaging/arch/*/PKGBUILD` (their sums come after publishing, step 7).
3. **Gate:** `cargo xtask ci` must pass. To try the whole release without publishing anything, start the Release
   workflow by hand (Actions › Release › Run workflow, on `main` or a branch, or
   `gh workflow run release.yml --ref <branch>`): it builds every file and runs the package tests, and makes no
   draft (that needs a tag) and never touches the signing key.
4. **Before a bigger release, on a desktop:** install the new packages on a Fedora VM (`.rpm`) and a Debian VM
   (`.deb`), each with a desktop session, and check what containers can't: the Open and Save dialogs (through
   the desktop portal), double-clicking a `.prosecraft` backup in the file manager, the icon in the dock, and a
   real GPU. On a Mac: the DMG opens with right-click › Open, and the native menu works. On Windows: the
   installer, the Start menu shortcut, and `prosecraft-mcp` on PATH in a new terminal.
5. **Tag** `main` after the release pull request is merged: `git pull`, then
   `git tag -a v<version> -m "ProseCraft-rs <version>"` on the merge commit and `git push origin v<version>`.
   Tags can't be moved or deleted afterwards, so check the commit first.
6. **Watch** the Release run. A failed package test means no draft: its log shows the distro, the step and the
   app's own output; fix it and tag a new version. When the draft has every file (and FreeBSD/riscv64, or their
   jobs have given up), read it over and **publish** it. Published releases are immutable: their files can't be
   changed afterwards.
7. **After publishing:** update the AUR recipes in `packaging/arch/` (`pkgver`, `updpkgsums`,
   `makepkg --printsrcinfo > .SRCINFO`) and push them to the AUR (`packaging/arch/README.md`).

## The signing key

- Releases are signed with the key `29E57E45001D294F72E2EC80C42E3F3BDD6A7FF4` ("ProseCraft-rs releases"); its
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
