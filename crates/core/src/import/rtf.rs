//! A small, tolerant RTF reader for running text (ProseCraft `src/lib/import/rtf.ts`, upstream
//! `7dae544` with jennraeross/ProseCraft#50), as Scrivener stores each document: paragraphs,
//! line breaks and the common character styles; layout, tables, images and every other
//! destination are ignored.
//!
//! This follows `rtf.ts`'s byte reader exactly, quirks included (a `\'` takes the next two
//! bytes whatever they are; a raw 8-bit byte is decoded on its own), so text comes out as
//! ProseCraft's does; craft-rtf's tokenizer reads such input differently. Code pages are the
//! browser's `TextDecoder` labels through `encoding_rs`: `shift_jis`, `gbk`, `big5`, `macintosh`
//! and UTF-8 for 932, 936, 950, 10000 and 65001, `windows-N` for the rest, falling back to
//! Windows-1252 where the browser has no such label.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct State {
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    uc: i64,
}

const SKIPPED_DESTINATIONS: [&[u8]; 11] = [
    b"fonttbl",
    b"colortbl",
    b"stylesheet",
    b"info",
    b"pict",
    b"header",
    b"footer",
    b"footnote",
    b"listtable",
    b"listoverridetable",
    b"rsidtbl",
];

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

struct Reader {
    decoder: &'static encoding_rs::Encoding,
    bytes: Vec<u8>,
    units: Vec<u16>,
    skip_chars: i64,
    state: State,
    stack: Vec<State>,
    skip_depth: usize,
    paragraphs: Vec<String>,
    current: String,
    open_tags: Vec<&'static str>,
    emitted: usize,
    has_text: bool,
}

impl Reader {
    fn new() -> Self {
        Self {
            decoder: encoding_rs::WINDOWS_1252,
            bytes: Vec::new(),
            units: Vec::new(),
            skip_chars: 0,
            state: State {
                uc: 1,
                ..State::default()
            },
            stack: Vec::new(),
            skip_depth: 0,
            paragraphs: Vec::new(),
            current: String::new(),
            open_tags: Vec::new(),
            emitted: 0,
            has_text: false,
        }
    }

    fn desired(&self) -> Vec<&'static str> {
        let s = self.state;
        [
            (s.bold, "strong"),
            (s.italic, "em"),
            (s.underline, "u"),
            (s.strike, "s"),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, t)| t)
        .collect()
    }

    fn emit_open_tags(&mut self) {
        if !self.has_text {
            return;
        }
        for tag in &self.open_tags[self.emitted..] {
            self.current.push_str(&format!("<{tag}>"));
        }
        self.emitted = self.open_tags.len();
    }

    /// `syncTags`: only the part that changed is closed or opened, so the HTML stays nested.
    fn sync_tags(&mut self) {
        self.flush_text();
        let desired = self.desired();
        let keep = self
            .open_tags
            .iter()
            .zip(&desired)
            .take_while(|(a, b)| a == b)
            .count();
        for index in (keep..self.emitted).rev() {
            self.current
                .push_str(&format!("</{}>", self.open_tags[index]));
        }
        self.emitted = self.emitted.min(keep);
        self.open_tags = desired;
        self.emit_open_tags();
    }

    fn append_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.has_text = true;
        self.emit_open_tags();
        self.current.push_str(&escape_html(text));
    }

    /// `decoder.decode(bytes)`: a fresh decode each time, so a byte-order mark at its start goes.
    fn flush_bytes(&mut self) {
        if self.bytes.is_empty() {
            return;
        }
        let bytes = std::mem::take(&mut self.bytes);
        let (text, _) = self.decoder.decode_with_bom_removal(&bytes);
        let text = text.into_owned();
        self.append_text(&text);
    }

    /// The collected `\uN` code units, joined so a surrogate pair is one character.
    fn flush_units(&mut self) {
        if self.units.is_empty() {
            return;
        }
        let units = std::mem::take(&mut self.units);
        self.append_text(&String::from_utf16_lossy(&units));
    }

    fn flush_text(&mut self) {
        self.flush_bytes();
        self.flush_units();
    }

    fn append_html(&mut self, html: &str) {
        self.flush_text();
        self.has_text = true;
        self.emit_open_tags();
        self.current.push_str(html);
    }

    fn close_tags(&mut self) {
        for index in (0..self.emitted).rev() {
            self.current
                .push_str(&format!("</{}>", self.open_tags[index]));
        }
        self.emitted = 0;
    }

    fn end_paragraph(&mut self) {
        self.flush_text();
        self.close_tags();
        self.paragraphs.push(std::mem::take(&mut self.current));
        self.has_text = false;
        self.sync_tags();
    }

    /// `setCodepage(String(number))`: `CODEPAGE_LABELS`, else `windows-N` (`windows-NaN` for an
    /// unreadable number); Windows-1252 where the browser knows no such label.
    fn set_codepage(&mut self, number: f64) {
        self.flush_text();
        let name = crate::js::number_to_string(number);
        let label = match name.as_str() {
            "932" => "shift_jis".to_owned(),
            "936" => "gbk".to_owned(),
            "950" => "big5".to_owned(),
            "10000" => "macintosh".to_owned(),
            "65001" => "utf-8".to_owned(),
            _ => format!("windows-{name}"),
        };
        self.decoder =
            encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::WINDOWS_1252);
    }

    fn control_word(&mut self, word: &[u8], parameter: Option<&str>) {
        // `Number.parseInt`: digits after an optional minus; "-" alone is NaN.
        let number: Option<f64> = parameter.map(|p| p.parse::<f64>().unwrap_or(f64::NAN));
        let flag = |n: Option<f64>| n != Some(0.0);
        match word {
            b"ansicpg" => {
                if let Some(n) = number {
                    self.set_codepage(n);
                }
            }
            b"uc" => {
                if let Some(n) = number {
                    // `Math.max(NaN, 0)` is NaN, which skips nothing.
                    self.state.uc = if n.is_nan() {
                        0
                    } else {
                        (n.max(0.0)).min(i64::MAX as f64) as i64
                    };
                }
            }
            b"u" => {
                // No flush of the code units: two escapes in a row form one surrogate pair.
                self.flush_bytes();
                if let Some(n) = number.filter(|n| n.is_finite()) {
                    let unit = if n < 0.0 { n + 65536.0 } else { n };
                    if (0.0..=65535.0).contains(&unit) {
                        self.units.push(unit as u16);
                    }
                }
                self.skip_chars = self.state.uc;
            }
            b"b" => {
                self.state.bold = flag(number);
                self.sync_tags();
            }
            b"i" => {
                self.state.italic = flag(number);
                self.sync_tags();
            }
            b"ul" => {
                self.state.underline = flag(number);
                self.sync_tags();
            }
            b"ulnone" => {
                self.state.underline = false;
                self.sync_tags();
            }
            b"strike" => {
                self.state.strike = flag(number);
                self.sync_tags();
            }
            b"plain" => {
                self.state = State {
                    uc: self.state.uc,
                    ..State::default()
                };
                self.sync_tags();
            }
            b"pard" => self.sync_tags(),
            b"par" => self.end_paragraph(),
            b"line" => self.append_html("<br>"),
            b"tab" => {
                self.flush_text();
                self.append_text("\t");
            }
            _ => {}
        }
    }

    fn process(&mut self, bytes: &[u8]) {
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            if byte == b'{' {
                if self.skip_depth > 0 {
                    self.skip_depth += 1;
                }
                self.stack.push(self.state);
                index += 1;
                continue;
            }
            if byte == b'}' {
                if self.skip_depth > 0 {
                    self.skip_depth -= 1;
                    // The state pushed by the `{` that started the skipped destination.
                    if self.skip_depth == 0 {
                        self.stack.pop();
                    }
                    index += 1;
                    continue;
                }
                self.flush_text();
                if let Some(state) = self.stack.pop() {
                    self.state = state;
                }
                self.sync_tags();
                index += 1;
                continue;
            }
            if self.skip_depth > 0 {
                index += 1;
                continue;
            }
            if byte == b'\\' {
                let Some(&next) = bytes.get(index + 1) else {
                    break;
                };
                if next == b'\'' {
                    index += 1;
                    if index + 2 >= bytes.len() {
                        break;
                    }
                    let hex = [bytes[index + 1], bytes[index + 2]];
                    index += 3;
                    if self.skip_chars > 0 {
                        self.skip_chars -= 1;
                        continue;
                    }
                    if hex.iter().all(u8::is_ascii_hexdigit)
                        && let Ok(value) =
                            u8::from_str_radix(std::str::from_utf8(&hex).unwrap_or("zz"), 16)
                    {
                        // Pending `\uN` text came first.
                        self.flush_units();
                        self.bytes.push(value);
                    }
                    continue;
                }
                // A control word or symbol is never one of the \ucN replacement characters.
                self.skip_chars = 0;
                // An escaped line break is a paragraph break; CRLF counts once.
                if next == b'\n' || next == b'\r' {
                    index += if next == b'\r' && bytes.get(index + 2) == Some(&b'\n') {
                        3
                    } else {
                        2
                    };
                    self.end_paragraph();
                    continue;
                }
                if next.is_ascii_alphabetic() {
                    let mut end = index + 1;
                    while end < bytes.len() && bytes[end].is_ascii_alphabetic() {
                        end += 1;
                    }
                    let word = bytes[index + 1..end].to_vec();
                    let mut parameter = String::new();
                    if bytes.get(end) == Some(&b'-') {
                        parameter.push('-');
                        end += 1;
                    }
                    while end < bytes.len() && bytes[end].is_ascii_digit() {
                        parameter.push(bytes[end] as char);
                        end += 1;
                    }
                    if bytes.get(end) == Some(&b' ') {
                        end += 1;
                    }
                    index = end;
                    if SKIPPED_DESTINATIONS.contains(&word.as_slice()) {
                        self.skip_depth = 1;
                        continue;
                    }
                    self.control_word(&word, (!parameter.is_empty()).then_some(parameter.as_str()));
                    continue;
                }
                index += 2;
                match next {
                    b'*' => self.skip_depth = 1,
                    b'~' => self.append_html("&nbsp;"),
                    b'_' => self.append_html("&#8209;"),
                    b'{' | b'}' | b'\\' => {
                        self.flush_text();
                        self.append_text(&(next as char).to_string());
                    }
                    _ => {}
                }
                continue;
            }
            // A bare CR or LF is only file layout, never text.
            if byte == b'\r' || byte == b'\n' {
                index += 1;
                continue;
            }
            if self.skip_chars > 0 {
                self.skip_chars -= 1;
                index += 1;
                continue;
            }
            self.flush_text();
            self.bytes.push(byte);
            index += 1;
        }
    }
}

/// `rtfToHtml`: the running text as `<p>` paragraphs (`<p><br></p>` when there is none).
pub fn rtf_to_html(rtf: &[u8]) -> String {
    let mut reader = Reader::new();
    reader.process(rtf);
    reader.flush_text();
    reader.close_tags();
    if reader.has_text || !reader.current.is_empty() {
        let last = std::mem::take(&mut reader.current);
        reader.paragraphs.push(last);
    }
    let visible: Vec<&String> = reader.paragraphs.iter().filter(|p| !p.is_empty()).collect();
    if visible.is_empty() {
        return "<p><br></p>".into();
    }
    visible.iter().map(|p| format!("<p>{p}</p>")).collect()
}
