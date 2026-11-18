//! Wiki mentions (ProseCraft's `src/lib/wiki-mentions.ts`): the titles of Wiki articles found
//! in a scene's text, case-insensitively and at word boundaries, the longest title first.

use std::ops::Range;

use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

use crate::richtext::{ATOM, HARD_BREAK, RichDoc};

/// `[\p{L}\p{N}_]`.
fn word(c: char) -> bool {
    c == '_'
        || matches!(
            c.general_category_group(),
            GeneralCategoryGroup::Letter | GeneralCategoryGroup::Number
        )
}

/// The `iu` flags' comparison: each character simply case-folded.
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(single), None) => single,
        _ => c,
    }
}

/// `createWikiMentionMatcher`: titles (trimmed, the first article of each lowercased title),
/// tried longest first.
pub struct Matcher {
    /// Each title, lowercased, and the index of its article.
    titles: Vec<(Vec<char>, usize)>,
}

impl Matcher {
    pub fn new<'a>(titles: impl IntoIterator<Item = &'a str>) -> Self {
        let mut seen = std::collections::HashSet::new();
        let mut list = Vec::new();
        for (index, title) in titles.into_iter().enumerate() {
            let key = title.trim().to_lowercase();
            if !key.is_empty() && seen.insert(key.clone()) {
                list.push((key, index));
            }
        }
        // `sort((a, b) => b.length - a.length)`: UTF-16 lengths, a stable sort.
        list.sort_by_key(|(key, _)| std::cmp::Reverse(key.encode_utf16().count()));
        Self {
            titles: list
                .into_iter()
                .map(|(key, index)| (key.chars().collect(), index))
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.titles.is_empty()
    }

    /// The title that starts at `at` and is followed by a boundary, as the alternation tries
    /// them.
    fn title_at(&self, text: &[char], at: usize) -> Option<(usize, usize)> {
        self.titles.iter().find_map(|(key, index)| {
            let end = at + key.len();
            let fits = end <= text.len()
                && text[at..end]
                    .iter()
                    .zip(key)
                    .all(|(a, b)| fold(*a) == fold(*b))
                && text.get(end).is_none_or(|c| !word(*c));
            fits.then_some((end, *index))
        })
    }

    /// The mentions in `text`: each one's characters and its article's index.
    pub fn find(&self, text: &[char]) -> Vec<(Range<usize>, usize)> {
        let mut out = Vec::new();
        if self.titles.is_empty() {
            return out;
        }
        // `(^|[^\p{L}\p{N}_])(titles)`, searched from where the last match ended.
        let mut from = 0;
        while from <= text.len() {
            let found = (from..=text.len()).find_map(|at| {
                if at == 0
                    && let Some(hit) = self.title_at(text, 0)
                {
                    return Some((0, hit));
                }
                let c = *text.get(at)?;
                if word(c) {
                    return None;
                }
                self.title_at(text, at + 1).map(|hit| (at + 1, hit))
            });
            let Some((start, (end, index))) = found else {
                break;
            };
            out.push((start..end, index));
            from = end;
        }
        out
    }
}

impl RichDoc {
    /// The text as ProseCraft's editor has it in text nodes it looks for mentions in: runs of
    /// characters with the same inline elements, without links, code and atoms (equations,
    /// diagrams, calculations and the editor's other widgets). Each run's first editor
    /// character and its characters.
    pub fn mention_runs(&self) -> Vec<(usize, Vec<char>)> {
        let mut runs: Vec<(usize, Vec<char>)> = Vec::new();
        let mut open = false;
        let mut last_tags = None;
        for (i, c) in self.text.chars().enumerate() {
            let tags = self.marks.get(i);
            let skipped = matches!(c, '\n' | ATOM | HARD_BREAK)
                || tags.is_some_and(|t| t.iter().any(|t| t.name == "a" || t.name == "code"));
            if skipped {
                open = false;
                continue;
            }
            if !open || last_tags != tags {
                runs.push((i, Vec::new()));
                open = true;
                last_tags = tags;
            }
            if let Some((_, chars)) = runs.last_mut() {
                chars.push(c);
            }
        }
        runs
    }

    /// The mentions in the editor's text: each one's editor characters and its article's
    /// index.
    pub fn mentions(&self, matcher: &Matcher) -> Vec<(Range<usize>, usize)> {
        if matcher.is_empty() {
            return Vec::new();
        }
        self.mention_runs()
            .into_iter()
            .flat_map(|(start, chars)| {
                matcher
                    .find(&chars)
                    .into_iter()
                    .map(move |(range, index)| (range.start + start..range.end + start, index))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ARTICLES: [&str; 3] = ["Mira Vale", "Mira", "The Clockmaker's Key"];

    fn found(titles: &[&str], text: &str) -> Vec<(String, String)> {
        let chars: Vec<char> = text.chars().collect();
        Matcher::new(titles.iter().copied())
            .find(&chars)
            .into_iter()
            .map(|(r, i)| (chars[r].iter().collect(), titles[i].to_owned()))
            .collect()
    }

    #[test]
    fn matches_titles_case_insensitively_at_word_boundaries() {
        assert_eq!(
            found(&ARTICLES, "MIRA VALE met Miranda near Mira."),
            [
                ("MIRA VALE".into(), "Mira Vale".into()),
                ("Mira".into(), "Mira".into())
            ]
        );
    }

    #[test]
    fn prefers_the_longest_matching_title_and_supports_punctuation() {
        let titles: Vec<String> =
            found(&ARTICLES, "She found the clockmaker's key; Mira followed.")
                .into_iter()
                .map(|(_, t)| t)
                .collect();
        assert_eq!(titles, ["The Clockmaker's Key", "Mira"]);
    }

    #[test]
    fn does_not_match_empty_article_titles() {
        assert!(found(&[" "], "anything").is_empty());
    }

    #[test]
    fn a_longer_title_cut_short_falls_back_to_a_shorter_one() {
        // "Mira Valerie": "Mira Vale" has no boundary after it, so the alternation tries "Mira".
        assert_eq!(
            found(&ARTICLES, "Mira Valerie"),
            [("Mira".into(), "Mira".into())]
        );
        assert_eq!(
            found(&ARTICLES, "Mira,Mira"),
            [
                ("Mira".into(), "Mira".into()),
                ("Mira".into(), "Mira".into())
            ]
        );
        assert!(found(&ARTICLES, "_Mira").is_empty());
        assert_eq!(found(&["Åsa"], "åSA."), [("åSA".into(), "Åsa".into())]);
    }

    #[test]
    fn links_code_and_formatting_split_the_text_as_text_nodes_do() {
        let doc = RichDoc::from_html(
            "<p>Mira <a href=\"https://x\">Mira</a> <code>Mira</code> <strong>Mira</strong> Vale</p>",
        )
        .expect("doc");
        let matcher = Matcher::new(ARTICLES);
        let chars: Vec<char> = doc.text.chars().collect();
        let hits: Vec<(String, usize)> = doc
            .mentions(&matcher)
            .into_iter()
            .map(|(r, i)| (chars[r].iter().collect(), i))
            .collect();
        // The bold "Mira" is its own text node, so "Mira Vale" isn't found across it.
        assert_eq!(hits, [("Mira".into(), 1), ("Mira".into(), 1)]);
    }
}
