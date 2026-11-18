//! The message of the `SyntaxError` that `JSON.parse` throws in V8 (Chrome, Node, Electron), for
//! code that shows it to the writer, as ProseCraft's citation import does. The engine's own
//! wording, positions and line and column numbers (UTF-16 units, as JavaScript counts them).
//! Other engines word these differently: ProseCraft shows V8's text in Chromium browsers and on
//! Windows (WebView2), WebKit's on macOS and Linux. V8's is the one its fixtures record (Node).

/// Where V8's parser stops, and what it reports.
enum Fail {
    /// "Unexpected end of JSON input".
    End,
    /// A message with the position it names (" at position N…" follows it).
    At(&'static str, usize),
    /// The character at a position was not expected (V8's `ReportUnexpectedToken` without a
    /// specific message).
    Token(usize),
}

/// Characters shown on each side of the unexpected one in a long text.
const CONTEXT: usize = 10;

/// Inputs V8 names as a whole rather than by their first character.
const SPECIAL: [&str; 4] = ["NaN", "Infinity", "undefined", "[object Object]"];

struct Scan {
    units: Vec<u16>,
    pos: usize,
}

fn is_digit(unit: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&unit)
}

fn is(unit: Option<u16>, c: u8) -> bool {
    unit == Some(u16::from(c))
}

impl Scan {
    fn peek(&self) -> Option<u16> {
        self.units.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(0x20 | 0x09 | 0x0a | 0x0d)) {
            self.pos += 1;
        }
    }

    fn digits(&mut self) {
        while self.peek().is_some_and(is_digit) {
            self.pos += 1;
        }
    }

    fn require_digit(&mut self, message: &'static str) -> Result<(), Fail> {
        if self.peek().is_some_and(is_digit) {
            self.digits();
            Ok(())
        } else {
            Err(Fail::At(message, self.pos))
        }
    }

    fn number(&mut self) -> Result<(), Fail> {
        if is(self.peek(), b'-') {
            self.pos += 1;
            if !self.peek().is_some_and(is_digit) {
                return Err(Fail::At("No number after minus sign in JSON", self.pos));
            }
        }
        if is(self.peek(), b'0') {
            self.pos += 1;
            if self.peek().is_some_and(is_digit) {
                return Err(Fail::Token(self.pos));
            }
        } else {
            self.digits();
        }
        if is(self.peek(), b'.') {
            self.pos += 1;
            self.require_digit("Unterminated fractional number in JSON")?;
        }
        if is(self.peek(), b'e') || is(self.peek(), b'E') {
            self.pos += 1;
            if is(self.peek(), b'+') || is(self.peek(), b'-') {
                self.pos += 1;
            }
            self.require_digit("Exponent part is missing a number in JSON")?;
        }
        Ok(())
    }

    fn escape(&mut self) -> Result<(), Fail> {
        let Some(unit) = self.peek() else {
            return Err(Fail::End);
        };
        if b"\"\\/bfnrt".iter().any(|c| u16::from(*c) == unit) {
            self.pos += 1;
            return Ok(());
        }
        if unit != u16::from(b'u') {
            return Err(Fail::At("Bad escaped character in JSON", self.pos));
        }
        self.pos += 1;
        for _ in 0..4 {
            let hex = self
                .peek()
                .and_then(|u| char::from_u32(u32::from(u)))
                .is_some_and(|c| c.is_ascii_hexdigit());
            if !hex {
                return Err(Fail::At("Bad Unicode escape in JSON", self.pos));
            }
            self.pos += 1;
        }
        Ok(())
    }

    fn string(&mut self) -> Result<(), Fail> {
        self.pos += 1;
        loop {
            match self.peek() {
                None => return Err(Fail::At("Unterminated string in JSON", self.pos)),
                Some(0x22) => {
                    self.pos += 1;
                    return Ok(());
                }
                Some(0x5c) => {
                    self.pos += 1;
                    self.escape()?;
                }
                Some(unit) if unit < 0x20 => {
                    return Err(Fail::At(
                        "Bad control character in string literal in JSON",
                        self.pos,
                    ));
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    fn literal(&mut self, word: &str) -> Result<(), Fail> {
        for c in word.bytes() {
            match self.peek() {
                None => return Err(Fail::End),
                Some(unit) if unit == u16::from(c) => self.pos += 1,
                Some(_) => return Err(Fail::Token(self.pos)),
            }
        }
        Ok(())
    }

    /// A property name and its colon.
    fn key(&mut self, missing: &'static str) -> Result<(), Fail> {
        self.skip_whitespace();
        if !is(self.peek(), b'"') {
            return Err(Fail::At(missing, self.pos));
        }
        self.string()?;
        self.skip_whitespace();
        if !is(self.peek(), b':') {
            return Err(Fail::At(
                "Expected ':' after property name in JSON",
                self.pos,
            ));
        }
        self.pos += 1;
        Ok(())
    }

    /// One value; an opened object or array is pushed on `open` (true for an object) and its
    /// first member is left for the caller. Returns whether a container was opened.
    fn value(&mut self, open: &mut Vec<bool>) -> Result<bool, Fail> {
        self.skip_whitespace();
        let Some(unit) = self.peek() else {
            return Err(Fail::End);
        };
        match u8::try_from(unit).unwrap_or(0) {
            b'{' => {
                self.pos += 1;
                self.skip_whitespace();
                if is(self.peek(), b'}') {
                    self.pos += 1;
                    return Ok(false);
                }
                self.key("Expected property name or '}' in JSON")?;
                open.push(true);
                Ok(true)
            }
            b'[' => {
                self.pos += 1;
                self.skip_whitespace();
                if is(self.peek(), b']') {
                    self.pos += 1;
                    return Ok(false);
                }
                open.push(false);
                Ok(true)
            }
            b'"' => self.string().map(|()| false),
            b'-' | b'0'..=b'9' => self.number().map(|()| false),
            b't' => self.literal("true").map(|()| false),
            b'f' => self.literal("false").map(|()| false),
            b'n' => self.literal("null").map(|()| false),
            _ => Err(Fail::Token(self.pos)),
        }
    }

    /// After a value: closes finished containers, and returns whether another value follows.
    fn after_value(&mut self, open: &mut Vec<bool>) -> Result<bool, Fail> {
        loop {
            self.skip_whitespace();
            let Some(&object) = open.last() else {
                return if self.pos < self.units.len() {
                    Err(Fail::At(
                        "Unexpected non-whitespace character after JSON",
                        self.pos,
                    ))
                } else {
                    Ok(false)
                };
            };
            let (close, expected) = if object {
                (b'}', "Expected ',' or '}' after property value in JSON")
            } else {
                (b']', "Expected ',' or ']' after array element in JSON")
            };
            if is(self.peek(), b',') {
                self.pos += 1;
                if object {
                    self.key("Expected double-quoted property name in JSON")?;
                }
                return Ok(true);
            }
            if !is(self.peek(), close) {
                return Err(Fail::At(expected, self.pos));
            }
            self.pos += 1;
            open.pop();
        }
    }

    fn run(&mut self) -> Result<(), Fail> {
        let mut open = Vec::new();
        loop {
            if self.value(&mut open)? {
                continue;
            }
            if !self.after_value(&mut open)? {
                return Ok(());
            }
        }
    }

    /// `(line L column C)` of a position: `\r\n`, `\r` and `\n` each end a line.
    fn line_and_column(&self, pos: usize) -> (usize, usize) {
        let mut line = 1;
        let mut line_start = 0;
        let mut i = 0;
        while i < pos.min(self.units.len()) {
            let unit = self.units[i];
            if unit == 0x0d || unit == 0x0a {
                if unit == 0x0d && self.units.get(i + 1) == Some(&0x0a) {
                    i += 1;
                }
                line += 1;
                line_start = i + 1;
            }
            i += 1;
        }
        (line, pos.saturating_sub(line_start) + 1)
    }

    fn at(&self, message: &str, pos: usize) -> String {
        let (line, column) = self.line_and_column(pos);
        format!("{message} at position {pos} (line {line} column {column})")
    }

    fn token(&self, text: &str, pos: usize) -> String {
        let Some(&unit) = self.units.get(pos) else {
            return "Unexpected end of JSON input".into();
        };
        if unit == 0x22 {
            return self.at("Unexpected string in JSON", pos);
        }
        if unit == u16::from(b'-') || is_digit(unit) {
            return self.at("Unexpected number in JSON", pos);
        }
        if SPECIAL.contains(&text) {
            return format!("\"{text}\" is not valid JSON");
        }
        let token = String::from_utf16_lossy(&[unit]);
        let len = self.units.len();
        let slice =
            |from: usize, to: usize| String::from_utf16_lossy(&self.units[from..to.min(len)]);
        if len <= 2 * CONTEXT {
            format!("Unexpected token '{token}', \"{text}\" is not valid JSON")
        } else if pos < CONTEXT {
            let shown = slice(0, pos + CONTEXT);
            format!("Unexpected token '{token}', \"{shown}\"... is not valid JSON")
        } else if pos < len - CONTEXT {
            let shown = slice(pos - CONTEXT, pos + CONTEXT);
            format!("Unexpected token '{token}', ...\"{shown}\"... is not valid JSON")
        } else {
            let shown = slice(pos - CONTEXT, len);
            format!("Unexpected token '{token}', ...\"{shown}\" is not valid JSON")
        }
    }
}

/// The `message` of the `SyntaxError` V8's `JSON.parse(text)` throws, or `None` when V8 reads
/// the text.
pub fn parse_error_message(text: &str) -> Option<String> {
    let mut scan = Scan {
        units: text.encode_utf16().collect(),
        pos: 0,
    };
    match scan.run() {
        Ok(()) => None,
        Err(Fail::End) => Some("Unexpected end of JSON input".into()),
        Err(Fail::At(message, pos)) => Some(scan.at(message, pos)),
        Err(Fail::Token(pos)) => Some(scan.token(text, pos)),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_error_message;

    /// Messages printed by `JSON.parse` in Node 26 (V8).
    #[test]
    fn messages_match_v8() {
        let cases: &[(&str, &str)] = &[
            (
                "[{\"id\": \"broken\", ",
                "Expected double-quoted property name in JSON at position 18 (line 1 column 19)",
            ),
            (
                "@article{q, title={T}}",
                "Unexpected token '@', \"@article{q\"... is not valid JSON",
            ),
            ("", "Unexpected end of JSON input"),
            (" ", "Unexpected end of JSON input"),
            (
                "01",
                "Unexpected number in JSON at position 1 (line 1 column 2)",
            ),
            (
                "1.",
                "Unterminated fractional number in JSON at position 2 (line 1 column 3)",
            ),
            (".5", "Unexpected token '.', \".5\" is not valid JSON"),
            ("[1,]", "Unexpected token ']', \"[1,]\" is not valid JSON"),
            (
                "{\"a\":1,}",
                "Expected double-quoted property name in JSON at position 7 (line 1 column 8)",
            ),
            (
                "\"\t\"",
                "Bad control character in string literal in JSON at position 1 (line 1 column 2)",
            ),
            (
                "{a:1}",
                "Expected property name or '}' in JSON at position 1 (line 1 column 2)",
            ),
            ("NaN", "\"NaN\" is not valid JSON"),
            ("  NaN", "Unexpected token 'N', \"  NaN\" is not valid JSON"),
            (
                "[1 2]",
                "Expected ',' or ']' after array element in JSON at position 3 (line 1 column 4)",
            ),
            (
                "\"\\x\"",
                "Bad escaped character in JSON at position 2 (line 1 column 3)",
            ),
            (
                "\"\\u12G4\"",
                "Bad Unicode escape in JSON at position 5 (line 1 column 6)",
            ),
            (
                "\"\\u",
                "Bad Unicode escape in JSON at position 3 (line 1 column 4)",
            ),
            ("\"\\", "Unexpected end of JSON input"),
            ("tru", "Unexpected end of JSON input"),
            (
                "1 2",
                "Unexpected non-whitespace character after JSON at position 2 (line 1 column 3)",
            ),
            (
                "{",
                "Expected property name or '}' in JSON at position 1 (line 1 column 2)",
            ),
            ("[", "Unexpected end of JSON input"),
            (
                "{\"a\"",
                "Expected ':' after property name in JSON at position 4 (line 1 column 5)",
            ),
            ("{\"a\":", "Unexpected end of JSON input"),
            (
                "{\"a\":1 \"b\"}",
                "Expected ',' or '}' after property value in JSON at position 7 (line 1 column 8)",
            ),
            (
                "-a",
                "No number after minus sign in JSON at position 1 (line 1 column 2)",
            ),
            (
                "1e+",
                "Exponent part is missing a number in JSON at position 3 (line 1 column 4)",
            ),
            (
                "\"abc",
                "Unterminated string in JSON at position 4 (line 1 column 5)",
            ),
            (
                "nulx 1234567890123456789012",
                "Unexpected token 'x', \"nulx 12345678\"... is not valid JSON",
            ),
            (
                "{\"é😀\":1,}",
                "Expected double-quoted property name in JSON at position 9 (line 1 column 10)",
            ),
            (
                "[1]\r\n x",
                "Unexpected non-whitespace character after JSON at position 6 (line 2 column 2)",
            ),
            (
                "[1]\n\r\n\t\t x",
                "Unexpected non-whitespace character after JSON at position 9 (line 3 column 4)",
            ),
            (
                "[\n1\n\"a\n",
                "Expected ',' or ']' after array element in JSON at position 4 (line 3 column 1)",
            ),
            (
                "[t1]",
                "Unexpected number in JSON at position 2 (line 1 column 3)",
            ),
            (
                "[t\"]",
                "Unexpected string in JSON at position 2 (line 1 column 3)",
            ),
            (
                "[fals]",
                "Unexpected token ']', \"[fals]\" is not valid JSON",
            ),
            (
                "{\"x\":truex}",
                "Expected ',' or '}' after property value in JSON at position 9 (line 1 column 10)",
            ),
            (
                "@2345678901234567890",
                "Unexpected token '@', \"@2345678901234567890\" is not valid JSON",
            ),
            (
                "@23456789012345678901",
                "Unexpected token '@', \"@234567890\"... is not valid JSON",
            ),
            (
                "[1,2,3,4,5,6,7,8,9,1,2,]",
                "Unexpected token ']', ...\"7,8,9,1,2,]\" is not valid JSON",
            ),
            (
                "é😀é😀é😀é😀é😀é😀é😀é😀é😀é😀@é😀é😀é😀é😀é😀é😀é😀é😀é😀é😀",
                "Unexpected token 'é', \"é😀é😀é😀é\"... is not valid JSON",
            ),
            (
                "[1,2,3,4,5,6,7,8,9,10,11,@,1,2,3,4,5,6,7,8,9,0]",
                "Unexpected token '@', ...\"8,9,10,11,@,1,2,3,4,\"... is not valid JSON",
            ),
            (
                "{\"a\":1}}",
                "Unexpected non-whitespace character after JSON at position 7 (line 1 column 8)",
            ),
        ];
        for (text, want) in cases {
            assert_eq!(
                parse_error_message(text).as_deref(),
                Some(*want),
                "{text:?}"
            );
        }
        assert_eq!(
            parse_error_message("{\"a\":[1,{\"b\":null}],\"c\":-1.5e3}"),
            None
        );
    }
}
