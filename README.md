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
