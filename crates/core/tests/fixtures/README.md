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
| `search.json` | `searchDocuments` (14 queries: folding, case, whole words, trash, wiki, astral, Hangul), `replaceInDocuments` (6, with titles, trash and a match spanning formatting) and `findFirstMatch` |
| `epub-import.json` | `parseEpub` on 25 books (ProseCraft's own test cases: EPUB 3 nav and EPUB 2 NCX, the spine fallback, scene breaks, wrappers, title and licence pages, images, covers, DRM, sanitizing, broken archives, traversal; plus HTML-fallback markup, and three books ProseCraft's exporter wrote), each with its bytes and the manuscript or the error; and `buildEpubProject` with counter ids |
| `scrivener-import.json` | `rtfToHtml` on 48 inputs (styles, groups and destinations, `\uN` and `\ucN`, code pages: Windows-125x, CJK labels the browser has or lacks, UTF-8 with a BOM, raw 8-bit bytes, malformed escapes), and `buildScrivenerProject` on 14 projects (Files/Docs and Files/Data layouts, folders, nested folders, research and other top-level items, trash, missing files, names, bad XML) and 4 zips (macOS junk, traversal, absolute paths, not a zip), with counter ids |
| `fountain.json` | `parseFountain` on 24 screenplays (ProseCraft's sample and test inputs, title pages, forced elements, scene numbers, cues and dual dialogue, transitions, sections, notes and boneyard, emphasis, line endings), each with `elementsToHtml`, `htmlToElements` of that HTML, and `serializeFountain` |
| `custom-starters.json` | `normalizeCustomStarters` on awkward stored templates (duplicate and missing ids, module keys known and unknown, non-objects, over-long titles and HTML, the folder, page and template limits) and what `saveCustomStarters` writes; and the custom folder and page edits on a preset, then `filterBlueprint` |
| `marked.json` | `markdownToHtml`, `markdownToDocument` and marked's tokens (type and raw text) on the 652 CommonMark 0.31.2 spec examples (CC-BY-SA 4.0, see `ATTRIBUTION.md`) and ProseCraft's own syntax: footnote definitions and references, Pandoc citations, inline and display math, diagram and calculation blocks, task lists |
| `marked-random.json` | The same (the document's footnotes only, its HTML being the HTML) for 800 inputs from fixed-seed generators, 400 of short pieces (delimiters, escapes, entities, HTML, non-ASCII and astral characters, ProseCraft's syntax) and 400 of whole lines (indents and tabs, list, quote and heading markers, fences, tables, definitions with continuation lines), plus 8 inputs that once differed; it catches differences between JavaScript's UTF-16 positions and the port's byte positions |
| `markdown-folder.json` | `markdown-folder.ts`: `parseFrontmatter` (lists, quotes, BOM and line endings, unclosed headers), `humanizeName`, `isImportableMarkdownPath`, `suggestRole`, `listImportFolders` (drafts, loose files, natural order), `roleOf`, `estimateStoredChars`, and `buildMarkdownProject` on ProseCraft's own test folder and five more (positions, wikilinks, nested and long folder names, astral names, ties, an empty import) in both chapter modes, with counter ids |
| `csl.json` | `normalizeCslItemsDetailed` (types, names, dates, links, numbers, control characters, length caps, fallback ids, duplicates, items left out, the 5,000-item cap) and `sanitizeCitationId` |
| `sync-open.json` | The open sync folder ProseCraft's `OpenFolderSyncProvider` writes for the same projects as `sync.json` (marker, Markdown documents with front matter and hashes, chapters, storyboard, headers), the projects normalized through Markdown and read back, 14 edits made outside ProseCraft (bodies, titles, removed hashes, new and duplicate files, bad front matter, chapters, storyboard, header, missing files) with the projects or error each gives, the six folders it refuses, `normalizeDocument` on seven documents, and a whole sync after an edit outside and one inside (`Date.now` and `Math.random` pinned) |
| `citations-format.json` | `formatCitation`, `formatBibliography` and `computeDisambiguation` in all eight styles, in English and German, on 22 sources (book, journal article, chapter, conference paper, web page, report, thesis; particles, suffixes, corporate and missing authors, editors, 3, 7 and 23 authors, editions, DOIs, links, no date, umlauts), each cited eight ways, and four clusters |
| `citations-import.json` | `importCitations` on ProseCraft's own import test inputs (a Zotero BibTeX export, RIS and CSL-JSON, and the malformed ones) plus 30 edge cases (string macros, `#` concatenation, unbalanced braces, duplicate and missing keys, LaTeX accents, RIS without `ER`, JSON errors, extension and content detection) and 3 inputs found by the differential comparison (`docs/citation-compare.md`); `parseBibtex`, `parseRis` and `decodeLatex` on their own; and the `library.ts` helpers (`generateCitationKey`, `mergeImportedCitations`, `countSourceCitations`, `matchesLibraryQuery`, author lists, years, type labels) |
| `citation-labels.json` | `renderCitationLabels` on a scene with eight markers (located, prefixed and suffixed sources, a cite key, sources missing from the library, an unreadable `data-cites`, a repeated source) in all eight styles, in English and German, and with an empty library |
| `export-citations.json` | `exportText`, `exportMarkdown`, `exportHtml` and `exportEpub` of the export book with a citation library, in six settings (APA, Chicago notes, IEEE with per-chapter footnotes, MLA in German, Harvard without bibliography, DIN 1505 with back matter): citations in scenes and in footnotes, a cite key, unknown sources and the bibliography |
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

The citation importers are also compared with ProseCraft on a large generated corpus that is not
kept here; see [`docs/citation-compare.md`](../../../../docs/citation-compare.md).
