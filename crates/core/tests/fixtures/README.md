# Golden fixtures from ProseCraft

These files were written by ProseCraft's own TypeScript (upstream `7dae544`), so the Rust port is
tested against what the original really reads and writes, not against our reading of it.

| File | What it pins |
|---|---|
| `json-cases.json` | `JSON.parse` / `JSON.stringify` (compact and indent 2) / `stableStringify`: lone surrogates, integer-like keys, duplicate keys, escapes, numbers |
| `numbers.json` | `String(n)` for 3,500 doubles (special values plus a fixed pseudo-random sample) |
| `crypto.json` | Argon2id key derivation (with NFKC and a lone surrogate in a passphrase), vault keys, and sealed records |
| `plain-text.json` | `htmlToPlainText` (equations and diagrams kept and left out) and `countWords` for the fixture scenes and HTML edge cases |
| `stamping.json` | `stampProject`: revisions, timestamps and tombstones after each kind of change |
| `rich-corpus.json` | Rich scene HTML as ProseCraft's Markdown converter and sanitizer write it, for the editor's lossless round trip |
| `export-text.json` | `compileBook` and `exportText` for a project with footnotes, lists, an empty and a trashed scene, under three sets of publish options |
| `sanitize.json` | `sanitizeHtml` and `sanitizeInlineHtml` on ProseCraft's own HTML, pasted markup and hostile input (scripts, `javascript:` links, bad images, forged markers) |
| `markdown.json` | `htmlToMarkdown` (with and without footnotes) and `htmlToInlineMarkdown`: escaping, whitespace, lists, code, links, images, equations, diagrams, footnotes, citations, plus the rich corpus |
| `export-markdown.json` | `exportMarkdown` for the same book and option sets as `export-text.json` |
| `export-html.json` | `exportHtml` for the same book under five option sets covering the four built-in styles, drop caps, per-chapter footnotes and every scene separator |
| `export-epub.json` | `exportEpub` for the same book under four option sets, as the unzipped entries in order (the random book id and the time replaced by `urn:uuid:ID` and `MODIFIED`) |
| `sync.json` | An encrypted sync folder and a single encrypted sync file written by ProseCraft's own providers, `safeName` cases, and eleven `mergeProjects` cases (every branch; `Math.random` pinned) |
| `comments.json` | `commentText` (ProseCraft's comment-dom over jsdom) for the rich corpus and fixture scenes, and `buildAnchor` / `locateAnchor` cases (repeated phrases, whitespace, astral characters, edits) |
| `starters.json` | `buildFromBlueprint` for every starter preset and the screenplay blueprint, with counter ids (the starter data itself is `crates/core/src/starters/data.json`, exported by the same generator) |
| `bundle-plain.prosecraft` | A plain backup with two projects that touch every stored field, including a field from a "future release" |
| `bundle-encrypted.prosecraft` | The same projects as an encrypted backup; `bundle-encrypted.key.json` holds its test passphrase and key |

The text inside is synthetic test data, written for these fixtures.

## Regenerating

The generator is a test file in a ProseCraft checkout (it is TypeScript, so it doesn't live in
this repository): `src/rust-fixtures.gen.test.ts` on the `tools/rust-fixtures` branch of the
fork. It replaces `crypto.getRandomValues` with a fixed stream, so the output is reproducible:

```sh
PROSECRAFT_FIXTURE_OUT=/path/to/prosecraft-rs/crates/core/tests/fixtures \
  npx vitest run src/rust-fixtures.gen.test.ts
```

Regenerate on the upstream commit named above, or note the new commit here and in
`docs/upstream-sync.md`.
