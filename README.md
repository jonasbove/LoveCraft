# ProseCraft-rs

**Outline. Write. Publish.** A novel-writing app, written in Rust with egui, that runs natively on Linux, macOS, Windows and BSD and in the browser.

> [!NOTE]
> **ProseCraft-rs is an independent Rust rewrite of [ProseCraft](https://github.com/jennraeross/ProseCraft)** by Jennifer Roylance and contributors, used under the MIT licence. It is not affiliated with or endorsed by the ProseCraft project. For the original app, go to [ProseCraft](https://github.com/jennraeross/ProseCraft). Report problems with ProseCraft-rs here, not there.

> [!CAUTION]
> **0.1.0: early release.** It does what ProseCraft does (up to upstream `8cdc694`, v0.6.1), but it is new: it has not been run on a Mac yet, and it hasn't carried a whole book. Keep backups, and keep your ProseCraft copy until you trust it.

## What it does
ProseCraft's feature set, rebuilt natively:
- chapters and scenes, labels, a binder with drag and drop, split view
- a wiki for characters and places, with files, web links and a relationship network
- outline, storyboard canvas and timeline
- a rich-text editor with footnotes, citations (Zotero, BibTeX, RIS, CSL), comments, snapshots, equations, diagrams, calculations, spelling and grammar, and focus modes
- publishing to PDF, EPUB, DOCX, HTML, Markdown, plain text and screenplay formats, with covers and front and back matter
- a local MCP server for ProseCraft open-folder sync directories (`apps/prosecraft-mcp`)

ProseCraft backups and sync folders open without losing anything; the formats are tested against fixtures made by ProseCraft's own code. Cloud sync is planned through [CraftCloud](https://github.com/jonasbove/craftcloud). See [docs/ROADMAP.md](docs/ROADMAP.md).

Known limits in 0.1.0: the macOS native menu has only been compile-checked; the desktop clipboard handling has only been tested headlessly; on the web, pasting a copied image file does nothing.

## Downloads
Every release from the one after 0.1.1 on the [releases page](https://github.com/jonasbove/ProseCraft-rs/releases) has
packages for every system, each with the app and the MCP server (`prosecraft-mcp`). The app also comes from the
Nix flake or from source. `<ver>` is the version number, for example `0.2.0`.

The builds are **not code-signed** by Apple or Microsoft yet, so your system asks once before the first start.

**Windows** (10 and 11, x64 and arm64)

| File | What it is |
|---|---|
| `prosecraft-rs-<ver>-windows-{x64,arm64}.msi` | Installer, for you only (no administrator rights). SmartScreen: **More info › Run anyway**. Adds ProseCraft-rs to the Start menu and to "Open with" for `.prosecraft` backups, and puts `prosecraft-mcp` on your PATH |
| `prosecraft-rs-<ver>-windows-{x64,arm64}-portable.zip` | The same `prosecraft-rs.exe` and `prosecraft-mcp.exe`, to run without installing |

**macOS** (11 or later, Apple silicon and Intel)

| File | What it is |
|---|---|
| `prosecraft-rs-<ver>-macos-universal.dmg` | Drag `ProseCraft-rs.app` to Applications, then **right-click › Open › Open** the first time (or `xattr -dr com.apple.quarantine /Applications/ProseCraft-rs.app`). The MCP server is `ProseCraft-rs.app/Contents/MacOS/prosecraft-mcp` |

**Linux** (x86_64 and aarch64)

| File | What it is |
|---|---|
| `prosecraft-rs-<ver>-linux-<arch>.deb` | Debian, Ubuntu, Mint, Pop!_OS (`sudo apt install ./prosecraft-rs-….deb`) |
| `prosecraft-rs-<ver>-linux-<arch>.rpm` | Fedora, openSUSE, RHEL (`sudo dnf install ./prosecraft-rs-….rpm`) |
| `prosecraft-rs-<ver>-linux-<arch>.pkg.tar.zst` | Arch Linux (`sudo pacman -U prosecraft-rs-….pkg.tar.zst`); AUR recipes are in `packaging/arch/` |
| `prosecraft-rs-<ver>-linux-<arch>.flatpak` | Flatpak (`flatpak install --user prosecraft-rs-….flatpak`); the MCP server is `flatpak run --command=prosecraft-mcp io.github.jonasbove.prosecraft_rs` |
| `prosecraft-rs-<ver>-linux-<arch>.AppImage` | Any distro: make it executable and run it; with `--mcp` first it runs the MCP server. The `.zsync` next to it lets AppImage updaters fetch only what changed |
| `prosecraft-rs-<ver>-linux-<arch>.tar.gz` | The binaries, menu entry, file type, icons and `install.sh` |

```sh
tar xzf prosecraft-rs-<ver>-linux-x86_64.tar.gz
cd prosecraft-rs-<ver>-linux-x86_64
./install.sh            # to ~/.local, no root; ./install.sh --remove undoes it
```

The packages register ProseCraft-rs for `.prosecraft` backups, so the file manager opens them with it.
FreeBSD (`prosecraft-rs-<ver>-freebsd-{x86_64,aarch64}.tar.gz`) and Linux riscv64 (`.deb`, `.rpm`, `.tar.gz`)
builds are attempted too, but ProseCraft-rs has never been tried there; a release may come without them.
Installing, updating or removing a package never touches your library.

**Checking a download**

Every release has `SHA256SUMS.txt`. Check the files you downloaded against it:

```sh
sha256sum -c SHA256SUMS.txt --ignore-missing              # macOS: shasum -a 256 -c SHA256SUMS.txt --ignore-missing
```

Every file also has a GPG signature (`.asc`) next to it, made with the ProseCraft-rs release key
`29E57E45001D294F72E2EC80C42E3F3BDD6A7FF4` ("ProseCraft-rs releases"; its signing subkey is
`F518B82ABC37347A5ADF0CB322B89C26692F6185`, valid until 2028-11-18). Its public key is `release-signing-key.asc`,
on every release and in this repository. Import it once and check that the fingerprint matches, then:

```sh
gpg --import release-signing-key.asc
gpg --verify SHA256SUMS.txt.asc SHA256SUMS.txt
gpg --verify prosecraft-rs-<ver>-linux-x86_64.AppImage.asc prosecraft-rs-<ver>-linux-x86_64.AppImage
```

Each file also carries a signed build-provenance attestation that ties it to this repository's release workflow
and the commit it was built from ([GitHub CLI](https://cli.github.com)):

```sh
gh attestation verify prosecraft-rs-<ver>-macos-universal.dmg --repo jonasbove/ProseCraft-rs
```

**Nix and NixOS**

```sh
nix run github:jonasbove/ProseCraft-rs                  # the app
nix run github:jonasbove/ProseCraft-rs#prosecraft-mcp   # the MCP server
```

To keep it installed, add the flake as an input and install its package
(`inputs.prosecraft-rs.packages.${pkgs.stdenv.hostPlatform.system}.default`, in `environment.systemPackages` or
Home Manager's `home.packages`). It brings its own menu entry, file type and icons.

How a release is made: [docs/RELEASING.md](docs/RELEASING.md). What changed: [docs/RELEASE-NOTES.md](docs/RELEASE-NOTES.md).

## Build from source
The fonts (Inter for the interface, Source Serif 4 for the manuscript) come from a checkout of
[craft-fonts](https://github.com/storytold/craft-fonts); without it the app builds and uses egui's
and your system's fonts.

```sh
git clone https://github.com/storytold/craft-fonts ../craft-fonts
export CRAFT_FONTS_DIR=$PWD/../craft-fonts
cargo run -p prosecraft-rs            # desktop
cargo xtask ci                        # fmt, clippy, tests, layers, wasm check
cd apps/prosecraft-rs-web && trunk build --release   # web build into dist/web
```

## Licence and credits
`MIT OR Apache-2.0`, see [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

ProseCraft-rs is a rewrite of ProseCraft, © 2026 Jennifer Roylance, MIT licence. That copyright and permission notice is kept in [LICENSE-MIT](LICENSE-MIT) and [NOTICE](NOTICE), and every asset or text taken from ProseCraft is listed in [ATTRIBUTION.md](ATTRIBUTION.md). Thank you to Jennifer and the ProseCraft contributors for building it and sharing it openly.
