//! marked's `src/helpers.ts`, and the JavaScript string operations the port needs.

use super::rules as r;
use crate::html::is_js_whitespace;
use std::borrow::Cow;

pub(super) fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

pub(super) fn js_trim_end(s: &str) -> &str {
    s.trim_end_matches(is_js_whitespace)
}

/// `s.substring(n)` for a byte count `n`, clamped and moved forward to a character boundary.
pub(super) fn skip(s: &str, n: usize) -> &str {
    let mut n = n.min(s.len());
    while !s.is_char_boundary(n) {
        n += 1;
    }
    &s[n..]
}

/// `s.slice(units)`: from a position counted in UTF-16 units. A cut through an astral
/// character leaves its low surrogate standing alone, which becomes U+FFFD (as it does once
/// JavaScript's string is written out as UTF-8); like the lone surrogate, it is not a space.
pub(super) fn js_slice_units(s: &str, units: usize) -> Cow<'_, str> {
    let mut seen = 0;
    for (i, c) in s.char_indices() {
        if seen >= units {
            return Cow::Borrowed(&s[i..]);
        }
        seen += c.len_utf16();
        if seen > units {
            return Cow::Owned(format!("\u{fffd}{}", &s[i + c.len_utf8()..]));
        }
    }
    Cow::Borrowed("")
}

/// `rtrim(str, c)`: `c` removed from the end, as often as it repeats.
pub(super) fn rtrim(s: &str, c: char) -> &str {
    s.trim_end_matches(c)
}

/// `normalizeLabel`.
pub(super) fn normalize_label(label: &str) -> String {
    js_trim(label).to_lowercase().to_uppercase().to_lowercase()
}

/// `trimTrailingBlankLines`: trailing blank lines dropped when there are two or more.
pub(super) fn trim_trailing_blank_lines(s: &str) -> String {
    let lines: Vec<&str> = s.split('\n').collect();
    let mut end = lines.len() as i64 - 1;
    while end >= 0 && r::other_blank_line().test(lines[end as usize]) {
        end -= 1;
    }
    if lines.len() as i64 - end <= 2 {
        return s.to_owned();
    }
    lines[..(end + 1) as usize].join("\n")
}

/// `findClosingBracket`: where the bracket that closes the text's first level is, -1 when
/// there is none, -2 when one is left open.
pub(super) fn find_closing_bracket(s: &str, open: u8, close: u8) -> i64 {
    let bytes = s.as_bytes();
    if !bytes.contains(&open) && !bytes.contains(&close) {
        return -1;
    }
    let mut level = 0i64;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 1;
        } else if bytes[i] == open {
            level += 1;
        } else if bytes[i] == close {
            level -= 1;
            if level < 0 {
                return i as i64;
            }
        }
        i += 1;
    }
    if level > 0 { -2 } else { -1 }
}

/// `expandTabs`: tabs as spaces to the next multiple of four, counting from `indent`.
pub(super) fn expand_tabs(line: &str, indent: usize) -> String {
    let mut column = indent;
    let mut out = String::new();
    for c in line.chars() {
        if c == '\t' {
            let added = 4 - column % 4;
            out.push_str(&" ".repeat(added));
            column += added;
        } else {
            out.push(c);
            column += 1;
        }
    }
    out
}

/// `splitCells`: a table row's cells, padded or cut to `count` when it is given.
pub(super) fn split_cells(row: &str, count: Option<usize>) -> Vec<String> {
    let bytes = row.as_bytes();
    let row = r::other_find_pipe().replace_with(row, true, |m| {
        let mut escaped = false;
        let mut at = m.start();
        while at > 0 && bytes[at - 1] == b'\\' {
            escaped = !escaped;
            at -= 1;
        }
        if escaped { "|".into() } else { " |".into() }
    });
    let mut cells: Vec<&str> = row.split(" |").collect();
    if cells.first().is_some_and(|c| js_trim(c).is_empty()) {
        cells.remove(0);
    }
    if cells.last().is_some_and(|c| js_trim(c).is_empty()) {
        cells.pop();
    }
    let mut cells: Vec<String> = cells.into_iter().map(str::to_owned).collect();
    if let Some(count) = count.filter(|&c| c > 0) {
        cells.resize(count, String::new());
    }
    cells
        .iter()
        .map(|cell| r::other_slash_pipe().replace(js_trim(cell), true, "|"))
        .collect()
}

/// `decodeNumericCharacterReferences`: `&#…;` references decoded, and a bare `&` as `&amp;`.
pub(super) fn decode_numeric_character_references(text: &str) -> String {
    if !r::other_numeric_character_reference().test(text) {
        return text.to_owned();
    }
    r::other_numeric_character_reference_or_ampersand().replace_with(text, true, |m| {
        let code = match (m.get(1), m.get(2)) {
            (None, None) => return "&amp;".into(),
            (Some(dec), _) => dec.parse().unwrap_or(0),
            (None, Some(hex)) => u32::from_str_radix(hex, 16).unwrap_or(0),
        };
        if code == 38 {
            return "&amp;".into();
        }
        match char::from_u32(code).filter(|_| code != 0) {
            Some(c) => c.to_string(),
            None => "\u{fffd}".into(),
        }
    })
}

/// `escapeHtmlEntities`: with `encode`, every `& < > " '`; without, an `&` that already
/// starts an entity is kept.
pub(super) fn escape_html_entities(html: &str, encode: bool) -> String {
    let replace = |m: &crate::js::regexp::Caps| {
        match m.whole() {
            "&" => "&amp;",
            "<" => "&lt;",
            ">" => "&gt;",
            "\"" => "&quot;",
            "'" => "&#39;",
            other => other,
        }
        .to_owned()
    };
    if encode {
        if r::other_escape_test().test(html) {
            return r::other_escape_replace().replace_with(html, true, replace);
        }
    } else if r::other_escape_test_no_encode().test(html) {
        return r::other_escape_replace_no_encode().replace_with(html, true, replace);
    }
    html.to_owned()
}

/// `cleanUrl`: `encodeURI`, keeping `%` signs as they were.
pub(super) fn clean_url(href: &str) -> String {
    const KEEP: &[u8] = b";,/?:@&=+$-_.!~*'()#";
    let mut out = String::with_capacity(href.len());
    for c in href.chars() {
        if c.is_ascii_alphanumeric() || (c.is_ascii() && KEEP.contains(&(c as u8))) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out.replace("%25", "%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_behave_as_marked_s() {
        assert_eq!(clean_url("a b/ü?x=%41"), "a%20b/%C3%BC?x=%41");
        assert_eq!(
            escape_html_entities("a & b &amp; <c>", false),
            "a &amp; b &amp; &lt;c&gt;"
        );
        assert_eq!(escape_html_entities("&amp;'", true), "&amp;amp;&#39;");
        assert_eq!(
            decode_numeric_character_references("&#169; & &#0; &#x26;"),
            "© &amp; \u{fffd} &amp;"
        );
        assert_eq!(split_cells("| a | b \\| c |", None), ["a", "b | c"]);
        assert_eq!(split_cells("a", Some(3)), ["a", "", ""]);
        assert_eq!(find_closing_bracket("a(b)c)d", b'(', b')'), 5);
        assert_eq!(find_closing_bracket("a(b", b'(', b')'), -2);
        assert_eq!(expand_tabs("\tx", 2), "  x");
        assert_eq!(trim_trailing_blank_lines("a\n\n\n"), "a");
        assert_eq!(trim_trailing_blank_lines("a\n"), "a\n");
        assert_eq!(js_slice_units("*😀\tx", 2), "\u{fffd}\tx");
        assert_eq!(js_slice_units("*😀\tx", 3), "\tx");
        assert_eq!(js_slice_units("ab", 5), "");
    }
}
