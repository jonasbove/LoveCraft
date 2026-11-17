<!-- craft-house-rules:start core source=279b2c09ef2b2f0b4f5e616a47d39e29db0a1eb3 sha256=bee0ffafad196b68c76425770e344e588463bbda685ab41581fcbabb85dfc930 -->
## House rules

These apply in every Craft repository. A later rule may be stricter, not weaker. The source is `standards/house-rules-core.md` in `storytold/craft-contrib`. Propose changes there, not in a copy.

1. **A person is accountable.** Whoever runs an agent answers for what it opens and for its review comments.
2. **Check, then claim.** Before writing code, read the issue comments, search pull requests for `#N` and the keywords, and check recent commits on `main`. Comment on the issue first. Release a claim you will not finish.
3. **Verify, don't guess.** Open a definition before using it. Confirm the cause in the code, even when someone names a location.
4. **Keep fixes narrow and tested.** Fix what the issue describes, where the bug is. Add a test that fails without the fix.
5. **Review the diff before opening a pull request.** Identifiers exist as used, the test fails without the fix, nothing unrelated changed, and the description is true.
6. **Look for a better pull request.** If another fixes the same thing, close yours when it is better or equal, and say why.
7. **Report only what ran.** Say what you tested and what you could not run. Without a local build or test, say so and rely on CI before marking ready.
8. **`main` changes through a pull request.** Do not push to it. Branch protection enforces this. There is no exception.
9. **Stop and ask only about** licensing outside the allowlist, publishing, releases, a new remote, secrets, spending money, destructive git, and anything else hard to undo. Otherwise keep working. `plan/` is a gitignored notebook, not rules. Without it, do not ask for it and do not stop.
10. **Keep secrets and personal details out** of the repository, logs and requests.
11. **CI stays shared.** Workflows call `storytold/craft-common-actions`. Do not change CI in a feature or fix pull request. If CI cannot cover a change, open an issue there.
12. **Guards say why.** A check that looks removable says what it prevents. Read that before changing it.
13. **Instructions must be readable by everyone.** No notes about one machine, and no links to files a reader cannot open. `craftrules` is retired. Do not link to it.
14. **Keep pull requests moving.** Concurrent pull requests are fine; unanswered actionable feedback is not. Respond in 7 days, a reminder at 14, closure eligible at 30, from when an actionable request waits on the author. Suggestions and unconfirmed bot findings do not start the clock. A fix, explanation or agreed plan resets it; unrelated activity does not. Waiting on a reviewer or maintainer pauses it and does not count against the author. A maintainer decides closure; it is never automatic. Reuse search results, do not poll, and stop at a rate limit.
15. **No agent attribution.** A pull request does not say an agent wrote it. A repository rule may not require that.
16. **Shared libraries first for Craft apps.** Before adding or changing reusable file-format, codec, protocol or UI code, check `storytold/craft-libs` and relevant open pull requests. Reuse its public APIs or extend the appropriate shared library instead of duplicating an implementation in an app. Keep app-specific policy and model mapping local; preserve behavior, security and resource limits, attribution, layering and regression coverage. Document API gaps and maintainer-approved exceptions rather than silently dropping behavior. This does not authorize dependencies on sibling application crates or override existing platform, licensing or dependency-source requirements.
<!-- craft-house-rules:end -->
<!-- craft-house-rules:start rust source=279b2c09ef2b2f0b4f5e616a47d39e29db0a1eb3 sha256=56ef2b873de68940cf903069bbd0927b119f5c9ec2717f7f627041d2ebe8e4f2 -->
## Rust Craft repositories

These apply to native Craft apps and to shared Rust libraries. Select them by repository type. A repository that merely contains some Rust does not take this block. Rules later in a repository's file may be stricter, but not weaker. The source is `standards/house-rules-rust.md` in [storytold/craft-contrib](https://github.com/storytold/craft-contrib).

1. **Never crash.** Non-test code does not panic and has no `unsafe`, except where the repository file names an exception. Return errors, treat input as hostile, and bound recursion and allocations. New production crate roots copy the `deny` attribute from an existing crate: `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::unimplemented`, `clippy::todo`, `clippy::unreachable`. Every crash fix comes with a test that panicked before the fix.
2. **Clean room.** Learn from observed behaviour and public specifications. Do not copy another product's code, assets, strings or artwork. Check a dependency's licence before adding it.
3. **Assets and fonts.** Every asset is original or suitably licensed, and is recorded in `ATTRIBUTION.md` in the same change. Font files live in [storytold/craft-fonts](https://github.com/storytold/craft-fonts), not in the app repository.
4. **Rust only for native Craft apps.** No handwritten JavaScript or TypeScript, and no Tauri, Electron or webview shell, in those apps. Shared tooling follows its documented technical scope and maintainer-approved exceptions. Craft PR Review's existing Tauri and TypeScript UI is one such exception. It does not relax this rule for native apps. Preserve any existing web build.
5. **Time goes through `chrono`.** Do not hand-roll calendar conversion. For dates beyond what `chrono` can represent, follow [standards/time.md](https://github.com/storytold/craft-contrib/blob/main/standards/time.md).
6. **Contributor credits.** Add or edit only your own person's entry in `contributors/people.toml`, and only after asking them. Never guess a name or hand-edit `contributors.json`.

## Licence allowlist

Dependencies and assets use SPDX identifiers. An expression is allowed when every identifier is in this list. A repository may name a shorter list. It may not add an identifier here by editing its own copy of these rules.

- `ISC`
- `MIT`
- `BSD-2-Clause`
- `BSD-3-Clause`
- `Apache-2.0`
- `Unicode-3.0`
- `OFL-1.1`
- `Ubuntu-font-1.0`
- `Bitstream-Vera`
- `CC0-1.0`
- `CC-BY-4.0`
- `CC-BY-SA-4.0`
- `Zlib`
- `BSL-1.0`
- `0BSD`
- `Unlicense`
- `IJG`
- `NCSA`
- `CDLA-Permissive-2.0`
- `Apache-2.0 WITH LLVM-exception`
- `LicenseRef-Public-Domain`

`MPL-2.0` is allowed for third-party dependencies used unmodified; vendoring, patching or copying its code needs a maintainer's approval. `MPL-2.0+` counts as `MPL-2.0`.

`LicenseRef-Storyteller-Trademark` is not in this list. It covers Storyteller brand marks only, where a repository already says so.
<!-- craft-house-rules:end -->

# prosecraft-rs: instructions for agents

prosecraft-rs is a native Rust/egui novel-writing app: outline, write, storyboard and publish. It is a rewrite of [ProseCraft](https://github.com/jennraeross/ProseCraft) by Jennifer Roylance and its contributors, which is MIT-licensed and written in SvelteKit/TypeScript in a Tauri shell. That copyright and licence notice is kept in `LICENSE` and `NOTICE`. Credit the original project wherever the app describes where it comes from (About window, README, store text).

## Start every session here
Read [`docs/ROADMAP.md`](docs/ROADMAP.md) and [`docs/upstream-sync.md`](docs/upstream-sync.md). `plan/` is an optional gitignored notebook. Do not ask for it, and do not stop if it is missing.

## People's novels come first
People trust this app with books that took them years. **Losing or corrupting a manuscript is the worst bug this repository can have.** It outranks every feature.
- **Reading a file never changes it.** Import copies the data. It never rewrites, renames or deletes the source.
- **Keep what you don't understand.** Every stored record keeps the fields it does not know (`#[serde(flatten)] extra`), and they are written back unchanged. HTML this app cannot represent opens read-only. It is never dropped.
- **Refuse to write what a newer build made.** A file whose `schemaVersion` or format version is newer than this build's opens read-only, with a message.
- **Writes are atomic.** Write to a temp file, then rename, using `craft-atomic`. Never truncate a file in place.
- **Compatibility is tested against fixtures, not memory.** The `crates/core/tests/fixtures/` corpus (planned for P1) is produced by the original TypeScript app. The round-trip tests must stay green: bundle, encrypted bundle, open folder, encrypted folder, the HTML dialect, `stableStringify` hashes and AAD strings. A format change lands with a new fixture.

## Never crash
- Non-test code has no panics and no `unsafe`. Production crate roots carry `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]`.
- Input is hostile: backups, sync folders, imported EPUB, Scrivener and RTF files, MCP parameters and Google Drive responses. Bound sizes, recursion and allocations.
- Every crash fix lands with a test that panicked before the fix.

## Architecture
| Crate | Job | Status |
|---|---|---|
| `crates/core` | Project model, the ProseCraft HTML dialect, storage formats (bundle, encrypted, open folder), sync merge, undo, search and replace, calc. No UI and no I/O policy | started (`about`) |
| `crates/formats` | Publish (EPUB, HTML, Markdown, text, DOCX, PDF, Fountain) and import (EPUB, Scrivener, RTF, Markdown folder, Fountain), on craft-libs | planned |
| `crates/native` | File system, folder sync, CraftCloud integration, Zotero, OS integration. Not built for wasm | planned |
| `crates/mcp` | The MCP server on `craft-mcp-server` | planned |
| `crates/ui-egui` | The egui front end, including the manuscript editor | started |
| `apps/prosecraft-rs`, `apps/prosecraft-rs-web` | Desktop and web (Trunk) binaries | started |
| `apps/prosecraft-rs-cli` | Convert, check, backup and MCP, headless | planned |

- Nothing below `ui-egui` depends on egui, eframe, winit or rfd. `core` and `formats` build for `wasm32-unknown-unknown`.
- **Shared libraries first** (core rule 16): ZIP, XML, HTML tokenizing, EPUB, RTF, DOCX, PDF, Markdown, math, proofing, settings, atomic writes, i18n, menus, keymaps, dialogs and MCP come from `storytold/craft-libs`, pinned by git rev. Do not add a crates.io crate that duplicates one of them.
- Time goes through `chrono`.

## Quality gates
Run `cargo xtask ci` before every commit. It covers fmt, clippy `-D warnings`, tests, layers and wasm. UI changes come with egui_kittest tests that drive the change by keyboard and check the AccessKit tree.

## Names
User-visible names follow [standards/naming.md](https://github.com/storytold/craft-contrib/blob/main/standards/naming.md). "Scrivener", "Zotero", "EPUB", "DOCX" and "Fountain" may appear only as file formats and integrations ("Import a Scrivener project"), never as the name of our own feature.

## Accessibility
The rules are [standards/house-rules-accessibility.md](https://github.com/storytold/craft-contrib/blob/main/standards/house-rules-accessibility.md), which apply in full. The manuscript editor is a drawn surface. It publishes AccessKit text nodes with caret, selection and editability.
