//! JSON exactly as ProseCraft's JavaScript reads and writes it.
//!
//! ProseCraft stores everything as JSON made by `JSON.stringify` and read by `JSON.parse`, and it
//! hashes and authenticates some of it byte for byte (`stableStringify`, the AAD of encrypted
//! records). A general JSON library is close but not exact, and the differences lose data or break
//! decryption:
//!
//! - **Strings are UTF-16.** A JavaScript string may hold a lone surrogate (a comment context cut
//!   by index next to an emoji, say), which `JSON.stringify` writes as `\ud83d`. Rust's `String`
//!   can't hold it, and `serde_json` refuses the whole file. [`JsString`] keeps UTF-16 code units.
//! - **Object key order.** JavaScript puts integer-like keys (`"2"`, `"10"`) first, in ascending
//!   order, then the rest in insertion order. A duplicate key keeps its first position.
//! - **Numbers.** `String(n)` has its own rules (`1e+21`, `1e-7`, `0.000001`, `-0` → `0`).
//!
//! Ported from the behaviour of `JSON.parse`, `JSON.stringify` (ECMA-262) and `stableStringify` in
//! ProseCraft's `src/lib/storage/schema.ts` (upstream `7dae544`).

mod number;
mod parse;
mod stringify;

pub use number::{number_to_string, string_to_number};
pub use parse::{MAX_DEPTH, ParseError, parse};
pub use stringify::{stable_stringify, stringify, stringify_pretty};

use std::fmt;

/// A JavaScript string: UTF-16 code units, lone surrogates included.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct JsString(Vec<u16>);

impl JsString {
    pub fn from_units(units: Vec<u16>) -> Self {
        Self(units)
    }

    pub fn units(&self) -> &[u16] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The text as UTF-8, with each lone surrogate replaced by U+FFFD, as `TextEncoder` does.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    /// The text, if it holds no lone surrogate.
    pub fn as_string(&self) -> Option<String> {
        String::from_utf16(&self.0).ok()
    }

    /// Whether this equals `text` exactly.
    pub fn eq_str(&self, text: &str) -> bool {
        self.0.iter().copied().eq(text.encode_utf16())
    }

    /// The array index this key names, if it is one (`"0"`, `"42"`; not `"01"`, `"-1"` or
    /// `"4294967295"`). JavaScript orders such keys first.
    fn array_index(&self) -> Option<u32> {
        let units = &self.0;
        let first = *units.first()?;
        if units.len() > 10 || !units.iter().all(|u| (0x30..=0x39).contains(u)) {
            return None;
        }
        if first == 0x30 && units.len() > 1 {
            return None;
        }
        let mut value: u64 = 0;
        for unit in units {
            value = value * 10 + u64::from(unit - 0x30);
        }
        u32::try_from(value).ok().filter(|v| *v != u32::MAX)
    }
}

impl From<&str> for JsString {
    fn from(text: &str) -> Self {
        Self(text.encode_utf16().collect())
    }
}

impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_string_lossy())
    }
}

/// A JSON value with JavaScript semantics.
#[derive(Clone, Debug, PartialEq)]
pub enum JsValue {
    Null,
    Bool(bool),
    Number(f64),
    String(JsString),
    Array(Vec<JsValue>),
    Object(JsObject),
}

impl JsValue {
    pub fn str(text: &str) -> Self {
        Self::String(JsString::from(text))
    }

    pub fn as_object(&self) -> Option<&JsObject> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut JsObject> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[JsValue]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_js_string(&self) -> Option<&JsString> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// `value[key]` for an object, `None` for anything else.
    pub fn get(&self, key: &str) -> Option<&JsValue> {
        self.as_object().and_then(|object| object.get(key))
    }
}

/// A JavaScript object made from JSON: own data properties in insertion order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JsObject {
    entries: Vec<(JsString, JsValue)>,
}

impl JsObject {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a property. A new key goes last; an existing key keeps its place, as in JavaScript.
    pub fn insert(&mut self, key: impl Into<JsString>, value: JsValue) {
        let key = key.into();
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsValue> {
        self.entries
            .iter()
            .find(|(k, _)| k.eq_str(key))
            .map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut JsValue> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k.eq_str(key))
            .map(|(_, v)| v)
    }

    pub fn remove(&mut self, key: &str) -> Option<JsValue> {
        let index = self.entries.iter().position(|(k, _)| k.eq_str(key))?;
        Some(self.entries.remove(index).1)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The properties in JavaScript's order (`Object.keys`): integer-like keys ascending, then
    /// the rest in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&JsString, &JsValue)> {
        let mut indexed: Vec<(u32, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(position, (key, _))| key.array_index().map(|index| (index, position)))
            .collect();
        indexed.sort_unstable();
        let named = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, (key, _))| key.array_index().is_none())
            .map(|(position, _)| position);
        let order: Vec<usize> = indexed
            .into_iter()
            .map(|(_, position)| position)
            .chain(named)
            .collect();
        order
            .into_iter()
            .filter_map(|position| self.entries.get(position).map(|(k, v)| (k, v)))
    }
}

impl From<String> for JsString {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}
