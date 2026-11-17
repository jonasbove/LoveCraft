//! `JSON.parse` (ECMA-262 §25.5.1), without a reviver.

use std::fmt;

use super::{JsObject, JsString, JsValue};

/// Deepest nesting accepted. ProseCraft's files nest about ten levels; this bounds recursion on
/// hostile input well inside the stack.
pub const MAX_DEPTH: usize = 256;

/// Why a text isn't JSON, and roughly where (a byte offset).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub offset: usize,
    pub reason: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid JSON at byte {}: {}", self.offset, self.reason)
    }
}

impl std::error::Error for ParseError {}

/// Parses JSON text as `JSON.parse` does: strict grammar, `\uXXXX` escapes kept as UTF-16 code
/// units (lone surrogates included), duplicate keys keep the last value.
pub fn parse(text: &str) -> Result<JsValue, ParseError> {
    let mut parser = Parser { text, pos: 0 };
    parser.skip_whitespace();
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if parser.pos != text.len() {
        return Err(parser.error("unexpected text after the value"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, reason: &'static str) -> ParseError {
        ParseError {
            offset: self.pos,
            reason,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect_literal(&mut self, literal: &str, value: JsValue) -> Result<JsValue, ParseError> {
        if self
            .text
            .get(self.pos..)
            .is_some_and(|rest| rest.starts_with(literal))
        {
            self.pos += literal.len();
            Ok(value)
        } else {
            Err(self.error("unknown literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsValue, ParseError> {
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(JsValue::String(self.string()?)),
            Some(b't') => self.expect_literal("true", JsValue::Bool(true)),
            Some(b'f') => self.expect_literal("false", JsValue::Bool(false)),
            Some(b'n') => self.expect_literal("null", JsValue::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end of text")),
        }
    }

    fn enter(&self, depth: usize) -> Result<usize, ParseError> {
        let next = depth + 1;
        if next > MAX_DEPTH {
            Err(self.error("nested too deeply"))
        } else {
            Ok(next)
        }
    }

    fn object(&mut self, depth: usize) -> Result<JsValue, ParseError> {
        let depth = self.enter(depth)?;
        self.pos += 1;
        let mut object = JsObject::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(JsValue::Object(object));
        }
        loop {
            self.skip_whitespace();
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a property name"));
            }
            let key = self.string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error("expected ':'"));
            }
            self.pos += 1;
            self.skip_whitespace();
            let value = self.value(depth)?;
            object.insert(key, value);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(JsValue::Object(object));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<JsValue, ParseError> {
        let depth = self.enter(depth)?;
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(JsValue::Array(items));
        }
        loop {
            self.skip_whitespace();
            items.push(self.value(depth)?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    return Ok(JsValue::Array(items));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - start
    }

    fn number(&mut self) -> Result<JsValue, ParseError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(self.error("expected a digit")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return Err(self.error("expected a digit after '.'"));
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(self.error("expected a digit in the exponent"));
            }
        }
        // The grammar above is a subset of what `f64::from_str` reads, and both round to the
        // nearest double, as JavaScript does. A huge exponent becomes an infinity, as in JS.
        self.text
            .get(start..self.pos)
            .and_then(|slice| slice.parse::<f64>().ok())
            .map(JsValue::Number)
            .ok_or_else(|| self.error("unreadable number"))
    }

    fn hex4(&mut self) -> Result<u16, ParseError> {
        let slice = self
            .text
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| self.error("short \\u escape"))?;
        if !slice.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(self.error("bad \\u escape"));
        }
        let unit = u16::from_str_radix(slice, 16).map_err(|_| self.error("bad \\u escape"))?;
        self.pos += 4;
        Ok(unit)
    }

    fn string(&mut self) -> Result<JsString, ParseError> {
        self.pos += 1;
        let mut units: Vec<u16> = Vec::new();
        loop {
            let rest = self
                .text
                .get(self.pos..)
                .ok_or_else(|| self.error("bad position"))?;
            let ch = rest
                .chars()
                .next()
                .ok_or_else(|| self.error("unterminated string"))?;
            match ch {
                '"' => {
                    self.pos += 1;
                    return Ok(JsString::from_units(units));
                }
                '\\' => {
                    self.pos += 1;
                    let escape = self
                        .peek()
                        .ok_or_else(|| self.error("unterminated escape"))?;
                    self.pos += 1;
                    let unit = match escape {
                        b'"' => 0x22,
                        b'\\' => 0x5c,
                        b'/' => 0x2f,
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'n' => 0x0a,
                        b'r' => 0x0d,
                        b't' => 0x09,
                        b'u' => self.hex4()?,
                        _ => return Err(self.error("unknown escape")),
                    };
                    units.push(unit);
                }
                c if (c as u32) < 0x20 => return Err(self.error("control character in string")),
                c => {
                    let mut buffer = [0u16; 2];
                    units.extend_from_slice(c.encode_utf16(&mut buffer));
                    self.pos += c.len_utf8();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_DEPTH, parse};
    use crate::js::{JsString, JsValue};

    #[test]
    fn rejects_what_json_parse_rejects() {
        for text in [
            "",
            " ",
            "01",
            "1.",
            ".5",
            "+1",
            "[1,]",
            "{\"a\":1,}",
            "'a'",
            "\"\t\"",
            "{a:1}",
            "NaN",
            "[1 2]",
            "\"\\x\"",
            "\"\\u12G4\"",
            "tru",
            "\u{feff}1",
            "1 2",
        ] {
            assert!(parse(text).is_err(), "accepted {text:?}");
        }
    }

    #[test]
    fn keeps_lone_surrogates_and_last_duplicate() {
        let value = parse(r#"{"a":"x\ud83dy","a":"second","b":1}"#).ok();
        let object = value.as_ref().and_then(JsValue::as_object);
        assert_eq!(object.map(super::super::JsObject::len), Some(2));
        let a = object
            .and_then(|o| o.get("a"))
            .and_then(JsValue::as_js_string);
        assert_eq!(a, Some(&JsString::from("second")));
        let lone = parse(r#""x\ud83dy""#).ok();
        assert_eq!(
            lone,
            Some(JsValue::String(JsString::from_units(vec![
                0x78, 0xd83d, 0x79
            ])))
        );
    }

    #[test]
    fn bounds_nesting_instead_of_overflowing_the_stack() {
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert!(parse(&deep).is_err());
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(&ok).is_ok());
        let hostile = "[".repeat(1_000_000);
        assert!(parse(&hostile).is_err());
    }
}
