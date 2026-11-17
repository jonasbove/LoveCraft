//! The EPUB stylesheet: ProseCraft's `stylesheet` (`src/lib/publish/epub.ts`, upstream
//! `7dae544`), generated from its template. `{{…}}` markers are filled from the style.

pub(super) const CSS: &str = r#"body {
  margin: 0;
  color: #1d211d;
  font-family: {{BODY_FONT}};
  font-size: 1em;
  line-height: {{LINE_HEIGHT}};
}
h1, h2, h3, p, div, blockquote, ul, ol, li { margin-top: 0; }
.cover-page-body { margin: 0; padding: 0; }
.cover { margin: 0; padding: 0; text-align: center; }
.cover img { max-width: 100%; max-height: 100vh; }
.titlepage, .dedication-page { text-align: center; }
.title-page { margin-top: 28%; }
.book-title { font-size: 2.2em; font-weight: 500; line-height: 1.2; margin-bottom: 0.4em; }
.book-subtitle { font-size: 1.1em; font-style: italic; color: #666; margin-bottom: 2.5em; }
.book-author { font-size: 1em; letter-spacing: 0.08em; text-transform: uppercase; color: #666; }
.book-copyright { margin-top: 4em; font-size: 0.72em; line-height: 1.6; color: #666; }
.dedication { margin-top: 28%; font-style: italic; }
.copyright-page { margin-top: 28%; text-align: center; }
.copyright-page p { margin: 0; color: #666; font-size: 0.82em; line-height: 1.65; }
.matter-title {
  margin: 0 0 1.2em;
  font-size: 1.5em;
  font-weight: 600;
  line-height: 1.25;
  text-align: center;
  text-indent: 0;
}
.matter-page { margin-top: 10%; }
.matter-page p {
  margin: 0 0 {{SPACING}}em;
  text-align: {{JUSTIFY}};
  text-indent: 0;
}
.bibliography p.bibliography-entry {
  margin: 0 0 0.7em;
  padding-left: 2em;
  text-indent: -2em;
  text-align: left;
}
.contents-page { margin-top: 8%; }
.contents-page .toc-list { margin: 0; padding: 0; list-style: none; }
.contents-page .toc-list li { margin: 0 0 0.7em; }
.contents-page a { color: inherit; text-decoration: none; }
.chapter-break { break-before: page; page-break-before: always; }
.chapter-title {
  margin: {{CHAPTER_BEFORE}}em 0 {{CHAPTER_AFTER}}em;
  font-size: {{HEADING_EM}}em;
  font-weight: 600;
  letter-spacing: {{HEADING_TRACKING}};
  line-height: 1.25;
  text-align: {{CHAPTER_ALIGN}};
  text-transform: {{HEADING_CASE}};
}
.scene p {
  margin: 0 0 {{SPACING}}em;
  text-indent: {{INDENT}}em;
  text-align: {{JUSTIFY}};
}
.scene p:first-of-type { text-indent: 0; }
.scene h1, .scene h2, .scene h3 {
  margin: 1.2em 0 0.55em;
  font-weight: 600;
  line-height: 1.3;
  text-indent: 0;
}
.scene h1 { font-size: 1.45em; }
.scene h2 { font-size: 1.28em; }
.scene h3 { font-size: 1.12em; }
.scene blockquote {
  margin: 1.1em 1.4em;
  padding-left: 1em;
  border-left: 2px solid #d9dbd4;
  color: #666;
  font-style: italic;
  text-indent: 0;
}
.scene ul, .scene ol { margin: 0 0 {{SPACING}}em; padding-left: 1.5em; }
.scene li { text-indent: 0; }
.scene a { color: inherit; text-decoration-color: #b9bdb5; text-underline-offset: 2px; }
.scene img { display: block; max-width: 100%; max-height: min(4in, 60vh); height: auto; margin: 0.75em auto; object-fit: contain; }
.scene .math-inline { display: inline; text-indent: 0; white-space: nowrap; }
.scene .math-inline img { display: inline-block; max-width: none; max-height: 1.6em; width: auto; height: auto; margin: 0 0.08em; vertical-align: middle; }
.scene .math-block, .scene .mermaid-diagram, .scene .calc-worksheet { display: block; max-width: 100%; margin: 0.85em auto; text-align: center; text-indent: 0; }
.scene .math-block img, .scene .mermaid-diagram img, .scene .calc-worksheet img { display: block; max-width: 100%; max-height: 60vh; width: auto; height: auto; margin: 0 auto; object-fit: contain; }
.scene .rich-render-error { font-family: monospace; font-size: 0.85em; white-space: pre-wrap; }
.scene .eyebrow {
  margin: 0 0 0.6em;
  color: #777;
  font-family: sans-serif;
  font-size: 0.62em;
  font-weight: 700;
  letter-spacing: 0.13em;
  line-height: 1.5;
  text-indent: 0;
}
.scene-title {
  margin: 1em 0 0.6em;
  font-size: 1em;
  font-style: italic;
  font-weight: 600;
  text-indent: 0;
}
.scene-break {
  margin: {{SCENE_BREAK}}em 0;
  color: #777;
  font-size: 0.9em;
  letter-spacing: 0.45em;
  text-align: center;
  text-indent: 0;
}
.scene-break-blank { height: {{SCENE_BREAK}}em; margin: 0; }
.scene-break-line span {
  display: block;
  width: 3.2em;
  margin: 0 auto;
  border-top: 1px solid #d9dbd4;
}
.footnotes {
  margin-top: 2em;
  padding-top: 0.9em;
  border-top: 1px solid #d9dbd4;
  font-size: 0.88em;
}
.footnotes aside { margin: 0 0 0.55em; text-indent: 0; }
.footnotes aside p { margin: 0; text-indent: 0; }
.footnote-number { font-weight: 600; margin-right: 0.25em; }
.footnote-ref { text-decoration: none; }
.footnote-backref { text-decoration: none; margin-left: 0.3em; }
{{DROP_CAPS}}
"#;

/// The drop-cap rules, in the place of `{{DROP_CAPS}}` when drop caps are on.
pub(super) const DROP_CAPS: &str = r#"
.scene .drop-cap { text-indent: 0; }
.drop-caps .scene .drop-cap::first-letter {
  float: left;
  padding: 0.06em 0.09em 0 0;
  font-size: 4.5em;
  font-weight: 600;
  line-height: 0.82;
}
"#;
