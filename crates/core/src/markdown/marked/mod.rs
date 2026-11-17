//! A port of marked 18.1.1 (MIT, © MarkedJS and Christopher Jeffrey), the Markdown parser
//! ProseCraft uses, with the default options ProseCraft keeps (GFM, no `breaks`, not
//! pedantic) and the extensions and renderers it adds in `configureMarked`: footnote
//! definitions and references, Pandoc citations, display and inline math, and diagram and
//! calculation code blocks. Only those paths are ported; marked's options, hooks and
//! extension API are not.
//!
//! The regular expressions are marked's own, run by `regress`, an ECMAScript regular
//! expression engine (`crate::js::regexp`), so they match as they do in JavaScript.
//!
//! Where the port deliberately differs: marked throws on input it cannot advance through
//! ("Infinite loop on byte"), and the port stops and keeps what it has read; and nesting
//! deeper than 200 levels, where marked eventually throws a `RangeError`, reads as plain text.
//! On pathological input (thousands of unclosed delimiters in one paragraph, brackets nested
//! thousands deep) both are slower than linear; the port by a few times more.
//!
//! marked's copyright and permission notice is in `licenses/marked.txt`.

mod helpers;
mod lexer;
mod render;
mod rules;

pub(crate) use lexer::{Kind, Token};

/// `marked.lexer`: the block tokens, their inline tokens filled in.
pub(crate) fn lex(markdown: &str) -> Vec<Token> {
    lexer::Lexer::new().lex(markdown)
}

/// `marked.parser`.
pub(crate) fn parse_tokens(tokens: &[Token]) -> String {
    render::parse(tokens)
}

/// `marked.parse`.
pub(crate) fn parse(markdown: &str) -> String {
    render::parse(&lex(markdown))
}

/// `marked.parseInline`.
pub(crate) fn parse_inline(markdown: &str) -> String {
    render::parse_inline(&lexer::Lexer::new().lex_inline(markdown), false)
}

#[cfg(test)]
mod tests {
    use super::{Token, lex};
    use crate::js::{self, JsObject, JsValue};
    use crate::markdown::{markdown_to_document, markdown_to_html};

    /// A token as the fixture generator writes it: its type and raw text, and its children.
    fn tree(tokens: &[Token]) -> JsValue {
        JsValue::Array(
            tokens
                .iter()
                .map(|t| {
                    let mut o = JsObject::new();
                    o.insert("type", JsValue::str(t.kind.name()));
                    o.insert("raw", JsValue::str(&t.raw));
                    if t.has_tokens {
                        o.insert("tokens", tree(&t.tokens));
                    }
                    if t.kind == super::Kind::List {
                        o.insert("items", tree(&t.items));
                    }
                    JsValue::Object(o)
                })
                .collect(),
        )
    }

    fn text(v: &JsValue, key: &str) -> String {
        v.get(key)
            .and_then(JsValue::as_js_string)
            .map(|s| s.to_string_lossy())
            .unwrap_or_default()
    }

    /// ProseCraft's `markdownToHtml` and `markdownToDocument`, and marked's tokens, on the
    /// CommonMark 0.31.2 examples and ProseCraft's own syntax (see `tests/fixtures/README.md`).
    #[test]
    fn markdown_reads_as_prosecraft_reads_it() {
        check("marked.json", 600);
    }

    /// The same on seeded random pieces and lines (delimiters, escapes, entities, indents and
    /// tabs, lists, quotes, fences, tables, non-ASCII and astral characters, ProseCraft's syntax)
    /// and on inputs that once differed.
    #[test]
    fn random_markdown_reads_as_prosecraft_reads_it() {
        check("marked-random.json", 800);
    }

    /// Nesting past the limit (marked throws a `RangeError` some thousands deep) reads as plain
    /// text instead of overflowing the stack, even on a 1 MiB stack like the web build's.
    #[test]
    fn deep_nesting_does_not_overflow_the_stack() {
        let inputs = [
            ">".repeat(20_000) + " x",
            (0..400)
                .map(|i| format!("{}- x\n", "  ".repeat(i)))
                .collect(),
            "[".repeat(400) + "x" + &"](y)".repeat(400),
            "*a ".repeat(1_000) + "b" + &" a*".repeat(1_000),
            "**".repeat(1_000) + "x" + &"**".repeat(1_000),
            "~~".repeat(1_000) + "x" + &"~~".repeat(1_000),
            "[".repeat(400) + "x" + &"][r]".repeat(400) + "\n\n[r]: /u",
            "> - ".repeat(2_000) + "x",
        ];
        let handle = std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(move || {
                inputs
                    .iter()
                    .map(|input| markdown_to_document(input).html.len())
                    .collect::<Vec<_>>()
            });
        let lengths = handle.ok().and_then(|h| h.join().ok()).unwrap_or_default();
        assert_eq!(lengths.len(), 8);
        assert!(lengths.iter().all(|&l| l > 0));
    }

    fn check(name: &str, at_least: usize) {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let fixture = std::fs::read_to_string(&path).unwrap_or_default();
        let fixture = js::parse(&fixture).unwrap_or(JsValue::Null);
        let cases = fixture
            .get("cases")
            .and_then(JsValue::as_array)
            .unwrap_or_default();
        assert!(cases.len() > at_least, "fixture missing: {path}");
        let mut failures = Vec::new();
        for (i, case) in cases.iter().enumerate() {
            let markdown = text(case, "markdown");
            let mut problems = Vec::new();
            let html = markdown_to_html(&markdown);
            if html != text(case, "html") {
                problems.push(format!(
                    "html\n  want {:?}\n  have {html:?}",
                    text(case, "html")
                ));
            }
            let want_tokens = case.get("tokens").map(js::stringify).unwrap_or_default();
            let have_tokens = js::stringify(&tree(&lex(&markdown)));
            if have_tokens != want_tokens {
                problems.push(format!(
                    "tokens\n  want {want_tokens}\n  have {have_tokens}"
                ));
            }
            let document = markdown_to_document(&markdown);
            // `marked-random.json` keeps only the footnotes: its document HTML is the HTML.
            let want_document = case.get("document");
            let want_html = want_document.map_or_else(|| text(case, "html"), |d| text(d, "html"));
            if document.html != want_html {
                problems.push(format!(
                    "document html\n  want {want_html:?}\n  have {:?}",
                    document.html
                ));
            }
            let want_notes: Vec<(String, String)> = want_document
                .unwrap_or(case)
                .get("footnotes")
                .and_then(JsValue::as_array)
                .unwrap_or_default()
                .iter()
                .map(|n| (text(n, "id"), text(n, "html")))
                .collect();
            let have_notes: Vec<(String, String)> = document
                .footnotes
                .iter()
                .map(|n| (n.id.clone(), n.html.clone()))
                .collect();
            if have_notes != want_notes {
                problems.push(format!(
                    "footnotes\n  want {want_notes:?}\n  have {have_notes:?}"
                ));
            }
            if !problems.is_empty() {
                failures.push(format!("case {i} {markdown:?}\n{}", problems.join("\n")));
            }
        }
        assert!(
            failures.is_empty(),
            "{name}: {} of {} cases differ:\n\n{}",
            failures.len(),
            cases.len(),
            failures
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n\n")
        );
    }
}
