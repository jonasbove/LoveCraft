//! Measuring and splitting text in the PDF standard fonts as jsPDF does (`getCharWidthsArray`,
//! `getStringUnitWidth`, `getTextWidth` and `splitTextToSize` in jsPDF 4), which ProseCraft's PDF
//! export lays its pages out with.

use super::pdf_metrics::{FACES, FaceMetrics, WIN_ANSI_EXTRA};

/// jsPDF's standard font families (a style's `pdfFont`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Times,
    Helvetica,
    Courier,
}

impl Family {
    pub fn from_name(name: &str) -> Self {
        match name {
            "helvetica" => Self::Helvetica,
            "courier" => Self::Courier,
            _ => Self::Times,
        }
    }
}

/// The twelve faces in the order of their metrics (and of the PDF writer's font names).
pub const FACES_IN_ORDER: [Face; 12] = {
    const fn f(family: Family, bold: bool, italic: bool) -> Face {
        Face {
            family,
            bold,
            italic,
        }
    }
    [
        f(Family::Times, false, false),
        f(Family::Times, true, false),
        f(Family::Times, false, true),
        f(Family::Times, true, true),
        f(Family::Helvetica, false, false),
        f(Family::Helvetica, true, false),
        f(Family::Helvetica, false, true),
        f(Family::Helvetica, true, true),
        f(Family::Courier, false, false),
        f(Family::Courier, true, false),
        f(Family::Courier, false, true),
        f(Family::Courier, true, true),
    ]
};

/// A family in one of jsPDF's four styles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Face {
    pub family: Family,
    pub bold: bool,
    pub italic: bool,
}

impl Face {
    pub fn new(family: Family, bold: bool, italic: bool) -> Self {
        Self {
            family,
            bold,
            italic,
        }
    }

    fn index(self) -> usize {
        let family = match self.family {
            Family::Times => 0,
            Family::Helvetica => 4,
            Family::Courier => 8,
        };
        family + usize::from(self.bold) + 2 * usize::from(self.italic)
    }

    fn metrics(self) -> &'static FaceMetrics {
        &FACES[self.index()]
    }

    /// The font's PostScript name (`/BaseFont`).
    pub fn postscript_name(self) -> &'static str {
        self.metrics().name
    }

    /// `getCharWidthsArray`: each UTF-16 code unit's width in ems, kerned against the one before.
    pub fn char_widths(self, text: &str) -> Vec<f64> {
        self.widths(text, true)
    }

    fn widths(self, text: &str, kerning: bool) -> Vec<f64> {
        let metrics = self.metrics();
        let default = f64::from(metrics.default_width) / 1000.0;
        let mut prior = 0u16;
        text.encode_utf16()
            .map(|unit| {
                let width = metrics
                    .widths
                    .binary_search_by_key(&unit, |(c, _)| *c)
                    .ok()
                    .map_or(default, |i| f64::from(metrics.widths[i].1) / 1000.0);
                let kerning = metrics
                    .kerning
                    .binary_search_by_key(&(unit, prior), |(c, p, _)| (*c, *p))
                    .ok()
                    .filter(|_| kerning)
                    .map_or(0.0, |i| f64::from(metrics.kerning[i].2) / 1000.0);
                prior = unit;
                width + kerning
            })
            .collect()
    }

    /// `getStringUnitWidth`: the width in ems.
    pub fn unit_width(self, text: &str) -> f64 {
        self.char_widths(text)
            .into_iter()
            .fold(0.0, |sum, w| sum + w)
    }

    /// The width jsPDF aligns text with (`doKerning: false`), in points at `size`.
    pub fn aligned_width(self, text: &str, size: f64) -> f64 {
        self.widths(text, false)
            .into_iter()
            .fold(0.0, |sum, w| sum + w)
            * size
    }

    /// `getTextWidth`: the width in points at `size`.
    pub fn text_width(self, text: &str, size: f64) -> f64 {
        self.unit_width(text) * size
    }

    /// `splitTextToSize(text, max_width)` at `size`: lines at most `max_width` points wide,
    /// broken at spaces (and at newlines); a word longer than a line is cut into pieces.
    pub fn split_text_to_size(self, text: &str, max_width: f64, size: f64) -> Vec<String> {
        let max = max_width / size;
        let mut output = Vec::new();
        for paragraph in split_lines(text) {
            output.extend(self.split_paragraph(paragraph, max));
        }
        output
    }

    /// `splitParagraphIntoLines` without indents (ProseCraft passes none).
    fn split_paragraph(self, text: &str, max: f64) -> Vec<String> {
        let space = self.char_widths(" ").first().copied().unwrap_or_default();
        let mut lines: Vec<Vec<String>> = vec![Vec::new()];
        let mut line_length = 0.0;
        let mut separator = 0.0;
        for word in text.split(' ') {
            let widths = self.char_widths(word);
            let mut word_length = widths.iter().fold(0.0, |sum, w| sum + w);
            if line_length + separator + word_length > max {
                if word_length > max {
                    let units: Vec<u16> = word.encode_utf16().collect();
                    let mut pieces =
                        split_long_word(&units, &widths, max - (line_length + separator), max);
                    let first = pieces.remove(0);
                    if let Some(line) = lines.last_mut() {
                        line.push(first);
                    }
                    let last = pieces.pop();
                    for piece in pieces {
                        lines.push(vec![piece]);
                    }
                    // `line = [tmp.pop()]`: `undefined` (joined as "") when the word fit in the
                    // first piece.
                    let tail_len = last.as_ref().map_or(0, |l| l.encode_utf16().count());
                    word_length = widths[widths.len() - tail_len..]
                        .iter()
                        .fold(0.0, |sum, w| sum + w);
                    lines.push(vec![last.unwrap_or_default()]);
                } else {
                    lines.push(vec![word.to_owned()]);
                }
                line_length = word_length;
                separator = space;
            } else {
                if let Some(line) = lines.last_mut() {
                    line.push(word.to_owned());
                }
                line_length += separator + word_length;
                separator = space;
            }
        }
        lines.into_iter().map(|l| l.join(" ")).collect()
    }
}

/// `String(text).split(/\r?\n/)`.
fn split_lines(text: &str) -> Vec<&str> {
    text.split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

/// `splitLongWord`: the piece that fits after what is on the line, then pieces of `max`.
fn split_long_word(units: &[u16], widths: &[f64], first_max: f64, max: f64) -> Vec<String> {
    let text = |range: std::ops::Range<usize>| String::from_utf16_lossy(&units[range]);
    let mut answer = Vec::new();
    let mut i = 0;
    let mut working = 0.0;
    while i != units.len() && working + widths[i] < first_max {
        working += widths[i];
        i += 1;
    }
    answer.push(text(0..i));
    let mut start = i;
    working = 0.0;
    while i != units.len() {
        if working + widths[i] > max {
            answer.push(text(start..i));
            working = 0.0;
            start = i;
        }
        working += widths[i];
        i += 1;
    }
    if start != i {
        answer.push(text(start..i));
    }
    answer
}

/// The character's byte in WinAnsiEncoding, if the standard fonts can show it.
pub fn win_ansi(c: char) -> Option<u8> {
    let code = u32::from(c);
    if (32..=126).contains(&code) || (160..=255).contains(&code) {
        return u8::try_from(code).ok();
    }
    let unit = u16::try_from(code).ok()?;
    WIN_ANSI_EXTRA
        .binary_search_by_key(&unit, |(u, _)| *u)
        .ok()
        .map(|i| WIN_ANSI_EXTRA[i].1)
}
