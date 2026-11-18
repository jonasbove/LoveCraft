# prosecraft-rs

**Outline. Write. Publish.** A novel-writing app, written in Rust with egui, that runs natively on Linux, macOS, Windows and BSD and in the browser.

> [!NOTE]
> **prosecraft-rs is an independent Rust rewrite of [ProseCraft](https://github.com/jennraeross/ProseCraft)** by Jennifer Roylance and contributors, used under the MIT licence. It is not affiliated with or endorsed by the ProseCraft project. For the original app, go to [ProseCraft](https://github.com/jennraeross/ProseCraft). Report problems with prosecraft-rs here, not there.

> [!CAUTION]
> 🚧 **Early planning: nothing works yet.** Right now this repository holds the workspace and the plan. Don't trust it with a manuscript.

## What it will do
The aim is ProseCraft's feature set, rebuilt natively:
- chapters and scenes
- a wiki for characters and places
- outline, storyboard canvas and timeline
- a rich-text editor with footnotes, comments, snapshots and focus modes
- publishing to PDF, EPUB, DOCX, HTML, Markdown and plain text

ProseCraft backups and sync folders are meant to open without losing anything. Cloud sync is planned through [CraftCloud](https://github.com/jonasbove/craftcloud). See [docs/ROADMAP.md](docs/ROADMAP.md).

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

prosecraft-rs is a rewrite of ProseCraft, © 2026 Jennifer Roylance, MIT licence. That copyright and permission notice is kept in [LICENSE-MIT](LICENSE-MIT) and [NOTICE](NOTICE), and every asset or text taken from ProseCraft is listed in [ATTRIBUTION.md](ATTRIBUTION.md). Thank you to Jennifer and the ProseCraft contributors for building it and sharing it openly.
