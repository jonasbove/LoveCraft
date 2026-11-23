# Roadmap: ProseCraft-rs

ProseCraft-rs is a native Rust/egui rewrite of [ProseCraft](https://github.com/jennraeross/ProseCraft), the MIT-licensed novel-writing app by Jennifer Roylance and its contributors. The aim is the same app: outline, write, storyboard and publish. It would run natively on Linux, macOS, Windows and BSD and on the web, with no webview, TypeScript or Tauri, and would follow the Craft suite's house rules.

Status: 0.1.0 has ProseCraft's feature set up to upstream `8cdc694` (v0.6.1). P10 for 0.2.0: local folder sync with conflict handling is done; on the desktop, CraftCloud's half is built (Open from CraftCloud…, Save to CraftCloud…, device sync in a CraftCloud folder that leaves CraftCloud's conflict copies and part files alone and reports them, through the `craftcloud` program; see the README's CraftCloud section). Still open for P10: CraftCloud's web-app bridge in the browser. The rest of this document is the plan as it was written.

## The goal for writers' data
A manuscript must never be lost or corrupted. The design aims for:
- Reading a file never changes it.
- Fields and formatting the app doesn't understand are kept.
- Files from a newer version open read-only.
- Every save is atomic.
- Compatibility with ProseCraft's backup (`.prosecraft`) and sync-folder formats is tested against fixtures made by the original app.

The way to move work over: export a backup or set up folder sync in ProseCraft, then open that in ProseCraft-rs.

## Proposed milestones
| # | Milestone | Done when |
|---|---|---|
| P0 | Workspace, CI, house rules | An empty window passes `cargo xtask ci` |
| P1 | Formats and fixtures | Every ProseCraft backup and sync format in the fixture corpus round-trips losslessly; readers are fuzzed |
| P1b | MCP server | A Rust MCP server with the existing tools, over the open sync folder |
| P2 | Editor spike | Typing, IME, clipboard, undo, accessibility tree, atomic footnote/citation nodes, image wrap. A go/no-go review follows |
| P3 | App shell | Projects, binder, outline, trash, labels, settings, goals, search & replace, snapshots, comments, autosave |
| P4 | Editor features | Footnotes, images, chapter mode, split view, Markdown source, focus/zen/typewriter, wiki mentions, new-project starters |
| P6 | Publish | EPUB, HTML, Markdown, text, DOCX, PDF, covers, styles, front/back matter, TOC, page preview |
| P7 | Storyboard & timeline | Canvas with keyboard alternatives to every drag |
| P8 | Import | EPUB, Scrivener projects, Markdown folders, Fountain |
| P9 | Writing extras | Screenplay mode, calculations, equations, citations, proofreading |
| P10 | Sync | Local folder sync with conflict handling, and cloud sync through [CraftCloud](https://github.com/jonasbove/craftcloud) (its sync folder on desktop, its web-app bridge in the browser) instead of Google Drive |
| P11 | Web and packages | Web build; Linux, macOS and Windows packages |

P5 was folded into P1b.

## Open questions
Name and branding, the scope of the web and phone experience, and whether to keep diagram rendering and WebDAV.

## Credits
ProseCraft © 2026 Jennifer Roylance, MIT licence. ProseCraft-rs keeps that notice (see `NOTICE`) and is an independent project, not affiliated with or endorsed by ProseCraft.
