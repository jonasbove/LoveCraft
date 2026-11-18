//! Proofreading (ProseCraft's `src/lib/proofread/harper.ts`): spelling and grammar from Harper,
//! the engine ProseCraft runs as harper.js 3.0.0, linting as harper.js's `LocalLinter` does
//! (plain text, the curated rules, overlapping lints removed). Offsets are characters.

use std::sync::Arc;

use harper_core::linting::{LintGroup, Linter, Suggestion};
use harper_core::parsers::PlainEnglish;
use harper_core::spell::{FstDictionary, MergedDictionary};
use harper_core::{Dialect as HarperDialect, Document, Lrc, remove_overlaps};

/// `HARPER_DIALECTS`: id and label.
pub const DIALECTS: [(&str, &str); 4] = [
    ("american", "American"),
    ("british", "British"),
    ("australian", "Australian"),
    ("canadian", "Canadian"),
];

/// `MAX_SUGGESTIONS`.
const MAX_SUGGESTIONS: usize = 4;

/// `HarperLint`, with character offsets into the checked text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lint {
    pub start: usize,
    pub end: usize,
    pub message: String,
    /// `lint_kind()`: the rule's category key.
    pub kind: String,
    /// `lint_kind_pretty()`.
    pub kind_pretty: String,
    /// The first four replacements ("" removes the text).
    pub suggestions: Vec<String>,
}

fn dialect(id: &str) -> HarperDialect {
    match id {
        "british" => HarperDialect::British,
        "australian" => HarperDialect::Australian,
        "canadian" => HarperDialect::Canadian,
        _ => HarperDialect::American,
    }
}

/// A linter for one dialect (`ensureHarperLinter`). Building it reads Harper's dictionary,
/// which takes a moment; keep it.
pub struct Proofreader {
    dialect: String,
    dictionary: Arc<MergedDictionary>,
    group: LintGroup,
}

impl Proofreader {
    pub fn new(dialect_id: &str) -> Self {
        let mut merged = MergedDictionary::new();
        merged.add_dictionary(FstDictionary::curated());
        let dictionary = Arc::new(merged);
        let mut group =
            LintGroup::new_curated_empty_config(dictionary.clone(), dialect(dialect_id));
        group.config.fill_with_curated();
        Self {
            dialect: dialect_id.to_owned(),
            dictionary,
            group,
        }
    }

    pub fn dialect(&self) -> &str {
        &self.dialect
    }

    /// `lintPlainText`: the text's lints with a span, in order.
    pub fn lint(&mut self, text: &str) -> Vec<Lint> {
        if text.trim().is_empty() {
            return Vec::new();
        }
        let source: Lrc<[char]> = text.chars().collect();
        let document = Document::new_from_chars(source, &PlainEnglish, &self.dictionary);
        let mut lints = self.group.lint(&document);
        remove_overlaps(&mut lints);
        lints
            .into_iter()
            .filter(|l| l.span.end > l.span.start)
            .map(|l| Lint {
                start: l.span.start,
                end: l.span.end,
                kind: l.lint_kind.to_string_key(),
                kind_pretty: l.lint_kind.to_string(),
                suggestions: l
                    .suggestions
                    .iter()
                    .take(MAX_SUGGESTIONS)
                    .map(|s| match s {
                        Suggestion::Remove => String::new(),
                        Suggestion::ReplaceWith(chars) | Suggestion::InsertAfter(chars) => {
                            chars.iter().collect()
                        }
                    })
                    .collect(),
                message: l.message,
            })
            .collect()
    }
}

/// `harperKey`: what Ignore remembers (the rule and the words).
pub fn ignore_key(lint: &Lint, text: &[char]) -> String {
    let words: String = text
        .get(lint.start..lint.end.min(text.len()))
        .unwrap_or_default()
        .iter()
        .collect();
    format!("{}:{words}", lint.kind)
}

/// A lint's editor characters (`rangeFromSpan`): from its first checked character to after its
/// last; `None` when it covers no editor text.
pub fn editor_range(lint: &Lint, at: &[Option<usize>]) -> Option<std::ops::Range<usize>> {
    let inside = at.get(lint.start..lint.end.min(at.len()))?;
    let first = inside.iter().flatten().next()?;
    let last = inside.iter().flatten().last()?;
    Some(*first..last + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::richtext::RichDoc;

    #[test]
    fn links_code_and_atoms_are_left_out_and_lints_map_back() {
        let doc = RichDoc::from_html(
            r#"<p>Teh <a href="https://x.org">linkk</a> cat <code>codee</code> sat.</p><p>Thier dog.</p>"#,
        )
        .expect("doc");
        let (text, at) = doc.proofread_map();
        let text: String = text.iter().collect();
        assert_eq!(text, "Teh  cat  sat.Thier dog.");
        let mut linter = Proofreader::new("american");
        let lints = linter.lint(&text);
        let words: Vec<String> = lints
            .iter()
            .map(|l| {
                let range = editor_range(l, &at).expect("on the page");
                doc.text
                    .chars()
                    .skip(range.start)
                    .take(range.len())
                    .collect()
            })
            .collect();
        // The link and the code aren't checked, but the two spaces left round each are (as in
        // ProseCraft, whose text nodes join the same way), and that lint's range spans them.
        assert_eq!(words, ["Teh", " linkk ", " codee ", "Thier"]);
        assert!(
            !lints
                .iter()
                .any(|l| l.message.contains("linkk") || l.message.contains("codee"))
        );
        assert_eq!(
            ignore_key(&lints[0], &text.chars().collect::<Vec<_>>()),
            format!("{}:Teh", lints[0].kind)
        );
    }
}
