# Arch Linux packages

- **`package.sh`**: the release's own `lovecraft-<ver>-linux-<arch>.pkg.tar.zst`, built with
  `makepkg` from the Linux tarball (`.github/workflows/release.yml` runs it in an
  `archlinux:base-devel` container). Install it with `sudo pacman -U lovecraft-<ver>-linux-<arch>.pkg.tar.zst`.
- **`lovecraft-bin/PKGBUILD`**: an AUR recipe for the prebuilt release tarball. It checks the
  tarball's SHA-256 and its GPG signature (`.asc`) against the release signing key (`_fingerprint`,
  the key in `release-signing-key.asc`). Import it once before building:
  `gpg --import release-signing-key.asc`.
- **`lovecraft/PKGBUILD`**: an AUR recipe that builds the app from the tagged source with cargo,
  with the fonts from storytold/craft-fonts at the commit the release workflow uses (`_fonts`;
  keep it equal to `CRAFT_FONTS_REF` in `.github/workflows/release.yml`).

Both AUR recipes conflict with each other; both install the app (`lovecraft`) and the MCP server
(`lovecraft-mcp`).

LoveCraft was ProseCraft-rs until 0.2.0, and its packages were `prosecraft-rs` and
`prosecraft-rs-bin`. The `lovecraft` packages replace them: they provide and conflict with
`prosecraft-rs` and list it in `replaces`, so `pacman -Syu` offers the switch for a repository
package and `pacman -U` asks to remove `prosecraft-rs`. They keep its commands, `prosecraft-rs`
and `prosecraft-mcp`, as links. Where `prosecraft-rs` or `prosecraft-rs-bin` is published on the AUR, a merge request
moves its voters to the new package.

## Updating the AUR recipes for a release

The recipes here always describe a **published** release, with its real checksums, and each has the
`.SRCINFO` the AUR needs beside it. They move to a new version only after that release is published
(its files, and the tag's source archive, are what the sums are of), in a pull request of their own
(docs/RELEASING.md, step 7). In a checkout of this repository, with `pacman-contrib` installed:

```sh
cd packaging/arch
# Downloads and builds go to scratch folders, not into the repository.
export GNUPGHOME="$(mktemp -d)" SRCDEST="$(mktemp -d)" BUILDDIR="$(mktemp -d)" PKGDEST="$(mktemp -d)"
gpg --import ../../release-signing-key.asc   # makepkg checks the tarballs' signatures with it
for pkg in lovecraft lovecraft-bin; do
  (cd "$pkg" &&
    sed -i 's/^pkgver=.*/pkgver=<ver>/; s/^pkgrel=.*/pkgrel=1/' PKGBUILD &&
    updpkgsums &&                            # downloads the sources and writes their sha256sums
    makepkg --verifysource &&                # checks them, and the signatures
    makepkg --printsrcinfo > .SRCINFO)
done
(cd lovecraft-bin && makepkg -f)             # builds the package (lovecraft too, but it compiles for a while)
git status --short .                         # only the PKGBUILDs and .SRCINFOs changed
```

The `.SRCINFO` is generated, never edited by hand; regenerate it after every change to a
`PKGBUILD`.

To publish, copy `PKGBUILD` and `.SRCINFO` into a clean checkout of the AUR repository
(`ssh://aur@aur.archlinux.org/<name>.git`), then
`git add PKGBUILD .SRCINFO && git commit -m "Update to <ver>" && git push`. Publishing to the AUR
is a maintainer's decision.
