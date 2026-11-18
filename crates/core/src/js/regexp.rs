//! JavaScript's `RegExp` calls (`exec`, `test`, `search`, `replace`, `matchAll`) over
//! `regress`, an ECMAScript regular expression engine, so ported patterns match as they do in
//! ProseCraft. Positions are bytes in UTF-8 text, not UTF-16 units.

/// A compiled rule. A source `regress` refuses never matches; a test compiles every rule, so
/// that only guards against a panic.
pub(crate) struct Re(Option<regress::Regex>, bool);

/// With `i` and without `u`, JavaScript compares characters by their upper case but never lets
/// a character past U+007F match one below it. "ſ" and "ı" are the only characters whose upper
/// case is ASCII ("S", "I"); `regress` lets them match `s` and `i`, JavaScript doesn't. Such a
/// rule searches a copy of the text with them swapped for letters of the same UTF-8 length
/// whose upper case isn't ASCII ("ŀ", "ľ"), so positions and groups still point into the text.
fn without_ascii_folds(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains(['\u{17f}', '\u{131}']) {
        std::borrow::Cow::Owned(
            text.replace('\u{17f}', "\u{140}")
                .replace('\u{131}', "\u{13e}"),
        )
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// A match and its groups, positions in the text it was found in.
pub(crate) struct Caps<'t> {
    text: &'t str,
    found: regress::Match,
}

impl<'t> Caps<'t> {
    /// Group `i` (0 is the whole match), `None` when it did not take part.
    pub fn get(&self, i: usize) -> Option<&'t str> {
        self.found.group(i).map(|r| &self.text[r])
    }

    /// Group `i`, or "" when it did not take part (JavaScript's `cap[i] || ''`).
    pub fn at(&self, i: usize) -> &'t str {
        self.get(i).unwrap_or_default()
    }

    pub fn whole(&self) -> &'t str {
        &self.text[self.found.range()]
    }

    pub fn start(&self) -> usize {
        self.found.start()
    }

    pub fn end(&self) -> usize {
        self.found.end()
    }
}

/// `[\s\S]*?` and `[\s\S]+?` as `(?:.|[\n\r\u2028\u2029])*?` and `+?`: the same characters (`.`
/// is anything but those four line terminators), but `regress` walks a lazy loop over a class
/// that matches everything to the end of the text on every match, which made a block of code
/// cost as much as the rest of the document after it. A `[\s\S]` after an odd number of
/// backslashes is not a class and is left alone.
fn lazy_any_as_alternation(source: &str) -> String {
    const ANY: &str = "[\\s\\S]";
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(at) = rest.find(ANY) {
        let before = &rest[..at];
        let backslashes = before.chars().rev().take_while(|&c| c == '\\').count();
        let after = &rest[at + ANY.len()..];
        out.push_str(before);
        let lazy = ["*?", "+?"].iter().find(|q| after.starts_with(**q));
        match lazy {
            Some(q) if backslashes % 2 == 0 => {
                out.push_str("(?:.|[\\n\\r\\u2028\\u2029])");
                out.push_str(q);
                rest = &after[q.len()..];
            }
            _ => {
                out.push_str(ANY);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

impl Re {
    pub fn new(source: &str, flags: &str) -> Self {
        let folds = flags.contains('i') && !flags.contains('u');
        Self(
            regress::Regex::with_flags(&lazy_any_as_alternation(source), flags).ok(),
            folds,
        )
    }

    #[cfg(test)]
    pub fn compiled(&self) -> bool {
        self.0.is_some()
    }

    /// `exec` on a non-global regex: the first match.
    pub fn exec<'t>(&self, text: &'t str) -> Option<Caps<'t>> {
        self.exec_from(text, 0)
    }

    /// `exec` with `lastIndex` at `start` (a byte position on a character boundary).
    pub fn exec_from<'t>(&self, text: &'t str, start: usize) -> Option<Caps<'t>> {
        let re = self.0.as_ref()?;
        let found = if self.1 {
            re.find_from(&without_ascii_folds(text), start).next()?
        } else {
            re.find_from(text, start).next()?
        };
        Some(Caps { text, found })
    }

    pub fn test(&self, text: &str) -> bool {
        self.exec(text).is_some()
    }

    /// `search`: where the first match starts.
    pub fn search(&self, text: &str) -> Option<usize> {
        self.exec(text).map(|c| c.start())
    }

    /// Every match, as `matchAll` or a global `exec` loop finds them, one at a time: after an
    /// empty match the search moves on one character, in `regress` as in JavaScript.
    pub fn each<'r, 't>(&'r self, text: &'t str) -> Box<dyn Iterator<Item = Caps<'t>> + 'r>
    where
        't: 'r,
    {
        if self.1
            && let std::borrow::Cow::Owned(masked) = without_ascii_folds(text)
        {
            let found: Vec<regress::Match> = self
                .0
                .iter()
                .flat_map(|re| re.find_iter(&masked).collect::<Vec<_>>())
                .collect();
            return Box::new(found.into_iter().map(move |found| Caps { text, found }));
        }
        Box::new(
            self.0
                .iter()
                .flat_map(move |re| re.find_iter(text))
                .map(move |found| Caps { text, found }),
        )
    }

    /// Every match, collected.
    pub fn all<'t>(&self, text: &'t str) -> Vec<Caps<'t>> {
        self.each(text).collect()
    }

    /// `replace` with a function: every match when `global`, else the first.
    pub fn replace_with(
        &self,
        text: &str,
        global: bool,
        mut f: impl FnMut(&Caps) -> String,
    ) -> String {
        let matches = if global {
            self.all(text)
        } else {
            self.exec(text).into_iter().collect()
        };
        if matches.is_empty() {
            return text.to_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        for caps in &matches {
            out.push_str(&text[last..caps.start()]);
            out.push_str(&f(caps));
            last = caps.end();
        }
        out.push_str(&text[last..]);
        out
    }

    /// `replace` with a fixed string, `$1` style references expanded.
    pub fn replace(&self, text: &str, global: bool, replacement: &str) -> String {
        self.replace_with(text, global, |caps| expand(replacement, caps))
    }
}

/// A replacement string's `$n` and `$$`, as marked's replacement strings use them.
fn expand(replacement: &str, caps: &Caps) -> String {
    let mut out = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('$') => {
                chars.next();
                out.push('$');
            }
            Some(d) if d.is_ascii_digit() => {
                chars.next();
                out.push_str(caps.at(d as usize - '0' as usize));
            }
            _ => out.push('$'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{Caps, Re, lazy_any_as_alternation};

    /// As in JavaScript, `i` doesn't let "ſ" or "ı" match "s" or "i"; other letters still fold,
    /// and positions and groups are the text's own.
    #[test]
    fn case_insensitive_rules_fold_as_javascript() {
        assert!(!Re::new("^s$", "i").test("\u{17f}"));
        assert!(!Re::new("^I$", "i").test("\u{131}"));
        assert!(Re::new("^S$", "i").test("s"));
        assert!(Re::new("^\u{e4}$", "i").test("\u{c4}"));
        assert!(Re::new(".", "i").test("\u{17f}"));
        let re = Re::new("(b+)", "i");
        let caps = re.exec("\u{17f}\u{131}aBb").expect("found");
        assert_eq!((caps.start(), caps.at(1)), (5, "Bb"));
        let all: Vec<&str> = re.all("b\u{17f}B").iter().map(Caps::whole).collect();
        assert_eq!(all, ["b", "B"]);
        assert_eq!(
            Re::new("x", "gi").replace("\u{17f}X\u{131}", true, "y"),
            "\u{17f}y\u{131}"
        );
    }

    #[test]
    fn lazy_any_is_rewritten_only_where_it_is_a_lazy_class() {
        assert_eq!(
            lazy_any_as_alternation(r"^a([\s\S]*?)b[\s\S]+?c[\s\S]*d\\[\s\S]e\[\s\S]*?"),
            r"^a((?:.|[\n\r\u2028\u2029])*?)b(?:.|[\n\r\u2028\u2029])+?c[\s\S]*d\\[\s\S]e\[\s\S]*?"
        );
    }
}
