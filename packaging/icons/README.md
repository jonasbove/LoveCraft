# The app icon

ProseCraft-rs's icon is an engraved octopus, artwork provided by Jonas Bové for ProseCraft-rs, on
a solid "ritual oxblood" (`#4b171e`) rounded square (corner radius 18 % of the side, transparent
outside the corners), the octopus scaled to 92 % and centred.

| File | What it is |
|---|---|
| `prosecraft-rs-icon-2048.png` | **The master.** Every icon the app and the packages ship is rendered from it |
| `octopus-original-coral.png` | The original artwork, the engraving on a flat coral background |
| `octopus-engraving.png` | The octopus alone (grey and alpha), lifted off the coral by `make_icon.py` |
| `make_icon.py` | The design-time script (Python, numpy, Pillow) that lifts the octopus off the coral (it reads the original as `source-octopus.png`) and made the candidate backgrounds; not part of any build |
| `render.sh` | Renders the master into every size below |

`render.sh` writes:

- the Linux hicolor PNGs, 16 to 512 px (`packaging/linux/icons/hicolor/`), which the deb, rpm, Arch
  package, tarball, AppImage, Flatpak, FreeBSD archive and Nix package install;
- `packaging/macos/prosecraft-rs-1024.png`, from which `packaging/macos/package.sh` makes the app's
  `.icns` with `iconutil`;
- `packaging/windows/prosecraft-rs.ico` (16 to 256 px), the MSI's icon in Apps & features and on
  the Start menu shortcut;
- `apps/prosecraft-rs/assets/icon-256.png`, the desktop window's icon, embedded in the binary;
- `apps/prosecraft-rs-web/assets/favicon.png`, the web app's favicon.

The 16, 24 and 32 px sizes are the same composition with a light unsharp mask: a tighter crop was
tried and cut the tentacles off without making 16 px any clearer.

## Changing the icon

1. Replace `prosecraft-rs-icon-2048.png` with the new master: a square PNG, 2048 px, with its
   rounded corners or transparent margin as it should look on a dock.
2. Run `packaging/icons/render.sh` (needs ImageMagick 7's `magick`; uses `optipng` when installed).
   It ends with `packaging/check-icons.sh`, which checks every file and its size.
3. Commit the master and what the script wrote, and update its row in `ATTRIBUTION.md`.

Nothing else names the icon's files, so no script, manifest or workflow changes.
