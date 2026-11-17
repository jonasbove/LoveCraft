//! The HTML export's stylesheet: ProseCraft's `manuscriptCss` (`src/lib/publish/html.ts`,
//! upstream `7dae544`) for an export, generated from its template with the preview-only
//! blocks (never part of an export) left out. `{{…}}` markers are filled from the style.

pub(super) const CSS: &str = r#"
    :root {
      --body-font: {{BODY_FONT}};
      --body-size: {{BODY_SIZE}}pt;
      --line-height: {{LINE_HEIGHT}};
      --paragraph-indent: {{INDENT}}em;
      --paragraph-spacing: {{SPACING}}em;
      --chapter-size: {{CHAPTER_SIZE}}pt;
      --chapter-align: {{CHAPTER_ALIGN}};
      --chapter-before: {{CHAPTER_BEFORE}}em;
      --chapter-after: {{CHAPTER_AFTER}}em;
      --scene-break-space: {{SCENE_BREAK}}em;
      --page-width: {{PAGE_WIDTH}}pt;
      --page-height: {{PAGE_HEIGHT}}pt;
      --page-margin-top: {{MARGIN_TOP}}pt;
      --page-margin-right: {{MARGIN_RIGHT}}pt;
      --page-margin-bottom: {{MARGIN_BOTTOM}}pt;
      --page-margin-left: {{MARGIN_LEFT}}pt;
      --ink: #1d211d;
      --muted: #73776f;
      --rule: #d9dbd4;
      --preview-desk: #e8e8e2;
      --page-bg: #ffffff;
      --page-border: #d8d8d1;
      --page-shadow: 0 14px 38px rgba(28, 32, 27, 0.12);
      color-scheme: light;
    }
    * { box-sizing: border-box; }
    html, body { margin: 0; padding: 0; }
    body {
      background: #ffffff;
      color: var(--ink);
      font-family: var(--body-font);
      font-size: var(--body-size);
      line-height: var(--line-height);
      -webkit-font-smoothing: antialiased;
    }
    .book { max-width: {{INCH_WIDTH}}; margin: 0 auto; }
    .cover-page {
      display: flex;
      align-items: center;
      justify-content: center;
      min-height: 80vh;
      padding: 0;
      overflow: hidden;
      page-break-after: always;
    }
    .cover-page img {
      display: block;
      width: auto;
      height: auto;
      max-width: 100%;
      max-height: calc(var(--page-height) - var(--page-margin-top) - var(--page-margin-bottom));
      object-fit: contain;
    }
    .title-page {
      display: flex;
      flex-direction: column;
      justify-content: center;
      min-height: 80vh;
      padding: 1.5in 0.6in;
      text-align: center;
      page-break-after: always;
    }
    .title-page-main { flex: 1; display: flex; flex-direction: column; justify-content: center; }
    .book-title {
      margin: 0;
      font-size: calc(var(--body-size) * 2.7);
      font-weight: 500;
      letter-spacing: -0.02em;
      line-height: 1.16;
    }
    .book-subtitle { margin: 0.65em 0 0; font-size: 1.15em; font-style: italic; color: var(--muted); }
    .book-author { margin: 2.2em 0 0; font-size: 1.05em; letter-spacing: 0.08em; text-transform: uppercase; color: var(--muted); }
    .book-copyright { margin: 0; font-size: 0.72em; line-height: 1.6; color: var(--muted); }
    .dedication {
      display: flex;
      align-items: center;
      justify-content: center;
      min-height: 70vh;
      padding: 1in;
      text-align: center;
      font-style: italic;
      page-break-after: always;
    }
    .dedication p { margin: 0; max-width: 22em; }
    .copyright-page {
      display: flex;
      align-items: center;
      justify-content: center;
      min-height: 70vh;
      padding: 1in;
      text-align: center;
      page-break-after: always;
    }
    .copyright-page p {
      max-width: 26em;
      margin: 0;
      color: var(--muted);
      font-size: 0.82em;
      line-height: 1.65;
    }
    .matter-page {
      min-height: 70vh;
      padding: 0.9in 0.6in;
      page-break-after: always;
    }
    .matter-title {
      margin: 0 0 1.2em;
      color: var(--ink);
      font-size: 1.5em;
      font-weight: 600;
      line-height: 1.25;
      text-align: center;
      text-indent: 0;
    }
    .matter-page p {
      margin: 0 0 var(--paragraph-spacing);
      text-align: {{JUSTIFY}};
      text-indent: 0;
      hyphens: auto;
    }
    .back-matter { break-before: page; page-break-before: always; }
    .bibliography p.bibliography-entry {
      margin: 0 0 0.7em;
      padding-left: 2em;
      text-indent: -2em;
      text-align: left;
    }
    .toc-entry { margin: 0 0 0.7em; text-indent: 0; }
    .toc-entry a {
      color: inherit;
      text-decoration: none;
      text-underline-offset: 2px;
    }
    .toc-entry a:hover { text-decoration: underline; }
    .chapter { padding: 0; }
    .chapter-break { break-before: page; page-break-before: always; }
    .chapter-title {
      margin: var(--chapter-before) 0 var(--chapter-after);
      color: var(--ink);
      font-size: var(--chapter-size);
      font-weight: 600;
      letter-spacing: {{HEADING_TRACKING}};
      line-height: 1.25;
      text-align: var(--chapter-align);
      text-transform: {{HEADING_CASE}};
    }
    .scene p, .scene div {
      margin: 0 0 var(--paragraph-spacing);
      text-align: {{JUSTIFY}};
      text-indent: var(--paragraph-indent);
      hyphens: auto;
    }
    .scene > :first-child { text-indent: 0; }
    .scene > .continuation { text-indent: var(--paragraph-indent); }
    .scene h1, .scene h2, .scene h3 {
      margin: 1.2em 0 0.55em;
      font-weight: 600;
      line-height: 1.3;
      text-indent: 0;
      break-after: avoid;
    }
    .scene h1 { font-size: 1.45em; }
    .scene h2 { font-size: 1.28em; }
    .scene h3 { font-size: 1.12em; }
    .scene blockquote {
      margin: 1.1em 1.4em;
      padding-left: 1em;
      border-left: 2px solid var(--rule);
      color: var(--muted);
      font-style: italic;
      text-indent: 0;
    }
    .scene ul, .scene ol { margin: 0 0 var(--paragraph-spacing); padding-left: 1.5em; }
    .scene li { text-indent: 0; }
    .scene a { color: inherit; text-decoration-color: var(--rule); text-underline-offset: 2px; }
    .scene img {
      display: block;
      max-width: 100%;
      max-height: min(4in, calc(var(--page-height) - var(--page-margin-top) - var(--page-margin-bottom) - 2em));
      height: auto;
      margin: 0.75em auto;
      object-fit: contain;
      text-indent: 0;
    }
    .scene .math-inline {
      display: inline;
      text-indent: 0;
      white-space: nowrap;
    }
    .scene .math-inline svg, .scene .math-inline img {
      display: inline-block;
      max-width: none;
      max-height: 1.6em;
      width: auto;
      height: auto;
      margin: 0 0.08em;
      vertical-align: middle;
      text-indent: 0;
    }
    .scene .math-block, .scene .mermaid-diagram, .scene .calc-worksheet {
      display: block;
      max-width: 100%;
      margin: 0.85em auto;
      text-align: center;
      text-indent: 0;
    }
    .scene .math-block svg, .scene .math-block img,
    .scene .mermaid-diagram svg, .scene .mermaid-diagram img,
    .scene .calc-worksheet svg, .scene .calc-worksheet img {
      display: block;
      max-width: 100%;
      max-height: min(4.5in, calc(var(--page-height) - var(--page-margin-top) - var(--page-margin-bottom) - 2em));
      width: auto;
      height: auto;
      margin: 0 auto;
      object-fit: contain;
    }
    .scene .rich-render-error {
      font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
      font-size: 0.85em;
      white-space: pre-wrap;
    }
    .scene .math-inline:empty::before { content: attr(data-tex); font-family: ui-monospace, monospace; }
    .scene .math-block:empty::before { content: attr(data-tex); display: block; font-family: ui-monospace, monospace; white-space: pre-wrap; }
    .scene .mermaid-diagram:empty::before, .scene .calc-worksheet:empty::before { content: attr(data-source); display: block; font-family: ui-monospace, monospace; white-space: pre-wrap; text-align: left; }
    .scene .eyebrow {
      margin: 0 0 0.6em;
      color: var(--muted);
      font-family: ui-sans-serif, sans-serif;
      font-size: 0.62em;
      font-weight: 700;
      letter-spacing: 0.13em;
      line-height: 1.5;
      text-indent: 0;
    }
    .scene-title {
      margin: 1em 0 0.6em;
      font-size: 1em;
      font-weight: 600;
      font-style: italic;
      letter-spacing: 0.01em;
      text-indent: 0;
    }
    .scene-break {
      margin: var(--scene-break-space) 0;
      color: var(--muted);
      font-size: 0.9em;
      letter-spacing: 0.45em;
      text-align: center;
      text-indent: 0;
    }
    .scene-break-hash, .scene-break-asterisks { }
    .scene-break-blank { height: var(--scene-break-space); margin: 0; }
    .scene-break-line span {
      display: block;
      width: 3.2em;
      margin: 0 auto;
      border-top: 1px solid var(--rule);
    }
    .footnotes {
      margin-top: 2em;
      padding-top: 0.9em;
      border-top: 1px solid var(--rule);
      font-size: 0.88em;
    }
    .footnotes ol { margin: 0; padding-left: 1.4em; }
    .footnotes li { margin: 0 0 0.45em; text-indent: 0; }
    .footnote-ref a { color: inherit; text-decoration: none; }
    .footnote-backref { color: var(--muted); text-decoration: none; margin-left: 0.3em; }
    {{DROP_CAPS}}
    
    @page {
      size: {{INCH_WIDTH}} {{INCH_HEIGHT}};
      margin: {{INCH_TOP}} {{INCH_RIGHT}} {{INCH_BOTTOM}} {{INCH_LEFT}};
    }
    @media print {
      body { background: #ffffff; }
      .book { max-width: none; }
      .chapter-break { break-before: page; }
      .cover-page, .title-page, .dedication, .copyright-page, .matter-page { break-after: page; }
      .back-matter { break-before: page; }
    }
  "#;

/// The drop-cap rules, in the place of `{{DROP_CAPS}}` when drop caps are on.
pub(super) const DROP_CAPS: &str = r#"
    .scene .drop-cap { text-indent: 0; }
    .scene .drop-cap::first-letter {
      float: left;
      padding: 0.06em 0.09em 0 0;
      font-size: 4.5em;
      font-weight: 600;
      line-height: 0.82;
    }
    "#;
